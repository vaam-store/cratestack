//! Per-model `<Model>Where` struct — one optional `FieldFilterInput<V>`
//! per filterable scalar field, plus a `to_filters()` method that turns
//! whichever operators the caller set into real `FilterExpr`s via the
//! model's own field accessors (`super::<model_snake>::<field>()`), the
//! same `FieldRef` calls the untyped REST `?where=` route already makes.
//! Split out from `inputs.rs` per the repo's 200-LoC file convention —
//! `to_filters()`'s per-field body itself lives in the sibling
//! `find_many_where_push.rs`, split out for the same reason.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::builder::{BuilderField, generate_builder};
use crate::shared::{
    generated_doc_attr, ident, rust_type_tokens, scalar_model_fields, to_snake_case,
};

use super::find_many_where_push::build_field_push;

/// Types `query_scalar_parser_tokens` (the untyped REST `?where=` route's
/// own value parser) already proves can round-trip through a filter —
/// `Json`/`Bytes`/custom-`type` fields are excluded here the same way
/// that function excludes them, rather than speculatively generating
/// `.eq()`/`.ne()` calls whose `IntoSqlValue` support isn't confirmed.
/// Enum fields (cratestack#928) are included: the enum's own generated
/// `impl IntoSqlValue` (`crate::types::enums::generate_enum_type`) covers
/// `.eq()`/`.ne()`/`.in_()` the same as every other scalar here.
///
/// `pub(super)` — `find_many_where_push.rs`'s `supports_ordering_ops`
/// gates on this too, since an enum field must stay filterable-but-not-
/// orderable rather than falling out of the filter set entirely.
pub(super) fn is_filterable_scalar(field: &Field, enum_names: &BTreeSet<&str>) -> bool {
    matches!(
        field.ty.name.as_str(),
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Uuid" | "DateTime" | "Decimal"
    ) || enum_names.contains(field.ty.name.as_str())
}

fn scalar_type_tokens(field: &Field) -> proc_macro2::TokenStream {
    let scalar_ty = cratestack_core::TypeRef {
        name: field.ty.name.clone(),
        name_span: field.ty.name_span,
        arity: TypeArity::Required,
        generic_args: Vec::new(),
        int_args: Vec::new(),
        ident_args: Vec::new(),
    };
    rust_type_tokens(&scalar_ty)
}

pub(crate) fn generate_where_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let where_ident = ident(&format!("{}Where", model.name));
    let module_ident = ident(&to_snake_case(&model.name));
    let docs = generated_doc_attr(format!(
        "Generated `where` filter for `{}` — every operator set is combined with implicit AND. \
         Used by `FindMany<{}>` procedure arguments.",
        model.name, model.name
    ));
    let fields = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| is_filterable_scalar(field, enum_names))
        .collect::<Vec<_>>();

    let field_defs = fields.iter().map(|field| {
        let field_ident = ident(&field.name);
        let scalar_type = scalar_type_tokens(field);
        quote! {
            pub #field_ident: Option<::cratestack::FieldFilterInput<#scalar_type>>,
        }
    });

    let filter_pushes = fields
        .iter()
        .map(|field| build_field_push(field, &module_ident, enum_names));

    // Every field is `Option<FieldFilterInput<_>>` — every operator on a
    // `Where` is optional, so the builder is non-generic (no required
    // slots to gate `build()` on).
    let where_builder_fields = fields
        .iter()
        .map(|field| BuilderField::new(ident(&field.name), scalar_where_field_type(field), false))
        .collect::<Vec<_>>();
    let builder = generate_builder(&where_ident, &where_builder_fields);

    quote! {
        #docs
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #where_ident {
            #(#field_defs)*
        }

        #builder

        impl #where_ident {
            pub fn to_filters(&self) -> Vec<::cratestack::FilterExpr> {
                let mut filters = Vec::new();
                #(#filter_pushes)*
                filters
            }
        }
    }
}

/// The exact type tokens [`generate_where_struct`]'s field definitions
/// use — extracted so the builder's setter argument type can never drift
/// from the field it fills (mirrors
/// [`crate::model::struct_only::struct_field_type`]'s role for model
/// structs).
fn scalar_where_field_type(field: &Field) -> proc_macro2::TokenStream {
    let scalar_type = scalar_type_tokens(field);
    quote! { Option<::cratestack::FieldFilterInput<#scalar_type>> }
}
