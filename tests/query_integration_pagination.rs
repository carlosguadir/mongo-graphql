#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongodb::bson::{doc, oid::ObjectId};

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    #[tokio::test]
    async fn test_pagination() {
        let db = get_db().await;
        let schema = get_schema().await;

        let hero = db.collection::<mongodb::bson::Document>("hero");

        let total_items: i64 = 10;
        let page_size: i64 = 3;
        let prefix = format!("IntTestPage{}", ObjectId::new().to_hex());

        for i in 0..total_items {
            let oid = ObjectId::new();
            hero.insert_one(doc! {
                "_id": oid,
                "id": oid,
                "alias": format!("{}_{}", prefix, i),
                "secret_identity": format!("Secret{}", i),
                "power_level": 1000 + i * 100,
                "active": true,
                "joined_at": mongodb::bson::DateTime::now(),
                "created_at": mongodb::bson::DateTime::now(),
            })
            .await
            .unwrap();
        }

        let total_pages = (total_items + page_size - 1) / page_size; // ceil division
        let mut cursor: Option<String> = None;

        for page_num in 0..total_pages {
            let after_clause = cursor
                .as_ref()
                .map(|c| format!(r#", after: "{}""#, c))
                .unwrap_or_default();

            let query = format!(
                r#"query {{ heroes(first: {}, {}where: {{ alias: {{ startsWith: "{}" }} }}) {{ edges {{ alias }} pageInfo {{ hasNextPage hasPreviousPage endCursor }} totalCount }} }}"#,
                page_size, after_clause, prefix
            );

            let result = executor::execute(schema, &query, async_graphql::Variables::default(), None, None)
                .await
                .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Page {} errors: {:?}", page_num, errors);
            }

            let page = &result["data"]["heroes"];
            let edges = page["edges"].as_array().unwrap();

            let expected_in_page = if page_num < total_pages - 1 {
                page_size
            } else {
                total_items - page_num * page_size
            };

            assert_eq!(
                edges.len() as i64,
                expected_in_page,
                "Page {}: expected {} items, got {}",
                page_num,
                expected_in_page,
                edges.len()
            );

            let start_index = (page_num * page_size) as usize;
            for (offset, edge) in edges.iter().enumerate() {
                let expected_alias = format!("{}_{}", prefix, start_index + offset);
                assert_eq!(
                    edge["alias"].as_str().unwrap(),
                    expected_alias,
                    "Page {}: alias mismatch at offset {}",
                    page_num,
                    offset
                );
            }

            let is_last_page = page_num == total_pages - 1;
            let is_first_page = page_num == 0;
            assert_eq!(
                page["pageInfo"]["hasNextPage"].as_bool().unwrap(),
                !is_last_page,
                "Page {}: hasNextPage",
                page_num
            );
            assert_eq!(
                page["pageInfo"]["hasPreviousPage"].as_bool().unwrap(),
                !is_first_page,
                "Page {}: hasPreviousPage",
                page_num
            );
            assert_eq!(
                page["totalCount"].as_i64().unwrap(),
                total_items as i64,
                "Page {}: totalCount",
                page_num
            );

            // Advance cursor for next page
            cursor = page["pageInfo"]["endCursor"].as_str().map(|s| s.to_string());
            if is_last_page {
                assert!(cursor.is_some(), "Last page should still have endCursor");
            }
        }
    }
}
