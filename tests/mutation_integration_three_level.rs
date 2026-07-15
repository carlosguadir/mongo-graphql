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

    fn unique_alias(prefix: &str, label: &str) -> String {
        let word: String = Word().fake();
        let suffix = &ObjectId::new().to_hex()[..6];
        format!("{}_{}_{}_{}", prefix, label, word, suffix)
    }

    fn unique_name(prefix: &str, label: &str) -> String {
        let name: String = Name().fake();
        let suffix = &ObjectId::new().to_hex()[..6];
        format!("{}_{}_{}_{}", prefix, label, name, suffix)
    }

    #[tokio::test]
    async fn three_level_nested_mutations() {
        let db = get_db().await;
        let schema = get_schema().await;

        let prefix = format!("3L_{}", ObjectId::new().to_hex());

        // ═══════════════════════════════════════════════════════════════
        // Chain: Mission → assigned_team.create → members.create
        // L1: Mission, L2: Team (FK assigned_team_id), L3: Hero (FK team_id)
        // ═══════════════════════════════════════════════════════════════
        {
            let mission_code = unique_alias(&prefix, "M_T_members");
            let mission_desc: String = Word().fake();
            let team_name = unique_name(&prefix, "M_T_members");
            let hero_alias = unique_alias(&prefix, "L3Hero");
            let hero_identity: String = Name().fake();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createMission(input: {{
                            code: "{}",
                            description: "{}",
                            date: "2024-01-01T00:00:00Z",
                            status: "active",
                            danger_level: 10,
                            created_at: "2024-01-01T00:00:00Z",
                            assigned_team: {{ create: {{
                                name: "{}",
                                founded_at: "2024-01-01T00:00:00Z",
                                is_official: true,
                                created_at: "2024-01-01T00:00:00Z",
                                members: {{ create: [{{
                                    alias: "{}",
                                    secret_identity: "{}",
                                    power_level: 999,
                                    active: true,
                                    joined_at: "2024-01-01T00:00:00Z",
                                    created_at: "2024-01-01T00:00:00Z"
                                }}] }}
                            }} }}
                        }}) {{ id }}
                    }}"#,
                    mission_code, mission_desc, team_name, hero_alias, hero_identity
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Mission → assigned_team.create → members.create failed: {:?}",
                    errors
                );
            }

            let mission_oid = ObjectId::parse_str(
                result["data"]["createMission"]["id"].as_str().unwrap(),
            )
            .unwrap();

            let mission_coll = db.collection::<mongodb::bson::Document>("mission");
            let mission_doc = mission_coll
                .find_one(mongodb::bson::doc! { "_id": mission_oid })
                .await
                .unwrap()
                .expect("L1 mission should exist");
            let team_oid = mission_doc
                .get_object_id("assigned_team_id")
                .expect("assigned_team_id should be set");

            let team_coll = db.collection::<mongodb::bson::Document>("team");
            let team_doc = team_coll
                .find_one(mongodb::bson::doc! { "_id": team_oid })
                .await
                .unwrap()
                .expect("L2 team should exist");
            assert_eq!(team_doc.get_str("name").unwrap(), team_name);

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let hero_doc = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &hero_alias })
                .await
                .unwrap()
                .expect("L3 hero should exist");
            assert_eq!(
                hero_doc.get_object_id("team_id").unwrap(),
                team_oid,
                "hero.team_id should match L2 team"
            );
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: Mission → assigned_team.create → missions.create
        // L1: Mission, L2: Team (FK assigned_team_id), L3: Mission (FK assigned_team_id)
        // Both missions are siblings assigned to the same team.
        // ═══════════════════════════════════════════════════════════════
        {
            let l1_code = unique_alias(&prefix, "M_T_M_L1");
            let l1_desc: String = Word().fake();
            let team_name = unique_name(&prefix, "M_T_M");
            let l3_code = unique_alias(&prefix, "M_T_M_L3");
            let l3_desc: String = Word().fake();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createMission(input: {{
                            code: "{}",
                            description: "{}",
                            date: "2024-01-01T00:00:00Z",
                            status: "active",
                            danger_level: 10,
                            created_at: "2024-01-01T00:00:00Z",
                            assigned_team: {{ create: {{
                                name: "{}",
                                founded_at: "2024-01-01T00:00:00Z",
                                is_official: true,
                                created_at: "2024-01-01T00:00:00Z",
                                missions: {{ create: [{{
                                    code: "{}",
                                    description: "{}",
                                    date: "2024-01-01T00:00:00Z",
                                    status: "pending",
                                    danger_level: 5,
                                    created_at: "2024-01-01T00:00:00Z"
                                }}] }}
                            }} }}
                        }}) {{ id }}
                    }}"#,
                    l1_code, l1_desc, team_name, l3_code, l3_desc
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Mission → assigned_team.create → missions.create failed: {:?}",
                    errors
                );
            }

            let l1_oid = ObjectId::parse_str(
                result["data"]["createMission"]["id"].as_str().unwrap(),
            )
            .unwrap();

            let mission_coll = db.collection::<mongodb::bson::Document>("mission");
            let l1_doc = mission_coll
                .find_one(mongodb::bson::doc! { "_id": l1_oid })
                .await
                .unwrap()
                .expect("L1 mission should exist");
            let team_oid = l1_doc
                .get_object_id("assigned_team_id")
                .expect("assigned_team_id should be set");

            let team_coll = db.collection::<mongodb::bson::Document>("team");
            let team_doc = team_coll
                .find_one(mongodb::bson::doc! { "_id": team_oid })
                .await
                .unwrap()
                .expect("L2 team should exist");
            assert_eq!(team_doc.get_str("name").unwrap(), team_name);

            let l3_doc = mission_coll
                .find_one(mongodb::bson::doc! { "code": &l3_code })
                .await
                .unwrap()
                .expect("L3 mission should exist");
            assert_eq!(
                l3_doc.get_object_id("assigned_team_id").unwrap(),
                team_oid,
                "L3 mission.assigned_team_id should match L2 team"
            );
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: Hero → team.create → members.create
        // L1: Hero (team_id = L2), L2: Team, L3: Hero (team_id = L2)
        // ═══════════════════════════════════════════════════════════════
        {
            let l1_alias = unique_alias(&prefix, "H_T_H_L1");
            let l1_identity: String = Name().fake();
            let team_name = unique_name(&prefix, "H_T_H");
            let l3_alias = unique_alias(&prefix, "H_T_H_L3");
            let l3_identity: String = Name().fake();

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
                                created_at: "2024-01-01T00:00:00Z",
                                members: {{ create: [{{
                                    alias: "{}",
                                    secret_identity: "{}",
                                    power_level: 600,
                                    active: true,
                                    joined_at: "2024-01-01T00:00:00Z",
                                    created_at: "2024-01-01T00:00:00Z"
                                }}] }}
                            }} }}
                        }}) {{ id }}
                    }}"#,
                    l1_alias, l1_identity, team_name, l3_alias, l3_identity
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Hero → team.create → members.create failed: {:?}",
                    errors
                );
            }

            let l1_oid = ObjectId::parse_str(
                result["data"]["createHero"]["id"].as_str().unwrap(),
            )
            .unwrap();

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let l1_doc = hero_coll
                .find_one(mongodb::bson::doc! { "_id": l1_oid })
                .await
                .unwrap()
                .expect("L1 hero should exist");
            let team_oid = l1_doc
                .get_object_id("team_id")
                .expect("team_id should be set");

            let team_coll = db.collection::<mongodb::bson::Document>("team");
            let team_doc = team_coll
                .find_one(mongodb::bson::doc! { "_id": team_oid })
                .await
                .unwrap()
                .expect("L2 team should exist");
            assert_eq!(team_doc.get_str("name").unwrap(), team_name);

            let l3_doc = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &l3_alias })
                .await
                .unwrap()
                .expect("L3 hero should exist");
            assert_eq!(
                l3_doc.get_object_id("team_id").unwrap(),
                team_oid,
                "L3 hero.team_id should match L2 team"
            );
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: Hero → team.create → missions.create
        // L1: Hero (team_id = L2), L2: Team, L3: Mission (assigned_team_id = L2)
        // ═══════════════════════════════════════════════════════════════
        {
            let l1_alias = unique_alias(&prefix, "H_T_M_L1");
            let l1_identity: String = Name().fake();
            let team_name = unique_name(&prefix, "H_T_M");
            let l3_code = unique_alias(&prefix, "H_T_M_L3");
            let l3_desc: String = Word().fake();

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
                                created_at: "2024-01-01T00:00:00Z",
                                missions: {{ create: [{{
                                    code: "{}",
                                    description: "{}",
                                    date: "2024-01-01T00:00:00Z",
                                    status: "pending",
                                    danger_level: 7,
                                    created_at: "2024-01-01T00:00:00Z"
                                }}] }}
                            }} }}
                        }}) {{ id }}
                    }}"#,
                    l1_alias, l1_identity, team_name, l3_code, l3_desc
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Hero → team.create → missions.create failed: {:?}",
                    errors
                );
            }

            let l1_oid = ObjectId::parse_str(
                result["data"]["createHero"]["id"].as_str().unwrap(),
            )
            .unwrap();

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let l1_doc = hero_coll
                .find_one(mongodb::bson::doc! { "_id": l1_oid })
                .await
                .unwrap()
                .expect("L1 hero should exist");
            let team_oid = l1_doc
                .get_object_id("team_id")
                .expect("team_id should be set");

            let team_coll = db.collection::<mongodb::bson::Document>("team");
            let team_doc = team_coll
                .find_one(mongodb::bson::doc! { "_id": team_oid })
                .await
                .unwrap()
                .expect("L2 team should exist");
            assert_eq!(team_doc.get_str("name").unwrap(), team_name);

            let mission_coll = db.collection::<mongodb::bson::Document>("mission");
            let l3_doc = mission_coll
                .find_one(mongodb::bson::doc! { "code": &l3_code })
                .await
                .unwrap()
                .expect("L3 mission should exist");
            assert_eq!(
                l3_doc.get_object_id("assigned_team_id").unwrap(),
                team_oid,
                "L3 mission.assigned_team_id should match L2 team"
            );
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: Power → heroes.create → archenemy.create
        // L1: Power, L2: Hero (ManyToMany via hero_power), L3: Villain (FK archenemy_id)
        // HeroCreateWithoutPowerInput keeps archenemy as nested type (Villain ≠ Power).
        // ═══════════════════════════════════════════════════════════════
        {
            let power_name = unique_name(&prefix, "P_H_V_create");
            let l2_alias = unique_alias(&prefix, "P_H_V_Hero");
            let l2_identity: String = Name().fake();
            let villain_alias = unique_alias(&prefix, "P_H_V_Villain");
            let villain_identity: String = Name().fake();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createPower(input: {{
                            name: "{}",
                            type: "elemental",
                            level: 1,
                            created_at: "2024-01-01T00:00:00Z",
                            heroes: {{ create: [{{
                                alias: "{}",
                                secret_identity: "{}",
                                power_level: 700,
                                active: true,
                                joined_at: "2024-01-01T00:00:00Z",
                                created_at: "2024-01-01T00:00:00Z",
                                archenemy: {{ create: {{
                                    alias: "{}",
                                    secret_identity: "{}",
                                    threat_level: 90,
                                    active: true,
                                    created_at: "2024-01-01T00:00:00Z"
                                }} }}
                            }}] }}
                        }}) {{ id }}
                    }}"#,
                    power_name, l2_alias, l2_identity, villain_alias, villain_identity
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Power → heroes.create → archenemy.create failed: {:?}",
                    errors
                );
            }

            let power_oid =
                ObjectId::parse_str(result["data"]["createPower"]["id"].as_str().unwrap())
                    .unwrap();

            // Verify hero_power junction
            let junction_coll = db.collection::<mongodb::bson::Document>("hero_power");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let l2_doc = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &l2_alias })
                .await
                .unwrap()
                .expect("L2 hero should exist");
            let junction = junction_coll
                .find_one(mongodb::bson::doc! { "hero_id": l2_doc.get_object_id("_id").unwrap() })
                .await
                .unwrap()
                .expect("hero_power junction should exist");
            assert_eq!(
                junction.get_object_id("power_id").unwrap(),
                power_oid,
                "junction.power_id should match L1 power"
            );

            let villain_coll = db.collection::<mongodb::bson::Document>("villain");
            let l3_doc = villain_coll
                .find_one(mongodb::bson::doc! { "alias": &villain_alias })
                .await
                .unwrap()
                .expect("L3 villain should exist");
            assert_eq!(
                l2_doc.get_object_id("archenemy_id").unwrap(),
                l3_doc.get_object_id("_id").unwrap(),
                "hero.archenemy_id should match L3 villain"
            );
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: Power → heroes.create → archenemy.connect
        // L1: Power, L2: Hero (ManyToMany via hero_power), L3: connect existing Villain
        // ═══════════════════════════════════════════════════════════════
        {
            let villain_oid = ObjectId::new();
            let villain_alias = unique_alias(&prefix, "P_H_V_connect");
            db.collection::<mongodb::bson::Document>("villain")
                .insert_one(mongodb::bson::doc! {
                    "_id": villain_oid,
                    "id": villain_oid,
                    "alias": &villain_alias,
                    "secret_identity": "Pre-existing Nemesis",
                    "threat_level": 95,
                    "active": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let power_name = unique_name(&prefix, "P_H_V_connect");
            let l2_alias = unique_alias(&prefix, "P_H_V_HeroC");
            let l2_identity: String = Name().fake();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createPower(input: {{
                            name: "{}",
                            type: "elemental",
                            level: 1,
                            created_at: "2024-01-01T00:00:00Z",
                            heroes: {{ create: [{{
                                alias: "{}",
                                secret_identity: "{}",
                                power_level: 700,
                                active: true,
                                joined_at: "2024-01-01T00:00:00Z",
                                created_at: "2024-01-01T00:00:00Z",
                                archenemy: {{ connect: {{ id: "{}" }} }}
                            }}] }}
                        }}) {{ id }}
                    }}"#,
                    power_name, l2_alias, l2_identity, villain_oid.to_hex()
                ),
                async_graphql::Variables::default(),
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Power → heroes.create → archenemy.connect failed: {:?}",
                    errors
                );
            }

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let l2_doc = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &l2_alias })
                .await
                .unwrap()
                .expect("L2 hero should exist");
            assert_eq!(
                l2_doc.get_object_id("archenemy_id").unwrap(),
                villain_oid,
                "hero.archenemy_id should match connected villain"
            );
        }
    }
}
