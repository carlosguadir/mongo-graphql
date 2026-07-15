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
    async fn query_one_to_many_relations() {
        let db = get_db().await;
        let schema = get_schema().await;

        // ── Forward to-one: hero { team { name } } ──
        let team_oid = ObjectId::new();
        let team_name: String = Name().fake();
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

        let hero_alias: String = Name().fake();
        let hero_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_oid,
                "id": hero_oid,
                "alias": &hero_alias,
                "secret_identity": "Team Hero",
                "power_level": 500,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
                "team_id": team_oid,
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ hero(where: {{ id: "{}" }}) {{ id alias team {{ id name }} }} }}"#,
                hero_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "Forward to-one: {:?}",
            result.get("errors")
        );
        let hero_data = &result["data"]["hero"];
        assert_eq!(hero_data["alias"].as_str().unwrap(), hero_alias);
        let team_data = &hero_data["team"];
        assert!(!team_data.is_null(), "Team should not be null");
        assert_eq!(team_data["name"].as_str().unwrap(), team_name);
        assert_eq!(team_data["id"].as_str().unwrap(), team_oid.to_hex());

        // ── Forward to-one null: hero without team → team is null ──
        let solo_alias: String = Name().fake();
        let solo_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": solo_oid,
                "id": solo_oid,
                "alias": &solo_alias,
                "secret_identity": "No Team Hero",
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
                r#"query {{ hero(where: {{ id: "{}" }}) {{ alias team {{ name }} }} }}"#,
                solo_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none());
        assert!(result["data"]["hero"]["team"].is_null());

        // ── Reverse to-many: team { members { alias } } ──
        let members_team_oid = ObjectId::new();
        let members_team_name: String = Name().fake();
        db.collection::<mongodb::bson::Document>("team")
            .insert_one(doc! {
                "_id": members_team_oid,
                "id": members_team_oid,
                "name": &members_team_name,
                "founded_at": mongodb::bson::DateTime::now(),
                "is_official": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let member_alias: String = Name().fake();
        let member_oid = ObjectId::new();
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": member_oid,
                "id": member_oid,
                "alias": &member_alias,
                "secret_identity": "Member Hero",
                "power_level": 600,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
                "team_id": members_team_oid,
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ team(where: {{ id: "{}" }}) {{ id name members {{ alias }} }} }}"#,
                members_team_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "Reverse to-many: {:?}",
            result.get("errors")
        );
        let members = result["data"]["team"]["members"]
            .as_array()
            .expect("members should be an array");
        assert!(!members.is_empty());
        assert_eq!(members[0]["alias"].as_str().unwrap(), member_alias);

        // ── Reverse to-many empty: team with no members → empty array ──
        let empty_team_oid = ObjectId::new();
        let empty_team_name: String = Name().fake();
        db.collection::<mongodb::bson::Document>("team")
            .insert_one(doc! {
                "_id": empty_team_oid,
                "id": empty_team_oid,
                "name": &empty_team_name,
                "founded_at": mongodb::bson::DateTime::now(),
                "is_official": true,
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ team(where: {{ id: "{}" }}) {{ name members {{ alias }} }} }}"#,
                empty_team_oid.to_hex()
            ),
            async_graphql::Variables::default(),
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none());
        let members = result["data"]["team"]["members"]
            .as_array()
            .expect("members should be an array");
        assert!(members.is_empty(), "members should be empty");
    }
}
