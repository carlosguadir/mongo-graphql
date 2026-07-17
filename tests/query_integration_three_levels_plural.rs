#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use fake::faker::name::en::Name;
    use fake::Fake;
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn three_level_nested_queries_plural() {
        let db = get_db().await;
        let schema = get_schema().await;

        // ═══════════════════════════════════════════════════════════════
        // Chain: heroes → missions → villains  +  heroes → powers
        // L1: Hero list, L2: Mission (M2M) + Power (M2M), L3: Villain (M2M)
        // ═══════════════════════════════════════════════════════════════
        {
            let hero_alias: String = Name().fake();
            let hero_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": "Plural Chain Hero",
                    "power_level": 900,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let mission_oid = ObjectId::new();
            let mission_code: String = Name().fake();
            db.collection::<mongodb::bson::Document>("mission")
                .insert_one(doc! {
                    "_id": mission_oid,
                    "id": mission_oid,
                    "code": &mission_code,
                    "description": "Plural three-level mission",
                    "date": mongodb::bson::DateTime::now(),
                    "status": "active",
                    "danger_level": 7,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let villain_alias: String = Name().fake();
            let villain_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("villain")
                .insert_one(doc! {
                    "_id": villain_oid,
                    "id": villain_oid,
                    "alias": &villain_alias,
                    "secret_identity": "Plural Mission Villain",
                    "threat_level": 85,
                    "active": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_mission")
                .insert_one(doc! {
                    "hero_id": hero_oid,
                    "mission_id": mission_oid,
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("villain_mission")
                .insert_one(doc! {
                    "villain_id": villain_oid,
                    "mission_id": mission_oid,
                })
                .await
                .unwrap();

            let power_name: String = Name().fake();
            let power_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("power")
                .insert_one(doc! {
                    "_id": power_oid,
                    "id": power_oid,
                    "name": &power_name,
                    "type": "elemental",
                    "level": 5,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_power")
                .insert_one(doc! {
                    "hero_id": hero_oid,
                    "power_id": power_oid,
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                r#"query {
                    heroes(last: 100) {
                        edges {
                            alias
                            missions { code villains { alias } }
                            powers { name }
                        }
                    }
                }"#,
                async_graphql::Variables::default(),
                None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "heroes→missions→villains + heroes→powers (plural): {:?}",
                result.get("errors")
            );

            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            let hero_edge = edges
                .iter()
                .find(|e| e["alias"].as_str() == Some(&hero_alias))
                .expect("hero edge should be found");

            let missions = hero_edge["missions"].as_array().unwrap();
            assert_eq!(missions.len(), 1);
            assert_eq!(missions[0]["code"].as_str().unwrap(), mission_code);

            let mission_villains = missions[0]["villains"].as_array().unwrap();
            assert_eq!(mission_villains.len(), 1);
            assert_eq!(
                mission_villains[0]["alias"].as_str().unwrap(),
                villain_alias
            );

            let powers = hero_edge["powers"].as_array().unwrap();
            assert_eq!(powers.len(), 1);
            assert_eq!(powers[0]["name"].as_str().unwrap(), power_name);
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: secretlairs → hero → powers
        // L1: SecretLair list, L2: Hero (reverse OneToOne), L3: Power (M2M)
        // ═══════════════════════════════════════════════════════════════
        {
            let lair_name: String = Name().fake();
            let lair_oid = ObjectId::new();

            let hero_alias: String = Name().fake();
            let hero_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": "Plural Lair Hero",
                    "power_level": 600,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                    "secret_lair_id": lair_oid,
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("secret_lair")
                .insert_one(doc! {
                    "_id": lair_oid,
                    "id": lair_oid,
                    "name": &lair_name,
                    "location": "Plural underground base",
                    "is_underground": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let power_name: String = Name().fake();
            let power_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("power")
                .insert_one(doc! {
                    "_id": power_oid,
                    "id": power_oid,
                    "name": &power_name,
                    "type": "psychic",
                    "level": 3,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_power")
                .insert_one(doc! {
                    "hero_id": hero_oid,
                    "power_id": power_oid,
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                r#"query {
                    secretlairs(last: 100) {
                        edges {
                            name
                            hero { alias powers { name } }
                        }
                    }
                }"#,
                async_graphql::Variables::default(),
                None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "secretlairs→hero→powers (plural): {:?}",
                result.get("errors")
            );

            let edges = result["data"]["secretlairs"]["edges"]
                .as_array()
                .unwrap();
            let lair_edge = edges
                .iter()
                .find(|e| e["name"].as_str() == Some(&lair_name))
                .expect("secret_lair edge should be found");
            assert_eq!(lair_edge["hero"]["alias"].as_str().unwrap(), hero_alias);

            let powers = lair_edge["hero"]["powers"].as_array().unwrap();
            assert_eq!(powers.len(), 1);
            assert_eq!(powers[0]["name"].as_str().unwrap(), power_name);
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: powers → heroes → archenemy + missions
        // L1: Power list, L2: Hero (reverse M2M), L3: Villain (OneToOne) + Mission (M2M)
        // ═══════════════════════════════════════════════════════════════
        {
            let power_name: String = Name().fake();
            let power_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("power")
                .insert_one(doc! {
                    "_id": power_oid,
                    "id": power_oid,
                    "name": &power_name,
                    "type": "cosmic",
                    "level": 9,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let villain_alias: String = Name().fake();
            let villain_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("villain")
                .insert_one(doc! {
                    "_id": villain_oid,
                    "id": villain_oid,
                    "alias": &villain_alias,
                    "secret_identity": "Plural Archenemy Villain",
                    "threat_level": 95,
                    "active": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let hero_alias: String = Name().fake();
            let hero_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": "Plural Powered Hero",
                    "power_level": 750,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                    "archenemy_id": villain_oid,
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_power")
                .insert_one(doc! {
                    "hero_id": hero_oid,
                    "power_id": power_oid,
                })
                .await
                .unwrap();

            let mission_code: String = Name().fake();
            let mission_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("mission")
                .insert_one(doc! {
                    "_id": mission_oid,
                    "id": mission_oid,
                    "code": &mission_code,
                    "description": "Plural power chain mission",
                    "date": mongodb::bson::DateTime::now(),
                    "status": "active",
                    "danger_level": 4,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_mission")
                .insert_one(doc! {
                    "hero_id": hero_oid,
                    "mission_id": mission_oid,
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                r#"query {
                    powers(last: 100) {
                        edges {
                            name
                            heroes { alias archenemy { alias } missions { code } }
                        }
                    }
                }"#,
                async_graphql::Variables::default(),
                None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "powers→heroes→archenemy+missions (plural): {:?}",
                result.get("errors")
            );

            let edges = result["data"]["powers"]["edges"].as_array().unwrap();
            let power_edge = edges
                .iter()
                .find(|e| e["name"].as_str() == Some(&power_name))
                .expect("power edge should be found");

            let heroes = power_edge["heroes"].as_array().unwrap();
            assert_eq!(heroes.len(), 1);
            assert_eq!(heroes[0]["alias"].as_str().unwrap(), hero_alias);
            assert_eq!(
                heroes[0]["archenemy"]["alias"].as_str().unwrap(),
                villain_alias
            );

            let missions = heroes[0]["missions"].as_array().unwrap();
            assert_eq!(missions.len(), 1);
            assert_eq!(missions[0]["code"].as_str().unwrap(), mission_code);
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: missions → heroes → team + missions → villains → archenemy_of
        // L1: Mission list, L2: Hero (reverse M2M) + Villain (reverse M2M),
        // L3: Team (OneToMany) + Hero (reverse OneToOne)
        // ═══════════════════════════════════════════════════════════════
        {
            let mission_code: String = Name().fake();
            let mission_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("mission")
                .insert_one(doc! {
                    "_id": mission_oid,
                    "id": mission_oid,
                    "code": &mission_code,
                    "description": "Plural dual chain mission",
                    "date": mongodb::bson::DateTime::now(),
                    "status": "active",
                    "danger_level": 6,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let team_name: String = Name().fake();
            let team_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("team")
                .insert_one(doc! {
                    "_id": team_oid,
                    "id": team_oid,
                    "name": &team_name,
                    "founded_at": mongodb::bson::DateTime::now(),
                    "is_official": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let mission_hero_alias: String = Name().fake();
            let mission_hero_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": mission_hero_oid,
                    "id": mission_hero_oid,
                    "alias": &mission_hero_alias,
                    "secret_identity": "Plural Mission Member",
                    "power_level": 500,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                    "team_id": team_oid,
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("hero_mission")
                .insert_one(doc! {
                    "hero_id": mission_hero_oid,
                    "mission_id": mission_oid,
                })
                .await
                .unwrap();

            let villain_alias: String = Name().fake();
            let villain_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("villain")
                .insert_one(doc! {
                    "_id": villain_oid,
                    "id": villain_oid,
                    "alias": &villain_alias,
                    "secret_identity": "Plural Mission Villain Dual",
                    "threat_level": 80,
                    "active": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            db.collection::<mongodb::bson::Document>("villain_mission")
                .insert_one(doc! {
                    "villain_id": villain_oid,
                    "mission_id": mission_oid,
                })
                .await
                .unwrap();

            let nemesis_alias: String = Name().fake();
            let nemesis_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": nemesis_oid,
                    "id": nemesis_oid,
                    "alias": &nemesis_alias,
                    "secret_identity": "Plural Nemesis of Villain",
                    "power_level": 850,
                    "active": true,
                    "joined_at": mongodb::bson::DateTime::now(),
                    "created_at": mongodb::bson::DateTime::now(),
                    "archenemy_id": villain_oid,
                })
                .await
                .unwrap();

            let result = executor::execute(
                schema,
                r#"query {
                    missions(last: 100) {
                        edges {
                            code
                            heroes { alias team { name } }
                            villains { alias archenemy_of { alias } }
                        }
                    }
                }"#,
                async_graphql::Variables::default(),
                None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "missions→heroes→team + villains→archenemy_of (plural): {:?}",
                result.get("errors")
            );

            let edges = result["data"]["missions"]["edges"]
                .as_array()
                .unwrap();
            let mission_edge = edges
                .iter()
                .find(|e| e["code"].as_str() == Some(&mission_code))
                .expect("mission edge should be found");

            let heroes = mission_edge["heroes"].as_array().unwrap();
            assert_eq!(heroes.len(), 1);
            assert_eq!(
                heroes[0]["alias"].as_str().unwrap(),
                mission_hero_alias
            );
            assert_eq!(heroes[0]["team"]["name"].as_str().unwrap(), team_name);

            let villains = mission_edge["villains"].as_array().unwrap();
            assert_eq!(villains.len(), 1);
            assert_eq!(villains[0]["alias"].as_str().unwrap(), villain_alias);
            assert_eq!(
                villains[0]["archenemy_of"]["alias"].as_str().unwrap(),
                nemesis_alias
            );
        }
    }
}
