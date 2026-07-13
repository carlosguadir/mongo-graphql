use async_graphql::dynamic::ResolverContext;
use mongodb::bson::{doc, oid::ObjectId, Document};
use mongodb::Database;

use crate::error::GraphQLError;
use crate::helpers::serialization::{document_to_graphql_value, input_doc_to_mongo};
use crate::resolvers::query::transform_id_filter;
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

    let mut doc = input_doc_to_mongo(input, coll_def);
    let oid = ObjectId::new();
    doc.insert("_id", oid);
    doc.insert("id", oid);

    coll.insert_one(&doc).await.map_err(|e| {
        if is_duplicate_key_error(&e) {
            GraphQLError::DuplicateKey {
                message: format!(
                    "Duplicate key in collection '{}'",
                    coll_def.collection
                ),
            }
        } else {
            GraphQLError::from(e)
        }
    })?;

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
    let update_input: Document = ctx
        .args
        .try_get("input")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let mut update_input = input_doc_to_mongo(update_input, coll_def);
    update_input.remove("_id");
    update_input.remove("id");

    let filter = transform_id_filter(where_input)?;

    let update_result = coll
        .update_one(filter.clone(), doc! { "$set": &update_input })
        .await?;

    if update_result.matched_count == 0 {
        return Err(GraphQLError::NotFound {
            message: format!(
                "Document not found in '{}' for update",
                coll_def.collection
            ),
        });
    }

    let updated = coll
        .find_one(filter)
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
        .get_str("id")
        .map(|s| s.to_owned())
        .or_else(|_| {
            where_input
                .get_object_id("id")
                .map(|oid| oid.to_hex())
        })
        .map_err(|_| GraphQLError::Internal("where.id is required for delete".into()))?;

    let filter = transform_id_filter(where_input)?;
    let result = coll.delete_one(filter).await?;

    Ok(Some(serde_json::json!({
        "success": result.deleted_count > 0,
        "deletedId": id,
    })))
}

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
        _ => false,
    }
}
