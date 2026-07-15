# MongoDB GraphQL

Dynamic GraphQL schema generation from MongoDB collections, optimized for AWS Lambda.

Given a JSON schema definition describing your MongoDB collections, this library generates a fully functional GraphQL API at runtime — no code generation, no schema duplication.

## Quick start

### 1. Define your schema (`schema-definition.json`)

```json
{
  "collections": [
    {
      "collection": "hero",
      "graphql_name": "Hero",
      "fields": [
        { "name": "id", "type": "ID", "required": true },
        { "name": "alias", "type": "String", "required": true, "unique": true },
        { "name": "power_level", "type": "Int", "required": true },
        { "name": "active", "type": "Boolean", "required": true },
        { "name": "rank", "type": "String", "enum": { "name": "Rank", "values": ["S", "A", "B", "C"] } },
        { "name": "joined_at", "type": "DateTime", "required": true },
        {
          "name": "team_id",
          "graphql_name": "team",
          "type": { "relation": { "kind": "one_to_many", "collection": "team", "reference_field": "id" } }
        }
      ]
    }
  ]
}
```

### 2. Build the schema

```rust
use mongo_graphql::schema::builder::{RuntimeConfig, SchemaBuilder};
use mongo_graphql::schema::parser::SchemaParser;

let json = std::fs::read_to_string("schema-definition.json")?;
let definition = SchemaParser::from_str(&json)?;

let config = RuntimeConfig { max_page_size: 100 };
let schema = SchemaBuilder::new(&config, &definition)
    .build(db)
    .await?;
```

### 3. Execute queries

```rust
use mongo_graphql::executor;

let result = executor::execute(
    &schema,
    r#"query { hero(where: { id: "..." }) { id alias powerLevel } }"#,
    async_graphql::Variables::default(),
).await?;
```

Generated queries and mutations per collection:

| Operation | GraphQL |
|-----------|---------|
| Singular lookup | `hero(where: { id: "..." }) { ... }` |
| Paginated list | `heroes(first: 10, after: "...", where: {...}, sort: { id: ASC })` |
| Create | `createHero(input: { alias: "...", ... }) { ... }` |
| Update | `updateHero(where: { id: "..." }, input: { alias: "..." }) { ... }` |
| Delete | `deleteHero(where: { id: "..." }) { success deletedId }` |

## Schema definition reference

### Field types

| JSON type | GraphQL type | BSON storage |
|-----------|-------------|--------------|
| `"ID"` | `ID` | `ObjectId` |
| `"String"` | `String` | `String` |
| `"Int"` | `Int` | `Int32` / `Int64` |
| `"Float"` | `Float` | `Double` |
| `"Boolean"` | `Boolean` | `Boolean` |
| `"DateTime"` | `DateTime` (ISO 8601) | `DateTime` |
| `"Json"` | `Json` | `Document` / `Array` |
| `["String"]` | `[String!]` | `Array` |

### Field options

| Option | Description |
|--------|-------------|
| `required: true` | Non-null in GraphQL schema |
| `unique: true` | Reserved for future index-aware filtering |
| `graphql_name` | Override the GraphQL field name (default: same as `name`) |
| `enum` | Inline enum definition `{ "name": "...", "values": [...] }` |
| `description` | Documentation string |

### Relation kinds

| Kind | Description |
|------|-------------|
| `one_to_many` | FK on this collection → referenced collection |
| `one_to_one` | Unique FK on one side |
| `many_to_many` | Requires `junction` with collection, local/foreign fields |

Relations are defined in the schema but not yet resolved in queries (V1).

## Commands

```bash
# Build
cargo build

# Unit tests (no external dependencies)
cargo test

# Integration tests (requires MongoDB on localhost:27017)
cargo test --features integration

# Specific test file
cargo test --test pagination_tests

# Lambda binary (build)
cargo build --features lambda

# Local Lambda dev server with hot reload (requires cargo-lambda + MongoDB)

# .env file
# MONGO_URI=mongodb://localhost:27017
# DATABASE_NAME=mongo-graphql

cargo lambda watch --features lambda

# Watch mode (requires cargo-watch)
cargo watch -x test
```

## Architecture

```
schema-definition.json
        │
        ▼
  SchemaParser        ← validation (relations, enums, identifiers)
        │
        ▼
  SchemaDefinition    ← CollectionDef, FieldDef, FieldType, EnumDef
        │
        ▼
  SchemaBuilder       ← registers types, inputs, queries, mutations
        │
        ▼
  DynamicSchema       ← async-graphql schema (ready to execute)
        │
        ▼
  executor::execute    ← query + variables → JSON response
```

### Source tree

```
src/
├── lib.rs                    # crate root
├── error.rs                  # GraphQLError (11 variants + error codes)
├── executor.rs               # execute(schema, query, variables) → JSON
├── helpers/
│   └── serialization.rs      # BSON ↔ JSON, document_to_graphql_value, input_doc_to_mongo
├── resolvers/
│   ├── query.rs              # resolve_get, resolve_list, transform_id_filter, transform_where_filter
│   ├── mutation.rs           # resolve_create, resolve_update, resolve_delete
│   └── pagination.rs         # PaginationArgs, encode/decode cursor
├── schema/
│   ├── definition.rs         # Schema data model (CollectionDef, FieldDef, FieldType, …)
│   ├── parser.rs             # JSON parsing + validation
│   ├── builder.rs            # DynamicSchema construction (SchemaBuilder)
│   └── inflect.rs            # to_plural, to_pascal_singular
└── types/
    └── scalars.rs            # DateTime/Json scalar registration + type_ref mapping

examples/
└── lambda.rs                 # AWS Lambda handler (API Gateway V2 → GraphQL endpoint)
```

## Local development

```bash
# Install cargo-lambda
cargo install cargo-lambda

# Start MongoDB (if not running)
docker run -d --name mongo-dev -p 27017:27017 mongo:7

# Start local Lambda emulator
MONGO_URI=mongodb://localhost:27017 DATABASE_NAME=test_graphql_mongodb \
  cargo lambda watch --example lambda
```

The emulator exposes `http://localhost:9000`. Send GraphQL queries via POST:

```bash
curl -s -X POST http://localhost:9000 \
  -H "Content-Type: application/json" \
  -d '{"query": "{ heroes(first: 3) { edges { alias } totalCount } }"}'
```

### Environment variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `MONGO_URI` | yes | — | MongoDB connection string |
| `DATABASE_NAME` | yes | — | Target database name |
| `MAX_PAGE_SIZE` | no | `100` | Max items per paginated query |
| `SCHEMA_PATH` | no | embedded at build time | Path to schema JSON |

## API error codes

| Code | HTTP status | Meaning |
|------|-------------|---------|
| `NOT_FOUND` | 200 (GraphQL) | Document not found |
| `UNAUTHORIZED` | 200 | Missing/invalid auth |
| `FORBIDDEN` | 200 | Insufficient permissions |
| `DUPLICATE_KEY` | 200 | Unique constraint violation |
| `INVALID_CURSOR` | 200 | Malformed pagination cursor |
| *(hidden)* | 200 | Internal / database / config / schema errors |

## License

MIT
