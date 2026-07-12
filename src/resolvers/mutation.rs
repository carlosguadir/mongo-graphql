use async_graphql::dynamic::ResolverContext;
use mongodb::bson::{doc, oid::ObjectId, Document};
use mongodb::Database;

use crate::error::GraphQLError;
use crate::helpers::serialization::document_to_graphql_value;
use crate::schema::definition::CollectionDef;

pub async fn resolve_create(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll = db.collection::<Document>(&coll_def.collection);

    let input: Document = ctx
        .args
        .try_get("input")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let mut doc = input;
    let oid = ObjectId::new();
    doc.insert("_id", oid);
    doc.insert("id", oid);

    coll.insert_one(&doc).await?;

    let created = coll
        .find_one(doc! { "_id": oid })
        .await
        ?
        .ok_or_else(|| GraphQLError::NotFound {
            message: format!(
                "Document not found in '{}' after insert",
                coll_def.collection
            ),
        })?;

    Ok(Some(document_to_graphql_value(&created, coll_def)))
}

pub async fn resolve_update(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
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

    update_input.remove("_id");
    update_input.remove("id");

    coll.update_one(where_input, doc! { "$set": &update_input })
        .await
        ?;

    let id = ctx
        .args
        .try_get("where")?
        .deserialize::<Document>()
        .map_err(|e| GraphQLError::Internal(e.message))?
        .get("id")
        .cloned();

    let updated = coll
        .find_one(doc! { "id": id })
        .await
        ?
        .ok_or_else(|| GraphQLError::NotFound {
            message: format!(
                "Document not found in '{}' after update",
                coll_def.collection
            ),
        })?;

    Ok(Some(document_to_graphql_value(&updated, coll_def)))
}

pub async fn resolve_delete(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll = db.collection::<Document>(&coll_def.collection);

    let where_input: Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let id = where_input
        .get("id")
        .cloned()
        .ok_or_else(|| GraphQLError::Internal("where.id is required for delete".into()))?;

    let result = coll.delete_one(where_input).await?;

    Ok(Some(serde_json::json!({
        "success": result.deleted_count > 0,
        "deletedId": id,
    })))
}
