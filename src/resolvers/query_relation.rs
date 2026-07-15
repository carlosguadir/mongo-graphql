use async_graphql::dynamic::ResolverContext;
use async_graphql::Value;
use futures::StreamExt;
use mongodb::bson::{doc, oid::ObjectId, Document};
use mongodb::{Cursor, Database};

use crate::error::GraphQLError;
use crate::helpers::serialization::document_to_graphql_value;
use crate::schema::definition::{CollectionDef, FieldType, JunctionDef, RelationKind};

fn extract_parent_oid(ctx: &ResolverContext<'_>) -> Option<ObjectId> {
    let hex = ctx
        .parent_value
        .as_value()
        .and_then(|parent| match parent {
            Value::Object(map) => match map.get("id")? {
                Value::String(hex_str) => Some(hex_str.clone()),
                _ => None,
            },
            _ => None,
        })?;
    ObjectId::parse_str(&hex).ok()
}

fn extract_fk_oid(ctx: &ResolverContext<'_>, fk_mongo_name: &str) -> Option<ObjectId> {
    let hex = ctx
        .parent_value
        .as_value()
        .and_then(|parent| match parent {
            Value::Object(map) => match map.get(fk_mongo_name)? {
                Value::String(hex_str) => Some(hex_str.clone()),
                _ => None,
            },
            _ => None,
        })?;
    ObjectId::parse_str(&hex).ok()
}

/// Build a MongoDB projection from the GraphQL selection set.
/// Always includes `_id` and `id`. For forward relation fields in the
/// selection, includes the FK field so deeper resolvers can access it.
fn build_selection_projection(
    ctx: &ResolverContext<'_>,
    collection_def: &CollectionDef,
) -> Document {
    let mut projection = doc! { "_id": 1, "id": 1 };
    for selection in ctx.field().selection_set() {
        let gql_name = selection.name();
        let field_def = match collection_def
            .fields
            .iter()
            .find(|field| field.graphql_name() == gql_name)
        {
            Some(field) => field,
            None => continue,
        };
        match &field_def.field_type {
            FieldType::Relation(rel) => match rel.kind {
                RelationKind::OneToMany | RelationKind::OneToOne => {
                    projection.insert(field_def.name.as_str(), 1);
                }
                RelationKind::ManyToMany => {}
            },
            _ => {
                projection.insert(field_def.name.as_str(), 1);
            }
        }
    }
    projection
}

async fn collect_cursor_docs(
    mut cursor: Cursor<Document>,
    target_def: &CollectionDef,
) -> Result<Vec<serde_json::Value>, GraphQLError> {
    let mut results = Vec::new();
    while let Some(doc) = cursor.next().await {
        results.push(document_to_graphql_value(&doc?, target_def));
    }
    Ok(results)
}

/// Resolve a forward to-one relation (OneToMany or OneToOne where the FK
/// is on the current document). Extracts the FK value from the parent
/// document and queries the target collection.
pub async fn resolve_forward_to_one(
    ctx: &ResolverContext<'_>,
    db: &Database,
    target_collection_name: &str,
    target_def: &CollectionDef,
    fk_mongo_name: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let fk_oid = match extract_fk_oid(ctx, fk_mongo_name) {
        Some(oid) => oid,
        None => return Ok(None),
    };

    let projection = build_selection_projection(ctx, target_def);
    let target_collection = db.collection::<Document>(target_collection_name);

    let result = target_collection
        .find_one(doc! { "_id": fk_oid })
        .projection(projection)
        .await?;

    match result {
        Some(doc) => Ok(Some(document_to_graphql_value(&doc, target_def))),
        None => Ok(None),
    }
}

/// Resolve a forward ManyToMany relation through a junction collection.
/// Extracts the parent `_id`, queries the junction for foreign keys,
/// then queries the target collection for the related documents.
pub async fn resolve_forward_many_to_many(
    ctx: &ResolverContext<'_>,
    db: &Database,
    junction: &JunctionDef,
    target_def: &CollectionDef,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let source_oid = match extract_parent_oid(ctx) {
        Some(oid) => oid,
        None => return Ok(None),
    };

    let junction_collection = db.collection::<Document>(&junction.collection);
    let mut junction_cursor = junction_collection
        .find(doc! { &junction.local_field: source_oid })
        .await?;

    let mut foreign_ids: Vec<ObjectId> = Vec::new();
    while let Some(entry) = junction_cursor.next().await {
        let entry = entry?;
        if let Ok(foreign_oid) = entry.get_object_id(&junction.foreign_field) {
            foreign_ids.push(foreign_oid);
        }
    }

    if foreign_ids.is_empty() {
        return Ok(Some(serde_json::Value::Array(vec![])));
    }

    let projection = build_selection_projection(ctx, target_def);
    let target_collection = db.collection::<Document>(&target_def.collection);
    let target_cursor = target_collection
        .find(doc! { "_id": { "$in": &foreign_ids } })
        .projection(projection)
        .await?;

    let results = collect_cursor_docs(target_cursor, target_def).await?;
    Ok(Some(serde_json::Value::Array(results)))
}

/// Resolve a reverse to-many relation (OneToMany where the FK is on the
/// target collection). Queries the target collection for documents where
/// the FK field matches the parent `_id`.
pub async fn resolve_reverse_to_many(
    ctx: &ResolverContext<'_>,
    db: &Database,
    target_def: &CollectionDef,
    fk_field: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let source_oid = match extract_parent_oid(ctx) {
        Some(oid) => oid,
        None => return Ok(None),
    };

    let projection = build_selection_projection(ctx, target_def);
    let target_collection = db.collection::<Document>(&target_def.collection);
    let cursor = target_collection
        .find(doc! { fk_field: source_oid })
        .projection(projection)
        .await?;

    let results = collect_cursor_docs(cursor, target_def).await?;
    Ok(Some(serde_json::Value::Array(results)))
}

/// Resolve a reverse to-one relation (OneToOne where the FK is on the
/// target collection). Queries the target collection for a single document
/// where the FK field matches the parent `_id`.
pub async fn resolve_reverse_to_one(
    ctx: &ResolverContext<'_>,
    db: &Database,
    target_def: &CollectionDef,
    fk_field: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let source_oid = match extract_parent_oid(ctx) {
        Some(oid) => oid,
        None => return Ok(None),
    };

    let projection = build_selection_projection(ctx, target_def);
    let target_collection = db.collection::<Document>(&target_def.collection);

    let result = target_collection
        .find_one(doc! { fk_field: source_oid })
        .projection(projection)
        .await?;

    match result {
        Some(doc) => Ok(Some(document_to_graphql_value(&doc, target_def))),
        None => Ok(None),
    }
}
