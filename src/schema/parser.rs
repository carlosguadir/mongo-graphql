use crate::error::GraphQLError;
use crate::schema::definition::{FieldType, RelationKind, SchemaDefinition};
use std::collections::HashMap;

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
        Self::validate_many_to_many(&schema)?;

        Ok(schema)
    }

    fn validate_referenced_collections(schema: &SchemaDefinition) -> Result<(), GraphQLError> {
        let names: std::collections::HashSet<&str> =
            schema.collections.iter().map(|c| c.collection.as_str()).collect();

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
            for field in &coll.fields {
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
}
