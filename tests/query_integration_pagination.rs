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
                .map(|c| format!(r#"after: "{}""#, c))
                .unwrap_or_default();
            let args = if after_clause.is_empty() {
                format!("first: {}", page_size)
            } else {
                format!("first: {}, {}", page_size, after_clause)
            };

            let query = format!(
                r#"query {{ heroes({}, where: {{ alias: {{ startsWith: "{}" }} }}) {{ edges {{ alias }} pageInfo {{ hasNextPage hasPreviousPage startCursor endCursor }} totalCount }} }}"#,
                args, prefix
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

            // startCursor / endCursor: both present, startCursor != endCursor
            // when the page has more than one item.
            let start_cursor = page["pageInfo"]["startCursor"].as_str().unwrap();
            let end_cursor = page["pageInfo"]["endCursor"].as_str().unwrap();
            assert!(!start_cursor.is_empty(), "Page {}: startCursor empty", page_num);
            assert!(!end_cursor.is_empty(), "Page {}: endCursor empty", page_num);
            if edges.len() > 1 {
                assert_ne!(
                    start_cursor, end_cursor,
                    "Page {}: startCursor equals endCursor on multi-item page",
                    page_num
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

    /// Backward pagination with `last` + `before`: page from the end of the
    /// result set toward the beginning. Covers the `is_backward` branch of
    /// `resolve_list` — reversed sort, `_id: $lt` filter, and edge reversal.
    #[tokio::test]
    async fn test_backward_pagination() {
        let db = get_db().await;
        let schema = get_schema().await;

        let hero = db.collection::<mongodb::bson::Document>("hero");

        let total_items: i64 = 10;
        let page_size: i64 = 3;
        let prefix = format!("IntTestBack{}", ObjectId::new().to_hex());

        for i in 0..total_items {
            let oid = ObjectId::new();
            hero.insert_one(doc! {
                "_id": oid,
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

        let total_pages = (total_items + page_size - 1) / page_size;
        let mut cursor: Option<String> = None;

        // Backward pages in forward order: [7,8,9], [4,5,6], [1,2,3], [0]
        for page_num in 0..total_pages {
            let before_clause = cursor
                .as_ref()
                .map(|c| format!(r#"before: "{}""#, c))
                .unwrap_or_default();
            let args = if before_clause.is_empty() {
                format!("last: {}", page_size)
            } else {
                format!("last: {}, {}", page_size, before_clause)
            };

            let query = format!(
                r#"query {{ heroes({}, where: {{ alias: {{ startsWith: "{}" }} }}) {{ edges {{ alias }} pageInfo {{ hasNextPage hasPreviousPage startCursor endCursor }} totalCount }} }}"#,
                args, prefix
            );

            let result = executor::execute(schema, &query, async_graphql::Variables::default(), None, None)
                .await
                .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!("Page {} errors: {:?}", page_num, errors);
            }

            let page = &result["data"]["heroes"];
            let edges = page["edges"].as_array().unwrap();

            // Expected aliases for this backward page.
            let end_exclusive = total_items - page_num * page_size;
            let start_inclusive = (end_exclusive - page_size).max(0);
            let expected: Vec<String> = (start_inclusive..end_exclusive)
                .map(|i| format!("{}_{}", prefix, i))
                .collect();

            assert_eq!(
                edges.len(),
                expected.len(),
                "Page {}: expected {} items, got {}",
                page_num,
                expected.len(),
                edges.len()
            );
            for (offset, edge) in edges.iter().enumerate() {
                assert_eq!(
                    edge["alias"].as_str().unwrap(),
                    expected[offset],
                    "Page {}: alias mismatch at offset {}",
                    page_num,
                    offset
                );
            }

            // startCursor / endCursor both present.
            let start_cursor = page["pageInfo"]["startCursor"].as_str().unwrap();
            let end_cursor = page["pageInfo"]["endCursor"].as_str().unwrap();
            assert!(!start_cursor.is_empty(), "Page {}: startCursor empty", page_num);
            assert!(!end_cursor.is_empty(), "Page {}: endCursor empty", page_num);
            if edges.len() > 1 {
                assert_ne!(
                    start_cursor, end_cursor,
                    "Page {}: startCursor equals endCursor on multi-item page",
                    page_num
                );
            }

            // hasNextPage: true whenever `before` was provided (there may be
            // newer items on the other side), false on the first page.
            let has_next = page["pageInfo"]["hasNextPage"].as_bool().unwrap();
            assert_eq!(
                has_next,
                page_num > 0,
                "Page {}: hasNextPage should be {}",
                page_num,
                page_num > 0
            );

            // hasPreviousPage: true when there are older items beyond this page.
            let has_previous = page["pageInfo"]["hasPreviousPage"].as_bool().unwrap();
            let is_oldest_page = start_inclusive == 0;
            assert_eq!(
                has_previous,
                !is_oldest_page,
                "Page {}: hasPreviousPage should be {}",
                page_num,
                !is_oldest_page
            );

            assert_eq!(
                page["totalCount"].as_i64().unwrap(),
                total_items,
                "Page {}: totalCount",
                page_num
            );

            // Move the cursor backward: `before` = startCursor of this page.
            cursor = page["pageInfo"]["startCursor"].as_str().map(|s| s.to_string());
        }
    }
}
