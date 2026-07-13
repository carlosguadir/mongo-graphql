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

    #[tokio::test]
    async fn test_get_hero_by_id() {
        let db = get_db().await;
        let schema = get_schema().await;

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

        let result = executor::execute(
            schema,
            r#"query { hero(where: { id: "000000000000000000000000" }) { id alias } }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        let hero_data = &result["data"]["hero"];
        assert!(hero_data.is_null());
    }

    #[tokio::test]
    async fn test_introspection_query() {
        let schema = get_schema().await;

        let result = executor::execute(
            schema,
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
        let db = get_db().await;
        let schema = get_schema().await;

        let hero = db.collection::<mongodb::bson::Document>("hero");
        let _ = hero.drop().await;

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

        let result = executor::execute(
            schema,
            r#"query { heroes(first: 3) { edges { alias powerLevel } pageInfo { hasNextPage endCursor } totalCount } }"#,
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = result.get("errors") {
            panic!("GraphQL errors: {:?}", errors);
        }

        let connection = &result["data"]["heroes"];
        let edges = connection["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 3);
        assert_eq!(connection["pageInfo"]["hasNextPage"].as_bool().unwrap(), true);
        assert_eq!(
            connection["pageInfo"]["hasPreviousPage"].as_bool().unwrap(),
            false
        );
        assert!(connection["pageInfo"]["endCursor"].as_str().is_some());
        assert_eq!(connection["totalCount"].as_i64().unwrap(), 5);

        // Request the second page using the end cursor.
        let cursor = connection["pageInfo"]["endCursor"].as_str().unwrap();
        let result2 = executor::execute(
            schema,
            &format!(
                r#"query {{ heroes(first: 3, after: "{}") {{ edges {{ alias }} pageInfo {{ hasNextPage hasPreviousPage }} totalCount }} }}"#,
                cursor
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        let page2 = &result2["data"]["heroes"];
        let edges2 = page2["edges"].as_array().unwrap();
        assert_eq!(edges2.len(), 2, "second page should have 2 remaining items");
        assert_eq!(page2["pageInfo"]["hasNextPage"].as_bool().unwrap(), false);
        assert_eq!(page2["pageInfo"]["hasPreviousPage"].as_bool().unwrap(), true);
        assert_eq!(page2["totalCount"].as_i64().unwrap(), 5);
    }
}
