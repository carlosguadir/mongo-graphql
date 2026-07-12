use graphql_mongodb_lib::error::GraphQLError;
use graphql_mongodb_lib::schema::parser::SchemaParser;

#[test]
fn test_empty_collections_is_valid() {
    let json = r#"{"collections": []}"#;
    let schema = SchemaParser::from_str(json).unwrap();
    assert!(schema.collections.is_empty());
}

#[test]
fn test_invalid_json_returns_error() {
    let result = SchemaParser::from_str("{invalid");
    assert!(result.is_err());
}

#[test]
fn test_many_to_many_missing_side_rejected() {
    // Solo un lado declara la junction → error
    let json = r#"{
        "collections": [
            {
                "collection": "hero",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    {
                        "name": "missions",
                        "type": {
                            "relation": {
                                "kind": "many_to_many",
                                "collection": "mission",
                                "reference_field": "id",
                                "junction": { "collection": "hero_mission", "local_field": "hero_id", "foreign_field": "mission_id", "foreign_reference": "id", "metadata_fields": [] }
                            }
                        }
                    }
                ]
            },
            {
                "collection": "mission",
                "fields": [
                    { "name": "id", "type": "ID", "required": true }
                ]
            },
            {
                "collection": "hero_mission",
                "fields": [
                    { "name": "id", "type": "ID", "required": true }
                ]
            }
        ]
    }"#;
    let result = SchemaParser::from_str(json);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("1 side"), "expected '1 side' error, got: {}", msg);
}

#[test]
fn test_unknown_collection_in_relation_rejected() {
    let json = r#"{
        "collections": [
            {
                "collection": "hero",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    {
                        "name": "team_id",
                        "type": { "relation": { "kind": "one_to_many", "collection": "nonexistent", "reference_field": "id" } }
                    }
                ]
            }
        ]
    }"#;
    let result = SchemaParser::from_str(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown collection"));
}

#[test]
fn test_empty_enum_values_rejected() {
    let json = r#"{
        "collections": [
            {
                "collection": "hero",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    { "name": "rank", "type": "String", "enum": { "name": "Rank", "values": [] } }
                ]
            }
        ]
    }"#;
    let result = SchemaParser::from_str(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no values"));
}

#[test]
fn test_duplicate_enum_values_rejected() {
    let json = r#"{
        "collections": [
            {
                "collection": "hero",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    { "name": "rank", "type": "String", "enum": { "name": "Rank", "values": ["A", "B", "A"] } }
                ]
            }
        ]
    }"#;
    let result = SchemaParser::from_str(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("duplicate"));
}

#[test]
fn test_three_sides_many_to_many_rejected() {
    // 3 lados para la misma junction → error
    let json = r#"{
        "collections": [
            {
                "collection": "hero",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    {
                        "name": "missions",
                        "type": {
                            "relation": {
                                "kind": "many_to_many",
                                "collection": "mission",
                                "reference_field": "id",
                                "junction": { "collection": "hero_mission", "local_field": "hero_id", "foreign_field": "mission_id", "foreign_reference": "id", "metadata_fields": [] }
                            }
                        }
                    }
                ]
            },
            {
                "collection": "mission",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    {
                        "name": "heroes",
                        "type": {
                            "relation": {
                                "kind": "many_to_many",
                                "collection": "hero",
                                "reference_field": "id",
                                "junction": { "collection": "hero_mission", "local_field": "mission_id", "foreign_field": "hero_id", "foreign_reference": "id", "metadata_fields": [] }
                            }
                        }
                    }
                ]
            },
            {
                "collection": "villain",
                "fields": [
                    { "name": "id", "type": "ID", "required": true },
                    {
                        "name": "missions",
                        "type": {
                            "relation": {
                                "kind": "many_to_many",
                                "collection": "mission",
                                "reference_field": "id",
                                "junction": { "collection": "hero_mission", "local_field": "villain_id", "foreign_field": "mission_id", "foreign_reference": "id", "metadata_fields": [] }
                            }
                        }
                    }
                ]
            },
            {
                "collection": "hero_mission",
                "fields": [
                    { "name": "id", "type": "ID", "required": true }
                ]
            }
        ]
    }"#;
    let result = SchemaParser::from_str(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("3 side"));
}
