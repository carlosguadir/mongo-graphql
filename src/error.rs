use async_graphql::ErrorExtensions;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum GraphQLError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Schema parse error at {location}: {message}")]
    SchemaParse { message: String, location: String },

    #[error("Schema build error: {0}")]
    SchemaBuild(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Not found: {message}")]
    NotFound { message: String },

    #[error("Duplicate key: {message}")]
    DuplicateKey { message: String },

    #[error("Pagination error: {0}")]
    Pagination(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    /// The message is NEVER exposed to the client.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl GraphQLError {
    /// Convert this error into an `async_graphql::Error` with structured extensions.
    pub fn into_graphql_error(self) -> async_graphql::Error {
        let message = self.to_string();
        match &self {
            GraphQLError::NotFound { .. } => {
                async_graphql::Error::new(message).extend_with(|_, extensions| {
                    extensions.set("code", "NOT_FOUND");
                })
            }
            GraphQLError::Unauthorized(_) => {
                async_graphql::Error::new(message).extend_with(|_, extensions| {
                    extensions.set("code", "UNAUTHORIZED");
                })
            }
            GraphQLError::Forbidden(_) => {
                async_graphql::Error::new(message).extend_with(|_, extensions| {
                    extensions.set("code", "FORBIDDEN");
                })
            }
            GraphQLError::DuplicateKey { .. } => {
                async_graphql::Error::new(message).extend_with(|_, extensions| {
                    extensions.set("code", "DUPLICATE_KEY");
                })
            }
            GraphQLError::Pagination(_) => {
                async_graphql::Error::new(message).extend_with(|_, extensions| {
                    extensions.set("code", "INVALID_CURSOR");
                })
            }
            GraphQLError::Database(_)
            | GraphQLError::Internal(_)
            | GraphQLError::SchemaParse { .. }
            | GraphQLError::SchemaBuild(_)
            | GraphQLError::Config(_) => {
                async_graphql::Error::new("Internal server error")
            }
        }
    }
}

impl From<async_graphql::Error> for GraphQLError {
    fn from(e: async_graphql::Error) -> Self {
        GraphQLError::Internal(e.message)
    }
}

impl From<mongodb::error::Error> for GraphQLError {
    fn from(e: mongodb::error::Error) -> Self {
        GraphQLError::Database(format!("MongoDB error: {}", e))
    }
}
