use graphql_mongodb_lib::error::GraphQLError;

/// Dev/test configuration. Not part of the core library API.
#[derive(Debug, Clone)]
pub struct Config {
    pub inflect_names: bool,
    pub introspection: bool,
    pub default_page_size: usize,
    pub max_page_size: usize,

    pub mongo_uri: String,
    pub database_name: String,

    /// e.g. https://cognito-idp.us-east-1.amazonaws.com/us-east-1_abc123/.well-known/jwks.json
    pub jwks_url: String,
    /// Token audience (aud claim).
    pub aud: String,

    pub query_timeout_ms: u64,
    pub connection_pool_size: u32,
    pub max_nested_depth: usize,
}

impl Config {
    pub fn from_env() -> Result<Self, GraphQLError> {
        Ok(Config {
            inflect_names: env_bool("INFLECT_NAMES", true),
            introspection: env_bool("INTROSPECTION", false),
            default_page_size: env_usize("DEFAULT_PAGE_SIZE", 20),
            max_page_size: env_usize("MAX_PAGE_SIZE", 100),
            mongo_uri: std::env::var("MONGO_URI")
                .map_err(|_| GraphQLError::Config("MONGO_URI not set".into()))?,
            database_name: std::env::var("DATABASE_NAME")
                .map_err(|_| GraphQLError::Config("DATABASE_NAME not set".into()))?,
            jwks_url: std::env::var("JWKS_URL")
                .map_err(|_| GraphQLError::Config("JWKS_URL not set".into()))?,
            aud: std::env::var("AUD")
                .map_err(|_| GraphQLError::Config("AUD not set".into()))?,
            query_timeout_ms: env_u64("QUERY_TIMEOUT_MS", 25000),
            connection_pool_size: env_u32("CONNECTION_POOL_SIZE", 5),
            max_nested_depth: env_usize("MAX_NESTED_DEPTH", 3),
        })
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
