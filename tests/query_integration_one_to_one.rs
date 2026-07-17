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
    async fn query_one_to_one_relations() {
        let db = get_db().await;
        let schema = get_schema().await;

        // ── Forward to-one: hero { archenemy { alias } } ──
        let villain_oid = ObjectId::new();
        let villain_alias: String = Name().fake();
        db.collection::<mongodb::bson::Document>("villain")
            .insert_one(doc! {
                "_id": villain_oid,
                "id": villain_oid,
                "alias": &villain_alias,
                "secret_identity": "Forward Villain",
                "threat_level": 90,
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
                "secret_identity": "Nemesis Hero",
                "power_level": 800,
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
                r#"query {{ hero(where: {{ id: "{}" }}) {{ alias archenemy {{ id alias }} }} }}"#,
                hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "Forward OneToOne: {:?}",
            result.get("errors")
        );
        let hero_data = &result["data"]["hero"];
        assert_eq!(hero_data["alias"].as_str().unwrap(), hero_alias);
        let archenemy = &hero_data["archenemy"];
        assert!(!archenemy.is_null(), "Archenemy should not be null");
        assert_eq!(archenemy["alias"].as_str().unwrap(), villain_alias);

        // ── Forward OneToOne null: hero without archenemy → null ──
        let solo_alias: String = Name().fake();
        let solo_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": solo_oid,
                "id": solo_oid,
                "alias": &solo_alias,
                "secret_identity": "No Nemesis",
                "power_level": 300,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ alias archenemy {{ alias }} }} }}"#,
                solo_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none());
        assert!(result["data"]["hero"]["archenemy"].is_null());

        // ──  OneToOne: villain { archenemy_of { alias } } ──
        let rev_villain_oid = ObjectId::new();
        let rev_villain_alias: String = Name().fake();
        db.collection::<mongodb::bson::Document>("villain")
            .insert_one(doc! {
                "_id": rev_villain_oid,
                "id": rev_villain_oid,
                "alias": &rev_villain_alias,
                "secret_identity": "Reverse Villain",
                "threat_level": 75,
                "active": true,
                "created_at": mongodb::bson::DateTime::now(),
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
                "secret_identity": "Reverse Nemesis",
                "power_level": 700,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
                "archenemy_id": rev_villain_oid,
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ villain(where: {{ id: "{}" }}) {{ alias archenemy_of {{ alias }} }} }}"#,
                rev_villain_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "Reverse OneToOne: {:?}",
            result.get("errors")
        );
        let villain_data = &result["data"]["villain"];
        assert_eq!(villain_data["alias"].as_str().unwrap(), rev_villain_alias);
        let archenemy_of = &villain_data["archenemy_of"];
        assert!(!archenemy_of.is_null(), "archenemy_of should not be null");
        assert_eq!(archenemy_of["alias"].as_str().unwrap(), nemesis_alias);

        // ── Reverse OneToOne null: villain with no hero pointing to it → null ──
        let lone_villain_oid = ObjectId::new();
        let lone_villain_alias: String = Name().fake();
        db.collection::<mongodb::bson::Document>("villain")
            .insert_one(doc! {
                "_id": lone_villain_oid,
                "id": lone_villain_oid,
                "alias": &lone_villain_alias,
                "secret_identity": "Lone Villain",
                "threat_level": 50,
                "active": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ villain(where: {{ id: "{}" }}) {{ alias archenemy_of {{ alias }} }} }}"#,
                lone_villain_oid.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none());
        assert!(result["data"]["villain"]["archenemy_of"].is_null());
    }
}
