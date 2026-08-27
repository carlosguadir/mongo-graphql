#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn relation_where_and_sort() {
        let db = get_db().await;
        let schema = get_schema().await;
        let prefix = format!("RWS{}", ObjectId::new().to_hex());
        let now = mongodb::bson::DateTime::now();
        let day_ms: i64 = 86_400_000;

        // Seed: one hero with 3 missions (danger_level 5, 3, 7; dates one
        // day apart) and one team with 3 members (charlie, alice, bob).
        let hero_id = ObjectId::new();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_id,
                "alias": format!("{}-hero", prefix),
                "secret_identity": "Seed",
                "power_level": 600,
                "active": true,
                "joined_at": &now,
                "created_at": &now,
            })
            .await
            .unwrap();

        let team_id = ObjectId::new();
        db.collection::<mongodb::bson::Document>("team")
            .insert_one(doc! {
                "_id": team_id,
                "name": format!("{}-team", prefix),
                "founded_at": &now,
                "is_official": true,
                "created_at": &now,
            })
            .await
            .unwrap();

        for (i, level) in [5, 3, 7].iter().enumerate() {
            let mission_id = ObjectId::new();
            db.collection::<mongodb::bson::Document>("mission")
                .insert_one(doc! {
                    "_id": mission_id,
                    "code": format!("{}-m{}", prefix, i),
                    "description": "Seed mission",
                    "date": mongodb::bson::DateTime::from_millis(now.timestamp_millis() - (i as i64) * day_ms),
                    "status": if *level >= 7 { "completed" } else { "active" },
                    "danger_level": *level,
                    "created_at": &now,
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_mission")
                .insert_one(doc! {
                    "hero_id": hero_id,
                    "mission_id": mission_id,
                    "role": "operative",
                })
                .await
                .unwrap();
        }

        for (i, alias) in ["charlie", "alice", "bob"].iter().enumerate() {
            let member_id = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": member_id,
                    "alias": format!("{}-{}", prefix, alias),
                    "secret_identity": "Seed",
                    "power_level": 400,
                    "active": i % 2 == 0,
                    "joined_at": &now,
                    "created_at": &now,
                    "team_id": team_id,
                })
                .await
                .unwrap();
        }

        // ── M2M where: only missions with danger_level >= 5 ──────────
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ missions(where: {{ danger_level: {{ gte: 5 }} }}) {{ code }} }} }}"#,
                hero_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "M2M where: {:?}", result.get("errors"));
        let codes: Vec<&str> = result["data"]["hero"]["missions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes.len(), 2, "expected 2 missions with danger_level >= 5");
        assert!(!codes.iter().any(|c| c.ends_with("-m1")), "m1 has danger_level 3");

        // ── M2M sort DESC by danger_level: m2 (7), m0 (5), m1 (3) ────
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ missions(sort: {{ danger_level: DESC }}) {{ code }} }} }}"#,
                hero_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "M2M sort: {:?}", result.get("errors"));
        let codes: Vec<&str> = result["data"]["hero"]["missions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["code"].as_str().unwrap())
            .collect();
        assert_eq!(
            codes,
            vec![format!("{}-m2", prefix), format!("{}-m0", prefix), format!("{}-m1", prefix)]
        );

        // ── M2M where + sort combined: active only, danger_level ASC ──
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ missions(where: {{ status: {{ eq: "active" }} }}, sort: {{ danger_level: ASC }}) {{ code }} }} }}"#,
                hero_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "M2M where+sort: {:?}", result.get("errors"));
        let codes: Vec<&str> = result["data"]["hero"]["missions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, vec![format!("{}-m1", prefix), format!("{}-m0", prefix)]);

        // ── M2M sort ASC by date: oldest first — m2, m1, m0 ──────────
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ missions(sort: {{ date: ASC }}) {{ code }} }} }}"#,
                hero_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "M2M date sort: {:?}", result.get("errors"));
        let codes: Vec<&str> = result["data"]["hero"]["missions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["code"].as_str().unwrap())
            .collect();
        assert_eq!(
            codes,
            vec![format!("{}-m2", prefix), format!("{}-m1", prefix), format!("{}-m0", prefix)]
        );

        // ── Reverse to-many where: only active members ───────────────
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ team(where: {{ id: "{}" }}) {{ members(where: {{ active: {{ eq: true }} }}) {{ alias }} }} }}"#,
                team_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "Rev where: {:?}", result.get("errors"));
        let aliases: Vec<&str> = result["data"]["team"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["alias"].as_str().unwrap())
            .collect();
        assert_eq!(aliases.len(), 2);
        assert!(aliases.contains(&format!("{}-charlie", prefix).as_str()));
        assert!(aliases.contains(&format!("{}-bob", prefix).as_str()));
        assert!(!aliases.iter().any(|a| a.ends_with("alice")));

        // ── Reverse to-many sort ASC by alias ────────────────────────
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ team(where: {{ id: "{}" }}) {{ members(sort: {{ alias: ASC }}) {{ alias }} }} }}"#,
                team_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "Rev sort: {:?}", result.get("errors"));
        let aliases: Vec<&str> = result["data"]["team"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["alias"].as_str().unwrap())
            .collect();
        assert_eq!(
            aliases,
            vec![
                format!("{}-alice", prefix),
                format!("{}-bob", prefix),
                format!("{}-charlie", prefix),
            ]
        );

        // ── Relation filter keys inside relation where are rejected ──
        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ missions(where: {{ heroes: {{ some: {{ alias: {{ eq: "x" }} }} }} }}) {{ code }} }} }}"#,
                hero_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        let errors = result.get("errors").expect("expected validation errors");
        let first = &errors[0];
        assert_eq!(first["extensions"]["code"].as_str().unwrap(), "VALIDATION");
    }
}
