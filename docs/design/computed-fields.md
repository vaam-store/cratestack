# Computed fields (`@computed`) — resolver-backed response-time fields

Status: implemented (v1). Source of truth for the `@computed` feature.

## Problem

A schema author wants fields that are *derived at response time* rather than stored —
e.g. a signed `proxyUrl` on an `Image` — computed by hand-written Rust the framework
invokes while composing the response:

```text
model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed
}

type Thumbnail {
  storageKey String
  url String @computed(params: ProxyParams?)
}

type ProxyParams {
  width Int?
  height Int?
}
```

The pre-existing `@custom` attribute (type-only) generated a `CustomFieldResolver`
trait that **nothing ever invoked** — the field stayed a plain struct field the author
had to fill by hand. `@computed` replaces it (parse error on `@custom` points here).

## Decisions (maintainer-confirmed)

1. `@custom` is **removed**, replaced by `@computed`. One concept.
2. Model computed fields resolve on **all model HTTP responses**: get, list,
   create, update/upsert, delete, and relation includes. Event/stream payloads are
   excluded in v1 (the server-side model struct simply doesn't carry the field).
3. `include_embedded_schema!` **rejects** schemas containing `@computed` at macro
   expansion (compile error) — embedded has no response-composition boundary.
4. Resolvers reach the router as a **new `router()` parameter**, always present:
   `router(db, registry, resolvers, codec, auth_provider, body_limit_bytes)`.
   When the schema has no computed fields the generated `ComputedFieldResolver`
   trait has no methods and a generated `impl ComputedFieldResolver for ()` lets
   callers pass `()`.

## Schema surface

- `@computed` — bare marker on a `type` or `model` field.
- `@computed(params: <Type>?)` — parameterized resolver. `<Type>` must be a declared
  `type` (not a model, not a scalar, not computed-bearing). The trailing `?` is
  **required in v1**: params are always optional (a required param would make plain
  CRUD reads unsatisfiable and has no wire slot on non-read paths). `@computed(params:
  <Type>)` without `?` → parse error “required computed params are not supported yet”.
- Accepted on `type` and `model` fields only. Rejected (with spanned errors) on
  mixins, views, auth blocks (`validate::fields::validate_computed_field_attribute`),
  and `@custom` everywhere (`validate::removed_attributes`).
- `@computed` cannot combine with any other field attribute (fail-closed).
- A computed field's own type must be a scalar, enum, or non-computed-bearing `type`;
  never a model. This holds on a `model` owner as well as a `type` owner: the
  storage-type rule that otherwise forbids a `type` block as a model field's type
  (`validate::type_names::reject_type_decl_as_model_field_type`, #230/#235) exempts
  `@computed` fields, because a computed field is never a column — `cratestack-migrate`
  drops it before column conversion, so neither the missing `CREATE TYPE` nor the
  encode/decode problem that rule guards against can arise. A *stored* model field
  typed with a `type` block is still rejected. A `type`-valued computed field is
  composed into the response as a nested object via `ProjectedValue::leaf`, which
  serializes the resolver's return value with its own `Serialize` impl; the
  `compose_<owner>` recursion is not involved, since the type is non-computed-bearing
  by the rule above and so has nothing left inside it to resolve.
- **Computed-bearing** (has a computed field, transitively through nested `type`
  fields — see `validate::computed::computed_bearing_names`): such names are rejected
  as procedure *argument* types (the client wire shape includes computed fields, the
  server shape doesn't, so inputs would silently drop data) and as `@stream` item
  types (no item-wise resolution inside the incremental encoder in v1).
- Composite `@@id`/`@@unique`/`@@index` field lists reject computed names.

## Generated server surface

Server-side structs (`models::*`, `types::*`) **exclude** computed fields — they are
never stored, fetched, or hand-constructed. Client-side shapes (generated Rust client
via `include_client_schema!`, Dart, TypeScript) **include** them (that is the wire
shape) but exclude them from create/update inputs, filters, and sorts.

The server's own embedded self/peer-calling client (an internal use of
`crate::client::generate_client_module`, shared with standalone
`include_client_schema!`) decodes computed-bearing model/type responses into
a dedicated `wire::<Owner>` struct set, not into the server-side
`models::*`/`types::*` structs above — so a server calling its own or a
peer's API observes resolved computed values instead of silently losing
them. The `wire` module is emitted only when the schema has at least one
computed-bearing owner.

Per schema, the macro emits a `computed` module:

```rust
pub mod computed {
    pub struct ComputedFieldDescriptor {
        pub owner: &'static str,          // "Image"
        pub field: &'static str,          // "proxyUrl"
        pub resolver_method: &'static str, // "resolve_image_proxy_url"
        pub params_type: Option<&'static str>, // Some("ProxyParams")
    }
    pub const FIELDS: &[ComputedFieldDescriptor] = &[ ... ];

    pub trait ComputedFieldResolver: Clone + Send + Sync + 'static {
        // without params:
        fn resolve_image_proxy_url(
            &self,
            db: &super::Cratestack,
            source: &super::Image,
            ctx: &::cratestack::CratestackContext,
        ) -> impl Future<Output = Result<String, CratestackError>> + Send;
        // with `@computed(params: ProxyParams?)`:
        fn resolve_thumbnail_url(
            &self,
            db: &super::Cratestack,
            source: &super::Thumbnail,
            params: Option<&super::ProxyParams>,
            ctx: &::cratestack::CratestackContext,
        ) -> impl Future<Output = Result<String, CratestackError>> + Send;
    }
    // Only when FIELDS is empty:
    impl ComputedFieldResolver for () {}
}
```

`source` is the *server-side* struct (computed fields excluded). Method naming:
`resolve_<owner_snake>_<field_snake>`.

## Response composition

### Models (Postgres server schemas)

`serialize_<model>_model_value` (the shared GET/list/include projection path) gains
`resolvers: &CR` and, after projecting stored fields, resolves and inserts each
computed field as a `ProjectedValue::leaf`. Selection semantics: with no `?fields=`,
every computed field resolves; with `?fields=`, only selected ones resolve (computed
names are legal in `allowed_fields` but never in sorts/filters).

Create/update/delete handlers today encode the struct directly. For models **with**
computed fields they switch to the projection serializer (full default selection) so
the wire shape includes computed values; models without computed fields keep the
existing direct encode, bit-identical.

`ModelRouterState` gains the resolver: `ModelRouterState<CR, C, Auth>`; `router()` and
`model_router()` thread it through. RPC dispatch reuses the same `*_dispatch`
functions and inherits the behavior.

### Procedure outputs

For every computed-bearing owner (type or model) the macro emits an async
`compose_<owner_snake>` helper turning `&T` into a `ProjectedValue` (stored fields as
leaves, computed fields resolved, nested computed-bearing `type`/model fields recursed
through their own compose helpers, `Option`/`Vec` arities handled). Procedures whose
`Output` (or `Page<T>` item / list item) is computed-bearing compose before encoding;
all other procedures encode exactly as today. `ProcedureRouterState` gains `CR` the
same way. Procedure-context resolution always passes `params: None` in v1.

### Parameterized resolvers on the wire

Both transports carry the same logical payload — a JSON object keyed by computed
field name, each value deserializing into the generated params `type` struct via
serde — just in different envelopes.

**REST.** Model GET/list requests accept one query parameter:

```
?computedParams=%7B%22proxyUrl%22%3A%7B%22width%22%3A800%7D%7D
   (URL-encoded {"proxyUrl": {"width": 800}})
```

**RPC.** `model.<X>.list` decodes `RpcListInput`, which carries
`fields`, `include`, `include_fields`, and `computedParams` fields (raw
JSON-object text — see below for why not `serde_json::Value`); `model.<X>.get`
decodes a dedicated `RpcGetInput { id, fields, include, include_fields,
computedParams }` rather than reusing `RpcPkInput` (which `delete` also
decodes, and would otherwise gain silently-ignored fields). Server-side, the
RPC dispatcher synthesizes the equivalent query string from the decoded fields
and hands it to the exact same fetch/list query parser REST uses
(`parse_model_fetch_query`/`parse_model_list_query`,
`cratestack-macros/src/axum/shared_support.rs`) — one validation
implementation, no drift between transports. On `/rpc/batch`, each frame
carries its own selection and `computedParams` inside that frame's `input`,
resolved independently per frame; in-frame selection and params are signed by
construction, since the canonical signed body under `transport rpc` is the raw
frame bytes themselves (`docs/design/rpc-transport.md` §5).

Why `computedParams` is a `String` (raw JSON text) on the RPC input types
rather than `serde_json::Value`: `RpcUpdateInput`'s own doc comment already
documents that round-tripping an `Option`-bearing value through
`serde_json::Value` corrupts CBOR `Option::None` (`minicbor-serde` encodes it
as `0xf6` simple-null, `serde_json::Value` encodes it as the CBOR empty-array
marker) — generated params types are bags of optionals, so they'd hit this
head-on. `/rpc/batch` additionally re-encodes each frame's opaque `input`
through `serde_json::Value` before re-dispatching it
(`crates/cratestack-macros/src/include/server/rpc_module/batch.rs`'s
`build_batch_block` — the *input* re-encode site, not response-frame
handling); a `String` field survives that round trip verbatim, a nested
object wouldn't.

Both transports:

- Malformed JSON, unknown field keys, keys naming non-computed or param-less
  fields, or a params payload for a field excluded by `?fields=` →
  `CratestackError::Validation`, same HTTP status either way. This now fires
  on both transports — see `crates/cratestack-pg/tests/rpc_get_projection.rs::rpc_get_rejects_computed_params_for_a_field_excluded_by_fields` for the proof.
- Absent `computedParams` (or absent key) → resolver gets `None`.
- Applies to the request's *root* model only in v1; relation-included records and
  all non-read paths resolve with `None`.
- Generated clients (Rust/Dart/TS) expose a **typed** per-model `computedParams`
  parameter on `get`/`list`, gated the same way on every language (offered only
  when the model has at least one parameterized `@computed(params: <Type>?)`
  field), over both REST and RPC transport — see "Downstream" below for the
  exact per-language shape.

## Exclusions (v1, documented)

- Event/change-stream payloads never carry computed fields.
- `@stream` procedures with computed-bearing items: parse error.
- Embedded (`include_embedded_schema!`): compile error when the schema has any
  computed field.
- Views cannot declare computed fields.
- Audit-log redaction (`@pii`/`@sensitive`) doesn't apply (cannot combine with
  `@computed`); resolvers must not return data needing redaction.
- **Create/update/delete commit the DB write before resolvers run.** The
  handler calls `.create()`/`.update()`/`.delete()` (each a real,
  already-committed write) and only afterward runs response composition
  (which invokes resolvers). A resolver error therefore always describes an
  error *response* for a write that already happened — there is no
  transactional rollback tying resolver success to the write.
- **`computedParams` value decoding is not pre-DB.** Only the *keys* of a
  `?computedParams=` object are validated before any database access (does
  the key name a parameterized computed field of this model, is it excluded
  by `?fields=`, is the payload even a JSON object at all). Decoding a key's
  *value* into its field's declared params type
  (`serde_json::from_value::<ParamsType>`) happens later, at
  response-serialization time, after the row (or rows) has already been
  fetched — see `cratestack-macros/src/axum/model/serializers/computed_fields.rs`.
- **Unknown keys *inside* a params object are silently ignored** — standard
  serde struct deserialization, not `#[serde(deny_unknown_fields)]`. Only the
  top-level `computedParams` object's keys (the computed field names) are
  validated; an extra, unrecognized key inside one field's params payload
  (e.g. `{"proxyUrl": {"width": 800, "typo": true}}`) is dropped, not
  rejected.

## Downstream

- `cratestack-migrate`: computed fields excluded from DDL/diff.
- Wiremock generator: computed fields fabricated like ordinary response fields.
- Dart clients: computed fields in response classes, excluded from inputs,
  filters, sorts. `get`/`list` gain a **typed** `<Model>ComputedParams`
  parameter — a generated class with a const constructor, one declared-type
  field per parameterized `@computed(params: <Type>?)` field, a `toWire()`
  encoder, and value `==`/`hashCode` (riverpod family providers key their
  cache on argument equality, so this is load-bearing, not decoration) — on
  both REST and RPC; RPC mode folds `jsonEncode(toWire())` into the frame via
  a shared runtime helper. Gated per model exactly like the Rust client below
  (offered only when the model has at least one parameterized computed
  field; a bare-`@computed`-only model gets neither the class nor the extra
  parameter, since the server would 422 any `computedParams` key for a field
  with no params type). The `@riverpod` get/list convenience providers gain
  the same gated parameter, and riverpod partition reachability now seeds
  params types referenced only from `@computed` attribute text — they
  weren't otherwise reachable from the response-type graph. Every parameterized
  `<Model>ComputedParams` also gets the standard fluent `<Model>ComputedParamsBuilder`
  (`ImageComputedParamsBuilder().proxyUrl(ProxyParams(width: 800)).build()`),
  matching the builder convention all other generated Dart data classes follow.
- TypeScript clients: computed fields in response classes, excluded from
  inputs, filters, sorts. `CratestackFetchQuery`/`CratestackRpcListQuery`
  become generic over `TComputedParams` (default `never`, so
  `computedParams` is unassignable on an ungated model — enforced by `tsc`
  at compile time, not a runtime check). A gated model gets a generated
  `<Model>ComputedParams` interface, used on its REST query config, its RPC
  list query, and a dedicated per-model RPC `get` options bag —
  `JSON.stringify`d to match the server's `Option<String>` frame field. The
  per-model RPC `get` options bag now also carries `fields`, `include`, and
  `includeFields`, emitted for every model (not just parameterized-field models),
  alongside `computedParams`. swr RPC `get` cache keys now incorporate
  `computedParams` too (previously two reads of the same model with different
  params collided on one cache key); the ownership graph deciding which types
  swr's generated module reaches was fixed the same way Dart's riverpod
  partition was — a params type referenced only from `@computed` attribute
  text is now reachable. TypeScript has no builder convention anywhere in its
  generated output, so `<Model>ComputedParams` stays a plain interface.
- **The generated Rust client** (both `include_client_schema!` and the
  server's own embedded self-client, since both go through the single
  `crate::client::generate_client_module` call site) has the same **typed**
  `computedParams` surface Dart and TypeScript expose above, via its own code
  path — `cratestack-macros/src/client/computed_params.rs` emits one
  `<Model>ComputedParams` struct per model with at least one *parameterized*
  `@computed(params: <Type>?)` field (same per-model gate Dart uses; a
  bare-`@computed`-only model gets neither the struct nor an extra
  parameter), with one `Option<super::types::<Params>>` field per resolver
  and a `to_query_value()` helper that serializes to the same JSON-object
  text both transports expect (`None` when every field is unset, matching
  the server's "absent key -> resolver gets `None`" default). Every
  `<Model>ComputedParams` struct gains the standard generated typestate builder
  (`<Model>ComputedParams::builder().<field>(Some(..)).build()`, non-generic
  because every field is optional — same shape `{Model}Where` gets). On REST,
  `get`/`list` on a gated model take an extra `computed_params:
  Option<&<Model>ComputedParams>` parameter; RPC's plain `get` is byte-identical
  and still decodes into the full model type, but every model gets a `get_view<P:
  ProjectionDecoder>(id, projection)` twin that carries NO `computed_params`,
  matching REST's `get_view` (which also can't send it). RPC's `list` carries
  `computed_params` and selection alongside pagination and filtering. An
  ungated model's `get`/`list` tokens are unchanged from before this surface
  existed.
  
  **Schema-evolution caveat:** the Rust client's `computed_params` parameter is
  positional, so adding a model's first `@computed(params: <Type>?)` field changes
  `get(id, headers)` into `get(id, computed_params, headers)` and breaks call sites.
  Unlike Dart (named optional) and TypeScript (options bag), which are additive,
  the builder does not fix this (it changes argument construction, not the parameter
  list). A Rust options-bag entry point is a tracked follow-up.
- LSP: `@computed` added to attribute completion if a list exists.

## Exclusions (post-v1, documented limits)

- RPC `get_view` (Rust) carries no `computedParams`, matching REST's `get_view`
  — projection-only reads are orthogonal to resolution parameters.
- swr's RPC `get` cache key does not incorporate `fields`/`include` — cache
  collision is still possible on same-id reads with different projections but
  same (or absent) computed params. Follow-up tracked.
