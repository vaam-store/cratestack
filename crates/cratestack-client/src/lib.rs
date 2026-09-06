//! CrateStack facade for pure HTTP-client SDK crates (cratestack#490).
//!
//! This crate re-exports **only** `include_client_schema!` from
//! `cratestack-macros` — not `include_server_schema!`, not
//! `include_embedded_schema!`. A consumer reaching for either of those two
//! is on the wrong facade and gets a plain name-resolution error at the
//! call site (`cannot find macro \`include_server_schema\` in \`cratestack\``)
//! rather than a confusing downstream failure once `cratestack-sqlx`/
//! `cratestack-axum` symbols turn out to be missing. `cratestack-pg`,
//! `cratestack-api`, and `cratestack-sqlite` all re-export every schema
//! macro their role can plausibly need; this facade names one specific
//! role — "I only ever call a cratestack server, I never run one" — and
//! offers nothing else.
//!
//! **The hard guarantee this crate exists for:** `cratestack-axum` — and
//! therefore `axum`/`tower`/`hyper`/`tower-http`, a full server framework —
//! is structurally absent from `Cargo.toml`, not merely switched off behind
//! a feature. `examples/client-only-verification` (its own standalone
//! `[workspace]` root, mirroring `examples/no-database-verification-api`'s
//! cratestack#347 precedent) proves this with a real `cargo tree`, re-run
//! on every PR by the `facade-disjointness` CI job.
//!
//! Every symbol below is re-exported because `include_client_schema!`'s
//! expansion genuinely emits a `::cratestack::<that path>` reference
//! somewhere — derived empirically by reading
//! `crates/cratestack-macros/src/client/`, `crates/cratestack-macros/src/
//! include/client.rs`, and every shared codegen helper client generation
//! calls into (`procedure::types`, `model::{struct_only,inputs,
//! find_many_input,find_many_where,find_many_order_by,field_module,
//! selection_module}`, `relation::flat`), not guessed from what the server
//! facades happen to carry. Concretely, that check ruled **out** several
//! things `cratestack-api`/`cratestack-sqlite` do carry:
//!
//! - `cratestack-parser`/`cratestack-policy` (`parse_schema`,
//!   `ProcedurePolicy`, `authorize_procedure`, …) — a generated **server**
//!   procedure module emits `ALLOW_POLICIES`/`DENY_POLICIES` consts typed
//!   `::cratestack::ProcedurePolicy` and calls `::cratestack::
//!   authorize_procedure(...)`
//!   (`crates/cratestack-macros/src/procedure/instrument.rs`); the
//!   **client** counterpart (`generate_client_procedure_module`) emits only
//!   an `Args` struct and an `Output` type alias — no policy consts, no
//!   authorize call, nothing that ever names a `cratestack-policy` symbol.
//!   `RelationQuantifier` — which client-generated relation-path code
//!   *does* need — is re-exported from `cratestack-sql` instead (see
//!   below), which re-exports the identical `cratestack-policy` type
//!   without this crate needing a direct dependency on the policy crate at
//!   all.
//! - `regex`, `async-stream` — both are exclusively server-side: `regex`
//!   backs `CreateModelInput`/`UpdateModelInput`'s generated `validate()`
//!   bodies (`crates/cratestack-macros/src/validators/emit.rs`), and client
//!   input structs (`generate_client_create_input_struct`/
//!   `generate_client_update_input_struct`) are bare structs with no
//!   `validate()` at all; `async-stream` backs the axum `@stream` procedure
//!   dispatcher (`crates/cratestack-macros/src/axum/procedure/
//!   invoke_call.rs`), which this facade never emits since it has no axum
//!   dependency to dispatch through in the first place.
//! - `tracing`, `futures`/`futures-util` — both are exclusively server-side
//!   too: `tracing` instruments axum route handlers and the server
//!   procedure `authorize`/`invoke` wrappers
//!   (`crates/cratestack-macros/src/procedure/instrument.rs`,
//!   `crates/cratestack-macros/src/axum/`); `futures::Stream` backs the
//!   server-side `ProcedureRegistry` trait method for `@stream` procedures.
//!   The client-side streaming counterpart (an RPC `Sequence` procedure's
//!   generated `call_streaming` method) returns `cratestack_client_rust::
//!   RpcStream`, not `::cratestack::futures::Stream` — no `futures`
//!   re-export needed to name it.
//! - `ModelPrimaryKey`, `ModelDescriptor`, `CreateModelInput`/
//!   `UpdateModelInput`/`UpsertModelInput`, `SqlValue`/`SqlColumnValue`,
//!   `RelationInclude` — all server/embedded-only. Client-generated model
//!   structs and CRUD inputs never implement these traits (they're bare
//!   `#[derive(Serialize, Deserialize)]` structs the wire codec reads and
//!   writes directly), and `as_include()` — the one call site that would
//!   need `RelationInclude` — is unconditionally skipped for client field
//!   modules (`FieldModuleKind::Client`, `crates/cratestack-macros/src/
//!   relation/root.rs`).
//!
//! Schema macros emit `::cratestack::*` paths, so consumers rename this
//! crate via Cargo's `package =` field:
//!
//! ```toml
//! [dependencies]
//! cratestack = { package = "cratestack-client", version = "0.7" }
//! ```
//!
//! ```text
//! cratestack::include_client_schema!("schema/foo.cstack");
//! ```
//!
//! See `examples/client-only-verification` for a full, real, compiling
//! example, and this crate's `README.md` for a quick-start.

// Re-exported so a generated `<Model>Client`'s `list_view`/`get_view`
// (`crates/cratestack-macros/src/client/rest/model.rs`) can bound their
// projection argument on `::cratestack::ProjectionDecoder` — the same
// trait `cratestack-pg`/`cratestack-api` expose, sourced from the same
// place (`cratestack-core`, so both the client's decoded `Selection` and
// the server's SQL-backed projection speak one shared contract).
pub use cratestack_client_rust as client_rust;
pub use cratestack_core::*;
// The *only* schema macro this facade offers — see this module's doc for
// why `include_server_schema!`/`include_embedded_schema!` are deliberately
// not re-exported here.
pub use cratestack_macros::include_client_schema;

// SQL primitives generated client code actually references: `FieldRef`
// (per-field accessor methods), `FilterExpr`/`IntoSqlValue`/`wrap_filter`
// (relation-path filter builders, `crates/cratestack-macros/src/relation/
// filter_builders.rs`), `OrderClause`/`Orderable`/`Unorderable`/
// `SortDirection`/`order_value_sql` (relation-path ordering,
// `crates/cratestack-macros/src/relation/flat.rs`), `RelationHop` (relation
// edge descriptors threaded through both), and `RelationQuantifier` (the
// to-many `.some()`/`.every()`/`.none()` quantifiers on `RelToMany` — the
// same type `cratestack-policy` defines and `cratestack-sql` re-exports
// verbatim via `pub use cratestack_policy::RelationQuantifier;`, so pulling
// it from here needs no direct `cratestack-policy` dependency of this
// crate's own). Everything else `cratestack-sql` exports
// (`ModelDescriptor`, `CreateModelInput`/`UpdateModelInput`, `SqlValue`,
// `RelationInclude`, the untyped-REST-route `OrderCatalog`/
// `OrderRelationEdge`, …) is server/embedded-only — see this module's doc.
pub use cratestack_sql::{
    FieldRef,
    FilterExpr,
    IntoSqlValue,
    OrderClause,
    Orderable,
    RelationHop,
    RelationQuantifier,
    // `SqlValue` is `IntoSqlValue::into_sql_value`'s return type. Exporting
    // the trait without it left the trait unimplementable by generated code
    // in this facade — which is what broke `include_client_schema!` for any
    // schema with an enum-typed model field (cratestack#928).
    SortDirection,
    SqlValue,
    Unorderable,
    order_value_sql,
    wrap_filter,
};

pub use chrono;
pub use serde;
pub use serde_json;
pub use uuid;
