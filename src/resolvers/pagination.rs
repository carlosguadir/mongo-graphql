use mongodb::bson::oid::ObjectId;

/// Relay pagination arguments.
#[derive(Debug, Clone, Default)]
pub struct PaginationArgs {
    pub first: Option<i64>,
    pub after: Option<String>,
}

impl PaginationArgs {
    pub fn effective_limit(&self, max_page_size: usize) -> i64 {
        let limit = self.first.unwrap_or(20);
        limit.min(max_page_size as i64).max(1)
    }
}

/// Encode an ObjectId as a base64 cursor string.
pub fn encode_cursor(id: &ObjectId) -> String {
    use base64::Engine;
    let payload = serde_json::json!({ "id": id.to_hex() });
    base64::engine::general_purpose::STANDARD.encode(payload.to_string())
}

/// Decode a base64 cursor string back to an ObjectId.
pub fn decode_cursor(cursor: &str) -> Result<ObjectId, crate::error::GraphQLError> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cursor)
        .map_err(|e| {
            crate::error::GraphQLError::Pagination(format!("Invalid cursor: {}", e))
        })?;

    let payload: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| {
            crate::error::GraphQLError::Pagination(format!("Invalid cursor payload: {}", e))
        })?;

    let id_str = payload["id"].as_str().ok_or_else(|| {
        crate::error::GraphQLError::Pagination("Invalid cursor format: missing id".into())
    })?;

    ObjectId::parse_str(id_str).map_err(|e| {
        crate::error::GraphQLError::Pagination(format!("Invalid cursor id: {}", e))
    })
}