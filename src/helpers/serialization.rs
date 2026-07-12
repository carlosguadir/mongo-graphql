use crate::schema::definition::{CollectionDef, FieldType};
use mongodb::bson::Bson;

/// Convert a MongoDB document to a GraphQL-compatible JSON value.
/// Applies name mapping (MongoDB → GraphQL) and omits relation fields in V1.
pub fn document_to_graphql_value(
    doc: &mongodb::bson::Document,
    collection_def: &CollectionDef,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for field_def in &collection_def.fields {
        // V1: skip relation fields in output
        if matches!(field_def.field_type, FieldType::Relation(_)) {
            continue;
        }

        let gql_name = field_def.graphql_name();
        match doc.get(&field_def.name) {
            Some(bson) => {
                map.insert(gql_name, bson_to_json(bson));
            }
            None if field_def.required => {
                map.insert(gql_name, serde_json::Value::Null);
            }
            None => {}
        }
    }

    serde_json::Value::Object(map)
}

/// Convert a BSON value to its JSON equivalent.
pub fn bson_to_json(bson: &Bson) -> serde_json::Value {
    match bson {
        Bson::ObjectId(oid) => serde_json::Value::String(oid.to_hex()),
        Bson::String(s) => serde_json::Value::String(s.clone()),
        Bson::Int32(i) => serde_json::Value::Number((*i).into()),
        Bson::Int64(i) => serde_json::Value::Number((*i).into()),
        Bson::Double(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Bson::Boolean(b) => serde_json::Value::Bool(*b),
        Bson::DateTime(dt) => {
            let millis = dt.timestamp_millis();
            let secs = millis / 1000;
            let nanos = ((millis % 1000) * 1_000_000) as u32;
            match chrono::DateTime::from_timestamp(secs, nanos) {
                Some(chrono_dt) => serde_json::Value::String(chrono_dt.to_rfc3339()),
                None => serde_json::Value::Null,
            }
        }
        Bson::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(bson_to_json).collect())
        }
        Bson::Document(subdoc) => {
            serde_json::Value::Object(
                subdoc.iter().map(|(k, v)| (k.clone(), bson_to_json(v))).collect(),
            )
        }
        Bson::Null | Bson::Undefined => serde_json::Value::Null,
        _ => serde_json::Value::Null,
    }
}
