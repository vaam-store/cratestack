//! `include_client_schema!` composer — emits the HTTP client surface:
//! model/input/procedure stubs for talking to a server over the wire.
//! No DB at all.

mod input_structs;

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::client::generate_client_module;
use crate::model::{
    generate_client_field_module, generate_client_model_struct, generate_find_many_types,
};
use crate::procedure::generate_client_procedure_module;
use crate::shared::decimal_backend::{DecimalBackend, with_decimal_backend};
use crate::shared::schema_lit;
use crate::types::{generate_client_enum_type, generate_client_type_struct};

use super::decimal_arg::resolve_decimal_backend;
use super::parse::parse_schema_literal;
use input_structs::client_input_structs;

pub(super) fn compose_client_schema(
    schema_path: &LitStr,
    decimal: Option<DecimalBackend>,
) -> TokenStream {
    let (schema_relative, resolved, schema, schema_sha256) = match parse_schema_literal(schema_path)
    {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    if let Err(error) =
        super::extension_gate::guard_client_declared_extensions(schema_path, &schema)
    {
        return error;
    }
    let decimal_backend = match resolve_decimal_backend(schema_path, &schema, decimal) {
        Ok(backend) => backend,
        Err(error) => return error,
    };

    // Wraps the rest of composition — see `include::embedded`'s matching
    // comment for why this needs to scope this widely (cratestack#505
    // Direction 2).
    with_decimal_backend(decimal_backend, move || {
        let resolved_literal = resolved.display().to_string();

        let model_names = schema.models.iter().map(|model| schema_lit(&model.name));
        let model_name_set = schema
            .models
            .iter()
            .map(|model| model.name.as_str())
            .collect::<BTreeSet<_>>();
        let type_names = schema.types.iter().map(|ty| schema_lit(&ty.name));
        let enum_names = schema
            .enums
            .iter()
            .map(|enum_decl| schema_lit(&enum_decl.name));
        let enum_name_set = crate::shared::enum_name_set(&schema.enums);
        let procedure_names = schema
            .procedures
            .iter()
            .map(|procedure| schema_lit(&procedure.name));
        let type_structs = schema.types.iter().map(generate_client_type_struct);
        let enum_types = schema.enums.iter().map(generate_client_enum_type);
        let model_structs = schema
            .models
            .iter()
            .map(|model| generate_client_model_struct(model, &model_name_set, &enum_name_set));
        // `Create<M>Input`/`Update<M>Input`, each filtered against
        // `@@internal(...)` — see `input_structs`'s module doc
        // (cratestack#743) for why this composer has to filter where
        // `include_server_schema!`'s doesn't.
        let (create_input_structs, update_input_structs) =
            client_input_structs(&schema, &model_name_set, &enum_name_set);
        // `<Model>Where`/`<Model>SortField`/`<Model>OrderByClause`/
        // `<Model>FindManyInput` — the types a `FindMany<Model>` procedure
        // argument needs (issue #371's redesign). No `build_<model>_query_
        // from_find_many` here: that entry point needs a live `Cratestack`
        // DB handle and query builder, neither of which a pure HTTP client
        // has — see `generate_find_many_types`'s doc.
        let find_many_input_structs = schema
            .models
            .iter()
            .map(|model| generate_find_many_types(model, &model_name_set, &enum_name_set));
        // Client field modules: same surface as server field modules minus
        // emissions that hard-reference `*_MODEL` descriptors (which the
        // client composer doesn't emit). See `FieldModuleKind::Client`.
        let field_modules = match schema
            .models
            .iter()
            .map(|model| generate_client_field_module(model, &model_name_set, &schema.models))
            .collect::<Result<Vec<_>, String>>()
        {
            Ok(field_modules) => field_modules,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };
        let procedure_modules = match schema
            .procedures
            .iter()
            .map(|procedure| {
                generate_client_procedure_module(procedure, &schema.types, &enum_name_set)
            })
            .collect::<Result<Vec<_>, String>>()
        {
            Ok(modules) => modules,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };
        // Always an empty bearing set: this composer's own `models`/
        // `types` already ARE the wire shape — see `crate::client`'s doc.
        let generated_client_module = match generate_client_module(
            &schema.models,
            &schema.procedures,
            schema.transport,
            &BTreeSet::new(),
        ) {
            Ok(module) => module,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };

        let expanded = quote! {
            pub mod cratestack_schema {
                pub const SCHEMA_PATH: &str = #schema_relative;
                pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
                /// Hex-encoded SHA-256 of `SCHEMA_SOURCE`'s raw bytes, computed
                /// once at macro-expansion time. Not cryptographic-strength
                /// integrity — it's a drift-detection fingerprint: the generated
                /// `client::Client::new` below stamps it onto every
                /// `CratestackClient` it wraps, so it rides along as the
                /// `x-cratestack-schema-sha` header on every request; the
                /// server-side counterpart `tracing::warn!`s on mismatch, never
                /// rejects. See issue #178.
                pub const SCHEMA_SHA256: &str = #schema_sha256;
                pub const MODELS: &[&str] = &[#(#model_names),*];
                pub const TYPES: &[&str] = &[#(#type_names),*];
                pub const ENUMS: &[&str] = &[#(#enum_names),*];
                pub const PROCEDURES: &[&str] = &[#(#procedure_names),*];

                pub const MODEL_COUNT: usize = MODELS.len();
                pub const TYPE_COUNT: usize = TYPES.len();
                pub const ENUM_COUNT: usize = ENUMS.len();
                pub const PROCEDURE_COUNT: usize = PROCEDURES.len();

                pub mod types {
                    use ::cratestack::serde;

                    #(#enum_types)*
                    #(#type_structs)*
                }

                pub use types::*;

                pub mod models {
                    use ::cratestack::serde;

                    #(#model_structs)*
                }

                pub use models::*;

                #(#field_modules)*

                pub mod inputs {
                    use ::cratestack::serde;

                    #(#create_input_structs)*
                    #(#update_input_structs)*
                    #(#find_many_input_structs)*
                }

                pub use inputs::*;

                #generated_client_module

                pub mod procedures {
                    use ::cratestack::serde;

                    #(#procedure_modules)*
                }
            }
        };

        expanded.into()
    })
}
