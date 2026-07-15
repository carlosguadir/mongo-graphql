use std::collections::{HashMap, HashSet};

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputObject, InputValue, Object, Schema,
    SchemaBuilder as AgSchemaBuilder, TypeRef,
};
use mongodb::{Client, Database};

use crate::error::GraphQLError;
use crate::helpers::serialization::json_to_field_value;
use crate::resolvers::{mutation, query};
use crate::schema::definition::{CollectionDef, EnumDef, FieldType, RelationKind, SchemaDefinition};
use crate::types::scalars;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub max_page_size: usize,
}

pub struct SchemaBuilder<'a> {
    config: &'a RuntimeConfig,
    definition: &'a SchemaDefinition,
    registered_enums: HashSet<String>,
    registered_nested_inputs: HashSet<String>,
}

impl<'a> SchemaBuilder<'a> {
    pub fn new(config: &'a RuntimeConfig, definition: &'a SchemaDefinition) -> Self {
        Self {
            config,
            definition,
            registered_enums: HashSet::new(),
            registered_nested_inputs: HashSet::new(),
        }
    }

    pub async fn build(mut self, client: Client, db: Database) -> Result<Schema, GraphQLError> {
        let mut ping_cmd = mongodb::bson::Document::new();
        ping_cmd.insert("ping", 1);
        db.run_command(ping_cmd)
            .await
            .map_err(|e| GraphQLError::Database(format!("Database ping failed: {}", e)))?;

        let mut builder = Schema::build("Query", Some("Mutation"), None);
        builder = scalars::register_all(builder);
        builder = self.register_page_info(builder);
        builder = self.register_delete_result(builder);
        builder = self.register_filter_types(builder);

        let mut query_root = Object::new("Query");
        let mut mutation_root = Object::new("Mutation");

        for collection in &self.definition.collections {
            let where_unique = InputObject::new(format!("{}WhereUniqueInput", collection.type_name()))
                .field(InputValue::new("id", TypeRef::named_nn("ID")));
            builder = builder.register(where_unique);
        }

        for collection in &self.definition.collections {
            (builder, query_root, mutation_root) = self.register_collection(
                builder, query_root, mutation_root, collection, &client, &db,
            )?;
        }

        builder = builder.register(query_root);
        builder = builder.register(mutation_root);

        let schema = builder
            .data(client)
            .data(db)
            .data(self.config.clone())
            .data(self.definition.clone())
            .finish()
            .map_err(|e| GraphQLError::SchemaBuild(format!("Failed to build schema: {}", e)))?;

        Ok(schema)
    }

    fn register_collection(
        &mut self,
        mut builder: AgSchemaBuilder,
        mut query_root: Object,
        mut mutation_root: Object,
        collection: &CollectionDef,
        client: &Client,
        db: &Database,
    ) -> Result<(AgSchemaBuilder, Object, Object), GraphQLError> {
        let type_name = collection.type_name();

        for field in &collection.fields {
            if let Some(enum_def) = &field.r#enum {
                builder = self.register_enum(builder, enum_def);
            }
        }

        let collection_map: HashMap<&str, &CollectionDef> = self
            .definition
            .collections
            .iter()
            .map(|c| (c.collection.as_str(), c))
            .collect();

        // Collect reverse relations: other collections with OneToMany or OneToOne
        // pointing to THIS collection get a reverse field on this collection's inputs.
        let (reverse_to_many, reverse_to_one) = self.collect_reverse_relations(collection);

        // ── Object type ──
        let mut obj = Object::new(type_name.clone());
        for field in &collection.fields {
            if matches!(field.field_type, FieldType::Relation(_)) {
                continue;
            }
            let field_type = scalars::type_ref(&field.field_type);
            let field_name = field.graphql_name();
            obj = obj.field(Field::new(
                field_name.clone(),
                field_type,
                move |ctx| {
                    let name = field_name.clone();
                    FieldFuture::new(async move {
                        let value = ctx
                            .parent_value
                            .as_value()
                            .and_then(|parent| extract_nested(parent, &name));
                        match value {
                            Some(val) => Ok(Some(FieldValue::value(val))),
                            None => Ok(None),
                        }
                    })
                },
            ));
        }
        builder = builder.register(obj);

        // ── Register nested inputs for forward relation fields ──
        for field in &collection.fields {
            if let FieldType::Relation(_rel) = &field.field_type {
                builder = self.register_nested_inputs_for_field(builder, collection, field);
            }
        }

        // ── Register reverse input types ──
        for (_, target_coll_name, _) in &reverse_to_many {
            if let Some(target_def) = collection_map.get(target_coll_name.as_str()) {
                (builder, _) = self.register_reverse_inputs(
                    builder, collection, target_def, RelationKind::OneToMany, "Create",
                );
            }
        }
        for (_, target_coll_name) in &reverse_to_one {
            if let Some(target_def) = collection_map.get(target_coll_name.as_str()) {
                (builder, _) = self.register_reverse_inputs(
                    builder, collection, target_def, RelationKind::OneToOne, "Create",
                );
            }
        }

        // ── CreateInput ──
        let mut create_input = InputObject::new(format!("{}CreateInput", type_name));
        create_input = self.build_input_fields(create_input, collection, &collection_map, "Create");
        create_input = self.add_reverse_fields_to_input(create_input, &reverse_to_many, &reverse_to_one, &collection_map, &type_name, "Create");
        builder = builder.register(create_input);

        // ── UpdateInput ──
        let mut update_input = InputObject::new(format!("{}UpdateInput", type_name));
        update_input = self.build_input_fields(update_input, collection, &collection_map, "Update");
        update_input = self.add_reverse_fields_to_input(update_input, &reverse_to_many, &reverse_to_one, &collection_map, &type_name, "Update");
        builder = builder.register(update_input);

        // ── WhereInput ──
        let mut where_input =
            InputObject::new(format!("{}WhereInput", type_name))
                .field(InputValue::new("id", TypeRef::named("IdFilter")));
        for field in &collection.fields {
            if field.name == "id" || field.name == "_id" {
                continue;
            }
            if matches!(field.field_type, FieldType::Relation(_)) {
                continue;
            }
            let filter_type = filter_type_name(&field.field_type);
            where_input =
                where_input.field(InputValue::new(field.graphql_name(), TypeRef::named(filter_type)));
        }
        builder = builder.register(where_input);

        // ── SortInput ──
        let mut sort_input = InputObject::new(format!("{}SortInput", type_name));
        for field in &collection.fields {
            if field.name == "id" || field.name == "_id" {
                continue;
            }
            if matches!(field.field_type, FieldType::Relation(_) | FieldType::Json | FieldType::List(_)) {
                continue;
            }
            sort_input =
                sort_input.field(InputValue::new(field.graphql_name(), TypeRef::named("SortDirection")));
        }
        builder = builder.register(sort_input);

        let object_ref = TypeRef::named(type_name.clone());

        // ── Connection type ──
        let conn_name = format!("{}Connection", type_name);
        let conn_obj = Object::new(conn_name.clone())
            .field(nested_field("edges", TypeRef::named_nn_list(type_name.clone())))
            .field(nested_field("pageInfo", TypeRef::named_nn("PageInfo")))
            .field(nested_field("totalCount", TypeRef::named_nn("Int")));
        builder = builder.register(conn_obj);

        // ── Query: singular ──
        let coll_for_get = collection.clone();
        let db_for_get = db.clone();
        query_root = query_root.field(
            Field::new(collection.singular_name(), object_ref.clone(), move |ctx| {
                let coll_def = coll_for_get.clone();
                let db = db_for_get.clone();
                FieldFuture::new(async move {
                    let result = query::resolve_get(ctx, &coll_def, &db).await;
                    (match result {
                        Ok(Some(value)) => json_to_field_value(value).map(Some),
                        Ok(None) => Ok(None),
                        Err(e) => Err(e),
                    })
                    .map_err(|e| e.into_graphql_error())
                })
            })
            .argument(InputValue::new(
                "where",
                TypeRef::named_nn(format!("{}WhereUniqueInput", type_name)),
            )),
        );

        // ── Query: plural ──
        let coll_for_list = collection.clone();
        let db_for_list = db.clone();
        let page_size = self.config.max_page_size;
        query_root = query_root.field(
            Field::new(
                collection.plural_name(),
                TypeRef::named_nn(conn_name.clone()),
                move |ctx| {
                    let coll_def = coll_for_list.clone();
                    let db = db_for_list.clone();
                    FieldFuture::new(async move {
                        let result = query::resolve_list(ctx, &coll_def, &db, page_size).await;
                        (match result {
                            Ok(value) => json_to_field_value(value).map(Some),
                            Err(e) => Err(e),
                        })
                        .map_err(|e| e.into_graphql_error())
                    })
                },
            )
            .argument(InputValue::new("first", TypeRef::named("Int")))
            .argument(InputValue::new("after", TypeRef::named("String")))
            .argument(InputValue::new("last", TypeRef::named("Int")))
            .argument(InputValue::new("before", TypeRef::named("String")))
            .argument(InputValue::new("where", TypeRef::named(format!("{}WhereInput", type_name))))
            .argument(InputValue::new("sort", TypeRef::named(format!("{}SortInput", type_name)))),
        );

        // ── Mutations ──
        let coll_for_create = collection.clone();
        let client_for_create = client.clone();
        let db_for_create = db.clone();
        mutation_root = mutation_root.field(
            Field::new(
                format!("create{}", type_name),
                object_ref.clone(),
                move |ctx| {
                    let coll_def = coll_for_create.clone();
                    let client = client_for_create.clone();
                    let db = db_for_create.clone();
                    FieldFuture::new(async move {
                        let result = mutation::resolve_create(ctx, &coll_def, &client, &db).await;
                        resolve_to_field_value(result)
                    })
                },
            )
            .argument(InputValue::new(
                "input",
                TypeRef::named_nn(format!("{}CreateInput", type_name)),
            )),
        );

        let coll_for_update = collection.clone();
        let client_for_update = client.clone();
        let db_for_update = db.clone();
        mutation_root = mutation_root.field(
            Field::new(
                format!("update{}", type_name),
                object_ref.clone(),
                move |ctx| {
                    let coll_def = coll_for_update.clone();
                    let client = client_for_update.clone();
                    let db = db_for_update.clone();
                    FieldFuture::new(async move {
                        let result = mutation::resolve_update(ctx, &coll_def, &client, &db).await;
                        resolve_to_field_value(result)
                    })
                },
            )
            .argument(InputValue::new(
                "where",
                TypeRef::named_nn(format!("{}WhereUniqueInput", type_name)),
            ))
            .argument(InputValue::new(
                "input",
                TypeRef::named_nn(format!("{}UpdateInput", type_name)),
            )),
        );

        let coll_for_delete = collection.clone();
        let client_for_delete = client.clone();
        let db_for_delete = db.clone();
        mutation_root = mutation_root.field(
            Field::new(
                format!("delete{}", type_name),
                TypeRef::named_nn("DeleteResult"),
                move |ctx| {
                    let coll_def = coll_for_delete.clone();
                    let client = client_for_delete.clone();
                    let db = db_for_delete.clone();
                    FieldFuture::new(async move {
                        let result = mutation::resolve_delete(ctx, &coll_def, &client, &db).await;
                        resolve_to_field_value(result)
                    })
                },
            )
            .argument(InputValue::new(
                "where",
                TypeRef::named_nn(format!("{}WhereUniqueInput", type_name)),
            )),
        );

        Ok((builder, query_root, mutation_root))
    }

    // ── Reverse relation collection ──

    fn collect_reverse_relations(
        &self,
        collection: &CollectionDef,
    ) -> (Vec<(String, String, String)>, Vec<(String, String)>) {
        let mut to_many = Vec::new();
        let mut to_one = Vec::new();
        for other in &self.definition.collections {
            if other.collection == collection.collection {
                continue;
            }
            for field in &other.fields {
                if let FieldType::Relation(rel) = &field.field_type {
                    if rel.collection != collection.collection {
                        continue;
                    }
                    match rel.kind {
                        RelationKind::OneToMany => {
                            let name = rel
                                .reverse_name
                                .clone()
                                .unwrap_or_else(|| other.plural_name());
                            to_many.push((name, other.collection.clone(), field.name.clone()));
                        }
                        RelationKind::OneToOne => {
                            let name = rel
                                .reverse_name
                                .clone()
                                .unwrap_or_else(|| other.singular_name());
                            to_one.push((name, other.collection.clone()));
                        }
                        _ => {}
                    }
                }
            }
        }
        (to_many, to_one)
    }

    // ── Input builders ──

    /// Build scalar + relation fields for CreateInput or UpdateInput.
    fn build_input_fields(
        &self,
        mut input: InputObject,
        collection: &CollectionDef,
        collection_map: &HashMap<&str, &CollectionDef>,
        mutation: &str,
    ) -> InputObject {
        for field in &collection.fields {
            if field.name == "id" || field.name == "_id" {
                continue;
            }
            if let FieldType::Relation(rel) = &field.field_type {
                let target_type = collection_map
                    .get(rel.collection.as_str())
                    .map(|c| c.type_name())
                    .unwrap_or_else(|| rel.collection.clone());
                let input_name = Self::relation_nested_input_name(rel.kind, &target_type, &collection.type_name(), mutation);
                input = input.field(InputValue::new(field.graphql_name(), TypeRef::named(&input_name)));
                continue;
            }
            let field_type = scalars::type_ref(&field.field_type);
            let input_type = if field.required && mutation == "Create" {
                TypeRef::named_nn(field_type.type_name())
            } else {
                field_type
            };
            input = input.field(InputValue::new(field.graphql_name(), input_type));
        }
        input
    }

    /// Add reverse to-many and to-one fields to a CreateInput or UpdateInput.
    fn add_reverse_fields_to_input(
        &self,
        mut input: InputObject,
        reverse_to_many: &[(String, String, String)],
        reverse_to_one: &[(String, String)],
        collection_map: &HashMap<&str, &CollectionDef>,
        source_type: &str,
        mutation: &str,
    ) -> InputObject {
        for (reverse_name, target_coll_name, _) in reverse_to_many {
            if let Some(target_def) = collection_map.get(target_coll_name.as_str()) {
                let input_name = Self::relation_nested_input_name(
                    RelationKind::ManyToMany, &target_def.type_name(), source_type, mutation,
                );
                input = input.field(InputValue::new(reverse_name.clone(), TypeRef::named(&input_name)));
            }
        }
        for (reverse_name, target_coll_name) in reverse_to_one {
            if let Some(target_def) = collection_map.get(target_coll_name.as_str()) {
                let input_name = Self::relation_nested_input_name(
                    RelationKind::OneToOne, &target_def.type_name(), source_type, mutation,
                );
                input = input.field(InputValue::new(reverse_name.clone(), TypeRef::named(&input_name)));
            }
        }
        input
    }

    /// Build the scalar + forward relation fields for a `CreateWithout` or
    /// `UpdateWithout` input. Skips id/_id and relations pointing back to
    /// `source_coll`. For `mutation == "Create"`, required scalars are NN.
    fn build_without_fields(
        &self,
        mut without_input: InputObject,
        target_coll: &CollectionDef,
        source_coll: &CollectionDef,
        mutation: &str,
    ) -> InputObject {
        let collection_map: HashMap<&str, &CollectionDef> = self
            .definition
            .collections
            .iter()
            .map(|c| (c.collection.as_str(), c))
            .collect();

        for field in &target_coll.fields {
            if field.name == "id" || field.name == "_id" {
                continue;
            }
            if let FieldType::Relation(rel) = &field.field_type {
                if rel.collection == source_coll.collection {
                    continue;
                }
                let target_type = collection_map
                    .get(rel.collection.as_str())
                    .map(|c| c.type_name())
                    .unwrap_or_else(|| rel.collection.clone());
                let input_name = Self::relation_nested_input_name(
                    rel.kind, &target_type, &target_coll.type_name(), mutation,
                );
                without_input =
                    without_input.field(InputValue::new(field.graphql_name(), TypeRef::named(&input_name)));
                continue;
            }
            let field_type = scalars::type_ref(&field.field_type);
            let input_type = if field.required && mutation == "Create" {
                TypeRef::named_nn(field_type.type_name())
            } else {
                field_type
            };
            without_input =
                without_input.field(InputValue::new(field.graphql_name(), input_type));
        }
        without_input
    }

    // ── Nested input registration ──

    fn register_nested_inputs_for_field(
        &mut self,
        mut builder: AgSchemaBuilder,
        source_coll: &CollectionDef,
        field: &crate::schema::definition::FieldDef,
    ) -> AgSchemaBuilder {
        let rel = match &field.field_type {
            FieldType::Relation(r) => r,
            _ => return builder,
        };
        let target_def = match self.definition.collection_by_name(&rel.collection) {
            Some(c) => c,
            None => return builder,
        };
        let target_type = target_def.type_name();
        let source_type = source_coll.type_name();

        let create_without = format!("{}CreateWithout{}Input", target_type, source_type);
        if !self.registered_nested_inputs.contains(&create_without) {
            let base = self.build_without_fields(
                InputObject::new(&create_without), target_def, source_coll, "Create",
            );
            let with_reverse;
            (builder, with_reverse) = self.add_reverse_fields_to_without(
                builder, base, target_def, source_coll, "Create",
            );
            builder = builder.register(with_reverse);
            self.registered_nested_inputs.insert(create_without.clone());
        }

        let update_without = format!("{}UpdateWithout{}Input", target_type, source_type);
        if !self.registered_nested_inputs.contains(&update_without) {
            let base = self.build_without_fields(
                InputObject::new(&update_without), target_def, source_coll, "Update",
            );
            let with_reverse;
            (builder, with_reverse) = self.add_reverse_fields_to_without(
                builder, base, target_def, source_coll, "Update",
            );
            builder = builder.register(with_reverse);
            self.registered_nested_inputs.insert(update_without.clone());
        }

        let is_to_many = matches!(rel.kind, RelationKind::ManyToMany);
        builder = self.register_one_or_many_inputs(
            builder, &target_type, &source_type, &create_without, &update_without, is_to_many,
        );

        builder
    }

    /// Register CreateNested{One/Many}Without{Source}Input and
    /// Update{One/Many}Without{Source}NestedInput.
    fn register_one_or_many_inputs(
        &mut self,
        mut builder: AgSchemaBuilder,
        target_type: &str,
        source_type: &str,
        create_without_name: &str,
        update_without_name: &str,
        is_to_many: bool,
    ) -> AgSchemaBuilder {
        let where_unique = format!("{}WhereUniqueInput", target_type);
        let (card, create_ref, connect_ref, disc_del_ref) = if is_to_many {
            ("Many", TypeRef::named_nn_list(create_without_name), TypeRef::named_nn_list(&where_unique), TypeRef::named_nn_list(&where_unique))
        } else {
            ("One", TypeRef::named(create_without_name), TypeRef::named(&where_unique), TypeRef::named("Boolean"))
        };

        let create_name = format!("{}CreateNested{}Without{}Input", target_type, card, source_type);
        if !self.registered_nested_inputs.contains(&create_name) {
            let input = InputObject::new(&create_name)
                .field(InputValue::new("create", create_ref.clone()))
                .field(InputValue::new("connect", connect_ref.clone()));
            builder = builder.register(input);
            self.registered_nested_inputs.insert(create_name);
        }

        // Build update field ref: to-one is nullable named, to-many needs WithWhere wrapper
        let update_ref = if is_to_many {
            let with_where_name = format!("{}UpdateWithWhereUniqueWithout{}Input", target_type, source_type);
            if !self.registered_nested_inputs.contains(&with_where_name) {
                let with_where = InputObject::new(&with_where_name)
                    .field(InputValue::new("where", TypeRef::named_nn(&where_unique)))
                    .field(InputValue::new("data", TypeRef::named_nn(update_without_name)));
                builder = builder.register(with_where);
                self.registered_nested_inputs.insert(with_where_name.clone());
            }
            TypeRef::named_nn_list(&with_where_name)
        } else {
            TypeRef::named(update_without_name)
        };

        let update_name = format!("{}Update{}Without{}NestedInput", target_type, card, source_type);
        if !self.registered_nested_inputs.contains(&update_name) {
            let input = InputObject::new(&update_name)
                .field(InputValue::new("create", create_ref))
                .field(InputValue::new("connect", connect_ref))
                .field(InputValue::new("disconnect", disc_del_ref.clone()))
                .field(InputValue::new("delete", disc_del_ref))
                .field(InputValue::new("update", update_ref));
            builder = builder.register(input);
            self.registered_nested_inputs.insert(update_name);
        }

        builder
    }

    /// Unified reverse input registration. Used for both OneToMany and OneToOne
    /// reverse fields. Threads `builder` through for recursive 3-level support.
    fn register_reverse_inputs(
        &mut self,
        mut builder: AgSchemaBuilder,
        source_coll: &CollectionDef,
        target_coll: &CollectionDef,
        kind: RelationKind,
        _mutation: &str,
    ) -> (AgSchemaBuilder, String) {
        let target_type = target_coll.type_name();
        let source_type = source_coll.type_name();
        let is_to_many = matches!(kind, RelationKind::OneToMany);

        // Build CreateWithout
        let create_without = format!("{}CreateWithout{}Input", target_type, source_type);
        if !self.registered_nested_inputs.contains(&create_without) {
            let base = self.build_without_fields(
                InputObject::new(&create_without), target_coll, source_coll, "Create",
            );
            let with_reverse;
            (builder, with_reverse) = self.add_reverse_fields_to_without(
                builder, base, target_coll, source_coll, "Create",
            );
            builder = builder.register(with_reverse);
            self.registered_nested_inputs.insert(create_without.clone());
        }

        // Build UpdateWithout
        let update_without = format!("{}UpdateWithout{}Input", target_type, source_type);
        if !self.registered_nested_inputs.contains(&update_without) {
            let base = self.build_without_fields(
                InputObject::new(&update_without), target_coll, source_coll, "Update",
            );
            let with_reverse;
            (builder, with_reverse) = self.add_reverse_fields_to_without(
                builder, base, target_coll, source_coll, "Update",
            );
            builder = builder.register(with_reverse);
            self.registered_nested_inputs.insert(update_without.clone());
        }

        builder = self.register_one_or_many_inputs(
            builder, &target_type, &source_type, &create_without, &update_without, is_to_many,
        );

        let nested_name = if is_to_many {
            format!("{}CreateNestedManyWithout{}Input", target_type, source_type)
        } else {
            format!("{}CreateNestedOneWithout{}Input", target_type, source_type)
        };

        (builder, nested_name)
    }

    /// Add reverse fields (OneToMany and OneToOne) to a CreateWithout or
    /// UpdateWithout input for 3-level nesting support.
    fn add_reverse_fields_to_without(
        &mut self,
        mut builder: AgSchemaBuilder,
        mut without_input: InputObject,
        target_coll: &CollectionDef,
        source_coll: &CollectionDef,
        mutation: &str,
    ) -> (AgSchemaBuilder, InputObject) {
        for other_coll in &self.definition.collections {
            if other_coll.collection == target_coll.collection {
                continue;
            }
            for other_field in &other_coll.fields {
                let rel = match &other_field.field_type {
                    FieldType::Relation(r) if r.collection == target_coll.collection => r,
                    _ => continue,
                };
                // Skip reverse fields originating from the source collection —
                // they would create a circular type reference back to source.
                if other_coll.collection == source_coll.collection {
                    continue;
                }
                let (reverse_name, kind) = match rel.kind {
                    RelationKind::OneToMany => (
                        rel.reverse_name.clone().unwrap_or_else(|| other_coll.plural_name()),
                        RelationKind::OneToMany,
                    ),
                    RelationKind::OneToOne => (
                        rel.reverse_name.clone().unwrap_or_else(|| other_coll.singular_name()),
                        RelationKind::OneToOne,
                    ),
                    _ => continue,
                };
                let type_name;
                (builder, type_name) = self.register_reverse_inputs(
                    builder, target_coll, other_coll, kind, mutation,
                );
                without_input = without_input.field(InputValue::new(
                    reverse_name,
                    TypeRef::named(&type_name),
                ));
            }
        }
        (builder, without_input)
    }

    fn relation_nested_input_name(kind: RelationKind, target_type: &str, source_type: &str, mutation: &str) -> String {
        let cardinality = if matches!(kind, RelationKind::ManyToMany) { "Many" } else { "One" };
        match mutation {
            "Create" => format!("{}CreateNested{}Without{}Input", target_type, cardinality, source_type),
            "Update" => format!("{}Update{}Without{}NestedInput", target_type, cardinality, source_type),
            _ => panic!("unknown mutation: {}", mutation),
        }
    }

    // ── Enum & filter registration ──

    fn register_enum(
        &mut self,
        mut builder: AgSchemaBuilder,
        enum_def: &EnumDef,
    ) -> AgSchemaBuilder {
        if self.registered_enums.contains(&enum_def.name) {
            return builder;
        }
        let items: Vec<async_graphql::dynamic::EnumItem> = enum_def
            .values
            .iter()
            .map(|value| async_graphql::dynamic::EnumItem::new(value.clone()))
            .collect();
        builder = builder.register(async_graphql::dynamic::Enum::new(
            enum_def.name.clone(),
        ).items(items));
        self.registered_enums.insert(enum_def.name.clone());
        builder
    }

    fn register_page_info(&self, builder: AgSchemaBuilder) -> AgSchemaBuilder {
        let fields = [
            ("hasNextPage", "Boolean", false),
            ("hasPreviousPage", "Boolean", false),
            ("startCursor", "String", false),
            ("endCursor", "String", false),
        ];
        let mut page_info = Object::new("PageInfo");
        for (field_name, type_name, non_null) in fields {
            let type_ref = if non_null { TypeRef::named_nn(type_name) } else { TypeRef::named(type_name) };
            page_info = page_info.field(nested_field(field_name, type_ref));
        }
        builder.register(page_info)
    }

    fn register_delete_result(&self, builder: AgSchemaBuilder) -> AgSchemaBuilder {
        let mut delete_result = Object::new("DeleteResult");
        delete_result = delete_result
            .field(nested_field("success", TypeRef::named_nn("Boolean")))
            .field(nested_field("deletedId", TypeRef::named_nn("ID")));
        builder.register(delete_result)
    }

    fn register_filter_types(&self, builder: AgSchemaBuilder) -> AgSchemaBuilder {
        builder
            .register(
                async_graphql::dynamic::Enum::new("SortDirection")
                    .item(async_graphql::dynamic::EnumItem::new("ASC"))
                    .item(async_graphql::dynamic::EnumItem::new("DESC")),
            )
            .register(InputObject::new("IdFilter")
                .field(InputValue::new("eq", TypeRef::named("ID")))
                .field(InputValue::new("ne", TypeRef::named("ID"))))
            .register(InputObject::new("StringFilter")
                .field(InputValue::new("eq", TypeRef::named("String")))
                .field(InputValue::new("ne", TypeRef::named("String")))
                .field(InputValue::new("contains", TypeRef::named("String")))
                .field(InputValue::new("startsWith", TypeRef::named("String")))
                .field(InputValue::new("endsWith", TypeRef::named("String"))))
            .register(InputObject::new("IntFilter")
                .field(InputValue::new("eq", TypeRef::named("Int")))
                .field(InputValue::new("ne", TypeRef::named("Int")))
                .field(InputValue::new("gt", TypeRef::named("Int")))
                .field(InputValue::new("gte", TypeRef::named("Int")))
                .field(InputValue::new("lt", TypeRef::named("Int")))
                .field(InputValue::new("lte", TypeRef::named("Int"))))
            .register(InputObject::new("FloatFilter")
                .field(InputValue::new("eq", TypeRef::named("Float")))
                .field(InputValue::new("ne", TypeRef::named("Float")))
                .field(InputValue::new("gt", TypeRef::named("Float")))
                .field(InputValue::new("gte", TypeRef::named("Float")))
                .field(InputValue::new("lt", TypeRef::named("Float")))
                .field(InputValue::new("lte", TypeRef::named("Float"))))
            .register(InputObject::new("BooleanFilter")
                .field(InputValue::new("eq", TypeRef::named("Boolean"))))
            .register(InputObject::new("DateTimeFilter")
                .field(InputValue::new("eq", TypeRef::named("DateTime")))
                .field(InputValue::new("ne", TypeRef::named("DateTime")))
                .field(InputValue::new("gt", TypeRef::named("DateTime")))
                .field(InputValue::new("gte", TypeRef::named("DateTime")))
                .field(InputValue::new("lt", TypeRef::named("DateTime")))
                .field(InputValue::new("lte", TypeRef::named("DateTime"))))
    }
}

// ── Free helpers ──

fn extract_nested(parent: &async_graphql::Value, field_name: &str) -> Option<async_graphql::Value> {
    match parent {
        async_graphql::Value::Object(map) => map.get(field_name).cloned(),
        _ => None,
    }
}

fn filter_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::ID => "IdFilter",
        FieldType::String => "StringFilter",
        FieldType::Int => "IntFilter",
        FieldType::Float => "FloatFilter",
        FieldType::Boolean => "BooleanFilter",
        FieldType::DateTime => "DateTimeFilter",
        FieldType::Json => "StringFilter",
        FieldType::List(_) => "StringFilter",
        FieldType::Relation(_) => "IdFilter",
    }
}

fn nested_field(name: &str, type_ref: TypeRef) -> Field {
    let field_name = name.to_string();
    Field::new(name, type_ref, move |ctx| {
        let name = field_name.clone();
        FieldFuture::new(async move {
            let value = ctx
                .parent_value
                .as_value()
                .and_then(|parent| extract_nested(parent, &name));
            match value {
                Some(val) => Ok(Some(FieldValue::value(val))),
                None => Ok(None),
            }
        })
    })
}

fn resolve_to_field_value(
    result: Result<Option<serde_json::Value>, GraphQLError>,
) -> Result<Option<FieldValue<'static>>, async_graphql::Error> {
    match result {
        Ok(Some(value)) => json_to_field_value(value).map(Some),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
    .map_err(|e| e.into_graphql_error())
}
