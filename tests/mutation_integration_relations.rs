#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::oid::ObjectId;

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn first_level_relation_mutations() {
        let db = get_db().await;
        let schema = get_schema().await;

        let prefix = format!("Rel_{}", ObjectId::new().to_hex());

        // ── Create Hero with team connect ──
        let team_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("team")
            .insert_one(mongodb::bson::doc! {
                "_id": team_oid,
                "id": team_oid,
                "name": format!("{}_Team", prefix),
                "founded_at": mongodb::bson::DateTime::now(),
                "is_official": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let create = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_Connect",
                        secret_identity: "Rel Tester",
                        power_level: 100,
                        active: true,
                        joined_at: "2024-01-01T00:00:00Z",
                        created_at: "2024-01-01T00:00:00Z",
                        team: {{ connect: {{ id: "{}" }} }}
                    }}) {{ id alias }}
                }}"#,
                prefix, team_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = create.get("errors") {
            panic!("Create with connect failed: {:?}", errors);
        }
        assert_eq!(
            create["data"]["createHero"]["alias"].as_str().unwrap(),
            format!("{}_Connect", prefix)
        );

        let hero_coll = db.collection::<mongodb::bson::Document>("hero");
        let hero_doc = hero_coll
            .find_one(mongodb::bson::doc! { "alias": format!("{}_Connect", prefix) })
            .await
            .unwrap()
            .expect("hero should exist in DB");
        assert_eq!(
            hero_doc.get_object_id("team_id").unwrap(),
            team_oid,
            "team_id should match connected team"
        );

        // ── Create Hero without relation ──
        let create_no_rel = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_NoRel",
                        secret_identity: "Solo Hero",
                        power_level: 50,
                        active: true,
                        joined_at: "2024-01-01T00:00:00Z",
                        created_at: "2024-01-01T00:00:00Z"
                    }}) {{ id }}
                }}"#,
                prefix
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert!(create_no_rel.get("errors").is_none());

        // ── Update Hero: connect to team ──
        let update_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": update_oid,
                "id": update_oid,
                "alias": format!("{}_UpdConnect", prefix),
                "secret_identity": "Updater",
                "power_level": 200,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let update = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(
                        where: {{ id: "{}" }},
                        input: {{ team: {{ connect: {{ id: "{}" }} }} }}
                    ) {{ id }}
                }}"#,
                update_oid.to_hex(),
                team_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = update.get("errors") {
            panic!("Update connect failed: {:?}", errors);
        }
        let updated = hero_coll
            .find_one(mongodb::bson::doc! { "_id": update_oid })
            .await
            .unwrap()
            .expect("updated hero should exist");
        assert_eq!(
            updated.get_object_id("team_id").unwrap(),
            team_oid,
            "team_id should be set after update connect"
        );

        // ── Update Hero: disconnect from team ──
        let disconnect_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": disconnect_oid,
                "id": disconnect_oid,
                "alias": format!("{}_Disconnect", prefix),
                "secret_identity": "Disconnector",
                "power_level": 300,
                "active": true,
                "team_id": team_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let disconnect_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(
                        where: {{ id: "{}" }},
                        input: {{ team: {{ disconnect: true }} }}
                    ) {{ id }}
                }}"#,
                disconnect_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = disconnect_result.get("errors") {
            panic!("Disconnect failed: {:?}", errors);
        }
        let disconnected = hero_coll
            .find_one(mongodb::bson::doc! { "_id": disconnect_oid })
            .await
            .unwrap()
            .expect("disconnected hero should exist");
        assert!(disconnected.get("team_id").and_then(|v| v.as_object_id()).is_none());

        // ── OneToOne connect: secret_lair ──
        let lair_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("secret_lair")
            .insert_one(mongodb::bson::doc! {
                "_id": lair_oid,
                "id": lair_oid,
                "name": format!("{}_Lair", prefix),
                "location": "Undisclosed",
                "is_underground": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let onetoone_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_OTO",
                        secret_identity: "OneToOne Tester",
                        power_level: 400,
                        active: true,
                        joined_at: "2024-01-01T00:00:00Z",
                        created_at: "2024-01-01T00:00:00Z",
                        secret_lair: {{ connect: {{ id: "{}" }} }}
                    }}) {{ id }}
                }}"#,
                prefix, lair_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = onetoone_result.get("errors") {
            panic!("OneToOne connect failed: {:?}", errors);
        }
        let onetoone_hero = hero_coll
            .find_one(mongodb::bson::doc! { "alias": format!("{}_OTO", prefix) })
            .await
            .unwrap()
            .expect("OneToOne hero should exist");
        assert_eq!(
            onetoone_hero.get_object_id("secret_lair_id").unwrap(),
            lair_oid
        );
    }
}
