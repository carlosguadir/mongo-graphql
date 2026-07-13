use mongodb::bson::{doc, oid::ObjectId, Bson};
use mongo_graphql::helpers::serialization::bson_to_json;
use serde_json::Value;

#[test]
fn test_bson_string_to_json() {
    let result = bson_to_json(&Bson::String("hello".into()));
    assert_eq!(result, Value::String("hello".into()));
}

#[test]
fn test_bson_int32_to_json() {
    let result = bson_to_json(&Bson::Int32(42));
    assert_eq!(result, Value::Number(serde_json::Number::from(42)));
}

#[test]
fn test_bson_bool_to_json() {
    let result = bson_to_json(&Bson::Boolean(true));
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_bson_object_id_to_json() {
    let oid = ObjectId::new();
    let result = bson_to_json(&Bson::ObjectId(oid));
    assert_eq!(result, Value::String(oid.to_hex()));
}

#[test]
fn test_bson_null_to_json() {
    let result = bson_to_json(&Bson::Null);
    assert_eq!(result, Value::Null);
}

#[test]
fn test_bson_array_to_json() {
    let arr = mongodb::bson::Bson::Array(vec![Bson::String("a".into()), Bson::String("b".into())]);
    let result = bson_to_json(&arr);
    assert_eq!(
        result,
        Value::Array(vec![Value::String("a".into()), Value::String("b".into())])
    );
}

#[test]
fn test_bson_document_to_json() {
    let d = doc! { "key": "value", "num": 1 };
    let result = bson_to_json(&Bson::Document(d));
    let obj = result.as_object().unwrap();
    assert_eq!(obj["key"], Value::String("value".into()));
    assert_eq!(obj["num"], Value::Number(serde_json::Number::from(1)));
}
