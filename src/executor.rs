use async_graphql::dynamic::Schema;
use async_graphql::{Request, Variables};

use crate::error::GraphQLError;

pub struct GraphQLExecutor;

impl GraphQLExecutor {
    pub async fn execute(
        schema: &Schema,
        query: &str,
        variables: Variables,
    ) -> Result<serde_json::Value, GraphQLError> {
        let request = Request::new(query).variables(variables);
        let response = schema.execute(request).await;
        serde_json::to_value(response)
            .map_err(|e| GraphQLError::Internal(format!("Serialization error: {}", e)))
    }
}
