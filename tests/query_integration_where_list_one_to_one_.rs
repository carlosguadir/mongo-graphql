#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_filter_one_to_one() {
        let db = get_db().await;
        db.drop().await.unwrap();
        let schema = get_schema().await;

        let hero_oid = ObjectId::new();
        let villain_oid = ObjectId::new();

        // Villain
        db.collection::<mongodb::bson::Document>("villain")
            .insert_one(doc! {
                "_id": villain_oid, "id": villain_oid,
                "alias": "TestVillain", "secret_identity": "Bad Guy",
                "threat_level": 10, "active": true,
                "rank": "ALPHA",
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        // Hero with FK to villain (archenemy)
        db.collection::<mongodb::bson::Document>("hero")
            .insert_one(doc! {
                "_id": hero_oid, "id": hero_oid,
                "alias": "OneToOneHero", "secret_identity": "Test",
                "power_level": 100, "active": true,
                "archenemy_id": villain_oid,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();

        // ---- test 1: matching archenemy alias → finds hero ----
        {
            let query = r#"query { heroes(where: { archenemy: { alias: { contains: "TestVillain" } } }) { edges { id alias } } }"#;
            let result = executor::execute(
                schema, query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();

            assert!(
                result.get("errors").is_none(),
                "test 1a failed: {:?}",
                result.get("errors")
            );

            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 1, "test 1a: should return exactly one hero");
            let hero = &edges[0];
            assert_eq!(hero["id"].as_str().unwrap(), hero_oid.to_hex());
        }

        // ---- test 2: non-matching archenemy alias → empty ----
        {
            let query = r#"query { heroes(where: { archenemy: { alias: { eq: "NonExistentVillain" } } }) { edges { id } } }"#;
            let result = executor::execute(
                schema, query, async_graphql::Variables::default(), None, None,
            )
            .await
            .unwrap();

            assert!(result.get("errors").is_none(), "test 2 failed: {:?}", result.get("errors"));
            let edges = result["data"]["heroes"]["edges"].as_array().unwrap();
            assert_eq!(edges.len(), 0, "test 2: should return no heroes; got {:?}", edges);
        }
    }
}
