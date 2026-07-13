#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};
    use mongodb::Database;
    use tokio::sync::OnceCell;

    use graphql_mongodb_lib::executor;
    use graphql_mongodb_lib::schema::builder::{RuntimeConfig, SchemaBuilder};
    use graphql_mongodb_lib::schema::parser::SchemaParser;
    use mongodb::Client;

    static DB: OnceCell<Database> = OnceCell::const_new();
    static SCHEMA: OnceCell<async_graphql::dynamic::Schema> = OnceCell::const_new();

    async fn get_db() -> &'static Database {
        DB.get_or_init(|| async {
            let mongo_uri = std::env::var("MONGO_URI")
                .unwrap_or_else(|_| "mongodb://localhost:27017".into());
            let client = Client::with_uri_str(&mongo_uri).await.unwrap();
            client.database("test_graphql_mongodb")
        })
        .await
    }

    async fn get_schema() -> &'static async_graphql::dynamic::Schema {
        SCHEMA
            .get_or_init(|| async {
                let db = get_db().await.clone();
                let json = include_str!("../schema-definition.json");
                let definition =
                    SchemaParser::from_str(json).expect("schema must be valid");
                let config = RuntimeConfig {
                    max_page_size: 100,
                };
                SchemaBuilder::new(&config, &definition)
                    .build(db)
                    .await
                    .expect("schema build")
            })
            .await
    }

    async fn clean_collection(name: &str) {
        let db = get_db().await;
        let _ = db.collection::<mongodb::bson::Document>(name).drop().await;
    }

    #[tokio::test]
    async fn test_create_hero() {
        clean_collection("hero").await;
        let schema = get_schema().await;

        let result = executor::execute(
            schema,
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
        clean_collection("hero").await;
        let schema = get_schema().await;

        let create = executor::execute(
            schema,
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

        let result = executor::execute(
            schema,
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
        clean_collection("hero").await;
        let schema = get_schema().await;

        let create = executor::execute(
            schema,
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

        let result = executor::execute(
            schema,
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
