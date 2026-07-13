use mongodb::bson::oid::ObjectId;

/// Relay pagination arguments — supports forward (`first`/`after`) and
/// backward (`last`/`before`) pagination. The spec requires exactly one of
/// `first` or `last`; providing both is an error.
#[derive(Debug, Clone, Default)]
pub struct PaginationArgs {
    pub first: Option<i64>,
    pub after: Option<String>,
    pub last: Option<i64>,
    pub before: Option<String>,
}

impl PaginationArgs {
    pub fn effective_limit(&self, max_page_size: usize) -> Result<i64, crate::error::GraphQLError> {
        let has_first = self.first.is_some();
        let has_last = self.last.is_some();

        if has_first && has_last {
            return Err(crate::error::GraphQLError::Pagination(
                "Provide either 'first' or 'last', not both".into(),
            ));
        }

        let limit = self.first.or(self.last).unwrap_or(20);
        if limit < 1 {
            return Err(crate::error::GraphQLError::Pagination(format!(
                "Pagination limit must be positive, got {}",
                limit
            )));
        }
        Ok(limit.min(max_page_size as i64))
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