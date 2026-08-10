use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_graphql::dynamic::ResolverContext;
use async_graphql::extensions::{Extension, ExtensionFactory};
use async_graphql::ServerResult;
use futures::StreamExt;
use mongodb::bson::{doc, oid::ObjectId, Document};
use mongodb::Database;

use crate::error::GraphQLError;
use crate::helpers::serialization::document_to_graphql_value;
use crate::schema::definition::SchemaDefinition;

/// Per-request batch loader that solves N+1 by collecting keys across
/// concurrent relation field resolvers and issuing batched `$in` queries.
///
/// Injected automatically via [`DataLoaderExtensionFactory`] registered at
/// schema build time, or explicitly via `executor::execute`'s `data` parameter.
#[derive(Clone)]
pub struct DataLoader {
    inner: Arc<DataLoaderInner>,
}

struct DataLoaderInner {
    db: Database,
    definition: SchemaDefinition,
    batches: Mutex<HashMap<BatchId, BatchState>>,
    batch_count: AtomicUsize,
}
/// Everything that changes the query shape. Sibling resolvers only share a
/// batch when they agree on collection, filter field, AND projection.
#[derive(Clone, PartialEq, Eq, Hash)]
enum BatchId {
    ById {
        collection: String,
        projection: Vec<String>,
    },
    ByFk {
        collection: String,
        fk_field: String,
        projection: Vec<String>,
    },
    Junction {
        collection: String,
        local_field: String,
        foreign_field: String,
    },
}

struct BatchState {
    pending: HashSet<String>,
    /// Entry present = resolved; `None` means the document was not found.
    results: HashMap<String, Option<serde_json::Value>>,
    in_flight: bool,
    notify: Arc<tokio::sync::Notify>,
    error: Option<Arc<GraphQLError>>,
}

impl Default for BatchState {
    fn default() -> Self {
        Self {
            pending: HashSet::new(),
            results: HashMap::new(),
            in_flight: false,
            notify: Arc::new(tokio::sync::Notify::new()),
            error: None,
        }
    }
}

impl DataLoader {
    pub fn new(db: Database, definition: SchemaDefinition) -> Self {
        Self {
            inner: Arc::new(DataLoaderInner {
                db,
                definition,
                batches: Mutex::new(HashMap::new()),
                batch_count: AtomicUsize::new(0),
            }),
        }
    }

    /// Number of batch queries executed so far (for test assertions).
    pub fn batch_execution_count(&self) -> usize {
        self.inner.batch_count.load(Ordering::Relaxed)
    }
    pub async fn load_forward_to_one(
        &self,
        collection: &str,
        projection: Vec<String>,
        id_hex: &str,
    ) -> Result<Option<serde_json::Value>, GraphQLError> {
        let batch_id = BatchId::ById {
            collection: collection.to_string(),
            projection,
        };
        let mut map = self.load_keys(batch_id, &[id_hex.to_string()]).await?;
        Ok(map.remove(id_hex).flatten())
    }

    pub async fn load_forward_to_one_many(
        &self,
        collection: &str,
        projection: Vec<String>,
        id_hexes: &[String],
    ) -> Result<HashMap<String, serde_json::Value>, GraphQLError> {
        let batch_id = BatchId::ById {
            collection: collection.to_string(),
            projection,
        };
        self.load_keys(batch_id, id_hexes)
            .await
            .map(|m| m.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect())
    }
    pub async fn load_reverse_to_many(
        &self,
        collection: &str,
        fk_field: &str,
        projection: Vec<String>,
        parent_hex: &str,
    ) -> Result<Vec<serde_json::Value>, GraphQLError> {
        let batch_id = BatchId::ByFk {
            collection: collection.to_string(),
            fk_field: fk_field.to_string(),
            projection,
        };
        let mut map = self.load_keys(batch_id, &[parent_hex.to_string()]).await?;
        match map.remove(parent_hex).flatten() {
            Some(serde_json::Value::Array(arr)) => Ok(arr),
            _ => Ok(vec![]),
        }
    }
    pub async fn load_reverse_to_one(
        &self,
        collection: &str,
        fk_field: &str,
        projection: Vec<String>,
        parent_hex: &str,
    ) -> Result<Option<serde_json::Value>, GraphQLError> {
        let batch_id = BatchId::ByFk {
            collection: collection.to_string(),
            fk_field: fk_field.to_string(),
            projection,
        };
        let mut map = self.load_keys(batch_id, &[parent_hex.to_string()]).await?;
        match map.remove(parent_hex).flatten() {
            Some(serde_json::Value::Array(mut arr)) if !arr.is_empty() => {
                Ok(Some(arr.swap_remove(0)))
            }
            _ => Ok(None),
        }
    }
    pub async fn load_junction_ids(
        &self,
        junction_collection: &str,
        local_field: &str,
        foreign_field: &str,
        parent_hex: &str,
    ) -> Result<Vec<String>, GraphQLError> {
        let batch_id = BatchId::Junction {
            collection: junction_collection.to_string(),
            local_field: local_field.to_string(),
            foreign_field: foreign_field.to_string(),
        };
        let mut map = self.load_keys(batch_id, &[parent_hex.to_string()]).await?;
        match map.remove(parent_hex).flatten() {
            Some(serde_json::Value::Array(arr)) => Ok(arr
                .into_iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()),
            _ => Ok(vec![]),
        }
    }

    async fn load_keys(
        &self,
        batch_id: BatchId,
        keys: &[String],
    ) -> Result<HashMap<String, Option<serde_json::Value>>, GraphQLError> {
        let key_set: HashSet<String> = keys.iter().cloned().collect();

        loop {
            let claim = {
                let mut map = self.inner.batches.lock().unwrap();
                let batch = map.entry(batch_id.clone()).or_default();

                if let Some(err) = &batch.error {
                    return Err((**err).clone());
                }

                // Fast path: all keys already cached.
                if key_set.iter().all(|k| batch.results.contains_key(k)) {
                    return Ok(key_set
                        .iter()
                        .map(|k| (k.clone(), batch.results[k].clone()))
                        .collect());
                }

                for k in &key_set {
                    if !batch.results.contains_key(k) {
                        batch.pending.insert(k.clone());
                    }
                }

                let was_idle = !batch.in_flight;
                if was_idle {
                    batch.in_flight = true;
                }
                was_idle
            };

            if claim {
                tokio::task::yield_now().await;
                tokio::task::yield_now().await;

                let inner = self.inner.clone();
                let id = batch_id.clone();
                tokio::spawn(async move { inner.flush_loop(id).await });
            }

            let notify = {
                let map = self.inner.batches.lock().unwrap();
                map.get(&batch_id).map(|b| b.notify.clone())
            };
            if let Some(notify) = notify {
                notify.notified().await;
            }
        }
    }
}
impl DataLoaderInner {
    async fn flush_loop(&self, batch_id: BatchId) {
        loop {
            let keys: Vec<String> = {
                let mut map = self.batches.lock().unwrap();
                let Some(batch) = map.get_mut(&batch_id) else { return };
                if batch.pending.is_empty() {
                    batch.in_flight = false;
                    batch.notify.notify_waiters();
                    return;
                }
                batch.pending.drain().collect()
            };

            self.batch_count.fetch_add(1, Ordering::Relaxed);

            let result = self.execute_batch(&batch_id, &keys).await;

            let mut map = self.batches.lock().unwrap();
            let Some(batch) = map.get_mut(&batch_id) else { return };
            match result {
                Ok(values) => batch.results.extend(values),
                Err(err) => {
                    batch.pending.clear();
                    batch.error = Some(Arc::new(err));
                    batch.in_flight = false;
                    batch.notify.notify_waiters();
                    return;
                }
            }
            batch.notify.notify_waiters();
        }
    }

    async fn execute_batch(
        &self,
        batch_id: &BatchId,
        keys: &[String],
    ) -> Result<HashMap<String, Option<serde_json::Value>>, GraphQLError> {
        match batch_id {
            BatchId::ById { collection, projection } => {
                run_batch_by_id(&self.db, &self.definition, collection, projection, keys).await
            }
            BatchId::ByFk { collection, fk_field, projection } => {
                run_batch_by_fk(&self.db, &self.definition, collection, fk_field, projection, keys).await
            }
            BatchId::Junction { collection, local_field, foreign_field } => {
                run_batch_junction(&self.db, collection, local_field, foreign_field, keys).await
            }
        }
    }
}
fn parse_keys(keys: &[String]) -> Vec<ObjectId> {
    keys.iter()
        .filter_map(|hex| ObjectId::parse_str(hex).ok())
        .collect()
}

fn build_projection(fields: &[String], extra: &[&str]) -> Document {
    let mut doc = doc! { "_id": 1, "id": 1 };
    for &field in extra {
        doc.insert(field, 1);
    }
    for field in fields {
        doc.insert(field.as_str(), 1);
    }
    doc
}

async fn collect_from_cursor<F>(
    mut cursor: mongodb::Cursor<Document>,
    mut handler: F,
) -> Result<(), GraphQLError>
where
    F: FnMut(&Document) -> Result<(), GraphQLError>,
{
    while let Some(doc_result) = cursor.next().await {
        handler(&doc_result?)?;
    }
    Ok(())
}
async fn run_batch_by_id(
    db: &Database,
    definition: &SchemaDefinition,
    collection_name: &str,
    projection: &[String],
    keys: &[String],
) -> Result<HashMap<String, Option<serde_json::Value>>, GraphQLError> {
    let oids = parse_keys(keys);

    let collection_def = definition
        .collections
        .iter()
        .find(|c| c.collection == collection_name);

    let projection_doc = build_projection(projection, &[]);

    // Pre-populate so missing docs are tracked (entry presence = resolved).
    let mut results: HashMap<String, Option<serde_json::Value>> = HashMap::new();
    for hex in keys {
        results.insert(hex.clone(), None);
    }

    let collection = db.collection::<Document>(collection_name);
    let cursor = collection
        .find(doc! { "_id": { "$in": &oids } })
        .projection(projection_doc)
        .await?;

    let results = Arc::new(Mutex::new(results));
    let target_def = collection_def;
    collect_from_cursor(cursor, |doc| {
        if let Ok(oid) = doc.get_object_id("_id") {
            let value = match target_def {
                Some(def) => document_to_graphql_value(doc, def),
                None => serde_json::Value::Null,
            };
            results.lock().unwrap().insert(oid.to_hex(), Some(value));
        }
        Ok(())
    }).await?;

    Ok(Arc::try_unwrap(results).unwrap().into_inner().unwrap())
}
async fn run_batch_by_fk(
    db: &Database,
    definition: &SchemaDefinition,
    collection_name: &str,
    fk_field: &str,
    projection: &[String],
    keys: &[String],
) -> Result<HashMap<String, Option<serde_json::Value>>, GraphQLError> {
    let oids = parse_keys(keys);

    let collection_def = definition
        .collections
        .iter()
        .find(|c| c.collection == collection_name);

    let projection_doc = build_projection(projection, &[fk_field]);

    let mut filter_doc = Document::new();
    filter_doc.insert(fk_field, doc! { "$in": &oids });

    let collection = db.collection::<Document>(collection_name);
    let cursor = collection.find(filter_doc).projection(projection_doc).await?;

    let grouped: Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let target_def = collection_def;
    collect_from_cursor(cursor, |doc| {
        if let Some(hex) = doc.get_object_id(fk_field).ok().map(|o| o.to_hex()) {
            let value = match target_def {
                Some(def) => document_to_graphql_value(doc, def),
                None => serde_json::Value::Null,
            };
            grouped.lock().unwrap().entry(hex).or_default().push(value);
        }
        Ok(())
    }).await?;

    let grouped = Arc::try_unwrap(grouped).unwrap().into_inner().unwrap();
    let results: HashMap<String, Option<serde_json::Value>> = keys
        .iter()
        .map(|hex| {
            let docs = grouped.get(hex.as_str()).cloned().unwrap_or_default();
            (hex.clone(), Some(serde_json::Value::Array(docs)))
        })
        .collect();
    Ok(results)
}
async fn run_batch_junction(
    db: &Database,
    collection_name: &str,
    local_field: &str,
    foreign_field: &str,
    keys: &[String],
) -> Result<HashMap<String, Option<serde_json::Value>>, GraphQLError> {
    let oids = parse_keys(keys);

    let mut projection_doc = Document::new();
    projection_doc.insert("_id", 0);
    projection_doc.insert(local_field, 1);
    projection_doc.insert(foreign_field, 1);

    let mut filter_doc = Document::new();
    filter_doc.insert(local_field, doc! { "$in": &oids });

    let collection = db.collection::<Document>(collection_name);
    let cursor = collection.find(filter_doc).projection(projection_doc).await?;

    let grouped: Arc<Mutex<HashMap<String, Vec<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    collect_from_cursor(cursor, |doc| {
        let local_hex = doc.get_object_id(local_field).ok().map(|o| o.to_hex());
        let foreign_hex = doc.get_object_id(foreign_field).ok().map(|o| o.to_hex());
        if let (Some(local), Some(foreign)) = (local_hex, foreign_hex) {
            grouped.lock().unwrap().entry(local).or_default().push(foreign);
        }
        Ok(())
    }).await?;

    let grouped = Arc::try_unwrap(grouped).unwrap().into_inner().unwrap();
    let results: HashMap<String, Option<serde_json::Value>> = keys
        .iter()
        .map(|hex| {
            let ids = grouped.get(hex.as_str()).cloned().unwrap_or_default();
            let arr =
                serde_json::Value::Array(ids.into_iter().map(serde_json::Value::String).collect());
            (hex.clone(), Some(arr))
        })
        .collect();
    Ok(results)
}
/// Registered at schema build time. Creates a fresh [`DataLoader`] for every
/// GraphQL request, so batching and caching are strictly per-request.
pub struct DataLoaderExtensionFactory {
    pub db: Database,
    pub definition: SchemaDefinition,
}

impl ExtensionFactory for DataLoaderExtensionFactory {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(DataLoaderExtension {
            loader: DataLoader::new(self.db.clone(), self.definition.clone()),
        })
    }
}

struct DataLoaderExtension {
    loader: DataLoader,
}

#[async_trait::async_trait]
impl Extension for DataLoaderExtension {
    async fn prepare_request(
        &self,
        context: &async_graphql::extensions::ExtensionContext<'_>,
        request: async_graphql::Request,
        next: async_graphql::extensions::NextPrepareRequest<'_>,
    ) -> ServerResult<async_graphql::Request> {
        let mut request = next.run(context, request).await?;
        if !request.data.contains_key(&std::any::TypeId::of::<DataLoader>()) {
            request.data.insert(self.loader.clone());
        }
        Ok(request)
    }
}
/// Build a sorted, deduplicated list of MongoDB field names from a GraphQL
/// selection set. Always includes `_id` and `id`. Forward relation FK fields
/// in the selection are included so deeper resolvers can access them.
pub fn selection_projection_fields(
    ctx: &ResolverContext<'_>,
    collection_def: &crate::schema::definition::CollectionDef,
) -> Vec<String> {
    let mut fields: Vec<String> = vec!["_id".into(), "id".into()];
    for selection in ctx.field().selection_set() {
        let gql_name = selection.name();
        let Some(field_def) = collection_def
            .fields
            .iter()
            .find(|field| field.graphql_name() == gql_name)
        else {
            continue;
        };
        match &field_def.field_type {
            crate::schema::definition::FieldType::Relation(rel) => match rel.kind {
                crate::schema::definition::RelationKind::OneToMany
                | crate::schema::definition::RelationKind::OneToOne => {
                    fields.push(field_def.name.clone());
                }
                crate::schema::definition::RelationKind::ManyToMany => {}
            },
            _ => {
                fields.push(field_def.name.clone());
            }
        }
    }
    fields.sort();
    fields.dedup();
    fields
}

