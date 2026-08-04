//! Resolve nested relation filters (Prisma-style `some`/`none`/`every`) by
//! issuing pre-queries against related collections and collecting the matching
//! parent-side `_id`s.
//!
//! Example GraphQL input:
//! ```graphql
//! heroes(where: { missions: { some: { code: { eq: "OP-1" } } } })
//! ```
//!
//! Becomes a BSON `where` document like:
//! ```json
//! { "missions": { "some": { "code": { "eq": "OP-1" } } } }
//! ```
//!
//! The module splits relation-filter keys from scalar-filter keys, resolves
//! each relation filter into a set of parent `ObjectId`s, intersects sets when
//! multiple relation filters are present, and returns the final ID set for the
//! caller to merge into the main query as `_id: { $in / $nin }`.

use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use mongodb::bson::{doc, oid::ObjectId, Bson, Document};
use mongodb::Database;

use crate::error::GraphQLError;
use crate::resolvers::query::transform_where_filter;
use crate::schema::definition::{CollectionDef, FieldType, RelationKind, SchemaDefinition};

/// Maximum nesting depth for relation filters (e.g. hero → mission → villain
/// is depth 2).
const MAX_FILTER_DEPTH: usize = 2;

#[derive(Debug, Default)]
struct RelationFilterOperators {
    some: Option<Document>,
    every: Option<Document>,
    none: Option<Document>,
}

impl RelationFilterOperators {
    fn from_document(doc: &Document) -> Self {
        Self {
            some: doc.get("some").and_then(|v| v.as_document()).cloned(),
            every: doc.get("every").and_then(|v| v.as_document()).cloned(),
            none: doc.get("none").and_then(|v| v.as_document()).cloned(),
        }
    }

    fn is_empty(&self) -> bool {
        self.some.is_none() && self.every.is_none() && self.none.is_none()
    }
}

fn is_relation_field(key: &str, collection_def: &CollectionDef) -> bool {
    collection_def
        .fields
        .iter()
        .any(|field| field.graphql_name() == key && matches!(field.field_type, FieldType::Relation(_)))
}

pub(crate) fn is_relation_filter_key(key: &str, collection_def: &CollectionDef) -> bool {
    is_relation_field(key, collection_def)
}

fn relation_field_type<'a>(
    key: &str,
    collection_def: &'a CollectionDef,
) -> Option<&'a crate::schema::definition::RelationFieldDef> {
    collection_def
        .fields
        .iter()
        .find(|field| field.graphql_name() == key)
        .and_then(|field| match &field.field_type {
            FieldType::Relation(rel) => Some(rel),
            _ => None,
        })
}

async fn collect_ids_from_cursor(
    cursor: &mut mongodb::Cursor<Document>,
) -> Result<Vec<ObjectId>, GraphQLError> {
    let mut ids = Vec::new();
    while let Some(result) = cursor.next().await {
        let doc = result?;
        if let Ok(oid) = doc.get_object_id("_id") {
            ids.push(oid);
        }
    }
    Ok(ids)
}

/// Merge pre-resolved operator ID sets. `some` intersects into the include set.
/// `every` and `none` add to the exclude set (`every` is resolved via negation
/// before reaching this function).
#[inline]
fn merge_operator_ids(
    include: &mut Option<HashSet<ObjectId>>,
    exclude: &mut HashSet<ObjectId>,
    some_ids: Option<HashSet<ObjectId>>,
    every_ids: Option<HashSet<ObjectId>>,
    none_ids: Option<HashSet<ObjectId>>,
) {
    if let Some(ids) = some_ids {
        *include = Some(match include.take() {
            None => ids,
            Some(existing) => existing.intersection(&ids).cloned().collect(),
        });
    }
    if let Some(ids) = every_ids {
        exclude.extend(ids);
    }
    if let Some(ids) = none_ids {
        exclude.extend(ids);
    }
}

fn try_negate_operator(op: &str) -> Option<&str> {
    match op {
        "eq" => Some("ne"),
        "ne" => Some("eq"),
        "gt" => Some("lte"),
        "gte" => Some("lt"),
        "lt" => Some("gte"),
        "lte" => Some("gt"),
        "in" => Some("nin"),
        "nin" => Some("in"),
        _ => None,
    }
}

fn try_negate_filter(filter: &Document) -> Option<Document> {
    let mut negated = Document::new();
    for (key, value) in filter {
        let inner = value.as_document()?;
        let mut negated_inner = Document::new();
        for (op, val) in inner {
            negated_inner.insert(try_negate_operator(op)?, val.clone());
        }
        negated.insert(key.clone(), Bson::Document(negated_inner));
    }
    Some(negated)
}

async fn resolve_forward_filter(
    definition: &SchemaDefinition,
    db: &Database,
    source_coll: &CollectionDef,
    fk_mongo_name: &str,
    target_coll_name: &str,
    ops: &RelationFilterOperators,
    depth: usize,
) -> Result<HashSet<ObjectId>, GraphQLError> {
    let target_def = definition
        .collection_by_name(target_coll_name)
        .ok_or_else(|| {
            GraphQLError::Internal(format!(
                "Target collection '{}' not found in schema definition",
                target_coll_name
            ))
        })?;
    let target_collection = db.collection::<Document>(target_coll_name);

    let mut include: Option<HashSet<ObjectId>> = None;
    let mut exclude = HashSet::new();
    let some_ids = if let Some(ref f) = ops.some {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, true, depth + 1).await?)
    } else { None };
    let every_ids = if let Some(ref f) = ops.every {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, true, depth + 1).await?)
    } else { None };
    let none_ids = if let Some(ref f) = ops.none {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, true, depth + 1).await?)
    } else { None };
    merge_operator_ids(&mut include, &mut exclude, some_ids, every_ids, none_ids);

    let effective = include.unwrap_or_default();
    if effective.is_empty() {
        return Ok(HashSet::new());
    }
    let oids: Vec<Bson> = effective.iter().map(|id| Bson::ObjectId(*id)).collect();
    let source_collection = db.collection::<Document>(&source_coll.collection);
    let mut cursor = source_collection
        .find(doc! { fk_mongo_name: { "$in": &oids } })
        .projection(doc! { "_id": 1 })
        .await?;

    Ok(collect_ids_from_cursor(&mut cursor).await?.into_iter().collect())
}

async fn resolve_reverse_filter(
    definition: &SchemaDefinition,
    db: &Database,
    target_coll_name: &str,
    fk_on_target: &str,
    ops: &RelationFilterOperators,
    depth: usize,
) -> Result<HashSet<ObjectId>, GraphQLError> {
    let target_def = definition
        .collection_by_name(target_coll_name)
        .ok_or_else(|| {
            GraphQLError::Internal(format!(
                "Target collection '{}' not found in schema definition",
                target_coll_name
            ))
        })?;
    let target_collection = db.collection::<Document>(target_coll_name);

    let mut include: Option<HashSet<ObjectId>> = None;
    let mut exclude = HashSet::new();
    let some_ids = if let Some(ref f) = ops.some {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, false, depth + 1).await?)
    } else { None };
    let every_ids = if let Some(ref f) = ops.every {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, false, depth + 1).await?)
    } else { None };
    let none_ids = if let Some(ref f) = ops.none {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, false, depth + 1).await?)
    } else { None };
    merge_operator_ids(&mut include, &mut exclude, some_ids, every_ids, none_ids);

    let effective = include.unwrap_or_default();
    if effective.is_empty() {
        return Ok(HashSet::new());
    }

    let oids: Vec<Bson> = effective.iter().map(|id| Bson::ObjectId(*id)).collect();
    let mut cursor = target_collection
        .find(doc! { "_id": { "$in": &oids } })
        .projection(doc! { fk_on_target: 1, "_id": 0 })
        .await?;

    let mut source_ids = HashSet::new();
    while let Some(result) = cursor.next().await {
        let doc = result?;
        if let Ok(oid) = doc.get_object_id(fk_on_target) {
            source_ids.insert(oid);
        }
    }
    Ok(source_ids)
}

async fn resolve_m2m_filter(
    definition: &SchemaDefinition,
    db: &Database,
    junction: &crate::schema::definition::JunctionDef,
    target_coll_name: &str,
    ops: &RelationFilterOperators,
    depth: usize,
) -> Result<HashSet<ObjectId>, GraphQLError> {
    let target_def = definition
        .collection_by_name(target_coll_name)
        .ok_or_else(|| {
            GraphQLError::Internal(format!(
                "Target collection '{}' not found in schema definition",
                target_coll_name
            ))
        })?;
    let target_collection = db.collection::<Document>(target_coll_name);
    let junction_collection = db.collection::<Document>(&junction.collection);

    let mut include: Option<HashSet<ObjectId>> = None;
    let mut exclude = HashSet::new();
    let some_ids = if let Some(ref f) = ops.some {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, false, depth + 1).await?)
    } else { None };
    let every_ids = if let Some(ref f) = ops.every {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, false, depth + 1).await?)
    } else { None };
    let none_ids = if let Some(ref f) = ops.none {
        Some(resolve_ids_for_operator(definition, db, &target_collection, f, target_def, false, depth + 1).await?)
    } else { None };
    merge_operator_ids(&mut include, &mut exclude, some_ids, every_ids, none_ids);

    let effective = include.unwrap_or_default();
    if effective.is_empty() {
        return Ok(HashSet::new());
    }

    let oids: Vec<Bson> = effective.iter().map(|id| Bson::ObjectId(*id)).collect();
    let mut cursor = junction_collection
        .find(doc! { &junction.foreign_field: { "$in": &oids } })
        .projection(doc! { &junction.local_field: 1, "_id": 0 })
        .await?;

    let mut source_ids = HashSet::new();
    while let Some(result) = cursor.next().await {
        let doc = result?;
        if let Ok(oid) = doc.get_object_id(&junction.local_field) {
            source_ids.insert(oid);
        }
    }
    Ok(source_ids)
}

/// Apply a filter document to a collection, returning the set of matching `_id`s.
/// When `resolve_nested` is true, also handle any relation-filter keys *within*
/// `filter` by recursing (e.g. `missions: { some: { villains: { some: {...} } } }`).
async fn resolve_ids_for_operator(
    definition: &SchemaDefinition,
    db: &Database,
    collection: &mongodb::Collection<Document>,
    filter: &Document,
    collection_def: &CollectionDef,
    resolve_nested: bool,
    depth: usize,
) -> Result<HashSet<ObjectId>, GraphQLError> {
    if depth > MAX_FILTER_DEPTH {
        return Err(GraphQLError::Internal(format!(
            "Maximum relation filter depth ({}) exceeded",
            MAX_FILTER_DEPTH
        )));
    }

    let mut scalar_filter = Document::new();
    let mut relation_keys: Vec<(String, Document)> = Vec::new();

    for (key, value) in filter {
        if resolve_nested && is_relation_field(&key, collection_def) {
            if let Bson::Document(rel_doc) = value {
                relation_keys.push((key.clone(), rel_doc.clone()));
            }
            continue;
        }
        scalar_filter.insert(key.clone(), value.clone());
    }

    if !relation_keys.is_empty() {
        let mut nested_id_set: Option<HashSet<ObjectId>> = None;

        for (rel_key, rel_doc) in &relation_keys {
            let ops = RelationFilterOperators::from_document(rel_doc);
            if ops.is_empty() {
                continue;
            }
            let rel_field = match relation_field_type(rel_key, collection_def) {
                Some(f) => f,
                None => continue,
            };

            let ids = Box::pin(resolve_single_relation(
                definition, db, collection_def, rel_key, rel_field, &ops, depth + 1,
            ))
            .await?;

            nested_id_set = Some(match nested_id_set {
                None => ids,
                Some(existing) => existing.intersection(&ids).cloned().collect(),
            });
        }

        if let Some(ref ids) = nested_id_set {
            let oids: Vec<Bson> = ids.iter().map(|id| Bson::ObjectId(*id)).collect();
            scalar_filter.insert("_id", doc! { "$in": &oids });
        }
    }

    let mongo_filter = if scalar_filter.is_empty() {
        doc! {}
    } else {
        transform_where_filter(scalar_filter.clone(), collection_def)?
    };

    let mut cursor = collection
        .find(mongo_filter)
        .projection(doc! { "_id": 1 })
        .await?;

    Ok(collect_ids_from_cursor(&mut cursor).await?.into_iter().collect())
}

async fn resolve_single_relation(
    definition: &SchemaDefinition,
    db: &Database,
    source_coll: &CollectionDef,
    graphql_field_name: &str,
    rel: &crate::schema::definition::RelationFieldDef,
    ops: &RelationFilterOperators,
    depth: usize,
) -> Result<HashSet<ObjectId>, GraphQLError> {
    match rel.kind {
        RelationKind::OneToMany | RelationKind::OneToOne => {
            let fk_name = source_coll
                .fields
                .iter()
                .find(|field| field.graphql_name() == graphql_field_name)
                .map(|field| field.name.as_str())
                .unwrap_or(graphql_field_name);

            resolve_forward_filter(
                definition, db, source_coll, fk_name, &rel.collection, ops, depth,
            )
            .await
        }
        RelationKind::ManyToMany => {
            let junction = rel.junction.as_ref().ok_or_else(|| {
                GraphQLError::Internal(
                    "ManyToMany relation filter requires a junction definition".into(),
                )
            })?;
            resolve_m2m_filter(definition, db, junction, &rel.collection, ops, depth).await
        }
    }
}

/// A relation from another collection pointing TO this one.
#[derive(Debug, Clone)]
pub struct ReverseRelation {
    /// GraphQL field name exposed on this collection (e.g. "members").
    pub graphql_name: String,
    /// The other collection that holds the FK.
    pub source_collection: String,
    /// The FK field on the other collection (e.g. "team_id").
    pub fk_field: String,
}

/// Collect reverse relations (OneToMany / OneToOne from other collections)
/// pointing TO `collection`.
pub fn collect_reverse_relations(
    collection: &CollectionDef,
    definition: &SchemaDefinition,
) -> (Vec<ReverseRelation>, Vec<ReverseRelation>) {
    let mut to_many = Vec::new();
    let mut to_one = Vec::new();

    for other in &definition.collections {
        if other.collection == collection.collection {
            continue;
        }
        for field in &other.fields {
            let rel = match &field.field_type {
                FieldType::Relation(rel) if rel.collection == collection.collection => rel,
                _ => continue,
            };
            match rel.kind {
                RelationKind::OneToMany => {
                    to_many.push(ReverseRelation {
                        graphql_name: rel.reverse_name.clone()
                            .unwrap_or_else(|| other.plural_name()),
                        source_collection: other.collection.clone(),
                        fk_field: field.name.clone(),
                    });
                }
                RelationKind::OneToOne => {
                    to_one.push(ReverseRelation {
                        graphql_name: rel.reverse_name.clone()
                            .unwrap_or_else(|| other.singular_name()),
                        source_collection: other.collection.clone(),
                        fk_field: field.name.clone(),
                    });
                }
                _ => {}
            }
        }
    }
    (to_many, to_one)
}

/// Result of resolving nested relation filters.
#[derive(Debug)]
pub struct ResolvedFilter {
    /// Source-collection `_id`s that match `some`/`every` relation filters.
    /// Intersected across all relation fields in the where document.
    pub include_ids: Option<Vec<ObjectId>>,
    /// Source-collection `_id`s to exclude (from `none` relation filters).
    pub exclude_ids: Vec<ObjectId>,
}

/// Resolve all relation-filter keys in `raw_filter` and return the sets of
/// source-collection `_id`s that should be included and excluded.
///
/// Returns `None` for `include_ids` when no positive relation filter is present.
pub async fn resolve_nested_filter(
    definition: &SchemaDefinition,
    db: &Database,
    collection_def: &CollectionDef,
    raw_filter: &Document,
) -> Result<ResolvedFilter, GraphQLError> {
    // Build reverse-relation lookup: GraphQL field name → (target_coll, fk_field)
    let (rev_to_many, rev_to_one) = collect_reverse_relations(collection_def, definition);
    let rev_lookup: HashMap<String, (&str, &str)> = rev_to_many
        .iter()
        .map(|r| (r.graphql_name.clone(), (r.source_collection.as_str(), r.fk_field.as_str())))
        .chain(rev_to_one.iter().map(|r| {
            (r.graphql_name.clone(), (r.source_collection.as_str(), r.fk_field.as_str()))
        }))
        .collect();

    let mut include_set: Option<HashSet<ObjectId>> = None;
    let mut exclude_set: HashSet<ObjectId> = HashSet::new();

    for (key, value) in raw_filter {
        let rel_doc = match value {
            Bson::Document(d) => d,
            _ => continue,
        };
        let mut ops = RelationFilterOperators::from_document(rel_doc);
        if ops.is_empty() {
            // Direct WhereInput — OneToMany from single side or OneToOne.
            ops.some = Some(rel_doc.clone());
        }

        if let Some(rel_field) = relation_field_type(&key, collection_def) {
            let with_some = |f: &Document| RelationFilterOperators {
                some: Some(f.clone()), ..Default::default()
            };
            let mut some_ids = None;
            let mut every_ids = None;
            let mut none_ids = None;

            if let Some(ref f) = ops.some {
                some_ids = Some(resolve_single_relation(definition, db, collection_def, &key, rel_field, &with_some(f), 0).await?);
            }
            if let Some(ref f) = ops.every {
                if let Some(negated) = try_negate_filter(f) {
                    every_ids = Some(resolve_single_relation(definition, db, collection_def, &key, rel_field, &with_some(&negated), 0).await?);
                } else {
                    // Can't negate — fall back to `some` semantics.
                    let ids = resolve_single_relation(definition, db, collection_def, &key, rel_field, &with_some(f), 0).await?;
                    some_ids = Some(match some_ids.take() {
                        None => ids,
                        Some(existing) => existing.intersection(&ids).cloned().collect(),
                    });
                }
            }
            if let Some(ref f) = ops.none {
                none_ids = Some(resolve_single_relation(definition, db, collection_def, &key, rel_field, &with_some(f), 0).await?);
            }
            merge_operator_ids(&mut include_set, &mut exclude_set, some_ids, every_ids, none_ids);
            continue;
        }

        if let Some((target_coll_name, fk_field)) = rev_lookup.get(key.as_str()) {
            let with_some = |f: &Document| RelationFilterOperators {
                some: Some(f.clone()), ..Default::default()
            };
            let mut some_ids = None;
            let mut every_ids = None;
            let mut none_ids = None;

            if let Some(ref f) = ops.some {
                some_ids = Some(resolve_reverse_filter(definition, db, target_coll_name, fk_field, &with_some(f), 0).await?);
            }
            if let Some(ref f) = ops.every {
                if let Some(negated) = try_negate_filter(f) {
                    every_ids = Some(resolve_reverse_filter(definition, db, target_coll_name, fk_field, &with_some(&negated), 0).await?);
                } else {
                    let ids = resolve_reverse_filter(definition, db, target_coll_name, fk_field, &with_some(f), 0).await?;
                    some_ids = Some(match some_ids.take() {
                        None => ids,
                        Some(existing) => existing.intersection(&ids).cloned().collect(),
                    });
                }
            }
            if let Some(ref f) = ops.none {
                none_ids = Some(resolve_reverse_filter(definition, db, target_coll_name, fk_field, &with_some(f), 0).await?);
            }
            merge_operator_ids(&mut include_set, &mut exclude_set, some_ids, every_ids, none_ids);
        }
    }

    Ok(ResolvedFilter {
        include_ids: include_set.map(|s| s.into_iter().collect()),
        exclude_ids: exclude_set.into_iter().collect(),
    })
}
