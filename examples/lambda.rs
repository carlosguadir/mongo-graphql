use std::sync::Arc;

use aws_lambda_events::apigw::{ApiGatewayV2httpRequest, ApiGatewayV2httpResponse};
use aws_lambda_events::encodings::Body;
use lambda_runtime::{run as run_lambda, service_fn, Error, LambdaEvent};
use mongodb::Client;

use mongo_graphql::executor;
use mongo_graphql::schema::builder::{RuntimeConfig, SchemaBuilder};
use mongo_graphql::schema::parser::SchemaParser;

/// Schema definition embedded at compile time.
const DEFAULT_SCHEMA: &str = include_str!("../tests/schema-definition.json");

async fn handler(
    schema: Arc<async_graphql::dynamic::Schema>,
    event: LambdaEvent<ApiGatewayV2httpRequest>,
) -> Result<ApiGatewayV2httpResponse, Error> {
    let body = event.payload.body.unwrap_or_else(|| r#"{"query": ""}"#.into());

    let request: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({"query": ""}));

    let query = request["query"].as_str().unwrap_or("");
    let variables = request
        .get("variables")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let result = executor::execute(&schema, query, variables).await?;

    Ok(ApiGatewayV2httpResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: Some(Body::Text(serde_json::to_string(&result)?)),
        is_base64_encoded: false,
        cookies: vec![],
    })
}

fn load_schema_definition() -> Result<String, Error> {
    if let Ok(path) = std::env::var("SCHEMA_PATH") {
        return Ok(std::fs::read_to_string(&path)?);
    }
    Ok(DEFAULT_SCHEMA.to_string())
}

async fn build_schema() -> Result<async_graphql::dynamic::Schema, Error> {
    let mongo_uri = std::env::var("MONGO_URI")
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "MONGO_URI not set"))?;
    let database_name = std::env::var("DATABASE_NAME")
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "DATABASE_NAME not set"))?;

    let schema_json = load_schema_definition()?;
    let definition = SchemaParser::from_str(&schema_json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let max_page_size: usize = std::env::var("MAX_PAGE_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);

    let client = Client::with_uri_str(&mongo_uri).await?;
    let db = client.database(&database_name);

    let config = RuntimeConfig { max_page_size };
    let schema = SchemaBuilder::new(&config, &definition)
        .build(db)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    Ok(schema)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let schema = Arc::new(build_schema().await?);

    let handler_fn = service_fn(move |event| {
        let schema = schema.clone();
        async move { handler(schema, event).await }
    });

    run_lambda(handler_fn).await
}
