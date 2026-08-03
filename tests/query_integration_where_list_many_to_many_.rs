#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_nested_relation_filters() {
        let db = get_db().await;
        let schema = get_schema().await;

        let hero_oid = ObjectId::new();
        let mission_alpha_oid = ObjectId::new();
        let mission_beta_oid = ObjectId::new();
        let villain_oid = ObjectId::new();

        let common_prefix = format!("CP-{}", &hero_oid.to_hex()[..8]);
        let alpha_code = format!("{}-ALPHA", common_prefix);
        let beta_code = format!("{}-BETA", common_prefix);

        // ---- seed data ----
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_oid, "id": hero_oid,
                "alias": "FilterHero", "secret_identity": "Test Identity",
                "power_level": 100, "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("mission")
            .insert_one(doc! {
                "_id": mission_alpha_oid, "id": mission_alpha_oid,
                "code": alpha_code.as_str(), "description": "Alpha mission",
                "date": mongodb::bson::DateTime::now(),
                "status": "active", "danger_level": 5, "reward": 1000,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("mission")
            .insert_one(doc! {
                "_id": mission_beta_oid, "id": mission_beta_oid,
                "code": beta_code.as_str(), "description": "Beta mission",
                "date": mongodb::bson::DateTime::now(),
                "status": "completed", "danger_level": 2, "reward": 500,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("villain")
            .insert_one(doc! {
                "_id": villain_oid, "id": villain_oid,
                "alias": "TestVillain", "secret_identity": "Bad Guy",
                "threat_level": 10, "active": true, "rank": "ALPHA",
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("hero_mission")
            .insert_one(doc! {
                "hero_id": hero_oid, "mission_id": mission_alpha_oid, "role": "leader",
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("hero_mission")
            .insert_one(doc! {
                "hero_id": hero_oid, "mission_id": mission_beta_oid, "role": "support",
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("villain_mission")
            .insert_one(doc! {
                "villain_id": villain_oid, "mission_id": mission_alpha_oid,
                "evil_plan": "world domination",
            })
            .await
            .unwrap();

        // ---- test 1: some filter finds matching hero ----
        {
            let query = format!(
                r#"query {{ heroes(where: {{ missions: {{ some: {{ code: {{ eq: "{}" }} }} }} }}) {{ edges {{ id alias missions {{ code }} }} }} }}"#,
                alpha_code
            );
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(
                result.get("errors").is_none(),
                "test 1 failed: {:?}",
                result.get("errors")
            );
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 1, "should return exactly one hero");
            let hero = &edges[0];
            assert_eq!(hero["id"].as_str().unwrap(), hero_oid.to_hex());
            assert_eq!(hero["alias"].as_str().unwrap(), "FilterHero");

            let missions = hero["missions"].as_array().unwrap();
            let codes: Vec<&str> = missions.iter().map(|m| m["code"].as_str().unwrap()).collect();
            assert!(
                codes.contains(&alpha_code.as_str()),
                "missions should include {}; got {:?}", alpha_code, codes
            );
            assert!(
                codes.contains(&beta_code.as_str()),
                "missions should include {}; got {:?}", beta_code, codes
            );
        }

        // ---- test 2: every — ALL missions must match ----
        {
            // Both missions contain the common prefix → hero matches.
            // `contains` cannot be negated, so `every` falls back to `some`.
            let query = format!(
                r#"query {{ heroes(where: {{ missions: {{ every: {{ code: {{ contains: "{}" }} }} }} }}) {{ edges {{ id alias missions {{ code }} }} }} }}"#,
                common_prefix
            );
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "test 2a failed: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 1, "test 2a: should return exactly one hero");
            let hero = &edges[0];
            assert_eq!(hero["id"].as_str().unwrap(), hero_oid.to_hex());
            assert_eq!(hero["alias"].as_str().unwrap(), "FilterHero");

            let missions = hero["missions"].as_array().unwrap();
            let codes: Vec<&str> = missions.iter().map(|m| m["code"].as_str().unwrap()).collect();
            assert!(
                codes.contains(&alpha_code.as_str()) && codes.contains(&beta_code.as_str()),
                "test 2a: should include both missions; got {:?}", codes
            );

            // Only alpha matches, beta does not → hero excluded.
            let query = format!(
                r#"query {{ heroes(where: {{ missions: {{ every: {{ code: {{ eq: "{}" }} }} }} }}) {{ edges {{ id }} }} }}"#,
                alpha_code
            );
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "test 2b failed: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let ids: Vec<&str> = edges.iter().map(|e| e["id"].as_str().unwrap()).collect();
            assert!(
                !ids.contains(&hero_oid.to_hex().as_str()),
                "test 2b: every-eq should exclude hero; got ids={:?}", ids
            );
        }

        // ---- test 3: none — NO mission must match ----
        {
            // No mission has code NONEXISTENT → hero matches.
            let query = format!(
                r#"query {{ heroes(where: {{ missions: {{ none: {{ code: {{ eq: "NONEXISTENT-{}" }} }} }} }}) {{ edges {{ id }} }} }}"#,
                hero_oid.to_hex()
            );
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "test 3a failed: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let ids: Vec<&str> = edges.iter().map(|e| e["id"].as_str().unwrap()).collect();
            assert!(
                ids.contains(&hero_oid.to_hex().as_str()),
                "test 3a: none-nonexistent should find hero; got ids={:?}", ids
            );

            // alpha_code exists → hero excluded.
            let query = format!(
                r#"query {{ heroes(where: {{ missions: {{ none: {{ code: {{ eq: "{}" }} }} }} }}) {{ edges {{ id }} }} }}"#,
                alpha_code
            );
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "test 3b failed: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let ids: Vec<&str> = edges.iter().map(|e| e["id"].as_str().unwrap()).collect();
            assert!(
                !ids.contains(&hero_oid.to_hex().as_str()),
                "test 3b: none-existing should exclude hero; got ids={:?}", ids
            );
        }
    }
}
