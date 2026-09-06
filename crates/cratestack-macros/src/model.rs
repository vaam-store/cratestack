//! Per-model token generation. Each of the include composers
//! ([`crate::include`]) calls a fixed set of generators from here once
//! per model in the schema. The crate-facing surface is a flat list
//! of `pub(crate) fn`s; the implementation lives in focused
//! submodules:
//!
//! - [`struct_only`]: model `struct` + primary-key accessor + the
//!   shared per-field token builder.
//! - [`row_pg`] / [`row_sqlite`]: `FromRow` impls for the two
//!   backends.
//! - [`inputs`]: `Create{Model}Input`, `Update{Model}Input`, upsert
//!   impl, client variants.
//! - [`descriptor`]: the `*_MODEL` const consulted at every CRUD/list
//!   call.
//! - [`field_module`]: per-model `pub mod <model>` field accessor
//!   module (drives the typed query builder).
//! - [`selection_module`]: the `selection` sub-module inside each
//!   field module (`Selection`, `Projected`, etc.).
//! - [`accessor`]: `Cratestack` and `BoundCratestack` method
//!   accessors that hand out per-model delegates.

mod accessor;
mod descriptor;
mod field_module;
mod find_many_input;
mod find_many_order_by;
mod find_many_where;
mod find_many_where_push;
mod inputs;
pub(crate) mod row_pg;
pub(crate) mod row_sqlite;
mod selection;
mod selection_module;
pub(crate) mod struct_only;

pub(crate) use accessor::{generate_bound_model_accessor, generate_model_accessor};
pub(crate) use descriptor::generate_model_descriptor;
pub(crate) use field_module::{
    FieldModuleKind, generate_client_field_module, generate_field_module,
};
pub(crate) use find_many_input::{generate_find_many_input, generate_find_many_types};
pub(crate) use inputs::{
    generate_client_create_input_struct, generate_client_update_input_struct,
    generate_create_input_struct, generate_update_input_struct, generate_upsert_input_struct,
};
pub(crate) use row_pg::generate_pg_from_row_impl;
pub(crate) use row_sqlite::generate_rusqlite_from_row_impl;
pub(crate) use struct_only::{
    generate_client_model_struct, generate_model_struct_only, generate_primary_key_accessor_impl,
};
