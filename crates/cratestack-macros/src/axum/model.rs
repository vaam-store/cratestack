//! Top-level per-model axum handler generation. Splits into:
//!
//! - [`prep`]: identifier names + reusable token chunks (etag, response
//!   types, paging, logging).
//! - [`builders`]: filter/query/order/validate helper-fn tokens.
//! - [`serializers`]: projection/serialization helper-fn tokens +
//!   the list-builder body.
//! - [`handlers_list`] / [`handlers_crud`]: the 5 axum handler fn
//!   tokens themselves.
//! - [`routes`]: `.route(...)` chain for the router.

mod builders;
mod computed;
mod handlers_crud;
mod handlers_list;
mod handlers_update;
mod prep;
mod routes;
mod serializers;

use std::collections::BTreeSet;

use cratestack_core::{Model, TypeArity};
use quote::quote;

use crate::relation::{
    generate_model_order_catalog, generate_relation_include_arm,
    generate_relation_include_fields_validation_arm, generate_relation_include_path_validation_arm,
    generate_relation_query_guard,
};
use crate::shared::{model_name_set, relation_model_fields, scalar_model_fields};

use super::filter_arms::{generate_order_by_arm, generate_query_filter_arm};

use builders::RelationArmCollections;

pub(crate) use routes::generate_model_axum_routes;

pub(crate) fn generate_model_axum_handlers(
    model: &Model,
    models: &[Model],
    enum_names: &BTreeSet<&str>,
) -> Result<proc_macro2::TokenStream, String> {
    let p = prep::build_prep(model)?;
    let model_names = model_name_set(models);
    let field_module_ident = &p.field_module_ident;

    let query_filter_arms = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter_map(|field| generate_query_filter_arm(field_module_ident, field, enum_names))
        .collect::<Vec<_>>();
    let relation_filter_guards = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_query_guard(model, field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let order_by_arms = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_order_by_arm(field_module_ident, field))
        .collect();
    let order_catalog = generate_model_order_catalog(model, models)?;
    let relation_include_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            generate_relation_include_arm(model, field, models, &p.project_serialized_value_ident)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let relation_include_path_validation_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_include_path_validation_arm(field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let relation_include_fields_validation_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_include_fields_validation_arm(field, model, models))
        .collect::<Result<Vec<_>, String>>()?;

    let arms = RelationArmCollections {
        query_filter_arms,
        relation_filter_guards,
        order_by_arms,
        relation_include_arms,
        relation_include_path_validation_arms,
        relation_include_fields_validation_arms,
    };

    let query_helpers = builders::build_query_helpers(&p, &arms);
    let validate_helpers = builders::build_validate_helpers(&p, &arms);
    let projection_helpers = serializers::build_projection_helpers(&p, model, &model_names);
    let computed_fields = computed::model_computed_fields(model);
    let parse_computed_params_fn = computed::build_parse_computed_params_fn(&p, &computed_fields);
    let serialize_helper = serializers::build_serialize_helper(&p, &arms, &computed_fields);
    let list_builder = serializers::build_list_builder(&p, &arms);
    let list_handler = handlers_list::build_list_handler(&p);
    let create_handler = handlers_crud::build_create_handler(&p);
    let get_handler = handlers_crud::build_get_handler(&p);
    let update_handler = handlers_update::build_update_handler(&p);
    let delete_handler = handlers_crud::build_delete_handler(&p);

    let _ = TypeArity::List; // ensure import used (referenced via prep)

    Ok(quote! {
        #order_catalog
        #query_helpers
        #validate_helpers
        #projection_helpers
        #parse_computed_params_fn
        #serialize_helper
        #list_builder
        #list_handler
        #create_handler
        #get_handler
        #update_handler
        #delete_handler
    })
}
