#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use mongo_graphql::executor;

    use crate::common::get_schema;

    /// All mutation operations tested sequentially.
    /// Tests are merged into one function because the MongoDB driver's
    /// internal runtime binds to the tokio runtime that created the Client;
    /// separate #[tokio::test] functions can cause "Server selection timeout".
    #[tokio::test]
    async fn test_mutations() {
        let schema = get_schema().await;

        // ---- create ----
        let create = executor::execute(
            schema,
            r#"mutation {
                createHero(input: {
                    alias: "IntTestCreateHero",
                    secret_identity: "Jane Doe",
                    power_level: 500,
                    active: true,
                    joined_at: "2024-01-01T00:00:00Z",
                    created_at: "2024-01-01T00:00:00Z",
                    bio: "A new hero"
                }) { id alias power_level }
            }"#,
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        if let Some(errors) = create.get("errors") {
            panic!("Create failed: {:?}", errors);
        }

        let hero = &create["data"]["createHero"];
        assert_eq!(hero["alias"].as_str().unwrap(), "IntTestCreateHero");
        assert_eq!(hero["power_level"].as_i64().unwrap(), 500);
        assert!(hero["id"].as_str().is_some());

        // ---- update ----
        let create_for_update = executor::execute(
            schema,
            r#"mutation {
                createHero(input: {
                    alias: "IntTestUpdateHero",
                    secret_identity: "Test",
                    power_level: 100,
                    active: true,
                    joined_at: "2024-01-01T00:00:00Z",
                    created_at: "2024-01-01T00:00:00Z"
                }) { id alias }
            }"#,
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        let update_id = create_for_update["data"]["createHero"]["id"]
            .as_str()
            .expect("createHero in update step should return id")
            .to_string();

        let update = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    updateHero(where: {{ id: "{}" }}, input: {{ alias: "IntTestUpdated" }}) {{
                        id alias
                    }}
                }}"#,
                update_id
            ),
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        if let Some(errors) = update.get("errors") {
            panic!("Update failed: {:?}", errors);
        }

        assert_eq!(
            update["data"]["updateHero"]["alias"].as_str().unwrap(),
            "IntTestUpdated"
        );

        // ---- delete ----
        let create_for_delete = executor::execute(
            schema,
            r#"mutation {
                createHero(input: {
                    alias: "IntTestDeleteHero",
                    secret_identity: "Test",
                    power_level: 100,
                    active: true,
                    joined_at: "2024-01-01T00:00:00Z",
                    created_at: "2024-01-01T00:00:00Z"
                }) { id }
            }"#,
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        let delete_id = create_for_delete["data"]["createHero"]["id"]
            .as_str()
            .expect("createHero in delete step should return id")
            .to_string();

        let delete = executor::execute(
            schema,
            &format!(
                r#"mutation {{
                    deleteHero(where: {{ id: "{}" }}) {{
                        success deletedId
                    }}
                }}"#,
                delete_id
            ),
            async_graphql::Variables::default(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            delete["data"]["deleteHero"]["success"].as_bool().unwrap(),
            true
        );
    }
}
