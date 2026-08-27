#[cfg(feature = "integration")]
mod helpers {
    use mongodb::{Client, Database};
    use tokio::sync::OnceCell;

    use mongo_graphql::schema::builder::{RuntimeConfig, SchemaBuilder};
    use mongo_graphql::schema::parser::SchemaParser;

    static DB: OnceCell<Database> = OnceCell::const_new();
    static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
    static SCHEMA: OnceCell<async_graphql::dynamic::Schema> = OnceCell::const_new();

    pub async fn get_db() -> &'static Database {
        DB.get_or_init(|| async {
            let client = get_client().clone();
            client.database("test_graphql_mongodb")
        })
        .await
    }

    /// The client is created on a dedicated thread with its own runtime that
    /// stays alive for the whole process. Its background tasks (connection
    /// pool, topology monitoring) would otherwise die with the first test's
    /// runtime, breaking every subsequent test in the same binary.
    fn get_client() -> &'static Client {
        CLIENT.get_or_init(|| {
            let mongo_uri = std::env::var("MONGO_URI")
                .unwrap_or_else(|_| "mongodb://localhost:27017".into());
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .expect("client runtime");
                runtime.block_on(async move {
                    let client = Client::with_uri_str(&mongo_uri).await.unwrap();
                    tx.send(client).unwrap();
                    std::future::pending::<()>().await;
                });
            });
            rx.recv().expect("mongo client")
        })
    }

    pub async fn get_schema() -> &'static async_graphql::dynamic::Schema {
        SCHEMA
            .get_or_init(|| async {
                let client = get_client().clone();
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
