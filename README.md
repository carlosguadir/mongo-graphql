# MongoDB GraphQL

Dynamic GraphQL API from MongoDB collections — define your schema in JSON, get a fully functional
GraphQL endpoint at runtime. No code generation, no schema duplication.

## Quick start

### 1. Define your schema (`schema-definition.json`)

```json
{
  "collections": [
    {
      "collection": "hero",
      "graphql_name": "Hero",
      "description": "Superheroes registered in the league",
      "fields": [
        { "name": "id", "type": "ID", "required": true },
        { "name": "alias", "type": "String", "required": true, "unique": true },
        { "name": "secret_identity", "type": "String", "required": true },
        { "name": "power_level", "type": "Int", "required": true },
        { "name": "height", "type": "Float" },
        { "name": "active", "type": "Boolean", "required": true },
        { "name": "rank", "type": "String", "enum": { "name": "Rank", "values": ["OMEGA", "ALPHA", "BETA"] } },
        { "name": "joined_at", "type": "DateTime", "required": true },
        { "name": "bio", "type": "String" },
        { "name": "custom_fields", "type": "Json" },
        { "name": "known_aliases", "type": ["String"] },
        {
          "name": "team_id",
          "graphql_name": "team",
          "type": { "relation": { "kind": "one_to_many", "collection": "team", "reference_field": "id", "reverse_name": "members" } }
        },
        {
          "name": "missions",
          "type": {
            "relation": {
              "kind": "many_to_many",
              "collection": "mission",
              "reference_field": "id",
              "junction": {
                "collection": "hero_mission",
                "local_field": "hero_id",
                "foreign_field": "mission_id",
                "foreign_reference": "id",
                "metadata_fields": ["role", "outcome"]
              }
            }
          }
        }
      ]
    },
    {
      "collection": "team",
      "graphql_name": "Team",
      "fields": [
        { "name": "id", "type": "ID", "required": true },
        { "name": "name", "type": "String", "required": true, "unique": true },
        { "name": "founded_at", "type": "DateTime", "required": true },
        { "name": "is_official", "type": "Boolean", "required": true }
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
    .build(client, db, None)
    .await?;
```

### 3. Execute queries

```rust
use mongo_graphql::executor;

let result = executor::execute(
    &schema,
    r#"query { hero(where: { id: "64a1b2c3..." }) { id alias powerLevel } }"#,
    async_graphql::Variables::default(),
    None,
    None,
).await?;
```

## Generated API

The library generates the following per-collection:

| Root field | GraphQL signature |
|------------|-------------------|
| Singular | `hero(where: { id: "..." }): Hero` |
| Paginated list | `heroes(first: Int, after: String, last: Int, before: String, where: HeroWhereInput, sort: HeroSortInput): HeroConnection` |
| Create | `createHero(input: HeroCreateInput!): Hero` |
| Update | `updateHero(where: HeroWhereUniqueInput!, input: HeroUpdateInput!): Hero` |
| Delete | `deleteHero(where: HeroWhereUniqueInput!): DeleteResult` |

### Pagination

Relay-style cursor pagination with `first`/`after` (forward) or `last`/`before` (backward):

```graphql
heroes(first: 10) {
  edges { alias }
  pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
  totalCount
}
```

### Sort

```graphql
heroes(sort: { powerLevel: DESC, alias: ASC }) { edges { alias } }
```

## Filtering

### Scalar operators

All scalar fields accept a filter object with these operators:

| Operator | MongoDB equivalent | Types |
|----------|-------------------|-------|
| `eq` | `$eq` | All |
| `ne` | `$ne` | All |
| `gt` | `$gt` | Int, Float, DateTime |
| `gte` | `$gte` | Int, Float, DateTime |
| `lt` | `$lt` | Int, Float, DateTime |
| `lte` | `$lte` | Int, Float, DateTime |
| `contains` | `$regex` | String |
| `startsWith` | `$regex` | String |
| `endsWith` | `$regex` | String |

Multiple operators on the same field combine with implicit AND:

```graphql
heroes(where: { joinedAt: { gte: "2024-01-01T00:00:00Z", lte: "2024-12-31T00:00:00Z" } }) { ... }
```

Multiple fields also combine with AND:

```graphql
heroes(where: { active: { eq: true }, powerLevel: { gt: 500 } }) { ... }
```

### Nested relation filters

Filter parent documents by conditions on related documents using `some`, `every`, or `none`:

```graphql
# Heroes who have at least one mission with danger_level > 5
heroes(where: { missions: { some: { dangerLevel: { gt: 5 } } } }) { ... }

# Heroes whose every mission is status "completed"
heroes(where: { missions: { every: { status: { eq: "completed" } } } }) { ... }

# Heroes with no mission in location "Metropolis"
heroes(where: { missions: { none: { location: { eq: "Metropolis" } } } }) { ... }
```

Reverse relations also support nested filters:

```graphql
# Teams whose members include at least one hero with power_level > 1000
teams(where: { members: { some: { powerLevel: { gt: 1000 } } } }) { ... }
```

## Relations

Three kinds of relations are supported:

| Kind | FK location | Example |
|------|-------------|---------|
| `one_to_many` | On current collection | `hero.team_id` → `team` |
| `one_to_one` | On current collection | `hero.secret_lair_id` ↔ `secret_lair` |
| `many_to_many` | Via junction collection | `hero` ↔ `hero_mission` → `mission` |

### OneToMany

```json
{ "name": "team_id", "graphql_name": "team",
  "type": { "relation": { "kind": "one_to_many", "collection": "team", "reference_field": "id", "reverse_name": "members" } } }
```

- Forward: `hero { team { name } }` — resolves `team_id` FK lookup
- Reverse: `team { members { alias } }` — resolves documents where `team_id` matches

### OneToOne

```json
{ "name": "archenemy_id", "graphql_name": "archenemy",
  "type": { "relation": { "kind": "one_to_one", "collection": "villain", "reference_field": "id", "reverse_name": "archenemy_of" } } }
```

- Forward: `hero { archenemy { alias } }` — resolves FK lookup, returns single object or null
- Reverse: `villain { archenemy_of { alias } }` — resolves reverse side

### ManyToMany

Both sides must be declared. The junction collection stores the relationship:

```json
{
  "name": "missions",
  "type": {
    "relation": {
      "kind": "many_to_many",
      "collection": "mission",
      "reference_field": "id",
      "junction": {
        "collection": "hero_mission",
        "local_field": "hero_id",
        "foreign_field": "mission_id",
        "foreign_reference": "id",
        "metadata_fields": ["role", "outcome"]
      }
    }
  }
}
```

- `junction.collection` — the junction/M2M table name in MongoDB
- `junction.local_field` — FK to the current collection
- `junction.foreign_field` — FK to the target collection
- `junction.foreign_reference` — field on the target collection to match against
- `junction.metadata_fields` — extra columns on the junction to include in resolution

## N+1 and DataLoader

Relation resolvers automatically batch queries using a per-request **DataLoader**. Without it,
a page of 20 heroes with `missions` would issue 41 queries (1 list + 20×2 junction+targets).
With the DataLoader, concurrent sibling resolvers register their keys during a short batching
window, then a single `$in` query resolves them all — **~3 queries instead of 41**.

The DataLoader is injected transparently via an async-graphql extension registered at schema
build time. No caller configuration needed — every request gets a fresh, isolated loader.

## Schema definition reference

### Field types

| JSON type | GraphQL type | BSON storage |
|-----------|-------------|--------------|
| `"ID"` | `ID` (hex string) | `ObjectId` → `_id` |
| `"String"` | `String` | `String` |
| `"Int"` | `Int` | `Int32` / `Int64` |
| `"Float"` | `Float` | `Double` |
| `"Boolean"` | `Boolean` | `Boolean` |
| `"DateTime"` | `DateTime` (ISO 8601) | `DateTime` |
| `"Json"` | `Json` (arbitrary JSON) | `Document` / `Array` |
| `["String"]` | `[String!]` | `Array` |

Note: `id` is always projected from MongoDB's `_id` field. There is no separate `id` column
in the database — the library handles the mapping transparently in both queries and mutations.

### Collection options

| Option | Description |
|--------|-------------|
| `collection` | MongoDB collection name (required) |
| `graphql_name` | Override the GraphQL type name (default: PascalCase singular of `collection`) |
| `description` | Documentation string exposed in the GraphQL schema |
| `directives` | Per-operation directives for extensions (see below) |

### Field options

| Option | Description |
|--------|-------------|
| `name` | MongoDB field name (required) |
| `type` | Field type — scalar, list, or relation object |
| `required` | Non-null in GraphQL schema (default: false) |
| `unique` | Reserved for future index-aware filtering |
| `graphql_name` | Override the GraphQL field name (default: same as `name`) |
| `enum` | Inline enum definition `{ "name": "...", "values": [...] }` |
| `description` | Documentation string exposed in the GraphQL schema |
| `exclude_from` | List of operation names to exclude this field from input types (e.g. `["Create", "Update"]`). The field remains in output types. Case-insensitive. |

### Directives

```json
{
  "directives": {
    "get": [{ "name": "auth", "args": { "requires": "ADMIN" } }],
    "list": [{ "name": "rateLimit", "args": { "window": "1m", "max": 100 } }]
  }
}
```

Directives are attached to auto-generated query/mutation root fields and are available to
extensions via `ctx.field().directives()`.

## API error codes

| Code | Meaning |
|------|---------|
| `NOT_FOUND` | Document not found |
| `UNAUTHORIZED` | Missing/invalid auth |
| `FORBIDDEN` | Insufficient permissions |
| `DUPLICATE_KEY` | Unique constraint violation |
| `INVALID_CURSOR` | Malformed pagination cursor |
| *(hidden)* | Internal / database / config / schema errors (message hidden from client) |

## Commands

```bash
cargo build
cargo test                        # unit tests (no external dependencies)
cargo test --features integration # integration tests (requires MongoDB on localhost:27017)
cargo test --test pagination_tests
```

## Architecture

```
schema-definition.json
        │
        ▼
  SchemaParser         ← validation: relations, enums, fields, junction consistency
        │
        ▼
  SchemaDefinition     ← CollectionDef, FieldDef, FieldType, EnumDef, JunctionDef
        │
        ▼
  SchemaBuilder        ← registers types, inputs, filters, queries, mutations
        │                 also registers the DataLoader extension
        ▼
  DynamicSchema        ← async-graphql schema ready to execute
        │
        ▼
  executor::execute    ← query + variables → JSON response
        │
        ▼
  Resolvers            ← query, mutation, nested relations batched via DataLoader
```

## License

MIT
