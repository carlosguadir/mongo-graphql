use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphQLError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Schema parse error at {location}: {message}")]
    SchemaParse {
        message: String,
        location: String,
    },

    #[error("Schema build error: {0}")]
    SchemaBuild(String),

    /// The message is NEVER exposed to the client.
    #[error("Internal error: {0}")]
    Internal(String),
}
