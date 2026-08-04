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
    collection_def: &CollectionDef,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let collection = db.collection::<mongodb::bson::Document>(&collection_def.collection);

    let where_input: mongodb::bson::Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|err| GraphQLError::Internal(err.message))?;

    let filter = transform_id_filter(where_input)?;

    let document = collection
        .find_one(filter)
        .await
        ?;

    match document {
        Some(d) => Ok(Some(document_to_graphql_value(&d, collection_def))),
        None => Ok(None),
    }
}

pub async fn resolve_list(
    ctx: ResolverContext<'_>,
    collection_def: &CollectionDef,
    db: &Database,
    max_page_size: usize,
) -> Result<serde_json::Value, GraphQLError> {
    let collection = db.collection::<mongodb::bson::Document>(&collection_def.collection);

    let first: Option<i64> = try_deserialize_optional(&ctx.args, "first")?;
    let after: Option<String> = try_deserialize_optional(&ctx.args, "after")?;
    let last: Option<i64> = try_deserialize_optional(&ctx.args, "last")?;
    let before: Option<String> = try_deserialize_optional(&ctx.args, "before")?;

    let pagination = PaginationArgs { first, after, last, before };
    let limit = pagination.effective_limit(max_page_size)?;
    let is_backward = last.is_some();

    let raw_filter: mongodb::bson::Document =
        try_deserialize_optional(&ctx.args, "where")?.unwrap_or_default();

    // Resolve nested relation filters (e.g. missions: { some: { code: { eq: "OP-1" } } })
    // into _id: { $in / $nin } clauses merged into the scalar filter.
    let definition = ctx
        .data_opt::<crate::schema::definition::SchemaDefinition>()
        .ok_or_else(|| GraphQLError::Internal("SchemaDefinition not found in context".into()))?;
    let nested = crate::resolvers::filter_relation::resolve_nested_filter(
        definition, db, collection_def, &raw_filter,
    )
    .await?;
    // Strip relation-filter keys (resolved above) so only scalar fields remain.
    let (rev_to_many, rev_to_one) =
        crate::resolvers::filter_relation::collect_reverse_relations(collection_def, definition);
    let is_reverse_key = |key: &str| -> bool {
        rev_to_many.iter().any(|r| r.graphql_name == key)
            || rev_to_one.iter().any(|r| r.graphql_name == key)
    };
    let mut scalar_filter = mongodb::bson::Document::new();
    for (key, value) in &raw_filter {
        if crate::resolvers::filter_relation::is_relation_filter_key(key, collection_def)
            || is_reverse_key(key)
        {
            continue;
        }
        scalar_filter.insert(key.clone(), value.clone());
    }
    if let Some(ref ids) = nested.include_ids {
        scalar_filter.insert("id", doc! { "$in": object_ids_to_bson(ids) });
    }
    if !nested.exclude_ids.is_empty() {
        scalar_filter.insert("id", doc! { "$nin": object_ids_to_bson(&nested.exclude_ids) });
    }

    let base_filter = transform_where_filter(scalar_filter, collection_def)?;

    let filter = if is_backward {
        if let Some(before) = &pagination.before {
            let cursor_id = decode_cursor(before)?;
            doc! { "$and": [base_filter.clone(), doc! { "_id": { "$lt": cursor_id } }] }
        } else {
            base_filter.clone()
        }
    } else {
        if let Some(after) = &pagination.after {
            let cursor_id = decode_cursor(after)?;
            doc! { "$and": [base_filter.clone(), doc! { "_id": { "$gt": cursor_id } }] }
        } else {
            base_filter.clone()
        }
    };

    let sort_raw: mongodb::bson::Document =
        try_deserialize_optional(&ctx.args, "sort")?.unwrap_or_else(|| doc! { "_id": if is_backward { -1 } else { 1 } });
    let mut sort = mongodb::bson::Document::new();
    for (gql_field, direction) in &sort_raw {
        let mongo_name = graphql_to_mongo_field(gql_field, collection_def);
        let dir = direction.as_i32().unwrap_or(1);
        sort.insert(mongo_name, dir);
    }

    // Cursor direction flips for backward pagination.
    let sort_direction: i32 = if is_backward { -1 } else { 1 };
    sort.insert("_id", sort_direction);

    let mut cursor = collection
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

    // For backward pagination, reverse the results so edges appear in forward order.
    if is_backward {
        docs.reverse();
    }

    let has_more = docs.len() > limit as usize;
    if has_more {
        if is_backward {
            docs.remove(0);
        } else {
            docs.pop();
        }
    }

    let edges: Vec<serde_json::Value> = docs
        .iter()
        .map(|document| document_to_graphql_value(document, collection_def))
        .collect();

    let start_cursor = docs
        .first()
        .and_then(|document| document.get_object_id("_id").ok())
        .map(|id| encode_cursor(&id));

    let end_cursor = docs
        .last()
        .and_then(|document| document.get_object_id("_id").ok())
        .map(|id| encode_cursor(&id));

    let has_next = if is_backward {
        pagination.before.is_some()
    } else {
        has_more
    };

    let has_previous = if is_backward {
        has_more
    } else {
        pagination.after.is_some()
    };

    let total_count = collection.count_documents(base_filter).await?;

    Ok(serde_json::json!({
        "edges": edges,
        "pageInfo": {
            "hasNextPage": has_next,
            "hasPreviousPage": has_previous,
            "startCursor": start_cursor,
            "endCursor": end_cursor,
        },
        "totalCount": total_count as i64
    }))
}

/// Deserialize an optional argument, returning `None` if absent and
/// an error if present but malformed.
pub(crate) fn try_deserialize_optional<T: serde::de::DeserializeOwned>(
    args: &async_graphql::dynamic::ObjectAccessor<'_>,
    name: &str,
) -> Result<Option<T>, GraphQLError> {
    match args.get(name) {
        Some(value) => value.deserialize().map(Some).map_err(|err| {
            GraphQLError::Internal(format!(
                "Failed to parse argument '{}': {}",
                name, err.message
            ))
        }),
        None => Ok(None),
    }
}

/// Transform a where input that may use `id` (hex string) to `_id` (ObjectId).
/// MongoDB stores the primary key as `_id`, but GraphQL exposes it as `id`.
pub(crate) fn transform_id_filter(
    mut filter: mongodb::bson::Document,
) -> Result<mongodb::bson::Document, GraphQLError> {
    if let Some(mongodb::bson::Bson::String(hex)) = filter.remove("id") {
        let oid = mongodb::bson::oid::ObjectId::parse_str(&hex).map_err(|_| {
            GraphQLError::Internal(format!("Invalid ObjectId: {}", hex))
        })?;
        filter.insert("_id", oid);
    }
    Ok(filter)
}

/// Translate a GraphQL `WhereInput` filter into MongoDB query operators.
/// Converts IdFilter structures: `{"id": {"eq": "hex"}}` → `{"_id": ObjectId("hex")}`
/// and `{"id": {"ne": "hex"}}` → `{"_id": {"$ne": ObjectId("hex")}}`.
pub(crate) fn transform_where_filter(
    filter: mongodb::bson::Document,
    collection_def: &CollectionDef,
) -> Result<mongodb::bson::Document, GraphQLError> {
    let mut result = mongodb::bson::Document::new();
    for (key, value) in filter {
        let mongo_key = graphql_to_mongo_field(&key, collection_def);
        let is_id_field = mongo_key == "_id";

        match value {
            mongodb::bson::Bson::Document(filter_doc) => {
                for (op, val) in filter_doc {
                    // Id field → convert hex string to ObjectId.
                    if is_id_field {
                        // $in / $nin already carry ObjectId arrays — pass through.
                        if op == "$in" || op == "$nin" {
                            result.insert(mongo_key.as_str(), doc! { op.as_str(): val });
                            continue;
                        }
                        let hex = val.as_str().ok_or_else(|| {
                            GraphQLError::Internal(format!(
                                "Id filter value must be a string for operator '{}'",
                                op
                            ))
                        })?;
                        let oid = mongodb::bson::oid::ObjectId::parse_str(hex)
                            .map_err(|_| {
                                GraphQLError::Internal(format!(
                                    "Invalid ObjectId in where filter: {}",
                                    hex
                                ))
                            })?;
                        match op.as_str() {
                            "ne" => {
                                result.insert(mongo_key.as_str(), doc! { "$ne": oid });
                            }
                            _ => {
                                // Unknown operators default to `$eq`.
                                result.insert(mongo_key.as_str(), oid);
                            }
                        }
                        continue;
                    }

                    let mongo_op = operator_to_mongo(&op);
                    if mongo_op == "$eq" {
                        result.insert(mongo_key.as_str(), val);
                    } else {
                        result.insert(mongo_key.as_str(), doc! { mongo_op.as_str(): val });
                    }
                }
            }
            // Plain value → default to $eq.
            _ => {
                result.insert(mongo_key.as_str(), value);
            }
        }
    }
    Ok(result)
}

/// Map a GraphQL field name to its MongoDB field name for a given collection.
fn graphql_to_mongo_field(gql_field: &str, collection_def: &CollectionDef) -> String {
    if gql_field == "id" {
        return "_id".to_string();
    }
    collection_def
        .fields
        .iter()
        .find(|field| field.graphql_name() == gql_field)
        .map(|field| field.name.clone())
        .unwrap_or_else(|| gql_field.to_string())
}

/// Map a GraphQL filter operator name to its MongoDB equivalent.
/// Unknown operators default to `$eq`.
fn operator_to_mongo(op: &str) -> String {
    match op {
        "eq" => "$eq".to_string(),
        "ne" => "$ne".to_string(),
        "gt" => "$gt".to_string(),
        "gte" => "$gte".to_string(),
        "lt" => "$lt".to_string(),
        "lte" => "$lte".to_string(),
        "contains" => "$regex".to_string(),
        "startsWith" => "$regex".to_string(),
        "endsWith" => "$regex".to_string(),
        _ => "$eq".to_string(),
    }
}

fn object_ids_to_bson(ids: &[mongodb::bson::oid::ObjectId]) -> Vec<mongodb::bson::Bson> {
    ids.iter().map(|id| mongodb::bson::Bson::ObjectId(*id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId, Bson};

    #[test]
    fn test_transform_id_filter_converts_hex_to_object_id() {
        let oid = ObjectId::new();
        let filter = doc! { "id": oid.to_hex() };
        let result = transform_id_filter(filter).unwrap();
        assert_eq!(result.get("_id"), Some(&Bson::ObjectId(oid)));
        assert!(!result.contains_key("id"));
    }

    #[test]
    fn test_transform_id_filter_preserves_other_fields() {
        let oid = ObjectId::new();
        let filter = doc! { "id": oid.to_hex(), "alias": "TestHero" };
        let result = transform_id_filter(filter).unwrap();
        assert_eq!(result.get("_id"), Some(&Bson::ObjectId(oid)));
        assert_eq!(
            result.get("alias"),
            Some(&Bson::String("TestHero".into()))
        );
    }

    #[test]
    fn test_transform_id_filter_invalid_hex_returns_error() {
        let filter = doc! { "id": "not-a-valid-hex" };
        let result = transform_id_filter(filter);
        assert!(result.is_err());
    }

    #[test]
    fn test_transform_id_filter_no_id_field_unchanged() {
        let filter = doc! { "alias": "TestHero", "active": true };
        let result = transform_id_filter(filter.clone()).unwrap();
        assert_eq!(result, filter);
    }

    #[test]
    fn test_transform_id_filter_empty_document() {
        let filter = doc! {};
        let result = transform_id_filter(filter.clone()).unwrap();
        assert_eq!(result, filter);
    }
}
