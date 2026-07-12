use async_graphql::dynamic::ResolverContext;
use futures::StreamExt;
use mongodb::bson::doc;
use mongodb::Database;

use crate::error::GraphQLError;
use crate::helpers::serialization::document_to_graphql_value;
use crate::resolvers::pagination::{decode_cursor, encode_cursor, PaginationArgs};
use crate::schema::definition::CollectionDef;

pub async fn resolve_get(
    ctx: ResolverContext<'_>,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll_def = ctx.data::<CollectionDef>()?;
    let db = ctx.data::<Database>()?;
    let coll = db.collection::<mongodb::bson::Document>(&coll_def.collection);

    let where_input: mongodb::bson::Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let doc = coll
        .find_one(where_input)
        .await
        .map_err(|e| GraphQLError::Internal(format!("MongoDB error: {}", e)))?;

    match doc {
        Some(d) => Ok(Some(document_to_graphql_value(&d, coll_def))),
        None => Ok(None),
    }
}

pub async fn resolve_list(
    ctx: ResolverContext<'_>,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll_def = ctx.data::<CollectionDef>()?;
    let db = ctx.data::<Database>()?;
    let max_page_size: usize = ctx
        .data::<crate::schema::builder::RuntimeConfig>()
        .map(|c| c.max_page_size)
        .unwrap_or(100);
    let coll = db.collection::<mongodb::bson::Document>(&coll_def.collection);

    let first: Option<i64> = ctx
        .args
        .get("first")
        .and_then(|v| v.deserialize().ok());
    let after: Option<String> = ctx
        .args
        .get("after")
        .and_then(|v| v.deserialize().ok());

    let pagination = PaginationArgs { first, after };
    let limit = pagination.effective_limit(max_page_size);

    let mut filter = ctx
        .args
        .get("where")
        .and_then(|v| v.deserialize::<mongodb::bson::Document>().ok())
        .unwrap_or_default();

    if let Some(after) = &pagination.after {
        let cursor_id = decode_cursor(after)?;
        filter = doc! { "$and": [filter, doc! { "_id": { "$gt": cursor_id } }] };
    }

    if let Ok(row_filter) = ctx.data::<mongodb::bson::Document>() {
        if !row_filter.is_empty() {
            filter = doc! { "$and": [filter, row_filter] };
        }
    }

    let sort: mongodb::bson::Document = ctx
        .args
        .get("sort")
        .and_then(|v| v.deserialize().ok())
        .unwrap_or_else(|| doc! { "_id": 1 });

    let mut cursor = coll
        .find(filter)
        .sort(sort)
        .limit(limit + 1)
        .await
        .map_err(|e| GraphQLError::Internal(format!("MongoDB error: {}", e)))?;

    let mut docs: Vec<mongodb::bson::Document> = Vec::new();
    while let Some(result) = cursor.next().await {
        let doc = result.map_err(|e| GraphQLError::Internal(format!("MongoDB error: {}", e)))?;
        docs.push(doc);
    }

    let has_next = docs.len() > limit as usize;
    if has_next {
        docs.pop();
    }

    let edges: Vec<serde_json::Value> = docs
        .iter()
        .map(|doc| document_to_graphql_value(doc, coll_def))
        .collect();

    let start_cursor = docs
        .first()
        .and_then(|d| d.get_object_id("_id").ok())
        .map(|id| encode_cursor(&id));

    let end_cursor = docs
        .last()
        .and_then(|d| d.get_object_id("_id").ok())
        .map(|id| encode_cursor(&id));

    Ok(Some(serde_json::json!({
        "edges": edges,
        "pageInfo": {
            "hasNextPage": has_next,
            "hasPreviousPage": pagination.after.is_some(),
            "startCursor": start_cursor,
            "endCursor": end_cursor,
        },
        "totalCount": serde_json::Value::Null
    })))
}
