#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_one_to_many_filters() {
        let db = get_db().await;
        db.drop().await.unwrap();
        let schema = get_schema().await;

        let hero_oid = ObjectId::new();
        let hero_rev_oid = ObjectId::new();
        let team_oid = ObjectId::new();
        let team_rev_oid = ObjectId::new();

        // ---- seed: team and hero for forward filter ----
        db.collection::<mongodb::bson::Document>("team")
            .insert_one(doc! {
                "_id": team_oid, "id": team_oid,
                "name": "JusticeLeague", "secret_base": "Watchtower",
                "founded_at": mongodb::bson::DateTime::now(),
                "budget": 1000000.0, "is_official": true,
                "motto": "Truth and Justice",
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_oid, "id": hero_oid,
                "alias": "OneToManyHero", "secret_identity": "Test",
                "power_level": 100, "active": true,
                "team_id": team_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        // ---- seed: team and hero for reverse filter ----
        db.collection::<mongodb::bson::Document>("team")
            .insert_one(doc! {
                "_id": team_rev_oid, "id": team_rev_oid,
                "name": "ReverseTestTeam", "secret_base": "Batcave",
                "founded_at": mongodb::bson::DateTime::now(),
                "budget": 500000.0, "is_official": false,
                "motto": "From the shadows",
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_alias = format!("RevH-{}", &hero_rev_oid.to_hex()[..8]);
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_rev_oid, "id": hero_rev_oid,
                "alias": rev_alias.as_str(), "secret_identity": "Test",
                "power_level": 50, "active": true,
                "team_id": team_rev_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        // ---- test 1: forward — filter heroes by team name ----
        {
            // Matching team → finds hero.
            let query = r#"query { heroes(where: { team: { name: { eq: "JusticeLeague" } } }) { edges { id alias team { id name } } } }"#;
            let result = executor::execute(
                schema, query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();

            assert!(result.get("errors").is_none(), "test 1a failed: {:?}", result.get("errors"));

            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 1, "test 1a: should return exactly one hero");
            let hero = &edges[0];
            assert_eq!(hero["id"].as_str().unwrap(), hero_oid.to_hex());

            let team = &hero["team"];
            assert_eq!(team["name"].as_str().unwrap(), "JusticeLeague");
            assert_eq!(team["id"].as_str().unwrap(), team_oid.to_hex());

            // Non-matching team → no hero returned.
            let query = r#"query { heroes(where: { team: { name: { eq: "NonExistentTeam" } } }) { edges { id } } }"#;
            let result = executor::execute(
                schema, query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "test 1b failed: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 0, "test 1b: should return no heroes; got {:?}", edges);
        }

        // ---- test 2: reverse — filter teams by member alias ----
        {
            // Verify the seeded data.
            let hero_doc = db
                .collection::<mongodb::bson::Document>("hero")
                .find_one(doc! { "alias": rev_alias.as_str() })
                .await
                .unwrap()
                .expect("seeded hero must exist");
            let hero_team_id = hero_doc.get_object_id("team_id").unwrap();
            assert_eq!(hero_team_id, team_rev_oid, "seeded hero has wrong team_id");

            let query = format!(r#"query {{ teams(where: {{ members: {{ some: {{ alias: {{ eq: "{}" }} }} }} }}) {{ edges {{ id name members {{ id alias }} }} }} }}"#, rev_alias);
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();

            assert!(result.get("errors").is_none(), "test 2a failed: {:?}", result.get("errors"));

            let edges = result["data"]["teams"]["edges"].as_array().unwrap();
            let ids: Vec<&str> = edges.iter().map(|e| e["id"].as_str().unwrap()).collect();
            assert!(
                ids.contains(&team_rev_oid.to_hex().as_str()),
                "test 2a: team should be returned; got ids={:?}", ids
            );

            let team = edges.iter().find(|e| e["id"].as_str().unwrap() == team_rev_oid.to_hex()).unwrap();
            assert_eq!(team["name"].as_str().unwrap(), "ReverseTestTeam");

            let members = team["members"].as_array().unwrap();
            let aliases: Vec<&str> = members.iter().map(|m| m["alias"].as_str().unwrap()).collect();
            assert!(
                aliases.contains(&rev_alias.as_str()),
                "test 2a: members should include {}; got {:?}", rev_alias, aliases
            );

            // Non-matching alias → no team returned.
            let query = format!(
                r#"query {{ teams(where: {{ members: {{ some: {{ alias: {{ eq: "NonExistent-{}" }} }} }} }}) {{ edges {{ id }} }} }}"#,
                hero_rev_oid.to_hex()
            );
            let result = executor::execute(
                schema, &query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();
            assert!(result.get("errors").is_none(), "test 2b failed: {:?}", result.get("errors"));
            let edges = result["data"]["teams"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 0, "test 2b: should return no teams; got {:?}", edges);
        }
    }
}
