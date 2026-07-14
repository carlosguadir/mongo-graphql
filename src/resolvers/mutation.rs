use std::collections::{HashMap, HashSet};

use async_graphql::dynamic::ResolverContext;
use mongodb::bson::{doc, oid::ObjectId, Bson, Document};
use mongodb::{Client, Database};

use crate::error::GraphQLError;
use crate::helpers::serialization::{document_to_graphql_value, input_doc_to_mongo};
use crate::resolvers::query::transform_id_filter;
use crate::schema::definition::{CollectionDef, FieldType, JunctionDef, RelationKind, SchemaDefinition};

// ── Public resolvers ──

pub async fn resolve_create(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    client: &Client,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let definition = ctx.data::<SchemaDefinition>()?;
    let coll = db.collection::<Document>(&coll_def.collection);

    let mut input: Document = ctx
        .args
        .try_get("input")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let oid = ObjectId::new();

    with_transaction!(client, session, {
        let fk_values = extract_relation_fields(
            &mut input, coll_def, definition, db, &mut session, &HashMap::new(), oid, 1,
        )
        .await?;

        let mut doc = input_doc_to_mongo(input, coll_def);
        for (mongo_name, value) in fk_values {
            doc.insert(mongo_name, value);
        }
        doc.insert("_id", oid);
        doc.insert("id", oid);

        coll.insert_one(&doc).session(&mut session).await.map_err(|e| {
            if is_duplicate_key_error(&e) {
                GraphQLError::DuplicateKey {
                    message: format!("Duplicate key in collection '{}'", coll_def.collection),
                }
            } else {
                GraphQLError::from(e)
            }
        })?;

        Ok(Some(document_to_graphql_value(&doc, coll_def)))
    })
}

pub async fn resolve_update(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    client: &Client,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let definition = ctx.data::<SchemaDefinition>()?;
    let coll = db.collection::<Document>(&coll_def.collection);

    let where_input: Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;
    let mut update_input: Document = ctx
        .args
        .try_get("input")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let filter = transform_id_filter(where_input)?;

    with_transaction!(client, session, {
        let existing = coll
            .find_one(filter.clone())
            .session(&mut session)
            .await?
            .ok_or_else(|| GraphQLError::NotFound {
                message: format!(
                    "Document not found in '{}' for update",
                    coll_def.collection
                ),
            })?;

        let source_oid = existing
            .get_object_id("_id")
            .map_err(|_| GraphQLError::Internal("Existing document missing _id".into()))?;
        let current_fks = build_current_fk_map(&existing, coll_def);

        let fk_values = extract_relation_fields(
            &mut update_input, coll_def, definition, db, &mut session, &current_fks, source_oid, 1,
        )
        .await?;

        let mut update_doc = input_doc_to_mongo(update_input, coll_def);
        if update_doc.contains_key("_id") || update_doc.contains_key("id") {
            return Err(GraphQLError::Internal(
                "Updating the id field is not allowed".into(),
            ));
        }

        for (mongo_name, value) in fk_values {
            update_doc.insert(mongo_name, value);
        }

        coll.update_one(filter, doc! { "$set": &update_doc })
            .session(&mut session)
            .await
            .map_err(GraphQLError::from)?;

        let mut merged = existing;
        for (key, value) in &update_doc {
            merged.insert(key.clone(), value.clone());
        }
        Ok(Some(document_to_graphql_value(&merged, coll_def)))
    })
}

pub async fn resolve_delete(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    client: &Client,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll = db.collection::<Document>(&coll_def.collection);

    let where_input: Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let id = where_input
        .get_str("id")
        .map(|s| s.to_owned())
        .or_else(|_| {
            where_input
                .get_object_id("id")
                .map(|oid| oid.to_hex())
        })
        .map_err(|_| GraphQLError::Internal("where.id is required for delete".into()))?;

    let filter = transform_id_filter(where_input)?;

    with_transaction!(client, session, {
        let result = coll
            .delete_one(filter)
            .session(&mut session)
            .await
            .map_err(GraphQLError::from)?;

        if result.deleted_count == 0 {
            return Err(GraphQLError::NotFound {
                message: format!(
                    "Document not found in '{}' for delete",
                    coll_def.collection
                ),
            });
        }

        Ok(Some(serde_json::json!({
            "success": true,
            "deletedId": &id,
        })))
    })
}

// ── Nested input processing ──

fn check_depth(depth: u8) -> Result<(), GraphQLError> {
    if depth >= 3 {
        return Err(GraphQLError::Internal(
            "Maximum nested mutation depth (3) exceeded".into(),
        ));
    }
    Ok(())
}

fn validate_single_operation(nested: &Document) -> Result<(), GraphQLError> {
    let ops = ["connect", "create", "disconnect", "delete"];
    if ops.iter().filter(|op| nested.contains_key(*op)).count() > 1 {
        return Err(GraphQLError::Internal(
            "Only one of create, connect, disconnect, or delete can be specified per relation field"
                .into(),
        ));
    }
    Ok(())
}

/// Process a forward to-one nested input (CreateOneInput / UpdateOneInput).
/// Returns Some(ObjectId) for connect/create, None for disconnect/delete (sets FK to null).
async fn process_nested_one_input(
    nested: &Document,
    target_coll_def: &CollectionDef,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    current_fk: Option<&Bson>,
    depth: u8,
) -> Result<Option<Bson>, GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    if let Some(hex) = nested
        .get("connect")
        .and_then(|v| v.as_document())
        .and_then(|d| d.get_str("id").ok())
    {
        let oid = ObjectId::parse_str(hex)
            .map_err(|_| GraphQLError::Internal(format!("Invalid ObjectId in connect: {}", hex)))?;
        return Ok(Some(Bson::ObjectId(oid)));
    }

    if let Some(create_doc) = nested.get("create").and_then(|v| v.as_document()) {
        let oid = create_nested_document(
            create_doc, target_coll_def, definition, db, session, None, depth,
        )
        .await?;
        return Ok(Some(Bson::ObjectId(oid)));
    }

    if nested.get("disconnect") == Some(&Bson::Boolean(true)) {
        return Ok(None);
    }

    if nested.get("delete") == Some(&Bson::Boolean(true)) {
        if let Some(fk_oid) = current_fk.and_then(|v| v.as_object_id()) {
            db.collection::<Document>(&target_coll_def.collection)
                .delete_one(doc! { "_id": fk_oid })
                .session(session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(None);
    }

    Ok(None)
}

/// Scan a mutation input for relation fields and dispatch to the appropriate handler.
/// Returns (mongo_field_name, Bson_value) pairs to merge into the MongoDB document.
///
/// `current_fks` is populated for updates (maps GraphQL name → current FK value),
/// `source_oid` is the _id of the document being created/updated,
/// `depth` tracks nesting level (max 3).
async fn extract_relation_fields(
    input: &mut Document,
    coll_def: &CollectionDef,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    current_fks: &HashMap<String, Option<Bson>>,
    source_oid: ObjectId,
    depth: u8,
) -> Result<Vec<(String, Bson)>, GraphQLError> {
    let mut fk_values: Vec<(String, Bson)> = Vec::new();
    let relation_field_keys: HashSet<String> = coll_def
        .fields
        .iter()
        .filter(|f| matches!(f.field_type, FieldType::Relation(_)))
        .map(|f| f.graphql_name())
        .collect();

    // ── Forward relation fields ──
    for gql_name in &relation_field_keys {
        let field_def = coll_def
            .fields
            .iter()
            .find(|f| f.graphql_name() == *gql_name)
            .unwrap();

        let rel = match &field_def.field_type {
            FieldType::Relation(r) => r,
            _ => continue,
        };

        let target_def = definition.collection_by_name(&rel.collection).ok_or_else(|| {
            GraphQLError::Internal(format!(
                "Target collection '{}' not found in schema definition",
                rel.collection
            ))
        })?;

        let value = match input.remove(field_def.graphql_name().as_str()) {
            Some(Bson::Document(nested)) => nested,
            Some(_) => {
                return Err(GraphQLError::Internal(format!(
                    "Expected a nested input object for relation field '{}'",
                    gql_name
                )));
            }
            None => continue,
        };

        match rel.kind {
            RelationKind::OneToMany | RelationKind::OneToOne => {
                let current_fk = current_fks.get(gql_name).and_then(|v| v.as_ref());
                let processed = process_nested_one_input(
                    &value, target_def, definition, db, session, current_fk, depth,
                )
                .await?;
                let bson = processed.unwrap_or(Bson::Null);
                fk_values.push((field_def.name.clone(), bson));
            }
            RelationKind::ManyToMany => {
                let junction = rel.junction.as_ref().ok_or_else(|| {
                    GraphQLError::Internal(
                        "ManyToMany relation missing junction definition".into(),
                    )
                })?;
                process_nested_many_to_many_input(
                    &value, junction, target_def, source_oid, definition, db, session, depth,
                )
                .await?;
            }
        }
    }

    // ── Reverse fields (auto-generated by builder) ──
    let remaining_keys: Vec<String> = input.keys().cloned().collect();
    for key in remaining_keys {
        if relation_field_keys.contains(&key) {
            continue;
        }

        let (target_coll, fk_field, kind) = match find_reverse_field(definition, coll_def, &key) {
            Some(info) => info,
            None => continue,
        };

        let value = match input.remove(&key) {
            Some(Bson::Document(nested)) => nested,
            Some(_) => {
                return Err(GraphQLError::Internal(format!(
                    "Expected a nested input object for reverse field '{}'",
                    key
                )));
            }
            None => continue,
        };

        match kind {
            RelationKind::OneToMany => {
                process_nested_many_input(
                    &value, target_coll, &fk_field, source_oid, definition, db, session, depth,
                )
                .await?;
            }
            RelationKind::OneToOne => {
                process_reverse_one_to_one_input(
                    &value, target_coll, fk_field.as_str(), source_oid, definition, db, session,
                    depth,
                )
                .await?;
            }
            _ => unreachable!(),
        }
    }

    Ok(fk_values)
}

/// Find a OneToMany or OneToOne relation in `definition` where the reverse field
/// matches `key` on `coll_def`. Returns (target_collection, fk_field_name, kind).
fn find_reverse_field<'a>(
    definition: &'a SchemaDefinition,
    coll_def: &CollectionDef,
    key: &str,
) -> Option<(&'a CollectionDef, String, RelationKind)> {
    for other in &definition.collections {
        if other.collection == coll_def.collection {
            continue;
        }
        for field in &other.fields {
            if let FieldType::Relation(rel) = &field.field_type {
                if rel.collection != coll_def.collection {
                    continue;
                }
                let match_result: Option<RelationKind> = match rel.kind {
                    RelationKind::OneToMany => {
                        let name = rel
                            .reverse_name
                            .clone()
                            .unwrap_or_else(|| other.plural_name());
                        if name == key { Some(RelationKind::OneToMany) } else { None }
                    }
                    RelationKind::OneToOne => {
                        let name = rel
                            .reverse_name
                            .clone()
                            .unwrap_or_else(|| other.singular_name());
                        if name == key { Some(RelationKind::OneToOne) } else { None }
                    }
                    _ => None,
                };
                if let Some(kind) = match_result {
                    return Some((other, field.name.clone(), kind));
                }
            }
        }
    }
    None
}

async fn process_nested_many_input(
    nested: &Document,
    target_coll_def: &CollectionDef,
    fk_field: &str,
    source_oid: ObjectId,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    depth: u8,
) -> Result<(), GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    let target_coll = db.collection::<Document>(&target_coll_def.collection);

    if let Some(ids) = nested.get("connect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids, IdFormat::Object)? {
            target_coll
                .update_one(
                    doc! { "_id": oid },
                    doc! { "$set": { fk_field: source_oid } },
                )
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(elems) = nested.get("create").and_then(|v| v.as_array()) {
        for create_val in elems {
            let create_doc = create_val.as_document().ok_or_else(|| {
                GraphQLError::Internal("create entries must be objects".into())
            })?;
            create_nested_document(
                create_doc, target_coll_def, definition, db, session,
                Some((fk_field, source_oid)), depth,
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("disconnect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids, IdFormat::Scalar)? {
            target_coll
                .update_one(
                    doc! { "_id": oid, fk_field: source_oid },
                    doc! { "$set": { fk_field: Bson::Null } },
                )
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("delete").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids, IdFormat::Scalar)? {
            target_coll
                .delete_one(doc! { "_id": oid, fk_field: source_oid })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    Ok(())
}

async fn process_nested_many_to_many_input(
    nested: &Document,
    junction: &JunctionDef,
    target_coll_def: &CollectionDef,
    source_oid: ObjectId,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    depth: u8,
) -> Result<(), GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    let junction_coll = db.collection::<Document>(&junction.collection);

    if let Some(ids) = nested.get("connect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids, IdFormat::Object)? {
            junction_coll
                .insert_one(&doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(elems) = nested.get("create").and_then(|v| v.as_array()) {
        for create_val in elems {
            let create_doc = create_val.as_document().ok_or_else(|| {
                GraphQLError::Internal("create entries must be objects".into())
            })?;
            let oid = create_nested_document(
                create_doc, target_coll_def, definition, db, session, None, depth,
            )
            .await?;

            junction_coll
                .insert_one(&doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("disconnect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids, IdFormat::Scalar)? {
            junction_coll
                .delete_one(doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("delete").and_then(|v| v.as_array()) {
        let target_coll = db.collection::<Document>(&target_coll_def.collection);
        for oid in &parse_id_array(ids, IdFormat::Scalar)? {
            junction_coll
                .delete_one(doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
            target_coll
                .delete_one(doc! { "_id": oid })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    Ok(())
}

async fn process_reverse_one_to_one_input(
    nested: &Document,
    target_coll_def: &CollectionDef,
    fk_field: &str,
    source_oid: ObjectId,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    depth: u8,
) -> Result<(), GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    let target_coll = db.collection::<Document>(&target_coll_def.collection);

    if let Some(hex) = nested
        .get("connect")
        .and_then(|v| v.as_document())
        .and_then(|d| d.get_str("id").ok())
    {
        let target_oid = ObjectId::parse_str(hex)
            .map_err(|_| GraphQLError::Internal("Invalid connect id".into()))?;
        target_coll
            .update_one(
                doc! { "_id": target_oid },
                doc! { "$set": { fk_field: source_oid } },
            )
            .session(&mut *session)
            .await
            .map_err(|e| {
                GraphQLError::Internal(format!("Reverse OneToOne connect failed: {}", e))
            })?;
    }

    if let Some(create_doc) = nested.get("create").and_then(|v| v.as_document()) {
        create_nested_document(
            create_doc, target_coll_def, definition, db, session,
            Some((fk_field, source_oid)), depth,
        )
        .await?;
    }

    if nested.get("disconnect") == Some(&Bson::Boolean(true)) {
        target_coll
            .update_many(
                doc! { fk_field: source_oid },
                doc! { "$unset": { fk_field: "" } },
            )
            .session(&mut *session)
            .await
            .map_err(|e| {
                GraphQLError::Internal(format!("Reverse OneToOne disconnect failed: {}", e))
            })?;
    }

    if nested.get("delete") == Some(&Bson::Boolean(true)) {
        target_coll
            .delete_many(doc! { fk_field: source_oid })
            .session(&mut *session)
            .await
            .map_err(|e| {
                GraphQLError::Internal(format!("Reverse OneToOne delete failed: {}", e))
            })?;
    }

    Ok(())
}

// ── Helpers ──

async fn create_nested_document(
    create_doc: &Document,
    target_coll_def: &CollectionDef,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    fk_field: Option<(&str, ObjectId)>,
    depth: u8,
) -> Result<ObjectId, GraphQLError> {
    let mut target_doc = input_doc_to_mongo(create_doc.clone(), target_coll_def);
    let oid = ObjectId::new();
    target_doc.insert("_id", oid);
    target_doc.insert("id", oid);

    if let Some((field_name, value)) = fk_field {
        target_doc.insert(field_name, value);
    }

    let next_depth = depth + 1;
    if next_depth < 3 {
        let mut create_doc_mut = create_doc.clone();
        let nested_fk_values = Box::pin(extract_relation_fields(
            &mut create_doc_mut,
            target_coll_def,
            definition,
            db,
            session,
            &HashMap::new(),
            oid,
            next_depth,
        ))
        .await?;
        for (mongo_name, value) in nested_fk_values {
            target_doc.insert(mongo_name, value);
        }
    }

    let target_coll = db.collection::<Document>(&target_coll_def.collection);
    target_coll
        .insert_one(&target_doc)
        .session(&mut *session)
        .await
        .map_err(|e| {
            if is_duplicate_key_error(&e) {
                GraphQLError::DuplicateKey {
                    message: format!(
                        "Duplicate key in nested create for collection '{}'",
                        target_coll_def.collection
                    ),
                }
            } else {
                GraphQLError::from(e)
            }
        })?;

    Ok(oid)
}

fn build_current_fk_map(
    existing: &Document,
    coll_def: &CollectionDef,
) -> HashMap<String, Option<Bson>> {
    coll_def
        .fields
        .iter()
        .filter(|f| matches!(f.field_type, FieldType::Relation(_)))
        .map(|f| (f.graphql_name(), existing.get(&f.name).cloned()))
        .collect()
}

enum IdFormat {
    Object,
    Scalar,
}

fn parse_id_array(
    arr: &mongodb::bson::Array,
    format: IdFormat,
) -> Result<Vec<ObjectId>, GraphQLError> {
    arr.iter()
        .map(|entry| match format {
            IdFormat::Object => {
                let doc = entry.as_document().ok_or_else(|| {
                    GraphQLError::Internal(
                        "Each connect entry must be an object with an id field".into(),
                    )
                })?;
                let hex = doc.get_str("id").map_err(|_| {
                    GraphQLError::Internal("Each connect entry must have an 'id' field".into())
                })?;
                ObjectId::parse_str(hex)
                    .map_err(|_| GraphQLError::Internal("Invalid ObjectId in connect".into()))
            }
            IdFormat::Scalar => {
                let hex = entry.as_str().ok_or_else(|| {
                    GraphQLError::Internal(
                        "Each disconnect/delete entry must be an ID string".into(),
                    )
                })?;
                ObjectId::parse_str(hex)
                    .map_err(|_| GraphQLError::Internal("Invalid ObjectId in array".into()))
            }
        })
        .collect()
}

// ── Transaction & error helpers ──

macro_rules! with_transaction {
    ($client:expr, $session:ident, $body:block) => {{
        let mut $session = $client.start_session().await?;
        $session.start_transaction().await.map_err(|e| {
            GraphQLError::Internal(format!("Failed to start transaction: {}", e))
        })?;
        let __result: Result<_, GraphQLError> = (|| async { $body })().await;
        match &__result {
            Ok(_) => {
                $session.commit_transaction().await.map_err(|e| {
                    GraphQLError::Internal(format!("Failed to commit transaction: {}", e))
                })?;
            }
            Err(_) => {
                let _ = $session.abort_transaction().await;
            }
        }
        __result
    }};
}
use with_transaction;

fn is_duplicate_key_error(error: &mongodb::error::Error) -> bool {
    match &*error.kind {
        mongodb::error::ErrorKind::Write(write_failure) => match write_failure {
            mongodb::error::WriteFailure::WriteError(write_error) => {
                write_error.code == 11000 || write_error.code == 11001
            }
            mongodb::error::WriteFailure::WriteConcernError(write_concern_error) => {
                write_concern_error.code == 11000 || write_concern_error.code == 11001
            }
            _ => false,
        },
        mongodb::error::ErrorKind::BulkWrite(bulk_failure) => {
            bulk_failure
                .write_errors
                .iter()
                .any(|(_, write_error)| write_error.code == 11000 || write_error.code == 11001)
        }
        _ => false,
    }
}
