use std::collections::HashSet;

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputObject, InputValue, Object, Schema,
    SchemaBuilder as AgSchemaBuilder, TypeRef,
};
use mongodb::Database;

use crate::error::GraphQLError;
use crate::helpers::serialization::json_to_field_value;
use crate::resolvers::{mutation, query};
use crate::schema::definition::{CollectionDef, EnumDef, FieldType, SchemaDefinition};
use crate::types::scalars;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub max_page_size: usize,
}

pub struct SchemaBuilder<'a> {
    config: &'a RuntimeConfig,
    definition: &'a SchemaDefinition,
    registered_enums: HashSet<String>,
}

impl<'a> SchemaBuilder<'a> {
    pub fn new(config: &'a RuntimeConfig, definition: &'a SchemaDefinition) -> Self {
        Self {
            config,
            definition,
            registered_enums: HashSet::new(),
        }
    }

    pub async fn build(mut self, db: Database) -> Result<Schema, GraphQLError> {
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
            (builder, query_root, mutation_root) = self.register_collection(
                builder, query_root, mutation_root, collection, &db,
            )?;
        }

        builder = builder.register(query_root);
        builder = builder.register(mutation_root);

        let schema = builder
            .data(db)
            .data(self.config.clone())
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
        db: &Database,
    ) -> Result<(AgSchemaBuilder, Object, Object), GraphQLError> {
        let type_name = collection.type_name();

        for field in &collection.fields {
            if let Some(enum_def) = &field.r#enum {
                builder = self.register_enum(builder, enum_def);
            }
        }

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

        let where_unique = InputObject::new(format!("{}WhereUniqueInput", type_name))
            .field(InputValue::new("id", TypeRef::named_nn("ID")));
        builder = builder.register(where_unique);

        let mut create_input = InputObject::new(format!("{}CreateInput", type_name));
        for field in &collection.fields {
            if field.name == "id" || field.name == "_id" {
                continue;
            }
            if matches!(field.field_type, FieldType::Relation(_)) {
                continue;
            }
            let field_type = scalars::type_ref(&field.field_type);
            let input_type = if field.required {
                TypeRef::named_nn(field_type.type_name())
            } else {
                field_type
            };
            create_input =
                create_input.field(InputValue::new(field.graphql_name(), input_type));
        }
        builder = builder.register(create_input);

        let mut update_input = InputObject::new(format!("{}UpdateInput", type_name));
        for field in &collection.fields {
            if field.name == "id" || field.name == "_id" {
                continue;
            }
            if matches!(field.field_type, FieldType::Relation(_)) {
                continue;
            }
            let field_type = scalars::type_ref(&field.field_type);
            update_input =
                update_input.field(InputValue::new(field.graphql_name(), field_type));
        }
        builder = builder.register(update_input);

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

        let mut sort_input =
            InputObject::new(format!("{}SortInput", type_name));
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
        let edge_name = format!("{}Edge", type_name);
        let edge_obj = Object::new(edge_name.clone())
            .field(Field::new(
                "node",
                TypeRef::named_nn(type_name.clone()),
                move |ctx| {
                    let name = "node".to_string();
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
            ))
            .field(Field::new(
                "cursor",
                TypeRef::named_nn("String"),
                move |ctx| {
                    let name = "cursor".to_string();
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
        builder = builder.register(edge_obj);

        let conn_name = format!("{}Connection", type_name);
        let conn_obj = Object::new(conn_name.clone())
            .field(Field::new(
                "edges",
                TypeRef::named_nn_list(edge_name),
                move |ctx| {
                    let name = "edges".to_string();
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
            ))
            .field(Field::new(
                "pageInfo",
                TypeRef::named_nn("PageInfo"),
                move |ctx| {
                    let name = "pageInfo".to_string();
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
            ))
            .field(Field::new(
                "totalCount",
                TypeRef::named_nn("Int"),
                move |ctx| {
                    let name = "totalCount".to_string();
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
        builder = builder.register(conn_obj);

        // ---- Query: singular ----
        let coll_for_get = collection.clone();
        let db_for_get = db.clone();
        let where_unique_type = TypeRef::named_nn(format!("{}WhereUniqueInput", type_name));

        query_root = query_root.field(
            Field::new(
                collection.singular_name(),
                object_ref.clone(),
                move |ctx| {
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
                },
            )
            .argument(InputValue::new("where", where_unique_type)),
        );

        // ---- Query: plural ----
        let coll_for_list = collection.clone();
        let db_for_list = db.clone();
        let conn_type = TypeRef::named_nn(conn_name.clone());
        let where_type = TypeRef::named(format!("{}WhereInput", type_name));
        let sort_type = TypeRef::named(format!("{}SortInput", type_name));
        let page_size = self.config.max_page_size;

        query_root = query_root.field(
            Field::new(
                collection.plural_name(),
                conn_type,
                move |ctx| {
                    let coll_def = coll_for_list.clone();
                    let db = db_for_list.clone();
                    FieldFuture::new(async move {
                        let result =
                            query::resolve_list(ctx, &coll_def, &db, page_size).await;
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
            .argument(InputValue::new("where", where_type))
            .argument(InputValue::new("sort", sort_type)),
        );

        // ---- Mutation: create ----
        let coll_for_create = collection.clone();
        let db_for_create = db.clone();
        let create_type = TypeRef::named_nn(format!("{}CreateInput", type_name));

        mutation_root = mutation_root.field(
            Field::new(
                format!("create{}", type_name),
                object_ref.clone(),
                move |ctx| {
                    let coll_def = coll_for_create.clone();
                    let db = db_for_create.clone();
                    FieldFuture::new(async move {
                        let result = mutation::resolve_create(ctx, &coll_def, &db).await;
                        (match result {
                            Ok(Some(value)) => json_to_field_value(value).map(Some),
                            Ok(None) => Ok(None),
                            Err(e) => Err(e),
                        })
                        .map_err(|e| e.into_graphql_error())
                    })
                },
            )
            .argument(InputValue::new("input", create_type)),
        );

        // ---- Mutation: update ----
        let coll_for_update = collection.clone();
        let db_for_update = db.clone();
        let update_type = TypeRef::named_nn(format!("{}UpdateInput", type_name));
        let where_unique_type_update =
            TypeRef::named_nn(format!("{}WhereUniqueInput", type_name));

        mutation_root = mutation_root.field(
            Field::new(
                format!("update{}", type_name),
                object_ref.clone(),
                move |ctx| {
                    let coll_def = coll_for_update.clone();
                    let db = db_for_update.clone();
                    FieldFuture::new(async move {
                        let result = mutation::resolve_update(ctx, &coll_def, &db).await;
                        (match result {
                            Ok(Some(value)) => json_to_field_value(value).map(Some),
                            Ok(None) => Ok(None),
                            Err(e) => Err(e),
                        })
                        .map_err(|e| e.into_graphql_error())
                    })
                },
            )
            .argument(InputValue::new("where", where_unique_type_update))
            .argument(InputValue::new("input", update_type)),
        );

        // ---- Mutation: delete ----
        let coll_for_delete = collection.clone();
        let db_for_delete = db.clone();
        let where_unique_type_delete =
            TypeRef::named_nn(format!("{}WhereUniqueInput", type_name));

        mutation_root = mutation_root.field(
            Field::new(
                format!("delete{}", type_name),
                TypeRef::named_nn("DeleteResult"),
                move |ctx| {
                    let coll_def = coll_for_delete.clone();
                    let db = db_for_delete.clone();
                    FieldFuture::new(async move {
                        let result = mutation::resolve_delete(ctx, &coll_def, &db).await;
                        (match result {
                            Ok(Some(value)) => json_to_field_value(value).map(Some),
                            Ok(None) => Ok(None),
                            Err(e) => Err(e),
                        })
                        .map_err(|e| e.into_graphql_error())
                    })
                },
            )
            .argument(InputValue::new("where", where_unique_type_delete)),
        );

        Ok((builder, query_root, mutation_root))
    }

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
            ("hasNextPage", "Boolean"),
            ("hasPreviousPage", "Boolean"),
            ("startCursor", "String"),
            ("endCursor", "String"),
        ];
        let mut page_info = Object::new("PageInfo");
        for (field_name, type_name) in fields {
            let name = field_name.to_string();
            page_info = page_info.field(Field::new(
                field_name,
                TypeRef::named(type_name),
                move |ctx| {
                    let field_name = name.clone();
                    FieldFuture::new(async move {
                        let value = ctx
                            .parent_value
                            .as_value()
                            .and_then(|parent| extract_nested(parent, &field_name));
                        match value {
                            Some(val) => Ok(Some(FieldValue::value(val))),
                            None => Ok(None),
                        }
                    })
                },
            ));
        }
        builder.register(page_info)
    }

    fn register_delete_result(&self, builder: AgSchemaBuilder) -> AgSchemaBuilder {
        let fields = [("success", "Boolean"), ("deletedId", "ID")];
        let mut delete_result = Object::new("DeleteResult");
        for (field_name, type_name) in fields {
            let name = field_name.to_string();
            delete_result = delete_result.field(Field::new(
                field_name,
                TypeRef::named_nn(type_name),
                move |ctx| {
                    let field_name = name.clone();
                    FieldFuture::new(async move {
                        let value = ctx
                            .parent_value
                            .as_value()
                            .and_then(|parent| extract_nested(parent, &field_name));
                        match value {
                            Some(val) => Ok(Some(FieldValue::value(val))),
                            None => Ok(None),
                        }
                    })
                },
            ));
        }
        builder.register(delete_result)
    }

    fn register_filter_types(&self, builder: AgSchemaBuilder) -> AgSchemaBuilder {
        builder
            .register(
                async_graphql::dynamic::Enum::new("SortDirection")
                    .item(async_graphql::dynamic::EnumItem::new("ASC"))
                    .item(async_graphql::dynamic::EnumItem::new("DESC")),
            )
            .register(
                InputObject::new("IdFilter")
                    .field(InputValue::new("eq", TypeRef::named("ID")))
                    .field(InputValue::new("ne", TypeRef::named("ID"))),
            )
            .register(
                InputObject::new("StringFilter")
                    .field(InputValue::new("eq", TypeRef::named("String")))
                    .field(InputValue::new("ne", TypeRef::named("String")))
                    .field(InputValue::new("contains", TypeRef::named("String")))
                    .field(InputValue::new("startsWith", TypeRef::named("String")))
                    .field(InputValue::new("endsWith", TypeRef::named("String"))),
            )
            .register(
                InputObject::new("IntFilter")
                    .field(InputValue::new("eq", TypeRef::named("Int")))
                    .field(InputValue::new("ne", TypeRef::named("Int")))
                    .field(InputValue::new("gt", TypeRef::named("Int")))
                    .field(InputValue::new("gte", TypeRef::named("Int")))
                    .field(InputValue::new("lt", TypeRef::named("Int")))
                    .field(InputValue::new("lte", TypeRef::named("Int"))),
            )
            .register(
                InputObject::new("FloatFilter")
                    .field(InputValue::new("eq", TypeRef::named("Float")))
                    .field(InputValue::new("ne", TypeRef::named("Float")))
                    .field(InputValue::new("gt", TypeRef::named("Float")))
                    .field(InputValue::new("gte", TypeRef::named("Float")))
                    .field(InputValue::new("lt", TypeRef::named("Float")))
                    .field(InputValue::new("lte", TypeRef::named("Float"))),
            )
            .register(
                InputObject::new("BooleanFilter")
                    .field(InputValue::new("eq", TypeRef::named("Boolean"))),
            )
            .register(
                InputObject::new("DateTimeFilter")
                    .field(InputValue::new("eq", TypeRef::named("DateTime")))
                    .field(InputValue::new("ne", TypeRef::named("DateTime")))
                    .field(InputValue::new("gt", TypeRef::named("DateTime")))
                    .field(InputValue::new("gte", TypeRef::named("DateTime")))
                    .field(InputValue::new("lt", TypeRef::named("DateTime")))
                    .field(InputValue::new("lte", TypeRef::named("DateTime"))),
            )
    }
}

/// Extract a nested field value from a `ConstValue::Object` by name.
fn extract_nested(parent: &async_graphql::Value, field_name: &str) -> Option<async_graphql::Value> {
    match parent {
        async_graphql::Value::Object(map) => map.get(field_name).cloned(),
        _ => None,
    }
}

/// Map a `FieldType` to the name of its corresponding filter input type.
fn filter_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::ID => "IdFilter",
        FieldType::String => "StringFilter",
        FieldType::Int => "IntFilter",
        FieldType::Float => "FloatFilter",
        FieldType::Boolean => "BooleanFilter",
        FieldType::DateTime => "DateTimeFilter",
        // TODO there will be a feature to add filter in json fields
        FieldType::Json => "StringFilter",
        FieldType::List(_) => "StringFilter",
        FieldType::Relation(_) => "IdFilter",
    }
}
