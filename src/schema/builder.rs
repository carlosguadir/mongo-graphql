use std::collections::HashSet;

use async_graphql::dynamic::{
    FieldFuture, FieldValue, InputObject, Object, Schema,
    SchemaBuilder as AgSchemaBuilder, TypeRef,
};
use mongodb::Database;

use crate::error::GraphQLError;
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

    pub fn build(mut self, db: Database) -> Result<Schema, GraphQLError> {
        let mut builder = Schema::build("Query", Some("Mutation"), None);
        builder = scalars::register_all(builder);
        builder = self.register_page_info(builder);
        builder = self.register_delete_result(builder);

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
        let object_type = TypeRef::named(type_name.clone());

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
            obj = obj.field(async_graphql::dynamic::Field::new(
                field.graphql_name(),
                field_type,
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ));
        }
        builder = builder.register(obj);

        let where_unique = InputObject::new(format!("{}WhereUniqueInput", type_name))
            .field(async_graphql::dynamic::InputValue::new(
                "id",
                TypeRef::named_nn("ID"),
            ));
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
            let nn = if field.required {
                TypeRef::named_nn(field_type.type_name())
            } else {
                field_type
            };
            create_input = create_input
                .field(async_graphql::dynamic::InputValue::new(field.graphql_name(), nn));
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
            update_input = update_input
                .field(async_graphql::dynamic::InputValue::new(field.graphql_name(), field_type));
        }
        builder = builder.register(update_input);

        let where_input = InputObject::new(format!("{}WhereInput", type_name)).field(
            async_graphql::dynamic::InputValue::new("id", TypeRef::named("IdFilter")),
        );
        builder = builder.register(where_input);

        let sort_input = InputObject::new(format!("{}SortInput", type_name)).field(
            async_graphql::dynamic::InputValue::new("id", TypeRef::named("SortDirection")),
        );
        builder = builder.register(sort_input);

        let conn_name = format!("{}Connection", type_name);
        let conn_obj = Object::new(conn_name.clone())
            .field(async_graphql::dynamic::Field::new(
                "edges",
                TypeRef::named_nn_list(type_name.clone()),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ))
            .field(async_graphql::dynamic::Field::new(
                "pageInfo",
                TypeRef::named_nn("PageInfo"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ))
            .field(async_graphql::dynamic::Field::new(
                "totalCount",
                TypeRef::named("Int"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ));
        builder = builder.register(conn_obj);

        let where_unique_type = TypeRef::named_nn(format!("{}WhereUniqueInput", type_name));
        let coll_for_get = collection.clone();
        let db_for_get = db.clone();

        query_root = query_root.field(
            async_graphql::dynamic::Field::new(
                collection.singular_name(),
                object_type.clone(),
                move |_| {
                    let _def = coll_for_get.clone();
                    let _db = db_for_get.clone();
                    FieldFuture::new(async { Ok(Some(FieldValue::NULL)) })
                },
            )
            .argument(async_graphql::dynamic::InputValue::new("where", where_unique_type)),
        );

        let coll_for_list = collection.clone();
        let db_for_list = db.clone();
        let conn_type = TypeRef::named_nn(conn_name);
        let where_type = TypeRef::named(format!("{}WhereInput", type_name));
        let sort_type = TypeRef::named(format!("{}SortInput", type_name));

        query_root = query_root.field(
            async_graphql::dynamic::Field::new(
                collection.plural_name(),
                conn_type,
                move |_| {
                    let _def = coll_for_list.clone();
                    let _db = db_for_list.clone();
                    FieldFuture::new(async { Ok(Some(FieldValue::NULL)) })
                },
            )
            .argument(async_graphql::dynamic::InputValue::new("first", TypeRef::named("Int")))
            .argument(async_graphql::dynamic::InputValue::new("after", TypeRef::named("String")))
            .argument(async_graphql::dynamic::InputValue::new("where", where_type))
            .argument(async_graphql::dynamic::InputValue::new(
                "sort",
                TypeRef::named_nn_list(sort_type.type_name()),
            )),
        );

        let coll_for_create = collection.clone();
        let db_for_create = db.clone();
        let create_type = TypeRef::named_nn(format!("{}CreateInput", type_name));

        mutation_root = mutation_root.field(
            async_graphql::dynamic::Field::new(
                format!("create{}", type_name),
                object_type.clone(),
                move |_| {
                    let _def = coll_for_create.clone();
                    let _db = db_for_create.clone();
                    FieldFuture::new(async { Ok(Some(FieldValue::NULL)) })
                },
            )
            .argument(async_graphql::dynamic::InputValue::new("input", create_type)),
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
            .map(|v| async_graphql::dynamic::EnumItem::new(v.clone()))
            .collect();
        builder = builder.register(
            async_graphql::dynamic::Enum::new(enum_def.name.clone()).items(items),
        );
        self.registered_enums.insert(enum_def.name.clone());
        builder
    }

    fn register_page_info(
        &mut self,
        builder: AgSchemaBuilder,
    ) -> AgSchemaBuilder {
        let page_info = Object::new("PageInfo")
            .field(async_graphql::dynamic::Field::new(
                "hasNextPage",
                TypeRef::named_nn("Boolean"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ))
            .field(async_graphql::dynamic::Field::new(
                "hasPreviousPage",
                TypeRef::named_nn("Boolean"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ))
            .field(async_graphql::dynamic::Field::new(
                "startCursor",
                TypeRef::named("String"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ))
            .field(async_graphql::dynamic::Field::new(
                "endCursor",
                TypeRef::named("String"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ));
        builder.register(page_info)
    }

    fn register_delete_result(
        &mut self,
        builder: AgSchemaBuilder,
    ) -> AgSchemaBuilder {
        let delete_result = Object::new("DeleteResult")
            .field(async_graphql::dynamic::Field::new(
                "success",
                TypeRef::named_nn("Boolean"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ))
            .field(async_graphql::dynamic::Field::new(
                "deletedId",
                TypeRef::named_nn("ID"),
                |_| FieldFuture::new(async { Ok(Some(FieldValue::NULL)) }),
            ));
        builder.register(delete_result)
    }
}
