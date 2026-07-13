use mongo_graphql::schema::definition::SchemaDefinition;
use mongo_graphql::schema::parser::SchemaParser;

fn load_definition() -> SchemaDefinition {
    let json = include_str!("schema-definition.json");
    SchemaParser::from_str(json).expect("schema-definition.json must be valid")
}

#[test]
fn test_schema_definition_loads() {
    let def = load_definition();
    assert_eq!(def.collections.len(), 6);
}

#[test]
fn test_hero_has_expected_fields() {
    let def = load_definition();
    let hero = def.collection_by_name("hero").unwrap();

    let field_names: Vec<&str> = hero.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(field_names.contains(&"alias"));
    assert!(field_names.contains(&"power_level"));
    assert!(field_names.contains(&"active"));
    assert!(field_names.contains(&"team_id"));
    assert!(field_names.contains(&"missions"));
    assert!(field_names.contains(&"powers"));
}

#[test]
fn test_hero_type_name() {
    let def = load_definition();
    let hero = def.collection_by_name("hero").unwrap();
    assert_eq!(hero.type_name(), "Hero");
    assert_eq!(hero.plural_name(), "heroes");
    assert_eq!(hero.singular_name(), "hero");
}

#[test]
fn test_rank_enum_shared() {
    let def = load_definition();
    let hero = def.collection_by_name("hero").unwrap();
    let villain = def.collection_by_name("villain").unwrap();

    let hero_rank = hero
        .fields
        .iter()
        .find(|f| f.name == "rank")
        .unwrap()
        .r#enum
        .as_ref()
        .unwrap();
    let villain_rank = villain
        .fields
        .iter()
        .find(|f| f.name == "rank")
        .unwrap()
        .r#enum
        .as_ref()
        .unwrap();

    assert_eq!(hero_rank.name, villain_rank.name);
    assert_eq!(hero_rank.values, villain_rank.values);
}
