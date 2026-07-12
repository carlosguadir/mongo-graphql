#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};
    use graphql_mongodb_lib::executor::GraphQLExecutor;
    use graphql_mongodb_lib::schema::builder::{RuntimeConfig, SchemaBuilder};
    use graphql_mongodb_lib::schema::parser::SchemaParser;
    use mongodb::Client;

    async fn setup() -> async_graphql::dynamic::Schema {
        let mongo_uri =
            std::env::var("MONGO_URI").unwrap_or_else(|_| "mongodb://localhost:27017".into());
        let client = Client::with_uri_str(&mongo_uri).await.unwrap();
        let db = client.database("test_graphql_mongodb");

        let _ = db.collection::<mongodb::bson::Document>("hero").drop().await;

        let json = include_str!("../../schema-definition.json");
        let definition = SchemaParser::from_str(json).expect("schema must be valid");

        let config = RuntimeConfig {
            max_page_size: 100,
        };

        SchemaBuilder::new(&config, &definition)
            .build(db)
            .expect("schema build")
    }

    #[tokio::test]
    async fn test_create_hero() {
        let schema = setup().await;
        let result = GraphQLExecutor::execute(
            &schema,
            r#"mutation {
                createHero(input: {
                    alias: "NewHero",
                    secretIdentity: "Jane Doe",
                    powerLevel: 500,
                    active: true,
                    joinedAt: "2024-01-01T00:00:00Z",
                    bio: "A new hero"
                }) { id alias powerLevel }
            }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        // Check for errors
        if let Some(errors) = result.get("errors") {
            panic!("GraphQL errors: {:?}", errors);
        }

        let hero = &result["data"]["createHero"];
        assert_eq!(hero["alias"].as_str().unwrap(), "NewHero");
        assert_eq!(hero["powerLevel"].as_i64().unwrap(), 500);
        assert!(hero["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_update_hero() {
        let schema = setup().await;

        // First create
        let create = GraphQLExecutor::execute(
            &schema,
            r#"mutation {
                createHero(input: {
                    alias: "UpdateMe",
                    secretIdentity: "Test",
                    powerLevel: 100,
                    active: true,
                    joinedAt: "2024-01-01T00:00:00Z"
                }) { id alias }
            }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        let id = create["data"]["createHero"]["id"].as_str().unwrap().to_string();

        // Then update
        let result = GraphQLExecutor::execute(
            &schema,
            &format!(
                r#"mutation {{
                    updateHero(where: {{ id: "{}" }}, input: {{ alias: "Updated" }}) {{
                        id alias
                    }}
                }}"#,
                id
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            result["data"]["updateHero"]["alias"].as_str().unwrap(),
            "Updated"
        );
    }

    #[tokio::test]
    async fn test_delete_hero() {
        let schema = setup().await;

        // Create first
        let create = GraphQLExecutor::execute(
            &schema,
            r#"mutation {
                createHero(input: {
                    alias: "DeleteMe",
                    secretIdentity: "Test",
                    powerLevel: 100,
                    active: true,
                    joinedAt: "2024-01-01T00:00:00Z"
                }) { id }
            }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        let id = create["data"]["createHero"]["id"].as_str().unwrap().to_string();

        // Delete
        let result = GraphQLExecutor::execute(
            &schema,
            &format!(
                r#"mutation {{
                    deleteHero(where: {{ id: "{}" }}) {{
                        success deletedId
                    }}
                }}"#,
                id
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert_eq!(result["data"]["deleteHero"]["success"].as_bool().unwrap(), true);
    }
}
