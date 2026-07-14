#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::oid::ObjectId;

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn many_to_many_relation_mutations() {
        let db = get_db().await;
        let schema = get_schema().await;

        let prefix = format!("MTM_{}", ObjectId::new().to_hex());

        // ── ManyToMany connect ──
        let mission_oid = ObjectId::new();
        let mission_code = format!("{}_Connect", prefix);
        let mission_coll = db.collection::<mongodb::bson::Document>("mission");
        mission_coll
            .insert_one(mongodb::bson::doc! {
                "_id": mission_oid,
                "id": mission_oid,
                "code": &mission_code,
                "description": "Test mission for ManyToMany connect",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 5,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let connect_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_Connect",
                        secret_identity: "M2M Tester",
                        power_level: 900,
                        active: true,
                        joined_at: "2024-01-01T00:00:00Z",
                        created_at: "2024-01-01T00:00:00Z",
                        missions: {{ connect: [{{ id: "{}" }}] }}
                    }}) {{ id }}
                }}"#,
                prefix, mission_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = connect_result.get("errors") {
            panic!("ManyToMany connect failed: {:?}", errors);
        }

        let junction_coll = db.collection::<mongodb::bson::Document>("hero_mission");
        let hero_oid =
            ObjectId::parse_str(connect_result["data"]["createHero"]["id"].as_str().unwrap())
                .unwrap();
        let junction_doc = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": hero_oid,
                "mission_id": mission_oid
            })
            .await
            .unwrap();
        assert!(
            junction_doc.is_some(),
            "Junction record should exist after ManyToMany connect"
        );

        // ── ManyToMany create ──
        let create_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_Create",
                        secret_identity: "M2M Creator",
                        power_level: 950,
                        active: true,
                        joined_at: "2024-01-01T00:00:00Z",
                        created_at: "2024-01-01T00:00:00Z",
                        missions: {{ create: [{{
                            code: "{}_Nested",
                            description: "Mission created via nested create",
                            date: "2024-01-01T00:00:00Z",
                            status: "active",
                            danger_level: 3,
                            created_at: "2024-01-01T00:00:00Z"
                        }}] }}
                    }}) {{ id }}
                }}"#,
                prefix, prefix
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = create_result.get("errors") {
            panic!("ManyToMany create failed: {:?}", errors);
        }

        let created_mission = mission_coll
            .find_one(mongodb::bson::doc! { "code": format!("{}_Nested", prefix) })
            .await
            .unwrap()
            .expect("nested mission should exist in DB");
        let created_mission_oid = created_mission.get_object_id("_id").unwrap();
        let create_hero_oid =
            ObjectId::parse_str(create_result["data"]["createHero"]["id"].as_str().unwrap())
                .unwrap();
        let nested_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": create_hero_oid,
                "mission_id": created_mission_oid
            })
            .await
            .unwrap();
        assert!(
            nested_junction.is_some(),
            "Junction record should exist after ManyToMany create"
        );

        // ── ManyToMany disconnect ──
        let disconnect_mission_oid = ObjectId::new();
        mission_coll
            .insert_one(mongodb::bson::doc! {
                "_id": disconnect_mission_oid,
                "id": disconnect_mission_oid,
                "code": format!("{}_Disconnect", prefix),
                "description": "Mission to disconnect",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 2,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let hero_coll = db.collection::<mongodb::bson::Document>("hero");
        let disconnect_hero_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": disconnect_hero_oid,
                "id": disconnect_hero_oid,
                "alias": format!("{}_Disconnect", prefix),
                "secret_identity": "M2M Disconnector",
                "power_level": 1000,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        junction_coll
            .insert_one(mongodb::bson::doc! {
                "hero_id": disconnect_hero_oid,
                "mission_id": disconnect_mission_oid,
            })
            .await
            .unwrap();

        let disconnect_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(
                        where: {{ id: "{}" }},
                        input: {{ missions: {{ disconnect: ["{}"] }} }}
                    ) {{ id }}
                }}"#,
                disconnect_hero_oid.to_hex(),
                disconnect_mission_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = disconnect_result.get("errors") {
            panic!("ManyToMany disconnect failed: {:?}", errors);
        }

        let disconnected_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": disconnect_hero_oid,
                "mission_id": disconnect_mission_oid
            })
            .await
            .unwrap();
        assert!(
            disconnected_junction.is_none(),
            "Junction record should be deleted after disconnect"
        );
        let still_mission = mission_coll
            .find_one(mongodb::bson::doc! { "_id": disconnect_mission_oid })
            .await
            .unwrap();
        assert!(
            still_mission.is_some(),
            "Mission should still exist after disconnect"
        );
        let still_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": disconnect_hero_oid })
            .await
            .unwrap();
        assert!(
            still_hero.is_some(),
            "Hero should still exist after disconnect"
        );

        // ── ManyToMany delete ──
        let delete_mission_oid = ObjectId::new();
        mission_coll
            .insert_one(mongodb::bson::doc! {
                "_id": delete_mission_oid,
                "id": delete_mission_oid,
                "code": format!("{}_Delete", prefix),
                "description": "Mission to delete",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 1,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let delete_hero_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": delete_hero_oid,
                "id": delete_hero_oid,
                "alias": format!("{}_Delete", prefix),
                "secret_identity": "M2M Deleter",
                "power_level": 1100,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        junction_coll
            .insert_one(mongodb::bson::doc! {
                "hero_id": delete_hero_oid,
                "mission_id": delete_mission_oid,
            })
            .await
            .unwrap();

        let delete_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(
                        where: {{ id: "{}" }},
                        input: {{ missions: {{ delete: ["{}"] }} }}
                    ) {{ id }}
                }}"#,
                delete_hero_oid.to_hex(),
                delete_mission_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = delete_result.get("errors") {
            panic!("ManyToMany delete failed: {:?}", errors);
        }

        let deleted_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": delete_hero_oid,
                "mission_id": delete_mission_oid
            })
            .await
            .unwrap();
        assert!(
            deleted_junction.is_none(),
            "Junction record should be deleted after delete"
        );
        let deleted_mission = mission_coll
            .find_one(mongodb::bson::doc! { "_id": delete_mission_oid })
            .await
            .unwrap();
        assert!(
            deleted_mission.is_none(),
            "Mission should be deleted from DB after nested delete"
        );
        let still_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": delete_hero_oid })
            .await
            .unwrap();
        assert!(
            still_hero.is_some(),
            "Hero should still exist after delete"
        );

        // ── Reverse: create Mission with heroes connect ──
        let reverse_hero_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": reverse_hero_oid,
                "id": reverse_hero_oid,
                "alias": format!("{}_RevConnect", prefix),
                "secret_identity": "Reverse Connect Hero",
                "power_level": 100,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_connect = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createMission(input: {{
                        code: "{}_RevConnect",
                        description: "Reverse ManyToMany connect",
                        date: "2024-01-01T00:00:00Z",
                        status: "active",
                        danger_level: 5,
                        created_at: "2024-01-01T00:00:00Z",
                        heroes: {{ connect: [{{ id: "{}" }}] }}
                    }}) {{ id }}
                }}"#,
                prefix, reverse_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_connect.get("errors") {
            panic!("Reverse ManyToMany connect failed: {:?}", errors);
        }

        let rev_mission_oid = ObjectId::parse_str(
            rev_connect["data"]["createMission"]["id"].as_str().unwrap(),
        )
        .unwrap();
        let rev_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": reverse_hero_oid,
                "mission_id": rev_mission_oid
            })
            .await
            .unwrap();
        assert!(
            rev_junction.is_some(),
            "Junction record should exist after reverse connect"
        );

        // ── Reverse: create Mission with nested heroes create ──
        let rev_create = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createMission(input: {{
                        code: "{}_RevCreate",
                        description: "Reverse ManyToMany create",
                        date: "2024-01-01T00:00:00Z",
                        status: "active",
                        danger_level: 3,
                        created_at: "2024-01-01T00:00:00Z",
                        heroes: {{ create: [{{
                            alias: "{}_RevCreate",
                            secret_identity: "Reverse Created Hero",
                            power_level: 700,
                            active: true,
                            joined_at: "2024-01-01T00:00:00Z",
                            created_at: "2024-01-01T00:00:00Z"
                        }}] }}
                    }}) {{ id }}
                }}"#,
                prefix, prefix
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_create.get("errors") {
            panic!("Reverse ManyToMany create failed: {:?}", errors);
        }

        let rev_create_mission_oid = ObjectId::parse_str(
            rev_create["data"]["createMission"]["id"].as_str().unwrap(),
        )
        .unwrap();
        let rev_created_hero = hero_coll
            .find_one(mongodb::bson::doc! { "alias": format!("{}_RevCreate", prefix) })
            .await
            .unwrap()
            .expect("reverse created hero should exist");
        let rev_created_hero_oid = rev_created_hero.get_object_id("_id").unwrap();
        let rev_create_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": rev_created_hero_oid,
                "mission_id": rev_create_mission_oid
            })
            .await
            .unwrap();
        assert!(
            rev_create_junction.is_some(),
            "Junction record should exist after reverse create"
        );

        // ── Reverse: update Mission — disconnect heroes ──
        let rev_disconnect_mission_oid = ObjectId::new();
        mission_coll
            .insert_one(mongodb::bson::doc! {
                "_id": rev_disconnect_mission_oid,
                "id": rev_disconnect_mission_oid,
                "code": format!("{}_RevDisconnect", prefix),
                "description": "Mission for reverse disconnect",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 2,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_disconnect_hero_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": rev_disconnect_hero_oid,
                "id": rev_disconnect_hero_oid,
                "alias": format!("{}_RevDisconnect", prefix),
                "secret_identity": "Reverse Disconnect Hero",
                "power_level": 50,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        junction_coll
            .insert_one(mongodb::bson::doc! {
                "hero_id": rev_disconnect_hero_oid,
                "mission_id": rev_disconnect_mission_oid,
            })
            .await
            .unwrap();

        let rev_disconnect_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateMission(
                        where: {{ id: "{}" }},
                        input: {{ heroes: {{ disconnect: ["{}"] }} }}
                    ) {{ id }}
                }}"#,
                rev_disconnect_mission_oid.to_hex(),
                rev_disconnect_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_disconnect_result.get("errors") {
            panic!("Reverse ManyToMany disconnect failed: {:?}", errors);
        }

        let rev_disconnected_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": rev_disconnect_hero_oid,
                "mission_id": rev_disconnect_mission_oid
            })
            .await
            .unwrap();
        assert!(
            rev_disconnected_junction.is_none(),
            "Junction record should be deleted after reverse disconnect"
        );
        let rev_disconnect_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": rev_disconnect_hero_oid })
            .await
            .unwrap();
        assert!(
            rev_disconnect_hero.is_some(),
            "Hero should still exist after reverse disconnect"
        );
        let rev_disconnect_mission = mission_coll
            .find_one(mongodb::bson::doc! { "_id": rev_disconnect_mission_oid })
            .await
            .unwrap();
        assert!(
            rev_disconnect_mission.is_some(),
            "Mission should still exist after reverse disconnect"
        );

        // ── Reverse: update Mission — delete heroes ──
        let rev_delete_mission_oid = ObjectId::new();
        mission_coll
            .insert_one(mongodb::bson::doc! {
                "_id": rev_delete_mission_oid,
                "id": rev_delete_mission_oid,
                "code": format!("{}_RevDelete", prefix),
                "description": "Mission for reverse delete",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 1,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_delete_hero_oid = ObjectId::new();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": rev_delete_hero_oid,
                "id": rev_delete_hero_oid,
                "alias": format!("{}_RevDelete", prefix),
                "secret_identity": "Reverse Delete Hero",
                "power_level": 25,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        junction_coll
            .insert_one(mongodb::bson::doc! {
                "hero_id": rev_delete_hero_oid,
                "mission_id": rev_delete_mission_oid,
            })
            .await
            .unwrap();

        let rev_delete_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateMission(
                        where: {{ id: "{}" }},
                        input: {{ heroes: {{ delete: ["{}"] }} }}
                    ) {{ id }}
                }}"#,
                rev_delete_mission_oid.to_hex(),
                rev_delete_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_delete_result.get("errors") {
            panic!("Reverse ManyToMany delete failed: {:?}", errors);
        }

        let rev_deleted_junction = junction_coll
            .find_one(mongodb::bson::doc! {
                "hero_id": rev_delete_hero_oid,
                "mission_id": rev_delete_mission_oid
            })
            .await
            .unwrap();
        assert!(
            rev_deleted_junction.is_none(),
            "Junction record should be deleted after reverse delete"
        );
        let rev_deleted_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": rev_delete_hero_oid })
            .await
            .unwrap();
        assert!(
            rev_deleted_hero.is_none(),
            "Hero should be deleted from DB after reverse delete"
        );
        let rev_deleted_mission = mission_coll
            .find_one(mongodb::bson::doc! { "_id": rev_delete_mission_oid })
            .await
            .unwrap();
        assert!(
            rev_deleted_mission.is_some(),
            "Mission should still exist after reverse delete"
        );
    }
}
