use async_graphql::dynamic::Schema;
use async_graphql::{Data, Request, Variables};

use crate::error::GraphQLError;

pub async fn execute(
    schema: &Schema,
    query: &str,
    variables: Variables,
    operation_name: Option<&str>,
    data: Option<Data>,
) -> Result<serde_json::Value, GraphQLError> {
    let mut request = Request::new(query);
    if let Some(name) = operation_name {
        request = request.operation_name(name);
    }
    request = request.variables(variables);
    if let Some(data) = data {
        request.data = data;
    }
    let response = schema.execute(request).await;
    serde_json::to_value(response)
        .map_err(|e| GraphQLError::Internal(format!("Serialization error: {}", e)))
}
