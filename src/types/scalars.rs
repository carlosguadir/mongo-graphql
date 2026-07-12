use async_graphql::dynamic::TypeRef;

pub fn register_all(mut builder: async_graphql::dynamic::SchemaBuilder) -> async_graphql::dynamic::SchemaBuilder {
    builder = builder.register(
        async_graphql::dynamic::Scalar::new("DateTime")
            .description("ISO 8601 date-time string"),
    );
    builder = builder.register(
        async_graphql::dynamic::Scalar::new("Json")
            .description("Arbitrary JSON value"),
    );
    builder
}

pub fn type_ref(ft: &crate::schema::definition::FieldType) -> TypeRef {
    use crate::schema::definition::FieldType;

    match ft {
        FieldType::ID => TypeRef::named("ID"),
        FieldType::String => TypeRef::named("String"),
        FieldType::Int => TypeRef::named("Int"),
        FieldType::Float => TypeRef::named("Float"),
        FieldType::Boolean => TypeRef::named("Boolean"),
        FieldType::DateTime => TypeRef::named("DateTime"),
        FieldType::Json => TypeRef::named("Json"),
        FieldType::List(inner) => TypeRef::named_list(type_ref(inner).type_name()),
        FieldType::Relation(_) => TypeRef::named("ID"),
    }
}
