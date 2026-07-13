use crate::error::GraphQLError;
use crate::schema::definition::{FieldType, RelationKind, SchemaDefinition};
use std::collections::{HashMap, HashSet};

pub struct SchemaParser;

impl SchemaParser {
    pub fn from_str(json: &str) -> Result<SchemaDefinition, GraphQLError> {
        let schema: SchemaDefinition =
            serde_json::from_str(json).map_err(|e| GraphQLError::SchemaParse {
                message: format!("Invalid JSON: {}", e),
                location: "root".into(),
            })?;

        Self::validate_referenced_collections(&schema)?;
        Self::validate_fields(&schema)?;
        Self::validate_enum_names(&schema)?;
        Self::validate_many_to_many(&schema)?;
        Self::validate_relation_fields(&schema)?;

        Ok(schema)
    }

    fn validate_referenced_collections(schema: &SchemaDefinition) -> Result<(), GraphQLError> {
        let names: std::collections::HashSet<&str> =
            schema.collections.iter().map(|coll_def| coll_def.collection.as_str()).collect();

        for coll in &schema.collections {
            for field in &coll.fields {
                if let FieldType::Relation(rel) = &field.field_type {
                    if !names.contains(rel.collection.as_str()) {
                        return Err(GraphQLError::SchemaParse {
                            message: format!(
                                "Collection '{}' references unknown collection '{}' in field '{}'",
                                coll.collection, rel.collection, field.name
                            ),
                            location: format!(
                                "$.collections.{}.fields.{}",
                                coll.collection, field.name
                            ),
                        });
                    }

                }
            }
        }

        Ok(())
    }

    fn validate_fields(schema: &SchemaDefinition) -> Result<(), GraphQLError> {
        for coll in &schema.collections {
            // Check for duplicate field names within the collection.
            let mut field_names = std::collections::HashSet::new();
            let mut gql_names = std::collections::HashSet::new();
            for field in &coll.fields {
                if !field_names.insert(&field.name) {
                    return Err(GraphQLError::SchemaParse {
                        message: format!(
                            "Duplicate field name '{}' in collection '{}'",
                            field.name, coll.collection
                        ),
                        location: format!(
                            "$.collections.{}.fields.{}",
                            coll.collection, field.name
                        ),
                    });
                }
                let gql_name = field.graphql_name();
                if !gql_names.insert(gql_name.clone()) {
                    return Err(GraphQLError::SchemaParse {
                        message: format!(
                            "Duplicate GraphQL field name '{}' in collection '{}'",
                            gql_name, coll.collection
                        ),
                        location: format!(
                            "$.collections.{}.fields.{}",
                            coll.collection, field.name
                        ),
                    });
                }
            }

            for field in &coll.fields {
                // V1: only a single level of nesting is supported for lists.
                if let FieldType::List(inner) = &field.field_type {
                    if matches!(**inner, FieldType::List(_)) {
                        return Err(GraphQLError::SchemaParse {
                            message: format!(
                                "Nested lists are not supported in {}.{}",
                                coll.collection, field.name
                            ),
                            location: format!(
                                "$.collections.{}.fields.{}.type",
                                coll.collection, field.name
                            ),
                        });
                    }
                }

                if let Some(enum_def) = &field.r#enum {
                    if enum_def.values.is_empty() {
                        return Err(GraphQLError::SchemaParse {
                            message: format!(
                                "Enum '{}' in {}.{} has no values",
                                enum_def.name, coll.collection, field.name
                            ),
                            location: format!(
                                "$.collections.{}.fields.{}.enum.values",
                                coll.collection, field.name
                            ),
                        });
                    }

                    let mut seen = std::collections::HashSet::new();
                    for val in &enum_def.values {
                        if !seen.insert(val) {
                            return Err(GraphQLError::SchemaParse {
                                message: format!(
                                    "Enum '{}' has duplicate value '{}' in {}.{}",
                                    enum_def.name, val, coll.collection, field.name
                                ),
                                location: format!(
                                    "$.collections.{}.fields.{}.enum.values",
                                    coll.collection, field.name
                                ),
                            });
                        }
                        if !is_valid_graphql_identifier(val) {
                            return Err(GraphQLError::SchemaParse {
                                message: format!(
                                    "Enum '{}' has invalid GraphQL identifier '{}' in {}.{}",
                                    enum_def.name, val, coll.collection, field.name
                                ),
                                location: format!(
                                    "$.collections.{}.fields.{}.enum.values",
                                    coll.collection, field.name
                                ),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_enum_names(schema: &SchemaDefinition) -> Result<(), GraphQLError> {
        let type_names: std::collections::HashSet<String> = schema
            .collections
            .iter()
            .map(|coll| coll.type_name())
            .collect();

        for coll_def in &schema.collections {
            for field_def in &coll_def.fields {
                if let Some(enum_def) = &field_def.r#enum {
                    if type_names.contains(&enum_def.name) {
                        return Err(GraphQLError::SchemaParse {
                            message: format!(
                                "Enum '{}' in {}.{} clashes with an auto-generated type name",
                                enum_def.name, coll_def.collection, field_def.name
                            ),
                            location: format!(
                                "$.collections.{}.fields.{}.enum.name",
                                coll_def.collection, field_def.name
                            ),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_many_to_many(schema: &SchemaDefinition) -> Result<(), GraphQLError> {
        let mut junctions: HashMap<&str, Vec<&str>> = HashMap::new();

        for coll in &schema.collections {
            for field in &coll.fields {
                if let FieldType::Relation(rel) = &field.field_type {
                    if matches!(rel.kind, RelationKind::ManyToMany) {
                        let junction = rel
                            .junction
                            .as_ref()
                            .map(|j| j.collection.as_str())
                            .unwrap_or("unknown");

                        junctions
                            .entry(junction)
                            .or_default()
                            .push(&coll.collection);
                    }
                }
            }
        }

        for (junction, sides) in &junctions {
            if sides.len() != 2 {
                return Err(GraphQLError::SchemaParse {
                    message: format!(
                        "many_to_many junction '{}' has {} side(s) declared. \
                         Exactly 2 required (one per participating collection). \
                         Declared by: [{}]",
                        junction,
                        sides.len(),
                        sides.join(", ")
                    ),
                    location: format!("junction: {}", junction),
                });
            }
        }

        Ok(())
    }

    fn validate_relation_fields(schema: &SchemaDefinition) -> Result<(), GraphQLError> {
        for coll in &schema.collections {
            for field in &coll.fields {
                let rel = match &field.field_type {
                    FieldType::Relation(rel) => rel,
                    _ => continue,
                };

                let target = match schema.collection_by_name(&rel.collection) {
                    Some(c) => c,
                    None => continue, // already caught by validate_referenced_collections
                };

                let mut target_field_names: HashSet<&str> = target
                    .fields
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect();
                for f in &target.fields {
                    if let Some(gql) = &f.graphql_name {
                        target_field_names.insert(gql.as_str());
                    }
                }

                if !target_field_names.contains(rel.reference_field.as_str()) {
                    return Err(GraphQLError::SchemaParse {
                        message: format!(
                            "reference_field '{}' not found in collection '{}' (relation from {}.{})",
                            rel.reference_field, rel.collection, coll.collection, field.name
                        ),
                        location: format!(
                            "$.collections.{}.fields.{}",
                            coll.collection, field.name
                        ),
                    });
                }

                if let Some(junction) = &rel.junction {
                    let jct_coll = match schema.collection_by_name(&junction.collection) {
                        Some(c) => c,
                        None => continue,
                    };

                    let mut jct_field_names: HashSet<&str> = jct_coll
                        .fields
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect();
                    for f in &jct_coll.fields {
                        if let Some(gql) = &f.graphql_name {
                            jct_field_names.insert(gql.as_str());
                        }
                    }

                    for jct_field in [&junction.local_field, &junction.foreign_field] {
                        if !jct_field_names.contains(jct_field.as_str()) {
                            return Err(GraphQLError::SchemaParse {
                                message: format!(
                                    "junction field '{}' not found in '{}' (relation from {}.{})",
                                    jct_field, junction.collection, coll.collection, field.name
                                ),
                                location: format!(
                                    "$.collections.{}.fields.{}",
                                    coll.collection, field.name
                                ),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

fn is_valid_graphql_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first_char) if first_char.is_ascii_alphabetic() || first_char == '_' => {
            chars.all(|rest_char| rest_char.is_ascii_alphanumeric() || rest_char == '_')
        }
        _ => false,
    }
}
