mod common;

use common::config::Config;

#[test]
fn test_default_values() {
    temp_env::with_vars(
        [
            ("MONGO_URI", Some("mongodb://localhost:27017")),
            ("DATABASE_NAME", Some("test_db")),
            ("JWKS_URL", Some("https://example.com/.well-known/jwks.json")),
            ("AUD", Some("test-client")),
            ("INFLECT_NAMES", None::<&str>),
            ("INTROSPECTION", None::<&str>),
            ("QUERY_TIMEOUT_MS", None::<&str>),
            ("CONNECTION_POOL_SIZE", None::<&str>),
            ("MAX_NESTED_DEPTH", None::<&str>),
        ],
        || {
            let config = Config::from_env().unwrap();
            assert_eq!(config.default_page_size, 20);
            assert_eq!(config.max_page_size, 100);
            assert_eq!(config.query_timeout_ms, 25000);
            assert_eq!(config.connection_pool_size, 5);
            assert_eq!(config.max_nested_depth, 3);
            assert_eq!(config.inflect_names, true);
            assert_eq!(config.introspection, false);
        },
    );
}

#[test]
fn test_missing_required_var_errors() {
    temp_env::with_vars(
        [
            ("MONGO_URI", None::<&str>),
            ("DATABASE_NAME", None::<&str>),
            ("JWKS_URL", None::<&str>),
            ("AUD", None::<&str>),
        ],
        || {
            let result = Config::from_env();
            assert!(result.is_err());
        },
    );
}
