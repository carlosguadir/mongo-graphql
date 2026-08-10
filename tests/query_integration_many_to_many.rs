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
    async fn query_many_to_many_relations() {
        let db = get_db().await;
        let schema = get_schema().await;

        // ── Forward ManyToMany: hero { missions { code } } ──
        let hero_oid = ObjectId::new();
        let hero_alias: String = Name().fake();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_oid,
                "alias": &hero_alias,
                "secret_identity": "M2M Hero",
                "power_level": 700,
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
                "code": &mission_code,
                "description": "A test mission",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 5,
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
                r#"query {{ hero(where: {{ id: "{}" }}) {{ alias missions {{ code }} }} }}"#,
                hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "Forward ManyToMany: {:?}",
            result.get("errors")
        );
        let missions = result["data"]["hero"]["missions"]
            .as_array()
            .expect("missions should be an array");
        assert!(!missions.is_empty(), "missions should not be empty");
        assert_eq!(missions[0]["code"].as_str().unwrap(), mission_code);

        // ── Forward ManyToMany empty: hero with no missions → empty list ──
        let empty_hero_oid = ObjectId::new();
        let empty_hero_alias: String = Name().fake();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": empty_hero_oid,
                "alias": &empty_hero_alias,
                "secret_identity": "No Missions Hero",
                "power_level": 100,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ alias missions {{ code }} }} }}"#,
                empty_hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none());
        let missions = result["data"]["hero"]["missions"]
            .as_array()
            .expect("missions should be an array");
        assert!(missions.is_empty(), "missions should be empty");

        // ── Reverse ManyToMany: mission { heroes { alias } } ──
        let rev_mission_oid = ObjectId::new();
        let rev_mission_code: String = Name().fake();
        db.collection::<mongodb::bson::Document>("mission")
            .insert_one(doc! {
                "_id": rev_mission_oid,
                "code": &rev_mission_code,
                "description": "Reverse M2M mission",
                "date": mongodb::bson::DateTime::now(),
                "status": "active",
                "danger_level": 3,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let rev_hero_alias: String = Name().fake();
        let rev_hero_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": rev_hero_oid,
                "alias": &rev_hero_alias,
                "secret_identity": "Reverse M2M Hero",
                "power_level": 400,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        db.collection::<mongodb::bson::Document>("hero_mission")
            .insert_one(doc! {
                "hero_id": rev_hero_oid,
                "mission_id": rev_mission_oid,
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ mission(where: {{ id: "{}" }}) {{ code heroes {{ alias }} }} }}"#,
                rev_mission_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "Reverse ManyToMany: {:?}",
            result.get("errors")
        );
        let heroes = result["data"]["mission"]["heroes"]
            .as_array()
            .expect("heroes should be an array");
        assert!(!heroes.is_empty(), "heroes should not be empty");
        assert_eq!(heroes[0]["alias"].as_str().unwrap(), rev_hero_alias);
    }
}
