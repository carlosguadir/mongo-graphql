use serde::de;
use serde::{Deserialize, Serialize};

use super::inflect::{to_pascal_singular, to_plural};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDefinition {
    pub collections: Vec<CollectionDef>,
}

impl SchemaDefinition {
    pub fn collection_by_name(&self, name: &str) -> Option<&CollectionDef> {
        self.collections.iter().find(|c| c.collection == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionDef {
    pub collection: String,

    #[serde(default)]
    pub graphql_name: Option<String>,

    #[serde(default)]
    pub description: Option<String>,

    pub fields: Vec<FieldDef>,
}

impl CollectionDef {
    pub fn type_name(&self) -> String {
        self.graphql_name
            .clone()
            .unwrap_or_else(|| to_pascal_singular(&self.collection))
    }

    pub fn plural_name(&self) -> String {
        to_plural(&self.collection)
    }

    pub fn singular_name(&self) -> String {
        self.collection.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,

    #[serde(default)]
    pub graphql_name: Option<String>,

    #[serde(rename = "type")]
    pub field_type: FieldType,

    #[serde(default)]
    pub required: bool,

    #[serde(default)]
    pub unique: bool,

    #[serde(default)]
    pub r#enum: Option<EnumDef>,

    #[serde(default)]
    pub description: Option<String>,
}

impl FieldDef {
    pub fn graphql_name(&self) -> String {
        self.graphql_name
            .clone()
            .unwrap_or_else(|| self.name.clone())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    ID,
    String,
    Int,
    Float,
    Boolean,
    DateTime,
    Json,
    /// e.g. ["String"] → [String!] in GraphQL.
    List(Box<FieldType>),
    Relation(RelationFieldDef),
}

impl<'de> Deserialize<'de> for FieldType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde_json::Value;

        let value = Value::deserialize(deserializer)?;

        match value {
            Value::String(s) => match s.to_lowercase().as_str() {
                "id" => Ok(FieldType::ID),
                "string" => Ok(FieldType::String),
                "int" => Ok(FieldType::Int),
                "float" => Ok(FieldType::Float),
                "boolean" => Ok(FieldType::Boolean),
                "datetime" => Ok(FieldType::DateTime),
                "json" => Ok(FieldType::Json),
                _ => Err(de::Error::custom(format!("unknown type: {}", s))),
            },
            Value::Array(arr) if arr.len() == 1 => {
                let first = arr.into_iter().next().ok_or_else(|| {
                    de::Error::custom("expected exactly one list element")
                })?;
                let inner: FieldType =
                    serde_json::from_value(first).map_err(de::Error::custom)?;
                Ok(FieldType::List(Box::new(inner)))
            }
            Value::Object(ref obj) => {
                let rel_value = if let Some(inner) = obj.get("relation") {
                    inner.clone()
                } else {
                    value
                };
                let rel: RelationFieldDef =
                    serde_json::from_value(rel_value).map_err(de::Error::custom)?;
                Ok(FieldType::Relation(rel))
            }
            _ => Err(de::Error::custom("expected string, [type], or relation object")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationFieldDef {
    pub kind: RelationKind,
    pub collection: String,
    pub reference_field: String,

    /// Custom name for the auto-generated reverse field on the target collection.
    #[serde(default)]
    pub reverse_name: Option<String>,

    /// Only for ManyToMany.
    #[serde(default)]
    pub junction: Option<JunctionDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// FK on this collection. e.g. hero.team_id → team.
    OneToMany,
    /// Unique FK on one side. e.g. hero.secret_lair_id ↔ secret_lair.
    OneToOne,
    /// Both sides must be declared.
    ManyToMany,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JunctionDef {
    pub collection: String,
    pub local_field: String,
    pub foreign_field: String,
    pub foreign_reference: String,

    #[serde(default)]
    pub metadata_fields: Vec<String>,
}
