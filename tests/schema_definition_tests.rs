use mongo_graphql::schema::definition::{FieldType, RelationKind, SchemaDefinition};
use mongo_graphql::schema::parser::SchemaParser;

fn load_test_schema() -> SchemaDefinition {
    let json = include_str!("schema-definition.json");
    SchemaParser::from_str(json).expect("schema-definition.json must be valid")
}

#[test]
fn test_parse_real_schema_six_collections() {
    let schema = load_test_schema();
    assert_eq!(schema.collections.len(), 6);

    let names: Vec<&str> = schema
        .collections
        .iter()
        .map(|coll_def| coll_def.collection.as_str())
        .collect();
    assert!(names.contains(&"hero"));
    assert!(names.contains(&"villain"));
    assert!(names.contains(&"team"));
    assert!(names.contains(&"mission"));
    assert!(names.contains(&"power"));
    assert!(names.contains(&"secret_lair"));
}

#[test]
fn test_hero_has_all_field_types() {
    let schema = load_test_schema();
    let hero = schema.collection_by_name("hero").unwrap();
    let field_types: Vec<&str> = hero
        .fields
        .iter()
        .map(|f| match &f.field_type {
            FieldType::ID => "ID",
            FieldType::String => "String",
            FieldType::Int => "Int",
            FieldType::Float => "Float",
            FieldType::Boolean => "Boolean",
            FieldType::DateTime => "DateTime",
            FieldType::Json => "Json",
            FieldType::List(_) => "List",
            FieldType::Relation(_) => "Relation",
        })
        .collect();

    assert!(field_types.contains(&"String"));
    assert!(field_types.contains(&"Int"));
    assert!(field_types.contains(&"Float"));
    assert!(field_types.contains(&"Boolean"));
    assert!(field_types.contains(&"DateTime"));
    assert!(field_types.contains(&"Json"));
    assert!(field_types.contains(&"List"));
    assert!(field_types.contains(&"Relation"));
}

#[test]
fn test_enum_deduplication_same_name() {
    let schema = load_test_schema();
    let hero = schema.collection_by_name("hero").unwrap();
    let villain = schema.collection_by_name("villain").unwrap();

    let hero_rank = hero.fields.iter().find(|f| f.name == "rank").unwrap();
    let villain_rank = villain.fields.iter().find(|f| f.name == "rank").unwrap();

    let hero_enum = hero_rank.r#enum.as_ref().unwrap();
    let villain_enum = villain_rank.r#enum.as_ref().unwrap();

    assert_eq!(hero_enum.name, "Rank");
    assert_eq!(villain_enum.name, "Rank");
    assert_eq!(hero_enum.values, villain_enum.values);
}

#[test]
fn test_one_to_many_relations_have_reverse_name() {
    let schema = load_test_schema();
    let hero = schema.collection_by_name("hero").unwrap();
    let team_field = hero.fields.iter().find(|f| f.name == "team_id").unwrap();

    if let FieldType::Relation(rel) = &team_field.field_type {
        assert!(matches!(rel.kind, RelationKind::OneToMany));
        assert_eq!(rel.reverse_name.as_deref(), Some("members"));
        assert_eq!(rel.collection, "team");
    } else {
        panic!("expected Relation field");
    }
}

#[test]
fn test_many_to_many_both_sides_declared() {
    let schema = load_test_schema();
    let hero = schema.collection_by_name("hero").unwrap();
    let mission = schema.collection_by_name("mission").unwrap();

    let hero_missions = hero.fields.iter().find(|f| f.name == "missions").unwrap();
    let mission_heroes = mission.fields.iter().find(|f| f.name == "heroes").unwrap();

    if let (FieldType::Relation(h_rel), FieldType::Relation(m_rel)) =
        (&hero_missions.field_type, &mission_heroes.field_type)
    {
        assert!(matches!(h_rel.kind, RelationKind::ManyToMany));
        assert!(matches!(m_rel.kind, RelationKind::ManyToMany));
        assert_eq!(
            h_rel.junction.as_ref().unwrap().collection,
            m_rel.junction.as_ref().unwrap().collection
        );
    } else {
        panic!("expected Relation fields");
    }
}

#[test]
fn test_list_field_deserialization() {
    let schema = load_test_schema();
    let hero = schema.collection_by_name("hero").unwrap();
    let aliases = hero
        .fields
        .iter()
        .find(|f| f.name == "known_aliases")
        .unwrap();

    match &aliases.field_type {
        FieldType::List(inner) => {
            assert!(matches!(**inner, FieldType::String));
        }
        _ => panic!("known_aliases should be List(String)"),
    }
}

#[test]
fn test_unique_and_required_fields() {
    let schema = load_test_schema();
    let mission = schema.collection_by_name("mission").unwrap();

    let code_field = mission.fields.iter().find(|f| f.name == "code").unwrap();
    assert!(code_field.required);
    assert!(code_field.unique);

    let location_field = mission
        .fields
        .iter()
        .find(|f| f.name == "location")
        .unwrap();
    assert!(!location_field.required);
    assert!(!location_field.unique);
}
