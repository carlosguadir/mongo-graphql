use async_graphql::dynamic::ResolverContext;
use async_graphql::Value;

use crate::dataloader::DataLoader;
use crate::error::GraphQLError;
use crate::schema::definition::{CollectionDef, JunctionDef};

fn extract_parent_id_hex(ctx: &ResolverContext<'_>) -> Option<String> {
    ctx.parent_value
        .as_value()
        .and_then(|parent| match parent {
            Value::Object(map) => match map.get("id")? {
                Value::String(s) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        })
}

fn extract_parent_hex_field(ctx: &ResolverContext<'_>, mongo_name: &str) -> Option<String> {
    ctx.parent_value
        .as_value()
        .and_then(|parent| match parent {
            Value::Object(map) => match map.get(mongo_name)? {
                Value::String(s) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        })
}

pub async fn resolve_forward_to_one(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    target_collection_name: &str,
    target_def: &CollectionDef,
    fk_mongo_name: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    let fk_hex = match extract_parent_hex_field(ctx, fk_mongo_name) {
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
    let parent_hex = match extract_parent_id_hex(ctx) {
        Some(hex) => hex,
        None => return Ok(None),
    };

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

    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    let targets = loader
        .load_forward_to_one_many(&target_def.collection, projection, &foreign_ids)
        .await?;

    let results: Vec<serde_json::Value> = foreign_ids
        .into_iter()
        .filter_map(|id| targets.get(&id).cloned())
        .collect();
    Ok(Some(serde_json::Value::Array(results)))
}

pub async fn resolve_reverse_to_many(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    target_def: &CollectionDef,
    fk_field: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let parent_hex = match extract_parent_id_hex(ctx) {
        Some(hex) => hex,
        None => return Ok(None),
    };

    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    let results = loader
        .load_reverse_to_many(&target_def.collection, fk_field, projection, &parent_hex)
        .await?;
    Ok(Some(serde_json::Value::Array(results)))
}

pub async fn resolve_reverse_to_one(
    ctx: &ResolverContext<'_>,
    loader: &DataLoader,
    target_def: &CollectionDef,
    fk_field: &str,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let parent_hex = match extract_parent_id_hex(ctx) {
        Some(hex) => hex,
        None => return Ok(None),
    };

    let projection = crate::dataloader::selection_projection_fields(ctx, target_def);
    loader
        .load_reverse_to_one(&target_def.collection, fk_field, projection, &parent_hex)
        .await
}
