#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_queries() {
        let db = get_db().await;
        let schema = get_schema().await;

        // ---- introspection ----
        let result = executor::execute(
            schema,
            r#"{ __schema { queryType { name } } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        let query_name = &result["data"]["__schema"]["queryType"]["name"];
        assert_eq!(query_name.as_str().unwrap(), "Query");

        // ---- get by id (existing and non-existing) ----
        let hero = db.collection::<mongodb::bson::Document>("hero");

        let oid = ObjectId::new();
        hero.insert_one(doc! {
            "_id": oid,
            "alias": "IntTestGetHero",
            "secret_identity": "John Doe",
            "power_level": 9000,
            "active": true,
            "joined_at": mongodb::bson::DateTime::now(),
            "created_at": mongodb::bson::DateTime::now(),
        })
        .await
        .unwrap();

        // Non-existent id returns null.
        let result = executor::execute(
            schema,
            r#"query { hero(where: { id: "000000000000000000000000" }) { id alias } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(result["data"]["hero"].is_null());

        // Existing id returns the hero.
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ id alias }} }}"#,
                oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        let hero_data = &result["data"]["hero"];
        assert_eq!(hero_data["id"].as_str().unwrap(), oid.to_hex());
        assert_eq!(hero_data["alias"].as_str().unwrap(), "IntTestGetHero");
    }
}
