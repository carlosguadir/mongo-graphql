#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::oid::ObjectId;

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn one_to_one_relation_mutations() {
        let db = get_db().await;
        let schema = get_schema().await;

        let prefix = format!("OTO_{}", ObjectId::new().to_hex());

        // ── OneToOne connect ──
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

        let connect_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_Connect",
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

        if let Some(errors) = connect_result.get("errors") {
            panic!("OneToOne connect failed: {:?}", errors);
        }

        let hero_coll = db.collection::<mongodb::bson::Document>("hero");
        let hero_doc = hero_coll
            .find_one(mongodb::bson::doc! { "alias": format!("{}_Connect", prefix) })
            .await
            .unwrap()
            .expect("OneToOne hero should exist");
        assert_eq!(
            hero_doc.get_object_id("secret_lair_id").unwrap(),
            lair_oid,
            "secret_lair_id should match connected lair"
        );

        // ── OneToOne nested create ──
        let create_result = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createHero(input: {{
                        alias: "{}_NestedCreate",
                        secret_identity: "Nested OneToOne",
                        power_level: 600,
                        active: true,
                        joined_at: "2024-01-01T00:00:00Z",
                        created_at: "2024-01-01T00:00:00Z",
                        secret_lair: {{ create: {{
                            name: "{}_NestedLair",
                            location: "Deep Underground",
                            is_underground: true,
                            created_at: "2024-01-01T00:00:00Z"
                        }} }}
                    }}) {{ id }}
                }}"#,
                prefix, prefix
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = create_result.get("errors") {
            panic!("OneToOne nested create failed: {:?}", errors);
        }

        let _id = create_result["data"]["createHero"]["id"].as_str().unwrap();
        let create_doc = hero_coll
            .find_one(mongodb::bson::doc! { "alias": format!("{}_NestedCreate", prefix) })
            .await
            .unwrap()
            .expect("OneToOne nested create hero should exist");
        let nested_lair_oid = create_doc
            .get_object_id("secret_lair_id")
            .expect("secret_lair_id should be set after nested create");

        let created_lair = db
            .collection::<mongodb::bson::Document>("secret_lair")
            .find_one(mongodb::bson::doc! { "_id": nested_lair_oid })
            .await
            .unwrap()
            .expect("nested lair should exist in DB");
        assert_eq!(
            created_lair.get_str("name").unwrap(),
            format!("{}_NestedLair", prefix),
            "nested lair name should match"
        );

        // ── Forward: update Hero — disconnect secret_lair ──
        let disconn_lair_oid = ObjectId::new();
        let disconn_hero_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("secret_lair")
            .insert_one(mongodb::bson::doc! {
                "_id": disconn_lair_oid,
                "id": disconn_lair_oid,
                "name": format!("{}_DiscoLair", prefix),
                "location": "Forward Disconnect",
                "is_underground": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": disconn_hero_oid,
                "id": disconn_hero_oid,
                "alias": format!("{}_DiscoHero", prefix),
                "secret_identity": "Forward Disconnect",
                "power_level": 50,
                "active": true,
                "secret_lair_id": disconn_lair_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let fwd_disconnect = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(
                        where: {{ id: "{}" }},
                        input: {{ secret_lair: {{ disconnect: true }} }}
                    ) {{ id }}
                }}"#,
                disconn_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = fwd_disconnect.get("errors") {
            panic!("Forward OneToOne disconnect failed: {:?}", errors);
        }

        let disconn_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": disconn_hero_oid })
            .await
            .unwrap()
            .expect("disconnected hero should exist");
        assert!(
            disconn_hero.get("secret_lair_id").and_then(|v| v.as_object_id()).is_none(),
            "secret_lair_id should be null after forward disconnect"
        );

        // ── Forward: update Hero — delete secret_lair ──
        let del_lair_oid = ObjectId::new();
        let del_hero_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("secret_lair")
            .insert_one(mongodb::bson::doc! {
                "_id": del_lair_oid,
                "id": del_lair_oid,
                "name": format!("{}_DelLair", prefix),
                "location": "Forward Delete",
                "is_underground": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": del_hero_oid,
                "id": del_hero_oid,
                "alias": format!("{}_DelHero", prefix),
                "secret_identity": "Forward Delete",
                "power_level": 25,
                "active": true,
                "secret_lair_id": del_lair_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let fwd_delete = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(
                        where: {{ id: "{}" }},
                        input: {{ secret_lair: {{ delete: true }} }}
                    ) {{ id }}
                }}"#,
                del_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = fwd_delete.get("errors") {
            panic!("Forward OneToOne delete failed: {:?}", errors);
        }

        let deleted_lair = db
            .collection::<mongodb::bson::Document>("secret_lair")
            .find_one(mongodb::bson::doc! { "_id": del_lair_oid })
            .await
            .unwrap();
        assert!(
            deleted_lair.is_none(),
            "lair should be deleted after forward delete"
        );

        // ── Reverse: create Lair with Hero connect ──
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
                    createSecretLair(input: {{
                        name: "{}_RevLair",
                        location: "Reverse Side",
                        is_underground: false,
                        created_at: "2024-01-01T00:00:00Z",
                        hero: {{ connect: {{ id: "{}" }} }}
                    }}) {{ id }}
                }}"#,
                prefix, reverse_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_connect.get("errors") {
            panic!("Reverse OneToOne connect failed: {:?}", errors);
        }

        let lair_oid_rev = ObjectId::parse_str(
            rev_connect["data"]["createSecretLair"]["id"].as_str().unwrap(),
        )
        .unwrap();

        let updated_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": reverse_hero_oid })
            .await
            .unwrap()
            .expect("reverse connect hero should exist");
        assert_eq!(
            updated_hero.get_object_id("secret_lair_id").unwrap(),
            lair_oid_rev,
            "hero.secret_lair_id should match the newly created lair"
        );

        // ── Reverse: create Lair with nested Hero create ──
        let rev_create = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    createSecretLair(input: {{
                        name: "{}_RevNestedLair",
                        location: "Reverse Nested",
                        is_underground: true,
                        created_at: "2024-01-01T00:00:00Z",
                        hero: {{ create: {{
                            alias: "{}_RevNestedHero",
                            secret_identity: "Reverse Nested",
                            power_level: 700,
                            active: true,
                            joined_at: "2024-01-01T00:00:00Z",
                            created_at: "2024-01-01T00:00:00Z"
                        }} }}
                    }}) {{ id }}
                }}"#,
                prefix, prefix
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_create.get("errors") {
            panic!("Reverse OneToOne nested create failed: {:?}", errors);
        }

        let rev_lair_oid = ObjectId::parse_str(
            rev_create["data"]["createSecretLair"]["id"].as_str().unwrap(),
        )
        .unwrap();

        let rev_hero_doc = hero_coll
            .find_one(mongodb::bson::doc! { "alias": format!("{}_RevNestedHero", prefix) })
            .await
            .unwrap()
            .expect("reverse nested create hero should exist");
        assert_eq!(
            rev_hero_doc.get_object_id("secret_lair_id").unwrap(),
            rev_lair_oid,
            "nested hero.secret_lair_id should match the created lair"
        );

        // ── Reverse: update Lair — disconnect Hero ──
        let disconnect_lair_oid = ObjectId::new();
        let disconnect_hero_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("secret_lair")
            .insert_one(mongodb::bson::doc! {
                "_id": disconnect_lair_oid,
                "id": disconnect_lair_oid,
                "name": format!("{}_DisconnectLair", prefix),
                "location": "Disconnect Cave",
                "is_underground": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": disconnect_hero_oid,
                "id": disconnect_hero_oid,
                "alias": format!("{}_DisconnectRevHero", prefix),
                "secret_identity": "Disconnect Rev",
                "power_level": 50,
                "active": true,
                "secret_lair_id": disconnect_lair_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_disconnect = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateSecretLair(
                        where: {{ id: "{}" }},
                        input: {{ hero: {{ disconnect: true }} }}
                    ) {{ id }}
                }}"#,
                disconnect_lair_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_disconnect.get("errors") {
            panic!("Reverse OneToOne disconnect failed: {:?}", errors);
        }

        let disconnected_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": disconnect_hero_oid })
            .await
            .unwrap()
            .expect("disconnected hero should exist");
        assert!(
            disconnected_hero
                .get("secret_lair_id")
                .and_then(|v| v.as_object_id())
                .is_none(),
            "hero.secret_lair_id should be null after reverse disconnect"
        );

        // ── Reverse: update Lair — delete Hero ──
        let delete_lair_oid = ObjectId::new();
        let delete_hero_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("secret_lair")
            .insert_one(mongodb::bson::doc! {
                "_id": delete_lair_oid,
                "id": delete_lair_oid,
                "name": format!("{}_DeleteLair", prefix),
                "location": "Delete Cave",
                "is_underground": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        hero_coll
            .insert_one(mongodb::bson::doc! {
                "_id": delete_hero_oid,
                "id": delete_hero_oid,
                "alias": format!("{}_DeleteRevHero", prefix),
                "secret_identity": "Delete Rev",
                "power_level": 25,
                "active": true,
                "secret_lair_id": delete_lair_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_delete = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateSecretLair(
                        where: {{ id: "{}" }},
                        input: {{ hero: {{ delete: true }} }}
                    ) {{ id }}
                }}"#,
                delete_lair_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        if let Some(errors) = rev_delete.get("errors") {
            panic!("Reverse OneToOne delete failed: {:?}", errors);
        }

        let deleted_hero = hero_coll
            .find_one(mongodb::bson::doc! { "_id": delete_hero_oid })
            .await
            .unwrap();
        assert!(
            deleted_hero.is_none(),
            "hero should be deleted after reverse delete"
        );
    }
}
