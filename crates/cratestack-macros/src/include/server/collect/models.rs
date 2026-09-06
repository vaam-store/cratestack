//! Per-model token gathering for [`super::collect_server_schema`]:
//! struct, FromRow impl, descriptor, field module, CRUD inputs,
//! accessors, axum handlers/routes, and transport constants/entries.

use std::collections::BTreeSet;

use cratestack_core::Schema;
use proc_macro::TokenStream;
use syn::LitStr;

use crate::axum::{generate_model_axum_handlers, generate_model_axum_routes};
use crate::model::{
    generate_bound_model_accessor, generate_create_input_struct, generate_field_module,
    generate_find_many_input, generate_model_accessor, generate_model_descriptor,
    generate_model_struct_only, generate_pg_from_row_impl, generate_primary_key_accessor_impl,
    generate_update_input_struct, generate_upsert_input_struct,
};
use crate::transport::{generate_model_transport_constants, generate_model_transport_entries};

use super::{Ts, compile_error};

pub(super) struct ModelCollected {
    pub(super) structs: Vec<Ts>,
    pub(super) pg_from_row_impls: Vec<Ts>,
    pub(super) primary_key_accessor_impls: Vec<Ts>,
    pub(super) descriptors: Vec<Ts>,
    pub(super) field_modules: Vec<Ts>,
    pub(super) create_input_structs: Vec<Ts>,
    pub(super) update_input_structs: Vec<Ts>,
    pub(super) upsert_input_impls: Vec<Ts>,
    pub(super) find_many_input_structs: Vec<Ts>,
    pub(super) accessors: Vec<Ts>,
    pub(super) bound_accessors: Vec<Ts>,
    pub(super) axum_handler_defs: Vec<Ts>,
    pub(super) axum_routes: Vec<Ts>,
    pub(super) transport_constants: Vec<Ts>,
    pub(super) transport_entries: Vec<Ts>,
}

pub(super) fn collect_models(
    schema: &Schema,
    schema_path: &LitStr,
    model_name_set: &BTreeSet<&str>,
    enum_name_set: &BTreeSet<&str>,
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<ModelCollected, TokenStream> {
    let structs = schema
        .models
        .iter()
        .map(|model| generate_model_struct_only(model, model_name_set, enum_name_set))
        .collect();
    let pg_from_row_impls = schema
        .models
        .iter()
        .map(|model| generate_pg_from_row_impl(model, model_name_set, enum_name_set))
        .collect();
    let primary_key_accessor_impls = schema
        .models
        .iter()
        .map(generate_primary_key_accessor_impl)
        .collect();
    let descriptors = schema
        .models
        .iter()
        .map(|model| {
            generate_model_descriptor(model, &schema.models, &schema.types, &schema.enums, auth)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let field_modules = schema
        .models
        .iter()
        .map(|model| generate_field_module(model, model_name_set, &schema.models))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let create_input_structs = schema
        .models
        .iter()
        .map(|model| generate_create_input_struct(model, model_name_set, enum_name_set))
        .collect();
    let update_input_structs = schema
        .models
        .iter()
        .map(|model| generate_update_input_struct(model, model_name_set, enum_name_set))
        .collect();
    let upsert_input_impls = schema
        .models
        .iter()
        .map(|model| generate_upsert_input_struct(model, model_name_set, enum_name_set))
        .collect();
    let find_many_input_structs = schema
        .models
        .iter()
        .map(|model| generate_find_many_input(model, model_name_set, enum_name_set))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let accessors = schema.models.iter().map(generate_model_accessor).collect();
    let bound_accessors = schema
        .models
        .iter()
        .map(generate_bound_model_accessor)
        .collect();
    let axum_handler_defs = schema
        .models
        .iter()
        .map(|model| generate_model_axum_handlers(model, &schema.models, enum_name_set))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let axum_routes = schema
        .models
        .iter()
        .map(generate_model_axum_routes)
        .collect();
    let transport_constants = schema
        .models
        .iter()
        .map(generate_model_transport_constants)
        .collect();
    let transport_entries = schema
        .models
        .iter()
        .flat_map(generate_model_transport_entries)
        .collect();

    Ok(ModelCollected {
        structs,
        pg_from_row_impls,
        primary_key_accessor_impls,
        descriptors,
        field_modules,
        create_input_structs,
        update_input_structs,
        upsert_input_impls,
        find_many_input_structs,
        accessors,
        bound_accessors,
        axum_handler_defs,
        axum_routes,
        transport_constants,
        transport_entries,
    })
}
