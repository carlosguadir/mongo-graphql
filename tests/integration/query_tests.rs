#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};
    use graphql_mongodb_lib::executor::GraphQLExecutor;
    use graphql_mongodb_lib::schema::builder::{RuntimeConfig, SchemaBuilder};
    use graphql_mongodb_lib::schema::definition::SchemaDefinition;
    use graphql_mongodb_lib::schema::parser::SchemaParser;
    use mongodb::{Client, Database};

    async fn setup() -> (Database, async_graphql::dynamic::Schema) {
        let mongo_uri =
            std::env::var("MONGO_URI").unwrap_or_else(|_| "mongodb://localhost:27017".into());
        let client = Client::with_uri_str(&mongo_uri).await.unwrap();
        let db = client.database("test_graphql_mongodb");

        // Load schema definition
        let json = include_str!("../../schema-definition.json");
        let definition: SchemaDefinition =
            SchemaParser::from_str(json).expect("schema must be valid");

        let config = RuntimeConfig {
            max_page_size: 100,
        };

        let schema = SchemaBuilder::new(&config, &definition)
            .build(db.clone())
            .expect("schema build");

        // Seed test data
        let hero = db.collection::<mongodb::bson::Document>("hero");
        let _ = hero.drop().await;
        let oid = ObjectId::new();
        hero.insert_one(doc! {
            "_id": oid,
            "id": oid,
            "alias": "TestHero",
            "secret_identity": "John Doe",
            "power_level": 9000,
            "active": true,
            "joined_at": mongodb::bson::DateTime::now(),
            "created_at": mongodb::bson::DateTime::now(),
        })
        .await
        .unwrap();

        (db, schema)
    }

    #[tokio::test]
    async fn test_get_hero_by_id() {
        let (_db, schema) = setup().await;
        let result = GraphQLExecutor::execute(
            &schema,
            r#"query { hero(where: { id: "000000000000000000000000" }) { id alias } }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        // Should return null for non-existent id
        let hero = &result["data"]["hero"];
        assert!(hero.is_null());
    }

    #[tokio::test]
    async fn test_introspection_query() {
        let (_db, schema) = setup().await;
        let result = GraphQLExecutor::execute(
            &schema,
            r#"{ __schema { queryType { name } } }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        let query_name = &result["data"]["__schema"]["queryType"]["name"];
        assert_eq!(query_name.as_str().unwrap(), "Query");
    }

    #[tokio::test]
    async fn test_list_heroes_pagination() {
        let (db, schema) = setup().await;

        // Insert a few heroes to paginate through
        let hero = db.collection::<mongodb::bson::Document>("hero");
        for i in 0..5 {
            let oid = ObjectId::new();
            hero.insert_one(doc! {
                "_id": oid,
                "id": oid,
                "alias": format!("PaginatedHero{}", i),
                "secret_identity": format!("Secret{}", i),
                "power_level": 1000 + i * 100,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        }

        let result = GraphQLExecutor::execute(
            &schema,
            r#"query { heroes(first: 3) { edges { alias powerLevel } pageInfo { hasNextPage endCursor } totalCount } }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        // Should not have errors
        if let Some(errors) = result.get("errors") {
            panic!("GraphQL errors: {:?}", errors);
        }

        let connection = &result["data"]["heroes"];
        let edges = connection["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 3);
        assert_eq!(connection["pageInfo"]["hasNextPage"].as_bool().unwrap(), true);
        assert!(connection["pageInfo"]["endCursor"].as_str().is_some());
    }
}
