#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_where_filters() {
        let db = get_db().await;
        db.drop().await.unwrap();
        let schema = get_schema().await;

        let hero = db.collection::<mongodb::bson::Document>("hero");

        // Seed heroes with varied field values for filter testing.
        let now = mongodb::bson::DateTime::now();
        let now_ms = now.timestamp_millis();
        let day_ms: i64 = 86_400_000;
        let hero_a = ObjectId::new();
        let hero_b = ObjectId::new();
        let hero_c = ObjectId::new();

        hero.insert_one(doc! {
            "_id": hero_a, "id": hero_a,
            "alias": "FilterAlpha",
            "secret_identity": "Alice",
            "power_level": 1000,
            "height": 1.75,
            "active": true,
            "joined_at": mongodb::bson::DateTime::from_millis(now_ms - 3 * day_ms),
            "created_at": now,
            "bio": "Fast and strong",
            "protected_city": "Metropolis",
        })
        .await
        .unwrap();

        hero.insert_one(doc! {
            "_id": hero_b, "id": hero_b,
            "alias": "FilterBeta",
            "secret_identity": "Bob",
            "power_level": 2000,
            "height": 1.82,
            "active": false,
            "joined_at": mongodb::bson::DateTime::from_millis(now_ms - 1 * day_ms),
            "created_at": now,
            "bio": "Stealth expert",
            "protected_city": "Gotham",
        })
        .await
        .unwrap();

        hero.insert_one(doc! {
            "_id": hero_c, "id": hero_c,
            "alias": "FilterGamma",
            "secret_identity": "Charlie",
            "power_level": 3000,
            "height": 1.90,
            "active": true,
            "joined_at": mongodb::bson::DateTime::from_millis(now_ms - 5 * day_ms),
            "created_at": now,
            "bio": "Tactical genius",
            "protected_city": "Metropolis",
        })
        .await
        .unwrap();

        // ---- String eq ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { alias: { eq: "FilterAlpha" } }) { edges { alias } totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        if let Some(errors) = result.get("errors") { panic!("eq: {:?}", errors); }
        let conn = &result["data"]["heroes"];

        assert_eq!(conn["totalCount"].as_i64().unwrap(), 1);
        assert_eq!(conn["edges"].as_array().unwrap()[0]["alias"].as_str().unwrap(), "FilterAlpha");

        // ---- String ne ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { alias: { ne: "FilterAlpha" } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- String contains ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { alias: { contains: "Beta" } }) { edges { alias } totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);
        assert_eq!(
            result["data"]["heroes"]["edges"].as_array().unwrap()[0]["alias"].as_str().unwrap(),
            "FilterBeta"
        );

        // ---- String startsWith ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { alias: { startsWith: "Filter" } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 3);

        // ---- String endsWith ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { alias: { endsWith: "mma" } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Int eq ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { power_level: { eq: 2000 } }) { edges { alias } totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Int gt ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { power_level: { gt: 1000 } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- Int gte ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { power_level: { gte: 2000 } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- Int lt ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { power_level: { lt: 2000 } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Int lte ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { power_level: { lte: 1000 } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Float eq ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { height: { eq: 1.75 } }) { edges { alias } totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Float gt ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { height: { gt: 1.80 } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- Boolean eq (true) ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { active: { eq: true } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- Boolean eq (false) ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { active: { eq: false } }) { totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Id eq ----
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ heroes(where: {{ id: {{ eq: "{}" }} }}) {{ edges {{ alias }} totalCount }} }}"#,
                hero_a.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        if let Some(errors) = result.get("errors") { panic!("id eq: {:?}", errors); }
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);

        // ---- Id ne ----
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ heroes(where: {{ id: {{ ne: "{}" }} }}) {{ totalCount }} }}"#,
                hero_a.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- Multiple conditions (AND) ----
        let result = executor::execute(
            schema,
            r#"query { heroes(where: { active: { eq: true }, protected_city: { eq: "Metropolis" } }) { edges { alias } totalCount } }"#,
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        if let Some(errors) = result.get("errors") { panic!("multi: {:?}", errors); }
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 2);

        // ---- DateTime between ----
        // hero_a at 3 days ago, hero_b at 1 day ago, hero_c at 5 days ago.
        // between 4 days ago and 2 days ago → captures hero_a only.
        let from_ms = now_ms - 4 * day_ms;
        let to_ms = now_ms - 2 * day_ms;
        let from_iso = chrono::DateTime::from_timestamp_millis(from_ms)
            .unwrap()
            .to_rfc3339();
        let to_iso = chrono::DateTime::from_timestamp_millis(to_ms)
            .unwrap()
            .to_rfc3339();
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ heroes(where: {{ joined_at: {{ between: {{ from: "{}", to: "{}" }} }} }}) {{ edges {{ alias }} totalCount }} }}"#,
                from_iso, to_iso,
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();
        if let Some(errors) = result.get("errors") { panic!("between: {:?}", errors); }
        assert_eq!(result["data"]["heroes"]["totalCount"].as_i64().unwrap(), 1);
        assert_eq!(
            result["data"]["heroes"]["edges"].as_array().unwrap()[0]["alias"].as_str().unwrap(),
            "FilterAlpha"
        );
    }
}
