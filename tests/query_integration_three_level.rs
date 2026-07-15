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
    async fn three_level_nested_queries() {
        let db = get_db().await;
        let schema = get_schema().await;

        // ═══════════════════════════════════════════════════════════════
        // Chain: hero → missions → villains  +  hero → powers
        // L1: Hero, L2: Mission (M2M) + Power (M2M), L3: Villain (M2M from mission)
        // ═══════════════════════════════════════════════════════════════
        {
            let hero_alias: String = Name().fake();
            let hero_oid = ObjectId::new();
            db.collection::<mongodb::bson::Document>("hero")
                .insert_one(doc! {
                    "_id": hero_oid,
                    "id": hero_oid,
                    "alias": &hero_alias,
                    "secret_identity": "Chain Hero",
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
                    "description": "Three-level mission",
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
                    "secret_identity": "Mission Villain",
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
                &format!(
                    r#"query {{
                        hero(where: {{ id: "{}" }}) {{
                            alias
                            missions {{ code villains {{ alias }} }}
                            powers {{ name }}
                        }}
                    }}"#,
                    hero_oid.to_hex()
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "hero→missions→villains + hero→powers: {:?}",
                result.get("errors")
            );

            let hero_data = &result["data"]["hero"];
            assert_eq!(hero_data["alias"].as_str().unwrap(), hero_alias);

            let missions = hero_data["missions"].as_array().unwrap();
            assert_eq!(missions.len(), 1);
            assert_eq!(missions[0]["code"].as_str().unwrap(), mission_code);

            let mission_villains = missions[0]["villains"].as_array().unwrap();
            assert_eq!(mission_villains.len(), 1);
            assert_eq!(
                mission_villains[0]["alias"].as_str().unwrap(),
                villain_alias
            );

            let powers = hero_data["powers"].as_array().unwrap();
            assert_eq!(powers.len(), 1);
            assert_eq!(powers[0]["name"].as_str().unwrap(), power_name);
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: secret_lair → hero → powers
        // L1: SecretLair, L2: Hero (reverse OneToOne), L3: Power (M2M)
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
                    "secret_identity": "Lair Hero",
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
                    "location": "Deep underground",
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
                &format!(
                    r#"query {{
                        secretLair(where: {{ id: "{}" }}) {{
                            name
                            hero {{ alias powers {{ name }} }}
                        }}
                    }}"#,
                    lair_oid.to_hex()
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "secret_lair→hero→powers: {:?}",
                result.get("errors")
            );

            let lair_data = &result["data"]["secretLair"];
            assert_eq!(lair_data["name"].as_str().unwrap(), lair_name);
            assert_eq!(lair_data["hero"]["alias"].as_str().unwrap(), hero_alias);

            let powers = lair_data["hero"]["powers"].as_array().unwrap();
            assert_eq!(powers.len(), 1);
            assert_eq!(powers[0]["name"].as_str().unwrap(), power_name);
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: power → heroes → archenemy + missions
        // L1: Power, L2: Hero (reverse M2M), L3: Villain (OneToOne) + Mission (M2M)
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
                    "secret_identity": "Archenemy Villain",
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
                    "secret_identity": "Powered Hero",
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
                    "description": "Hero mission via power chain",
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
                &format!(
                    r#"query {{
                        power(where: {{ id: "{}" }}) {{
                            name
                            heroes {{ alias archenemy {{ alias }} missions {{ code }} }}
                        }}
                    }}"#,
                    power_oid.to_hex()
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "power→heroes→archenemy+missions: {:?}",
                result.get("errors")
            );

            let power_data = &result["data"]["power"];
            assert_eq!(power_data["name"].as_str().unwrap(), power_name);

            let heroes = power_data["heroes"].as_array().unwrap();
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
        // Chain: mission → heroes → team + mission → villains → archenemy_of
        // L1: Mission, L2: Hero (reverse M2M) + Villain (reverse M2M),
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
                    "description": "Dual chain mission",
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
                    "secret_identity": "Mission Member",
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
                    "secret_identity": "Mission Villain Dual",
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
                    "secret_identity": "Nemesis of Villain",
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
                &format!(
                    r#"query {{
                        mission(where: {{ id: "{}" }}) {{
                            code
                            heroes {{ alias team {{ name }} }}
                            villains {{ alias archenemy_of {{ alias }} }}
                        }}
                    }}"#,
                    mission_oid.to_hex()
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "mission→heroes→team + villains→archenemy_of: {:?}",
                result.get("errors")
            );

            let mission_data = &result["data"]["mission"];
            assert_eq!(mission_data["code"].as_str().unwrap(), mission_code);

            let heroes = mission_data["heroes"].as_array().unwrap();
            assert_eq!(heroes.len(), 1);
            assert_eq!(
                heroes[0]["alias"].as_str().unwrap(),
                mission_hero_alias
            );
            assert_eq!(heroes[0]["team"]["name"].as_str().unwrap(), team_name);

            let villains = mission_data["villains"].as_array().unwrap();
            assert_eq!(villains.len(), 1);
            assert_eq!(villains[0]["alias"].as_str().unwrap(), villain_alias);
            assert_eq!(
                villains[0]["archenemy_of"]["alias"].as_str().unwrap(),
                nemesis_alias
            );
        }
    }
}
