use mongodb::bson::oid::ObjectId;
use graphql_mongodb_lib::resolvers::pagination::{decode_cursor, encode_cursor, PaginationArgs};

#[test]
fn test_encode_decode_roundtrip() {
    let id = ObjectId::new();
    let cursor = encode_cursor(&id);
    let decoded = decode_cursor(&cursor).unwrap();
    assert_eq!(id, decoded);
}

#[test]
fn test_decode_invalid_cursor() {
    assert!(decode_cursor("not-valid-base64!!!").is_err());
}

#[test]
fn test_pagination_defaults() {
    let args = PaginationArgs::default();
    assert_eq!(args.effective_limit(100).unwrap(), 20);
}

#[test]
fn test_pagination_clamped_to_max() {
    let args = PaginationArgs {
        first: Some(500),
        after: None,
    };
    assert_eq!(args.effective_limit(100).unwrap(), 100);
}

#[test]
fn test_pagination_explicit_first() {
    let args = PaginationArgs {
        first: Some(5),
        after: None,
    };
    assert_eq!(args.effective_limit(100).unwrap(), 5);
}

#[test]
fn test_pagination_negative_first_rejected() {
    let args = PaginationArgs {
        first: Some(-5),
        after: None,
    };
    assert!(args.effective_limit(100).is_err());
}

#[test]
fn test_pagination_zero_first_rejected() {
    let args = PaginationArgs {
        first: Some(0),
        after: None,
    };
    assert!(args.effective_limit(100).is_err());
}
