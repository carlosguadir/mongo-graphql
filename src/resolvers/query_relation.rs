use async_graphql::dynamic::ResolverContext;
use async_graphql::Value;
use mongodb::bson::{Bson, Document};

use crate::dataloader::DataLoader;
use crate::error::GraphQLError;
use crate::resolvers::filter_relation::{is_relation_filter_key, is_reverse_relation_filter_key};
use crate::resolvers::query::{
    graphql_to_mongo_field, transform_sort_input, transform_where_filter, try_deserialize_optional,
};
use crate::schema::definition::{CollectionDef, JunctionDef, SchemaDefinition};

fn extract_parent_string_field(ctx: &ResolverContext<'_>, field: &str) -> Option<String> {
    let parent = ctx.parent_value.as_value()?;
    let Value::Object(map) = parent else { return None };
    match map.get(field)? {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn build_filter_bytes(
    where_raw: Option<Document>,
    target_def: &CollectionDef,
) -> Result<Option<Vec<u8>>, GraphQLError> {
    let Some(raw) = where_raw else { return Ok(None) };
    let transformed = transform_where_filter(raw, target_def)?;
    if transformed.is_empty() {
        Ok(None)
    } else {
        mongodb::bson::to_vec(&transformed)
            .map(Some)
            .map_err(|err| GraphQLError::Internal(format!("Failed to encode filter: {}", err)))
    }
}

fn reject_relation_filter_keys(
    where_raw: Option<&Document>,
    target_def: &CollectionDef,
    definition: &SchemaDefinition,
) -> Result<(), GraphQLError> {
    let Some(filter) = where_raw else { return Ok(()) };
    for key in filter.keys() {
        if is_relation_filter_key(key, target_def)
            || is_reverse_relation_filter_key(key, target_def, definition)
        {
            return Err(GraphQLError::Validation(format!(
                "Relation filters inside relation-field where arguments are not supported: '{}'",
                key
            )));
        }
    }
    Ok(())
}

fn compare_numbers(left: &serde_json::Number, right: &serde_json::Number) -> std::cmp::Ordering {
    // Compare integers exactly first; going straight to f64 loses precision
    // beyond 2^53 and collapses distinct values into a tie.
    match (left.as_i64(), right.as_i64()) {
        (Some(left_integer), Some(right_integer)) => return left_integer.cmp(&right_integer),
        _ => {}
    }
    match (left.as_u64(), right.as_u64()) {
        (Some(left_integer), Some(right_integer)) => return left_integer.cmp(&right_integer),
        _ => {}
    }
    left.as_f64()
        .partial_cmp(&right.as_f64())
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn compare_json_values(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
    let json_type_rank = |value: &serde_json::Value| -> u8 {
        match value {
            serde_json::Value::Null => 0,
            serde_json::Value::Bool(_) => 1,
            serde_json::Value::Number(_) => 2,
            serde_json::Value::String(_) => 3,
            serde_json::Value::Array(_) => 4,
            serde_json::Value::Object(_) => 5,
        }
    };
    let rank_order = json_type_rank(left).cmp(&json_type_rank(right));
    if rank_order != std::cmp::Ordering::Equal {
        return rank_order;
    }
    match (left, right) {
        (serde_json::Value::Bool(left_bool), serde_json::Value::Bool(right_bool)) => {
            left_bool.cmp(right_bool)
        }
        (serde_json::Value::Number(left_number), serde_json::Value::Number(right_number)) => {
            compare_numbers(left_number, right_number)
        }
        (serde_json::Value::String(left_string), serde_json::Value::String(right_string)) => {
            left_string.cmp(right_string)
        }
        _ => std::cmp::Ordering::Equal,
    }
}

fn sort_json_array_by_spec(array: &mut [serde_json::Value], sort_spec: &Document) {
    array.sort_by(|left_doc, right_doc| {
        for (gql_field, direction) in sort_spec {
            let left_value = left_doc
                .as_object()
                .and_then(|object| object.get(gql_field))
                .unwrap_or(&serde_json::Value::Null);
            let right_value = right_doc
                .as_object()
                .and_then(|object| object.get(gql_field))
                .unwrap_or(&serde_json::Value::Null);
            let ordering = compare_json_values(left_value, right_value);
            if ordering != std::cmp::Ordering::Equal {
                return if direction.as_str() == Some("DESC") {
                    ordering.reverse()
                } else {
                    ordering
                };
            }
        }
        std::cmp::Ordering::Equal
    });
}

pub async fn resolve_forward_to_one(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    target_collection_name: &str,
    target_def: &CollectionDef,
    fk_mongo_name: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    let fk_hex = match extract_parent_string_field(ctx, fk_mongo_name) {
        Some(hex) => hex,
        None => return Ok(None),
    };
    loader
        .load_forward_to_one(target_collection_name, projection, &fk_hex)
        .await
}

pub async fn resolve_forward_many_to_many(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    junction: &JunctionDef,
    target_def: &CollectionDef,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let parent_hex = match extract_parent_string_field(ctx, "id") {
        Some(hex) => hex,
        None => return Ok(None),
    };

    let where_raw: Option<Document> = try_deserialize_optional(&ctx.args, "where")?;
    let mut sort_raw: Option<Document> = try_deserialize_optional(&ctx.args, "sort")?;
    if let Some(sort) = sort_raw.as_mut() {
        // SortInput never exposes `id`, so ties fall back to `_id` order —
        // the same tiebreaker resolve_list and resolve_reverse_to_many apply.
        if !sort.contains_key("id") {
            sort.insert("id", Bson::String("ASC".into()));
        }
    }

    let definition = ctx
        .data_opt::<SchemaDefinition>()
        .ok_or_else(|| GraphQLError::Internal("SchemaDefinition not found in context".into()))?;
    reject_relation_filter_keys(where_raw.as_ref(), target_def, definition)?;

    let foreign_ids = loader
        .load_junction_ids(
            &junction.collection,
            &junction.local_field,
            &junction.foreign_field,
            &parent_hex,
        )
        .await?;

    if foreign_ids.is_empty() {
        return Ok(Some(serde_json::Value::Array(vec![])));
    }

    let mut projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    if let Some(sort) = &sort_raw {
        for (gql_field, _) in sort {
            let mongo_name = graphql_to_mongo_field(gql_field, target_def);
            if !projection.contains(&mongo_name) {
                projection.push(mongo_name);
            }
        }
    }

    let filter_bytes = build_filter_bytes(where_raw, target_def)?;
    let targets = loader
        .load_forward_to_one_many(&target_def.collection, projection, filter_bytes, &foreign_ids)
        .await?;

    let mut results: Vec<serde_json::Value> = foreign_ids
        .into_iter()
        .filter_map(|id| targets.get(&id).cloned())
        .collect();
    if let Some(sort) = &sort_raw {
        sort_json_array_by_spec(&mut results, sort);
    }
    Ok(Some(serde_json::Value::Array(results)))
}

pub async fn resolve_reverse_to_many(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    target_def: &CollectionDef,
    fk_field: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let parent_hex = match extract_parent_string_field(ctx, "id") {
        Some(hex) => hex,
        None => return Ok(None),
    };

    let where_raw: Option<Document> = try_deserialize_optional(&ctx.args, "where")?;
    let sort_raw: Option<Document> = try_deserialize_optional(&ctx.args, "sort")?;

    let definition = ctx
        .data_opt::<SchemaDefinition>()
        .ok_or_else(|| GraphQLError::Internal("SchemaDefinition not found in context".into()))?;
    reject_relation_filter_keys(where_raw.as_ref(), target_def, definition)?;

    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    let filter_bytes = build_filter_bytes(where_raw, target_def)?;

    let sort_bytes = match sort_raw {
        Some(sort) => {
            let mut sort_doc = transform_sort_input(sort, target_def);
            sort_doc.insert("_id", 1);
            mongodb::bson::to_vec(&sort_doc)
                .map(Some)
                .map_err(|err| GraphQLError::Internal(format!("Failed to encode sort: {}", err)))?
        }
        None => None,
    };

    let results = loader
        .load_reverse_to_many(&target_def.collection, fk_field, projection, filter_bytes, sort_bytes, &parent_hex)
        .await?;
    Ok(Some(serde_json::Value::Array(results)))
}

pub async fn resolve_reverse_to_one(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    target_def: &CollectionDef,
    fk_field: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let parent_hex = match extract_parent_string_field(ctx, "id") {
        Some(hex) => hex,
        None => return Ok(None),
    };

    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    loader
        .load_reverse_to_one(&target_def.collection, fk_field, projection, &parent_hex)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use serde_json::json;

    fn mission(id: &str, code: &str, danger_level: i64) -> serde_json::Value {
        json!({ "id": id, "code": code, "danger_level": danger_level })
    }

    fn codes(docs: &[serde_json::Value]) -> Vec<&str> {
        docs.iter().map(|doc| doc["code"].as_str().unwrap()).collect()
    }

    #[test]
    fn test_sort_json_array_by_spec_orders_desc() {
        let mut docs = vec![
            mission("id-1", "m-low", 5),
            mission("id-2", "m-high", 7),
            mission("id-3", "m-mid", 3),
        ];
        sort_json_array_by_spec(&mut docs, &doc! { "danger_level": "DESC" });
        assert_eq!(codes(&docs), vec!["m-high", "m-low", "m-mid"]);
    }

    #[test]
    fn test_sort_json_array_by_spec_uses_id_tiebreaker() {
        let mut docs = vec![
            mission("id-3", "m-c", 5),
            mission("id-1", "m-a", 5),
            mission("id-2", "m-b", 5),
        ];
        sort_json_array_by_spec(&mut docs, &doc! { "danger_level": "ASC", "id": "ASC" });
        assert_eq!(codes(&docs), vec!["m-a", "m-b", "m-c"]);
    }

    #[test]
    fn test_sort_json_array_by_spec_missing_field_ranks_first_ascending() {
        let mut docs = vec![
            json!({ "id": "id-1", "code": "with-power", "power_level": 10 }),
            json!({ "id": "id-2", "code": "without-power" }),
        ];
        sort_json_array_by_spec(&mut docs, &doc! { "power_level": "ASC" });
        assert_eq!(codes(&docs), vec!["without-power", "with-power"]);
    }

    #[test]
    fn test_compare_json_values_distinguishes_large_integers() {
        // 2^53 + 1 and 2^53 collapse to the same f64; the i64 path must win.
        assert_eq!(
            compare_json_values(&json!(9_007_199_254_740_993i64), &json!(9_007_199_254_740_992i64)),
            std::cmp::Ordering::Greater
        );
    }
}
