#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};
    use mongodb::Database;

    use mongo_graphql::dataloader::DataLoader;
    use mongo_graphql::executor;
    use mongo_graphql::schema::parser::SchemaParser;

    use crate::common::{get_db, get_schema};

    fn build_loader(
        db: &Database,
        definition: &mongo_graphql::schema::definition::SchemaDefinition,
    ) -> DataLoader {
        DataLoader::new(db.clone(), definition.clone())
    }

    fn loader_definition() -> mongo_graphql::schema::definition::SchemaDefinition {
        let json = include_str!("schema-definition.json");
        SchemaParser::from_str(json).expect("schema must be valid")
    }

    /// Verify batch-loaded queries return correct results across all three
    /// relation shapes, and that each request gets an isolated DataLoader.
    #[tokio::test]
    async fn dataloader_batches_and_isolates() {
        let db = get_db().await;
        let schema = get_schema().await;
        let definition = loader_definition();

        let now = mongodb::bson::DateTime::now();
        let hero_coll = db.collection::<mongodb::bson::Document>("hero");
        let mission_coll = db.collection::<mongodb::bson::Document>("mission");
        let junction = db.collection::<mongodb::bson::Document>("hero_mission");
        let team_coll = db.collection::<mongodb::bson::Document>("team");

        let team_id = ObjectId::new();
        team_coll.insert_one(doc! {
            "_id": team_id,
            "name": "DL-Batch-Team",
            "founded_at": &now,
            "is_official": true,
            "created_at": &now,
        }).await.unwrap();

        let prefix = format!("DL-{}", ObjectId::new().to_hex());
        for i in 0..8 {
            let hero_id = ObjectId::new();
            let alias = format!("{}-H{}", prefix, i);
            hero_coll.insert_one(doc! {
                "_id": hero_id,
                "alias": &alias,
                "secret_identity": "DL-Test",
                "power_level": 500,
                "active": true,
                "joined_at": &now,
                "created_at": &now,
                "team_id": team_id,
            }).await.unwrap();

            let mission_id = ObjectId::new();
            mission_coll.insert_one(doc! {
                "_id": mission_id,
                "code": format!("{}-M{}", prefix, i),
                "description": "DL test mission",
                "date": &now,
                "status": "active",
                "danger_level": 3,
                "created_at": &now,
            }).await.unwrap();

            junction.insert_one(doc! {
                "hero_id": hero_id,
                "mission_id": mission_id,
                "role": "operative",
            }).await.unwrap();
        }

        // M2M + forward to-one: heroes → missions + team.
        let loader = build_loader(db, &definition);
        let mut data = async_graphql::Data::default();
        data.insert(loader.clone());

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ heroes(first: 10, where: {{ alias: {{ startsWith: "{}" }} }}) {{ edges {{ alias missions {{ code }} team {{ name }} }} totalCount }} }}"#,
                prefix
            ),
            async_graphql::Variables::default(),
            None,
            Some(data),
        )
        .await
        .unwrap();

        assert!(
            result.get("errors").is_none(),
            "M2M + fwd to-one: {:?}",
            result.get("errors")
        );
        let conn = &result["data"]["heroes"];
        assert_eq!(conn["totalCount"].as_i64().unwrap(), 8);
        for edge in conn["edges"].as_array().unwrap() {
            assert_eq!(edge["missions"].as_array().unwrap().len(), 1);
            assert_eq!(edge["team"]["name"].as_str().unwrap(), "DL-Batch-Team");
        }

        let count = loader.batch_execution_count();
        assert!(
            count >= 1 && count <= 5,
            "expected 1-5 batches, got {} (N+1 would be 24+)",
            count
        );

        // Reverse to-many: team → members.
        let loader2 = build_loader(db, &definition);
        let mut data2 = async_graphql::Data::default();
        data2.insert(loader2.clone());

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ team(where: {{ id: "{}" }}) {{ name members {{ alias }} }} }}"#,
                team_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            Some(data2),
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none(), "Rev to-many: {:?}", result.get("errors"));
        let members = result["data"]["team"]["members"].as_array().unwrap();
        assert_eq!(members.len(), 8);
        let rev_count = loader2.batch_execution_count();
        assert_eq!(rev_count, 1, "reverse to-many should be 1 batch, got {}", rev_count);

        // Per-request isolation: fresh loader starts with empty cache.
        let loader3 = build_loader(db, &definition);
        let mut data3 = async_graphql::Data::default();
        data3.insert(loader3.clone());

        let result = executor::execute(
            schema,
            &format!(
                r#"query {{ team(where: {{ id: "{}" }}) {{ name members {{ alias }} }} }}"#,
                team_id.to_hex()
            ),
            async_graphql::Variables::default(),
            None,
            Some(data3),
        )
        .await
        .unwrap();

        assert!(result.get("errors").is_none());
        assert_eq!(loader3.batch_execution_count(), 1);
    }
}
