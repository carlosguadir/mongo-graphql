use std::collections::{HashMap, HashSet};

use async_graphql::dynamic::ResolverContext;
use mongodb::bson::{doc, oid::ObjectId, Bson, Document};
use mongodb::{Client, Database};

use crate::error::GraphQLError;
use crate::helpers::serialization::{document_to_graphql_value, input_doc_to_mongo};
use crate::resolvers::query::transform_id_filter;
use crate::schema::definition::{CollectionDef, FieldType, JunctionDef, RelationKind, SchemaDefinition};

pub async fn resolve_create(
    ctx: ResolverContext<'_>,
    collection_def: &CollectionDef,
    client: &Client,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let definition = ctx.data::<SchemaDefinition>()?;
    let collection = db.collection::<Document>(&collection_def.collection);

    let mut input: Document = ctx
        .args
        .try_get("input")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let oid = ObjectId::new();

    with_transaction!(client, session, {
        let fk_values = extract_relation_fields(
            &mut input, collection_def, definition, db, &mut session, &HashMap::new(), oid, 1,
        )
        .await?;

        let mut doc = input_doc_to_mongo(input, collection_def);
        for (mongo_name, value) in fk_values {
            doc.insert(mongo_name, value);
        }
        doc.insert("_id", oid);
        doc.insert("id", oid);

        collection.insert_one(&doc).session(&mut session).await.map_err(|e| {
            if is_duplicate_key_error(&e) {
                GraphQLError::DuplicateKey {
                    message: format!("Duplicate key in collection '{}'", collection_def.collection),
                }
            } else {
                GraphQLError::from(e)
            }
        })?;

        Ok(Some(document_to_graphql_value(&doc, collection_def)))
    })
}

pub async fn resolve_update(
    ctx: ResolverContext<'_>,
    collection_def: &CollectionDef,
    client: &Client,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let definition = ctx.data::<SchemaDefinition>()?;
    let collection = db.collection::<Document>(&collection_def.collection);

    let where_input: Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;
    let mut update_input: Document = ctx
        .args
        .try_get("input")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let filter = transform_id_filter(where_input)?;

    with_transaction!(client, session, {
        let existing = collection
            .find_one(filter.clone())
            .session(&mut session)
            .await?
            .ok_or_else(|| GraphQLError::NotFound {
                message: format!(
                    "Document not found in '{}' for update",
                    collection_def.collection
                ),
            })?;

        let source_oid = existing
            .get_object_id("_id")
            .map_err(|_| GraphQLError::Internal("Existing document missing _id".into()))?;
        let current_fks = build_current_fk_map(&existing, collection_def);

        let fk_values = extract_relation_fields(
            &mut update_input, collection_def, definition, db, &mut session, &current_fks, source_oid, 1,
        )
        .await?;

        let mut update_doc = input_doc_to_mongo(update_input, collection_def);
        if update_doc.contains_key("_id") || update_doc.contains_key("id") {
            return Err(GraphQLError::Internal(
                "Updating the id field is not allowed".into(),
            ));
        }

        for (mongo_name, value) in fk_values {
            update_doc.insert(mongo_name, value);
        }

        collection.update_one(filter, doc! { "$set": &update_doc })
            .session(&mut session)
            .await
            .map_err(GraphQLError::from)?;

        let mut merged = existing;
        for (key, value) in &update_doc {
            merged.insert(key.clone(), value.clone());
        }
        Ok(Some(document_to_graphql_value(&merged, collection_def)))
    })
}

pub async fn resolve_delete(
    ctx: ResolverContext<'_>,
    collection_def: &CollectionDef,
    client: &Client,
    db: &Database,
) -> Result<Option<serde_json::Value>, GraphQLError> {
    let collection = db.collection::<Document>(&collection_def.collection);

    let where_input: Document = ctx
        .args
        .try_get("where")?
        .deserialize()
        .map_err(|e| GraphQLError::Internal(e.message))?;

    let id = where_input
        .get_str("id")
        .map(|s| s.to_owned())
        .or_else(|_| {
            where_input
                .get_object_id("id")
                .map(|oid| oid.to_hex())
        })
        .map_err(|_| GraphQLError::Internal("where.id is required for delete".into()))?;

    let filter = transform_id_filter(where_input)?;

    with_transaction!(client, session, {
        let result = collection
            .delete_one(filter)
            .session(&mut session)
            .await
            .map_err(GraphQLError::from)?;

        if result.deleted_count == 0 {
            return Err(GraphQLError::NotFound {
                message: format!(
                    "Document not found in '{}' for delete",
                    collection_def.collection
                ),
            });
        }

        Ok(Some(serde_json::json!({
            "success": true,
            "deletedId": &id,
        })))
    })
}

fn check_depth(depth: u8) -> Result<(), GraphQLError> {
    if depth >= 3 {
        return Err(GraphQLError::Internal(
            "Maximum nested mutation depth (3) exceeded".into(),
        ));
    }
    Ok(())
}

fn validate_single_operation(nested: &Document) -> Result<(), GraphQLError> {
    let ops = ["connect", "create", "disconnect", "delete", "update"];
    if ops.iter().filter(|op| nested.contains_key(*op)).count() > 1 {
        return Err(GraphQLError::Internal(
            "Only one of create, connect, disconnect, delete, or update can be specified per relation field"
                .into(),
        ));
    }
    Ok(())
}

/// Process a forward to-one nested input (CreateOneInput / UpdateOneInput).
/// Returns Some(ObjectId) for connect/create, None for disconnect/delete (sets FK to null).
async fn process_nested_one_input(
    nested: &Document,
    target_collection_def: &CollectionDef,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    current_fk: Option<&Bson>,
    depth: u8,
) -> Result<Option<Bson>, GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    if let Some(hex) = nested
        .get("connect")
        .and_then(|v| v.as_document())
        .and_then(|d| d.get_str("id").ok())
    {
        let oid = ObjectId::parse_str(hex)
            .map_err(|_| GraphQLError::Internal(format!("Invalid ObjectId in connect: {}", hex)))?;
        return Ok(Some(Bson::ObjectId(oid)));
    }

    if let Some(create_doc) = nested.get("create").and_then(|v| v.as_document()) {
        let oid = create_nested_document(
            create_doc, target_collection_def, definition, db, session, None, depth,
        )
        .await?;
        return Ok(Some(Bson::ObjectId(oid)));
    }

    if nested.get("disconnect") == Some(&Bson::Boolean(true)) {
        return Ok(None);
    }

    if nested.get("delete") == Some(&Bson::Boolean(true)) {
        if let Some(fk_oid) = current_fk.and_then(|v| v.as_object_id()) {
            db.collection::<Document>(&target_collection_def.collection)
                .delete_one(doc! { "_id": fk_oid })
                .session(session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(None);
    }

    if let Some(update_data) = nested.get("update").and_then(|v| v.as_document()) {
        if let Some(fk_oid) = current_fk.and_then(|v| v.as_object_id()) {
            apply_nested_update(
                doc! { "_id": fk_oid }, update_data, target_collection_def, db, session,
            )
            .await?;
        }
        return Ok(current_fk.cloned());
    }

    Ok(None)
}

/// Scan a mutation input for relation fields and dispatch to the appropriate handler.
/// Returns (mongo_field_name, Bson_value) pairs to merge into the MongoDB document.
///
/// `current_fks` is populated for updates (maps GraphQL name → current FK value),
/// `source_oid` is the _id of the document being created/updated,
/// `depth` tracks nesting level (max 3).
async fn extract_relation_fields(
    input: &mut Document,
    collection_def: &CollectionDef,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    current_fks: &HashMap<String, Option<Bson>>,
    source_oid: ObjectId,
    depth: u8,
) -> Result<Vec<(String, Bson)>, GraphQLError> {
    let mut fk_values: Vec<(String, Bson)> = Vec::new();
    let relation_field_keys: HashSet<String> = collection_def
        .fields
        .iter()
        .filter(|f| matches!(f.field_type, FieldType::Relation(_)))
        .map(|f| f.graphql_name())
        .collect();

    for field_name in &relation_field_keys {
        let field_def = collection_def
            .fields
            .iter()
            .find(|f| f.graphql_name() == *field_name)
            .unwrap();

        let relation = match &field_def.field_type {
            FieldType::Relation(relation) => relation,
            _ => continue,
        };

        let target_def = definition.collection_by_name(&relation.collection).ok_or_else(|| {
            GraphQLError::Internal(format!(
                "Target collection '{}' not found in schema definition",
                relation.collection
            ))
        })?;

        let value = match input.remove(field_def.graphql_name().as_str()) {
            Some(Bson::Document(nested)) => nested,
            Some(_) => {
                return Err(GraphQLError::Internal(format!(
                    "Expected a nested input object for relation field '{}'",
                    field_name
                )));
            }
            None => continue,
        };

        match relation.kind {
            RelationKind::OneToMany | RelationKind::OneToOne => {
                let current_fk = current_fks.get(field_name).and_then(|v| v.as_ref());
                let processed = process_nested_one_input(
                    &value, target_def, definition, db, session, current_fk, depth,
                )
                .await?;
                let bson = processed.unwrap_or(Bson::Null);
                fk_values.push((field_def.name.clone(), bson));
            }
            RelationKind::ManyToMany => {
                let junction = relation.junction.as_ref().ok_or_else(|| {
                    GraphQLError::Internal(
                        "ManyToMany relation missing junction definition".into(),
                    )
                })?;
                process_nested_many_to_many_input(
                    &value, junction, target_def, source_oid, definition, db, session, depth,
                )
                .await?;
            }
        }
    }

    let remaining_keys: Vec<String> = input.keys().cloned().collect();
    for key in remaining_keys {
        if relation_field_keys.contains(&key) {
            continue;
        }

        let (target_collection, fk_field, kind) = match find_reverse_field(definition, collection_def, &key) {
            Some(info) => info,
            None => continue,
        };

        let value = match input.remove(&key) {
            Some(Bson::Document(nested)) => nested,
            Some(_) => {
                return Err(GraphQLError::Internal(format!(
                    "Expected a nested input object for reverse field '{}'",
                    key
                )));
            }
            None => continue,
        };

        match kind {
            RelationKind::OneToMany => {
                process_nested_many_input(
                    &value, target_collection, &fk_field, source_oid, definition, db, session, depth,
                )
                .await?;
            }
            RelationKind::OneToOne => {
                process_reverse_one_to_one_input(
                    &value, target_collection, fk_field.as_str(), source_oid, definition, db, session,
                    depth,
                )
                .await?;
            }
            _ => unreachable!(),
        }
    }

    Ok(fk_values)
}

/// Find a OneToMany or OneToOne relation in `definition` where the reverse field
/// matches `key` on `collection_def`. Returns (target_collection, fk_field_name, kind).
fn find_reverse_field<'a>(
    definition: &'a SchemaDefinition,
    collection_def: &CollectionDef,
    key: &str,
) -> Option<(&'a CollectionDef, String, RelationKind)> {
    for other in &definition.collections {
        if other.collection == collection_def.collection {
            continue;
        }
        for field in &other.fields {
            if let FieldType::Relation(relation) = &field.field_type {
                if relation.collection != collection_def.collection {
                    continue;
                }
                let match_result: Option<RelationKind> = match relation.kind {
                    RelationKind::OneToMany => {
                        let name = relation
                            .reverse_name
                            .clone()
                            .unwrap_or_else(|| other.plural_name());
                        if name == key { Some(RelationKind::OneToMany) } else { None }
                    }
                    RelationKind::OneToOne => {
                        let name = relation
                            .reverse_name
                            .clone()
                            .unwrap_or_else(|| other.singular_name());
                        if name == key { Some(RelationKind::OneToOne) } else { None }
                    }
                    _ => None,
                };
                if let Some(kind) = match_result {
                    return Some((other, field.name.clone(), kind));
                }
            }
        }
    }
    None
}

async fn process_nested_many_input(
    nested: &Document,
    target_collection_def: &CollectionDef,
    fk_field: &str,
    source_oid: ObjectId,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    depth: u8,
) -> Result<(), GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    let target_collection = db.collection::<Document>(&target_collection_def.collection);

    if let Some(ids) = nested.get("connect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids)? {
            target_collection
                .update_one(
                    doc! { "_id": oid },
                    doc! { "$set": { fk_field: source_oid } },
                )
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(elems) = nested.get("create").and_then(|v| v.as_array()) {
        for create_val in elems {
            let create_doc = create_val.as_document().ok_or_else(|| {
                GraphQLError::Internal("create entries must be objects".into())
            })?;
            create_nested_document(
                create_doc, target_collection_def, definition, db, session,
                Some((fk_field, source_oid)), depth,
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("disconnect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids)? {
            target_collection
                .update_one(
                    doc! { "_id": oid, fk_field: source_oid },
                    doc! { "$set": { fk_field: Bson::Null } },
                )
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("delete").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids)? {
            target_collection
                .delete_one(doc! { "_id": oid, fk_field: source_oid })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(updates) = nested.get("update").and_then(|v| v.as_array()) {
        return process_nested_many_update(updates, target_collection_def, db, session).await;
    }

    Ok(())
}

async fn process_nested_many_to_many_input(
    nested: &Document,
    junction: &JunctionDef,
    target_collection_def: &CollectionDef,
    source_oid: ObjectId,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    depth: u8,
) -> Result<(), GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    let junction_collection = db.collection::<Document>(&junction.collection);

    if let Some(ids) = nested.get("connect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids)? {
            junction_collection
                .insert_one(&doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(elems) = nested.get("create").and_then(|v| v.as_array()) {
        for create_val in elems {
            let create_doc = create_val.as_document().ok_or_else(|| {
                GraphQLError::Internal("create entries must be objects".into())
            })?;
            let oid = create_nested_document(
                create_doc, target_collection_def, definition, db, session, None, depth,
            )
            .await?;

            junction_collection
                .insert_one(&doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("disconnect").and_then(|v| v.as_array()) {
        for oid in &parse_id_array(ids)? {
            junction_collection
                .delete_one(doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(ids) = nested.get("delete").and_then(|v| v.as_array()) {
        let target_collection = db.collection::<Document>(&target_collection_def.collection);
        for oid in &parse_id_array(ids)? {
            junction_collection
                .delete_one(doc! {
                    &junction.local_field: source_oid,
                    &junction.foreign_field: oid,
                })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
            target_collection
                .delete_one(doc! { "_id": oid })
                .session(&mut *session)
                .await
                .map_err(GraphQLError::from)?;
        }
        return Ok(());
    }

    if let Some(updates) = nested.get("update").and_then(|v| v.as_array()) {
        return process_nested_many_update(updates, target_collection_def, db, session).await;
    }

    Ok(())
}

async fn process_reverse_one_to_one_input(
    nested: &Document,
    target_collection_def: &CollectionDef,
    fk_field: &str,
    source_oid: ObjectId,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    depth: u8,
) -> Result<(), GraphQLError> {
    check_depth(depth)?;
    validate_single_operation(nested)?;

    let target_collection = db.collection::<Document>(&target_collection_def.collection);

    if let Some(hex) = nested
        .get("connect")
        .and_then(|v| v.as_document())
        .and_then(|d| d.get_str("id").ok())
    {
        let target_oid = ObjectId::parse_str(hex)
            .map_err(|_| GraphQLError::Internal("Invalid connect id".into()))?;
        target_collection
            .update_one(
                doc! { "_id": target_oid },
                doc! { "$set": { fk_field: source_oid } },
            )
            .session(&mut *session)
            .await
            .map_err(|e| {
                GraphQLError::Internal(format!("Reverse OneToOne connect failed: {}", e))
            })?;
    }

    if let Some(create_doc) = nested.get("create").and_then(|v| v.as_document()) {
        create_nested_document(
            create_doc, target_collection_def, definition, db, session,
            Some((fk_field, source_oid)), depth,
        )
        .await?;
    }

    if nested.get("disconnect") == Some(&Bson::Boolean(true)) {
        target_collection
            .update_many(
                doc! { fk_field: source_oid },
                doc! { "$unset": { fk_field: "" } },
            )
            .session(&mut *session)
            .await
            .map_err(|e| {
                GraphQLError::Internal(format!("Reverse OneToOne disconnect failed: {}", e))
            })?;
    }

    if nested.get("delete") == Some(&Bson::Boolean(true)) {
        target_collection
            .delete_many(doc! { fk_field: source_oid })
            .session(&mut *session)
            .await
            .map_err(|e| {
                GraphQLError::Internal(format!("Reverse OneToOne delete failed: {}", e))
            })?;
    }

    if let Some(update_data) = nested.get("update").and_then(|v| v.as_document()) {
        apply_nested_update(
            doc! { fk_field: source_oid }, update_data, target_collection_def, db, session,
        )
        .await?;
    }

    Ok(())
}

async fn create_nested_document(
    create_doc: &Document,
    target_collection_def: &CollectionDef,
    definition: &SchemaDefinition,
    db: &Database,
    session: &mut mongodb::ClientSession,
    fk_field: Option<(&str, ObjectId)>,
    depth: u8,
) -> Result<ObjectId, GraphQLError> {
    let mut target_doc = input_doc_to_mongo(create_doc.clone(), target_collection_def);
    let oid = ObjectId::new();
    target_doc.insert("_id", oid);
    target_doc.insert("id", oid);

    if let Some((field_name, value)) = fk_field {
        target_doc.insert(field_name, value);
    }

    let next_depth = depth + 1;
    if next_depth < 3 {
        let mut create_doc_mut = create_doc.clone();
        let nested_fk_values = Box::pin(extract_relation_fields(
            &mut create_doc_mut,
            target_collection_def,
            definition,
            db,
            session,
            &HashMap::new(),
            oid,
            next_depth,
        ))
        .await?;
        for (mongo_name, value) in nested_fk_values {
            target_doc.insert(mongo_name, value);
        }
    }

    let target_collection = db.collection::<Document>(&target_collection_def.collection);
    target_collection
        .insert_one(&target_doc)
        .session(&mut *session)
        .await
        .map_err(|e| {
            if is_duplicate_key_error(&e) {
                GraphQLError::DuplicateKey {
                    message: format!(
                        "Duplicate key in nested create for collection '{}'",
                        target_collection_def.collection
                    ),
                }
            } else {
                GraphQLError::from(e)
            }
        })?;

    Ok(oid)
}

/// Apply scalar field updates to an existing document found by `filter`.
async fn apply_nested_update(
    filter: Document,
    update_data: &Document,
    target_collection_def: &CollectionDef,
    db: &Database,
    session: &mut mongodb::ClientSession,
) -> Result<(), GraphQLError> {
    let update_doc = input_doc_to_mongo(update_data.clone(), target_collection_def);
    if update_doc.is_empty() {
        return Ok(());
    }
    let target_collection = db.collection::<Document>(&target_collection_def.collection);
    target_collection
        .update_one(filter, doc! { "$set": &update_doc })
        .session(&mut *session)
        .await
        .map_err(GraphQLError::from)?;
    Ok(())
}

/// Process a to-many `update` array: `[{ where: …, data: … }]`.
/// Used by both reverse to-many and ManyToMany nested update handlers.
async fn process_nested_many_update(
    updates: &mongodb::bson::Array,
    target_collection_def: &CollectionDef,
    db: &Database,
    session: &mut mongodb::ClientSession,
) -> Result<(), GraphQLError> {
    for entry in updates {
        let entry_doc = entry.as_document().ok_or_else(|| {
            GraphQLError::Internal("update entries must be objects".into())
        })?;
        let where_clause = entry_doc.get_document("where").map_err(|_| {
            GraphQLError::Internal("update entry must have a 'where' field".into())
        })?;
        let data = entry_doc.get_document("data").map_err(|_| {
            GraphQLError::Internal("update entry must have a 'data' field".into())
        })?;
        let filter = transform_id_filter(where_clause.clone())?;
        apply_nested_update(filter, data, target_collection_def, db, session).await?;
    }
    Ok(())
}

fn build_current_fk_map(
    existing: &Document,
    collection_def: &CollectionDef,
) -> HashMap<String, Option<Bson>> {
    collection_def
        .fields
        .iter()
        .filter(|f| matches!(f.field_type, FieldType::Relation(_)))
        .map(|f| (f.graphql_name(), existing.get(&f.name).cloned()))
        .collect()
}

fn parse_id_array(
    arr: &mongodb::bson::Array,
) -> Result<Vec<ObjectId>, GraphQLError> {
    arr.iter()
        .map(|entry| {
            let doc = entry.as_document().ok_or_else(|| {
                GraphQLError::Internal(
                    "Each entry must be an object with an id field".into(),
                )
            })?;
            let hex = doc.get_str("id").map_err(|_| {
                GraphQLError::Internal("Each entry must have an 'id' field".into())
            })?;
            ObjectId::parse_str(hex)
                .map_err(|_| GraphQLError::Internal("Invalid ObjectId in entry".into()))
        })
        .collect()
}

macro_rules! with_transaction {
    ($client:expr, $session:ident, $body:block) => {{
        let mut $session = $client.start_session().await?;
        $session.start_transaction().await.map_err(|e| {
            GraphQLError::Internal(format!("Failed to start transaction: {}", e))
        })?;
        let __result: Result<_, GraphQLError> = (|| async { $body })().await;
        match &__result {
            Ok(_) => {
                $session.commit_transaction().await.map_err(|e| {
                    GraphQLError::Internal(format!("Failed to commit transaction: {}", e))
                })?;
            }
            Err(_) => {
                let _ = $session.abort_transaction().await;
            }
        }
        __result
    }};
}
use with_transaction;

fn is_duplicate_key_error(error: &mongodb::error::Error) -> bool {
    match &*error.kind {
        mongodb::error::ErrorKind::Write(write_failure) => match write_failure {
            mongodb::error::WriteFailure::WriteError(write_error) => {
                write_error.code == 11000 || write_error.code == 11001
            }
            mongodb::error::WriteFailure::WriteConcernError(write_concern_error) => {
                write_concern_error.code == 11000 || write_concern_error.code == 11001
            }
            _ => false,
        },
        mongodb::error::ErrorKind::BulkWrite(bulk_failure) => {
            bulk_failure
                .write_errors
                .iter()
                .any(|(_, write_error)| write_error.code == 11000 || write_error.code == 11001)
        }
        _ => false,
    }
}
