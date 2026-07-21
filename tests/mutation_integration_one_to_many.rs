#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use fake::faker::lorem::en::Word;
    use fake::faker::name::en::Name;
    use fake::Fake;
    use mongodb::bson::oid::ObjectId;

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    /// Generate a unique alias prefixed with the test run and a short
    /// hex suffix to avoid collisions even when `Word()` repeats.
    fn unique_alias(prefix: &str, label: &str) -> String {
        let word: String = Word().fake();
        let suffix = &ObjectId::new().to_hex()[..6];
        format!("{}_{}_{}_{}", prefix, label, word, suffix)
    }

    /// Generate a unique name with a short hex suffix.
    fn unique_name(prefix: &str, label: &str) -> String {
        let name: String = Name().fake();
        let suffix = &ObjectId::new().to_hex()[..6];
        format!("{}_{}_{}_{}", prefix, label, name, suffix)
    }

    #[tokio::test]
    async fn one_to_many_relation_mutations() {
        let db = get_db().await;
        let schema = get_schema().await;

        let prefix = format!("OTM_{}", ObjectId::new().to_hex());

        // ── a) Create Hero with team connect ──
        {
            let team_oid = ObjectId::new();
            let team_name = unique_name(&prefix, "Connect");
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(mongodb::bson::doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_alias = unique_alias(&prefix, "Connect");
            let secret_identity = unique_name(&prefix, "Hero");
            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createHero(input: {{
                            alias: "{}",
                            secret_identity: "{}",
                            power_level: 100,
                            active: true,
                            joined_at: "2024-01-01T00:00:00Z",
                            created_at: "2024-01-01T00:00:00Z",
                            team: {{ connect: {{ id: "{}" }} }}
                        }}) {{ id alias }}
                    }}"#,
                    hero_alias, secret_identity, team_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Create with connect failed: {:?}", errors);
            }

            assert_eq!(
                result["data"]["createHero"]["alias"].as_str().unwrap(),
                hero_alias
            );

            let hero_doc = db
                .collection::<mongodb::bson::Document>("hero")
                .find_one(mongodb::bson::doc! { "alias": &hero_alias })
                .await
                .unwrap()
                .expect("hero should exist in DB");
            assert_eq!(
                hero_doc.get_object_id("team_id").unwrap(),
                team_oid,
                "team_id should match connected team"
            );
        }

        // ── b) Create Hero without relation ──
        {
            let hero_alias = unique_alias(&prefix, "NoRel");
            let secret_identity = unique_name(&prefix, "Hero");
            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createHero(input: {{
                            alias: "{}",
                            secret_identity: "{}",
                            power_level: 50,
                            active: true,
                            joined_at: "2024-01-01T00:00:00Z",
                            created_at: "2024-01-01T00:00:00Z"
                        }}) {{ id }}
                    }}"#,
                    hero_alias, secret_identity
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "Create without relation should not error: {:?}",
                result.get("errors")
            );
        }

        // ── c) Update Hero connect to team ──
        {
            let team_oid = ObjectId::new();
            let team_name = unique_name(&prefix, "UpdConnect");
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(mongodb::bson::doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_oid = ObjectId::new();
            let hero_alias = unique_alias(&prefix, "UpdConnect");
            let secret_identity = unique_name(&prefix, "Hero");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            hero_coll
                .insert_one(mongodb::bson::doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": &secret_identity,
                    "power_level": 200,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        updateHero(
                            where: {{ id: "{}" }},
                            input: {{ team: {{ connect: {{ id: "{}" }} }} }}
                        ) {{ id }}
                    }}"#,
                    hero_oid.to_hex(),
                    team_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Update connect failed: {:?}", errors);
            }

            let updated = hero_coll
                .find_one(mongodb::bson::doc! { "_id": hero_oid })
                .await
                .unwrap()
                .expect("updated hero should exist");
            assert_eq!(
                updated.get_object_id("team_id").unwrap(),
                team_oid,
                "team_id should be set after update connect"
            );
        }

        // ── d) Update Hero disconnect from team ──
        {
            let team_oid = ObjectId::new();
            let hero_oid = ObjectId::new();
            let hero_alias = unique_alias(&prefix, "Disconnect");
            let secret_identity = unique_name(&prefix, "Hero");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            hero_coll
                .insert_one(mongodb::bson::doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": &secret_identity,
                    "power_level": 300,
                    "active": true,
                    "team_id": team_oid,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        updateHero(
                            where: {{ id: "{}" }},
                            input: {{ team: {{ disconnect: true }} }}
                        ) {{ id }}
                    }}"#,
                    hero_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Disconnect failed: {:?}", errors);
            }

            let disconnected = hero_coll
                .find_one(mongodb::bson::doc! { "_id": hero_oid })
                .await
                .unwrap()
                .expect("disconnected hero should exist");
            assert!(
                disconnected.get("team_id").and_then(|v| v.as_object_id()).is_none(),
                "team_id should be null after disconnect"
            );
        }

        // ── e) Create Hero with nested team create ──
        {
            let hero_alias = unique_alias(&prefix, "NestedCreate");
            let secret_identity = unique_name(&prefix, "Hero");
            let team_name = unique_name(&prefix, "Nested");
            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createHero(input: {{
                            alias: "{}",
                            secret_identity: "{}",
                            power_level: 500,
                            active: true,
                            joined_at: "2024-01-01T00:00:00Z",
                            created_at: "2024-01-01T00:00:00Z",
                            team: {{ create: {{
                                name: "{}",
                                founded_at: "2024-01-01T00:00:00Z",
                                is_official: true,
                                created_at: "2024-01-01T00:00:00Z"
                            }} }}
                        }}) {{ id alias }}
                    }}"#,
                    hero_alias, secret_identity, team_name
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Nested create failed: {:?}", errors);
            }

            let _id = result["data"]["createHero"]["id"]
                .as_str()
                .expect("id should be present");

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let nested_hero = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &hero_alias })
                .await
                .unwrap()
                .expect("nested create hero should exist");
            let team_oid = nested_hero
                .get_object_id("team_id")
                .expect("team_id should be set after nested create");

            let created_team = db
                .collection::<mongodb::bson::Document>("team")
                .find_one(mongodb::bson::doc! { "_id": team_oid })
                .await
                .unwrap()
                .expect("nested team should exist in DB");
            assert_eq!(
                created_team.get_str("name").unwrap(),
                team_name,
                "nested team name should match"
            );
        }

        // ── f) Update Hero nested team delete ──
        {
            let team_oid = ObjectId::new();
            let team_name = unique_name(&prefix, "Delete");
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(mongodb::bson::doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_oid = ObjectId::new();
            let hero_alias = unique_alias(&prefix, "Delete");
            let secret_identity = unique_name(&prefix, "Hero");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            hero_coll
                .insert_one(mongodb::bson::doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": &secret_identity,
                    "power_level": 700,
                    "active": true,
                    "team_id": team_oid,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        updateHero(
                            where: {{ id: "{}" }},
                            input: {{ team: {{ delete: true }} }}
                        ) {{ id }}
                    }}"#,
                    hero_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Nested delete failed: {:?}", errors);
            }

            let deleted_rel_hero = hero_coll
                .find_one(mongodb::bson::doc! { "_id": hero_oid })
                .await
                .unwrap()
                .expect("hero after delete should exist");
            assert!(
                deleted_rel_hero.get("team_id").and_then(|v| v.as_object_id()).is_none(),
                "team_id should be null after nested delete"
            );

            let deleted_team = db
                .collection::<mongodb::bson::Document>("team")
                .find_one(mongodb::bson::doc! { "_id": team_oid })
                .await
                .unwrap();
            assert!(
                deleted_team.is_none(),
                "team should be deleted from DB after nested delete"
            );
        }

        // ── g) Mutual exclusivity: create + connect ──
        {
            let team_oid = ObjectId::new();
            let team_name = unique_name(&prefix, "Exclusive");
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(mongodb::bson::doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_alias = unique_alias(&prefix, "Exclusive");
            let secret_identity = unique_name(&prefix, "Hero");
            let should_not_exist = unique_alias(&prefix, "ShouldNotExist");
            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createHero(input: {{
                            alias: "{}",
                            secret_identity: "{}",
                            power_level: 800,
                            active: true,
                            joined_at: "2024-01-01T00:00:00Z",
                            created_at: "2024-01-01T00:00:00Z",
                            team: {{ create: {{
                                name: "{}",
                                founded_at: "2024-01-01T00:00:00Z",
                                is_official: true,
                                created_at: "2024-01-01T00:00:00Z"
                            }}, connect: {{ id: "{}" }} }}
                        }}) {{ id }}
                    }}"#,
                    hero_alias, secret_identity, should_not_exist, team_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_some(),
                "Providing both create and connect should error"
            );
        }

        // ── h) Reverse: create Team with members connect ──
        {
            let hero_oid = ObjectId::new();
            let hero_alias = unique_alias(&prefix, "RevConnect");
            let hero_identity = unique_name(&prefix, "Hero");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            hero_coll
                .insert_one(mongodb::bson::doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": &hero_identity,
                    "power_level": 100,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let team_name = unique_name(&prefix, "RevConnect");
            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createTeam(input: {{
                            name: "{}",
                            founded_at: "2024-01-01T00:00:00Z",
                            is_official: true,
                            created_at: "2024-01-01T00:00:00Z",
                            members: {{ connect: [{{ id: "{}" }}] }}
                        }}) {{ id }}
                    }}"#,
                    team_name, hero_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Reverse members connect failed: {:?}", errors);
            }

            let team_oid = ObjectId::parse_str(
                result["data"]["createTeam"]["id"].as_str().unwrap(),
            )
            .unwrap();

            let updated_hero = hero_coll
                .find_one(mongodb::bson::doc! { "_id": hero_oid })
                .await
                .unwrap()
                .expect("reverse connect hero should exist");
            assert_eq!(
                updated_hero.get_object_id("team_id").unwrap(),
                team_oid,
                "hero.team_id should match the newly created team"
            );
        }

        // ── i) Reverse: create Team with nested members create ──
        {
            let team_name = unique_name(&prefix, "RevCreate");
            let hero_alias = unique_alias(&prefix, "RevCreate");
            let hero_identity = unique_name(&prefix, "Hero");

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createTeam(input: {{
                            name: "{}",
                            founded_at: "2024-01-01T00:00:00Z",
                            is_official: true,
                            created_at: "2024-01-01T00:00:00Z",
                            members: {{ create: [{{
                                alias: "{}",
                                secret_identity: "{}",
                                power_level: 600,
                                active: true,
                                joined_at: "2024-01-01T00:00:00Z",
                                created_at: "2024-01-01T00:00:00Z"
                            }}] }}
                        }}) {{ id }}
                    }}"#,
                    team_name, hero_alias, hero_identity
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Reverse members create failed: {:?}", errors);
            }

            let team_oid = ObjectId::parse_str(
                result["data"]["createTeam"]["id"].as_str().unwrap(),
            )
            .unwrap();

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let created_hero = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &hero_alias })
                .await
                .unwrap()
                .expect("reverse created hero should exist");
            assert_eq!(
                created_hero.get_object_id("team_id").unwrap(),
                team_oid,
                "nested hero.team_id should match the created team"
            );
        }

        // ── j) Reverse: update Team — disconnect members ──
        {
            let team_oid = ObjectId::new();
            let team_name = unique_name(&prefix, "RevDisconnect");
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(mongodb::bson::doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_oid = ObjectId::new();
            let hero_alias = unique_alias(&prefix, "RevDisconnect");
            let hero_identity = unique_name(&prefix, "Hero");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            hero_coll
                .insert_one(mongodb::bson::doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": &hero_identity,
                    "power_level": 300,
                    "active": true,
                    "team_id": team_oid,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        updateTeam(
                            where: {{ id: "{}" }},
                            input: {{ members: {{ disconnect: [{{ id: "{}" }}] }} }}
                        ) {{ id }}
                    }}"#,
                    team_oid.to_hex(),
                    hero_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Reverse members disconnect failed: {:?}", errors);
            }

            let disconnected = hero_coll
                .find_one(mongodb::bson::doc! { "_id": hero_oid })
                .await
                .unwrap()
                .expect("disconnected hero should exist");
            assert!(
                disconnected.get("team_id").and_then(|v| v.as_object_id()).is_none(),
                "hero.team_id should be null after reverse disconnect"
            );
        }

        // ── k) Reverse: update Team — delete members ──
        {
            let team_oid = ObjectId::new();
            let team_name = unique_name(&prefix, "RevDelete");
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(mongodb::bson::doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_oid = ObjectId::new();
            let hero_alias = unique_alias(&prefix, "RevDelete");
            let hero_identity = unique_name(&prefix, "Hero");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            hero_coll
                .insert_one(mongodb::bson::doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": &hero_identity,
                    "power_level": 25,
                    "active": true,
                    "team_id": team_oid,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        updateTeam(
                            where: {{ id: "{}" }},
                            input: {{ members: {{ delete: [{{ id: "{}" }}] }} }}
                        ) {{ id }}
                    }}"#,
                    team_oid.to_hex(),
                    hero_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Reverse members delete failed: {:?}", errors);
            }

            let deleted_hero = hero_coll
                .find_one(mongodb::bson::doc! { "_id": hero_oid })
                .await
                .unwrap();
            assert!(
                deleted_hero.is_none(),
                "hero should be deleted after reverse delete"
            );
        }
    }
}
