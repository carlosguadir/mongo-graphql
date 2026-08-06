#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_sort_fields() {
        let db = get_db().await;
        db.drop().await.unwrap();
        let schema = get_schema().await;

        let hero_a = ObjectId::new();
        let hero_b = ObjectId::new();
        let hero_c = ObjectId::new();

        let now_ms = mongodb::bson::DateTime::now().timestamp_millis();
        let day_ms: i64 = 86_400_000;

        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_a, "id": hero_a,
                "alias": "SortAlpha",
                "secret_identity": "Alice",
                "power_level": 1000,
                "height": 1.60,
                "active": true,
                "joined_at": mongodb::bson::DateTime::from_millis(now_ms - 3 * day_ms),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_b, "id": hero_b,
                "alias": "SortBeta",
                "secret_identity": "Bob",
                "power_level": 3000,
                "height": 1.90,
                "active": true,
                "joined_at": mongodb::bson::DateTime::from_millis(now_ms - 1 * day_ms),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_c, "id": hero_c,
                "alias": "SortGamma",
                "secret_identity": "Charlie",
                "power_level": 2000,
                "height": 1.75,
                "active": true,
                "joined_at": mongodb::bson::DateTime::from_millis(now_ms - 5 * day_ms),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        // ---- DateTime ASC: oldest first (Gamma → Alpha → Beta) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { joined_at: ASC }) { edges { alias } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "DateTime ASC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let aliases: Vec<&str> = edges.iter().map(|e| e["alias"].as_str().unwrap()).collect();
            assert_eq!(
                aliases,
                vec!["SortGamma", "SortAlpha", "SortBeta"],
                "DateTime ASC: got {:?}", aliases
            );
        }

        // ---- DateTime DESC: newest first (Beta → Alpha → Gamma) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { joined_at: DESC }) { edges { alias } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "DateTime DESC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let aliases: Vec<&str> = edges.iter().map(|e| e["alias"].as_str().unwrap()).collect();
            assert_eq!(
                aliases,
                vec!["SortBeta", "SortAlpha", "SortGamma"],
                "DateTime DESC: got {:?}", aliases
            );
        }

        // ---- String ASC: alphabetical (Alpha → Beta → Gamma) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { alias: ASC }) { edges { alias } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "String ASC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let aliases: Vec<&str> = edges.iter().map(|e| e["alias"].as_str().unwrap()).collect();
            assert_eq!(
                aliases,
                vec!["SortAlpha", "SortBeta", "SortGamma"],
                "String ASC: got {:?}", aliases
            );
        }

        // ---- String DESC: reverse alphabetical ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { alias: DESC }) { edges { alias } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "String DESC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let aliases: Vec<&str> = edges.iter().map(|e| e["alias"].as_str().unwrap()).collect();
            assert_eq!(
                aliases,
                vec!["SortGamma", "SortBeta", "SortAlpha"],
                "String DESC: got {:?}", aliases
            );
        }

        // ---- Int ASC: smallest first (1000 → 2000 → 3000) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { power_level: ASC }) { edges { power_level } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "Int ASC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let levels: Vec<i64> = edges.iter().map(|e| e["power_level"].as_i64().unwrap()).collect();
            assert_eq!(levels, vec![1000, 2000, 3000], "Int ASC: got {:?}", levels);
        }

        // ---- Int DESC: largest first (3000 → 2000 → 1000) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { power_level: DESC }) { edges { power_level } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "Int DESC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let levels: Vec<i64> = edges.iter().map(|e| e["power_level"].as_i64().unwrap()).collect();
            assert_eq!(levels, vec![3000, 2000, 1000], "Int DESC: got {:?}", levels);
        }

        // ---- Float ASC: smallest first (1.60 → 1.75 → 1.90) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { height: ASC }) { edges { height } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "Float ASC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let heights: Vec<f64> = edges.iter().map(|e| e["height"].as_f64().unwrap()).collect();
            assert_eq!(heights, vec![1.60, 1.75, 1.90], "Float ASC: got {:?}", heights);
        }

        // ---- Float DESC: largest first (1.90 → 1.75 → 1.60) ----
        {
            let result = executor::execute(
                schema,
                r#"query { heroes(sort: { height: DESC }) { edges { height } } }"#,
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "Float DESC: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let heights: Vec<f64> = edges.iter().map(|e| e["height"].as_f64().unwrap()).collect();
            assert_eq!(heights, vec![1.90, 1.75, 1.60], "Float DESC: got {:?}", heights);
        }
    }
}
