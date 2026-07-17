#[cfg(feature = "integration")]
mod helpers {
    use mongodb::{Client, Database};
    use tokio::sync::OnceCell;

    use mongo_graphql::schema::builder::{RuntimeConfig, SchemaBuilder};
    use mongo_graphql::schema::parser::SchemaParser;

    static DB: OnceCell<Database> = OnceCell::const_new();
    static CLIENT: OnceCell<Client> = OnceCell::const_new();
    static SCHEMA: OnceCell<async_graphql::dynamic::Schema> = OnceCell::const_new();

    pub async fn get_db() -> &'static Database {
        DB.get_or_init(|| async {
            let client = get_client().await.clone();
            client.database("test_graphql_mongodb")
        })
        .await
    }

    async fn get_client() -> &'static Client {
        CLIENT
            .get_or_init(|| async {
                let mongo_uri = std::env::var("MONGO_URI")
                    .unwrap_or_else(|_| "mongodb://localhost:27017".into());
                Client::with_uri_str(&mongo_uri).await.unwrap()
            })
            .await
    }

    pub async fn get_schema() -> &'static async_graphql::dynamic::Schema {
        SCHEMA
            .get_or_init(|| async {
                let client = get_client().await.clone();
                let db = get_db().await.clone();
                let json = include_str!("../schema-definition.json");
                let definition =
                    SchemaParser::from_str(json).expect("schema must be valid");
                let config = RuntimeConfig {
                    max_page_size: 100,
                };
                SchemaBuilder::new(&config, &definition)
                    .build(client, db, None)
                    .await
                    .expect("schema build")
            })
            .await
    }
}

#[cfg(feature = "integration")]
#[allow(unused_imports)]
pub use helpers::{get_db, get_schema};
