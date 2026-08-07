use crate::error::GraphQLError;
use crate::schema::definition::{CollectionDef, FieldType, RelationKind};
use async_graphql::dynamic::FieldValue;
use mongodb::bson::Bson;

/// Convert a MongoDB document to a GraphQL-compatible JSON value.
pub fn document_to_graphql_value(
    doc: &mongodb::bson::Document,
    collection_def: &CollectionDef,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for field_def in &collection_def.fields {
        if let FieldType::Relation(rel) = &field_def.field_type {
            match rel.kind {
                RelationKind::OneToMany | RelationKind::OneToOne => {
                    if let Some(bson) = doc.get(&field_def.name) {
                        map.insert(field_def.name.clone(), bson_to_json(bson));
                    }
                }
                RelationKind::ManyToMany => {}
            }
            continue;
        }

        let gql_name = field_def.graphql_name();
        match doc.get(&field_def.name) {
            Some(bson) => {
                map.insert(gql_name, bson_to_json(bson));
            }
            // Omit missing fields rather than inserting null — async-graphql will
            // enforce the non-null contract and surface a field error if a required
            // field is absent from the MongoDB document.
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
                subdoc.iter().map(|(key, value)| (key.clone(), bson_to_json(value))).collect(),
            )
        }
        Bson::Null | Bson::Undefined => serde_json::Value::Null,
        _ => serde_json::Value::Null,
    }
}

/// Map GraphQL input field names back to MongoDB field names.
/// Relation fields with hex string values are converted to ObjectId.
/// Fields without an explicit `graphql_name` are returned unchanged.
/// TODO this function look unnecessary, let's review
pub fn input_doc_to_mongo(doc: mongodb::bson::Document, collection_def: &CollectionDef) -> mongodb::bson::Document {
    let mut mapped = mongodb::bson::Document::new();
    for (key, value) in doc {
        let field_def = collection_def
            .fields
            .iter()
            .find(|field| field.graphql_name() == key);
        let mongo_name = field_def
            .map(|field| field.name.clone())
            .unwrap_or_else(|| key.clone());

        let mapped_value = match field_def {
            Some(field_def) if matches!(field_def.field_type, FieldType::Relation(_)) => {
                // Relation fields are extracted and processed by the mutation resolver
                // (process_nested_one_input / process_nested_many_input).
                // They no longer arrive here as plain hex strings.
                continue;
            }
            Some(f) if matches!(f.field_type, FieldType::DateTime) => {
                match value.as_str() {
                    Some(s) => match mongodb::bson::DateTime::parse_rfc3339_str(s) {
                        Ok(dt) => mongodb::bson::Bson::DateTime(dt),
                        Err(_) => value,
                    },
                    None => value,
                }
            }
            _ => value,
        };
        mapped.insert(mongo_name, mapped_value);
    }
    mapped
}

/// Convert a `serde_json::Value` into a dynamic `FieldValue` for resolver responses.
pub fn json_to_field_value(json: serde_json::Value) -> Result<FieldValue<'static>, GraphQLError> {
    async_graphql::Value::try_from(json)
        .map(FieldValue::value)
        .map_err(|e| GraphQLError::Internal(format!("Value conversion: {}", e)))
}
