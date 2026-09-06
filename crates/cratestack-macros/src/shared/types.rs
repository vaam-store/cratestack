//! Rust-type token generation + scalar parser tokens used by route
//! handlers when decoding query parameters.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity, TypeRef};
use quote::quote;

use super::enum_query_parser::query_enum_parser_tokens;
use super::{doc_attrs, ident};

#[cfg(test)]
mod tests;

pub(crate) fn rust_type_tokens(type_ref: &TypeRef) -> proc_macro2::TokenStream {
    rust_type_tokens_with_scope(type_ref, true)
}

pub(crate) fn rust_type_tokens_with_scope(
    type_ref: &TypeRef,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = rust_type_tokens_with_scope(item, custom_in_super);
        return quote! { ::cratestack::Page<#item_type> };
    }

    let inner = match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => crate::shared::decimal_backend::current_decimal_type_tokens(),
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        // `Vector(n)` (see `docs/design/extensions.md` §6) — a plain
        // `Vec<f32>` on the Rust side, kept ergonomic and dependency-
        // free (no `pgvector` crate reference in the public struct
        // field) regardless of which macro composer generates this:
        // server model, Create/Update input, or the Rust client's own
        // copy of the same struct shape. The `pgvector` crate only
        // enters the picture at the sqlx row-decode/bind boundary
        // (`model::row_pg`, `cratestack-sqlx`'s `push_bind_value`),
        // which is server-only.
        "Vector" => quote! { Vec<f32> },
        // `Geography`/`Geometry` (see `docs/design/extensions.md`
        // §6b, cratestack#842) — EWKB bytes, so the public Rust type is
        // the same `Vec<u8>` a `Bytes` field gets. Keeping it a plain
        // `Vec<u8>` rather than a spatial newtype means the client
        // codegen, wire encoding and serde treatment all reuse the
        // existing bytes path unchanged; the PostGIS-specific typing
        // only enters at the sqlx row-decode boundary
        // (`cratestack-sqlx`'s `Ewkb`), which is server-only.
        "Geography" | "Geometry" => quote! { Vec<u8> },
        other => {
            let ident = ident(other);
            if custom_in_super {
                quote! { super::#ident }
            } else {
                quote! { #ident }
            }
        }
    };

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}

/// The exact type tokens [`field_definition`] puts on the field —
/// extracted so [`crate::builder`] can type a setter argument from the
/// same source rather than re-deriving it.
pub(crate) fn field_type(
    field: &Field,
    wrap_for_patch: bool,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    let base_type = rust_type_tokens_with_scope(&field.ty, custom_in_super);
    if wrap_for_patch {
        quote! { Option<#base_type> }
    } else {
        base_type
    }
}

pub(crate) fn field_definition(
    field: &Field,
    wrap_for_patch: bool,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let field_type = field_type(field, wrap_for_patch, custom_in_super);
    // A `type`-block field carries no serde attributes of its own, so a
    // `Bytes` field brings its whole `#[serde(...)]` list — including the
    // `default` that keeps an omitted nullable field decoding as `None`
    // once `deserialize_with` suppresses serde-derive's implicit handling.
    // See `super::bytes_serde` (cratestack#783).
    let serde_attr = super::bytes_serde_attr(&field.ty, wrap_for_patch);

    quote! {
        #docs
        #serde_attr
        pub #field_ident: #field_type,
    }
}

pub(crate) fn query_scalar_parser_tokens(
    ty: &TypeRef,
    value_expr: proc_macro2::TokenStream,
    field_name: &str,
    enum_names: &BTreeSet<&str>,
) -> Option<proc_macro2::TokenStream> {
    // Issue #928: an enum-typed field is a first-class query-filter
    // scalar too — checked ahead of the fixed catch-all match below
    // since `ty.name` is schema-authored and can't collide with one of
    // the builtin scalar names matched there.
    if enum_names.contains(ty.name.as_str()) {
        return Some(query_enum_parser_tokens(ty, value_expr, field_name));
    }

    Some(match ty.name.as_str() {
        "String" => quote! { Ok((#value_expr).to_owned()) },
        "Cuid" => quote! { ::cratestack::parse_cuid(#value_expr) },
        "Int" => quote! {
            (#value_expr).parse::<i64>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Float" => quote! {
            (#value_expr).parse::<f64>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Boolean" => quote! {
            (#value_expr).parse::<bool>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Uuid" => quote! {
            (#value_expr).parse::<::cratestack::uuid::Uuid>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "DateTime" => quote! {
            (#value_expr)
                .parse::<::cratestack::chrono::DateTime<::cratestack::chrono::FixedOffset>>()
                .map(|value| value.with_timezone(&::cratestack::chrono::Utc))
                .map_err(|error| {
                    CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
        },
        "Decimal" => {
            let decimal_ty = crate::shared::decimal_backend::current_decimal_type_tokens();
            quote! {
                (#value_expr).parse::<#decimal_ty>().map_err(|error| {
                    CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
            }
        }
        _ => return None,
    })
}

pub(crate) fn query_scalar_list_parser_tokens(
    ty: &TypeRef,
    field_name: &str,
    enum_names: &BTreeSet<&str>,
) -> Option<proc_macro2::TokenStream> {
    let scalar_parser =
        query_scalar_parser_tokens(ty, quote! { raw_value }, field_name, enum_names)?;

    Some(quote! {{
        let parsed = value
            .split(',')
            .map(str::trim)
            .filter(|raw_value| !raw_value.is_empty())
            .map(|raw_value| -> Result<_, CratestackError> { #scalar_parser })
            .collect::<Result<Vec<_>, CratestackError>>()?;
        if parsed.is_empty() {
            return Err(CratestackError::BadRequest(format!(
                "{}__in requires at least one value",
                #field_name,
            )));
        }
        parsed
    }})
}
