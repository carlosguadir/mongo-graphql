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
    coll_def: &CollectionDef,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll = db.collection::<mongodb::bson::Document>(&coll_def.collection);

    let where_input: mongodb::bson::Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let filter = transform_id_filter(where_input);

    let document = coll
        .find_one(filter)
        .await
        ?;

    match document {
        Some(d) => Ok(Some(document_to_graphql_value(&d, coll_def))),
        None => Ok(None),
    }
}

pub async fn resolve_list(
    ctx: ResolverContext<'_>,
    coll_def: &CollectionDef,
    db: &Database,
    max_page_size: usize,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let coll = db.collection::<mongodb::bson::Document>(&coll_def.collection);

    let first: Option<i64> = try_deserialize_optional(&ctx.args, "first")?;
    let after: Option<String> = try_deserialize_optional(&ctx.args, "after")?;

    let pagination = PaginationArgs { first, after };
    let limit = pagination.effective_limit(max_page_size);

    let mut filter: mongodb::bson::Document =
        try_deserialize_optional(&ctx.args, "where")?.unwrap_or_default();

    if let Some(after) = &pagination.after {
        let cursor_id = decode_cursor(after)?;
        filter = doc! { "$and": [filter, doc! { "_id": { "$gt": cursor_id } }] };
    }

    let sort: mongodb::bson::Document =
        try_deserialize_optional(&ctx.args, "sort")?.unwrap_or_else(|| doc! { "_id": 1 });

    let mut cursor = coll
        .find(filter)
        .sort(sort)
        .limit(limit + 1)
        .await
        ?;

    let mut docs: Vec<mongodb::bson::Document> = Vec::new();
    while let Some(result) = cursor.next().await {
        let document = result?;
        docs.push(document);
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
        .and_then(|document| document.get_object_id("_id").ok())
        .map(|id| encode_cursor(&id));

    let end_cursor = docs
        .last()
        .and_then(|document| document.get_object_id("_id").ok())
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

/// Deserialize an optional argument, returning `None` if absent and
/// an error if present but malformed.
pub(crate) fn try_deserialize_optional<T: serde::de::DeserializeOwned>(
    args: &async_graphql::dynamic::ObjectAccessor<'_>,
    name: &str,
) -> Result<Option<T>, GraphQLError> {
    match args.get(name) {
        Some(value) => value
            .deserialize()
            .map(Some)
            .map_err(|e| GraphQLError::Internal(e.message)),
        None => Ok(None),
    }
}

/// Transform a where input that may use `id` (hex string) to `_id` (ObjectId).
/// MongoDB stores the primary key as `_id`, but GraphQL exposes it as `id`.
pub(crate) fn transform_id_filter(mut filter: mongodb::bson::Document) -> mongodb::bson::Document {
    if let Some(mongodb::bson::Bson::String(hex)) = filter.remove("id") {
        if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(&hex) {
            filter.insert("_id", oid);
        }
    }
    filter
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId, Bson};

    #[test]
    fn test_transform_id_filter_converts_hex_to_object_id() {
        let oid = ObjectId::new();
        let filter = doc! { "id": oid.to_hex() };
        let result = transform_id_filter(filter);
        assert_eq!(result.get("_id"), Some(&Bson::ObjectId(oid)));
        assert!(!result.contains_key("id"));
    }

    #[test]
    fn test_transform_id_filter_preserves_other_fields() {
        let oid = ObjectId::new();
        let filter = doc! { "id": oid.to_hex(), "alias": "TestHero" };
        let result = transform_id_filter(filter);
        assert_eq!(result.get("_id"), Some(&Bson::ObjectId(oid)));
        assert_eq!(
            result.get("alias"),
            Some(&Bson::String("TestHero".into()))
        );
    }

    #[test]
    fn test_transform_id_filter_invalid_hex_ignored() {
        let filter = doc! { "id": "not-a-valid-hex" };
        let result = transform_id_filter(filter);
        assert!(!result.contains_key("_id"));
        assert!(!result.contains_key("id"));
    }

    #[test]
    fn test_transform_id_filter_no_id_field_unchanged() {
        let filter = doc! { "alias": "TestHero", "active": true };
        let result = transform_id_filter(filter.clone());
        assert_eq!(result, filter);
    }

    #[test]
    fn test_transform_id_filter_empty_document() {
        let filter = doc! {};
        let result = transform_id_filter(filter.clone());
        assert_eq!(result, filter);
    }
}
