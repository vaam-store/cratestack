//! `to_filters()`'s per-field body — one `if let Some(filter) = ...`
//! block per filterable scalar field, gating each operator on whether
//! the field's type actually supports it. Split out from
//! `find_many_where.rs` per the repo's 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::ident;

use super::find_many_where::is_filterable_scalar;

/// `lt`/`lte`/`gt`/`gte` — every filterable scalar except `Boolean` and
/// enum (cratestack#928: declaration order is not a meaningful ordering
/// to expose, so enum fields stay equality/`in`-shaped only, matching the
/// untyped REST route's own `supports_comparison`). Unlike that route,
/// not gated to `Required` arity for the remaining scalars:
/// `FieldRef<M, T>`'s comparison methods never actually inspect `T` (see
/// `cratestack-sql::filter::field_ref`), so there's no technical reason
/// to withhold them from optional fields — this is a deliberate, real
/// improvement over the untyped route, not an inconsistency with it.
fn supports_ordering_ops(field: &Field, enum_names: &BTreeSet<&str>) -> bool {
    is_filterable_scalar(field, enum_names)
        && field.ty.name != "Boolean"
        && !enum_names.contains(field.ty.name.as_str())
}

/// `contains`/`startsWith` — `String`/`Cuid` only (the only two types
/// `FieldRef::contains`/`starts_with` are actually implemented for; a
/// `Uuid` field's `FieldRef<M, uuid::Uuid>` has no such impl), and only at
/// `Required`/`Optional` arity: those two impls are scoped to
/// `FieldRef<M, String>` / `FieldRef<M, Option<String>>` specifically, so
/// a scalar `String[]`/`Cuid[]` field's `FieldRef<M, Vec<String>>` has
/// neither method — a pre-existing gap independent of this ticket's
/// builder work, surfaced by it once a schema-authored scalar list field
/// existed to compile against (`Where`-clause filtering on a list column
/// has no obvious `contains`/`startsWith` semantics to begin with).
fn supports_string_ops(field: &Field) -> bool {
    matches!(field.ty.name.as_str(), "String" | "Cuid") && field.ty.arity != TypeArity::List
}

pub(super) fn build_field_push(
    field: &Field,
    module_ident: &syn::Ident,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_fn = ident(&field.name);
    let mut ops = vec![quote! {
        if let Some(value) = &filter.eq {
            filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().eq(value.clone())));
        }
        if let Some(value) = &filter.ne {
            filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().ne(value.clone())));
        }
        if let Some(values) = &filter.in_ {
            filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().in_(values.clone())));
        }
    }];

    if supports_ordering_ops(field, enum_names) {
        ops.push(quote! {
            if let Some(value) = &filter.lt {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().lt(value.clone())));
            }
            if let Some(value) = &filter.lte {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().lte(value.clone())));
            }
            if let Some(value) = &filter.gt {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().gt(value.clone())));
            }
            if let Some(value) = &filter.gte {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().gte(value.clone())));
            }
        });
    }

    if supports_string_ops(field) {
        ops.push(quote! {
            if let Some(value) = &filter.contains {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().contains(value.clone())));
            }
            if let Some(value) = &filter.starts_with {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().starts_with(value.clone())));
            }
        });
    }

    if field.ty.arity == TypeArity::Optional {
        ops.push(quote! {
            if let Some(is_null) = filter.is_null {
                filters.push(if is_null {
                    ::cratestack::FilterExpr::from(super::#module_ident::#field_fn().is_null())
                } else {
                    ::cratestack::FilterExpr::from(super::#module_ident::#field_fn().is_not_null())
                });
            }
        });
    }

    quote! {
        if let Some(filter) = &self.#field_ident {
            #(#ops)*
        }
    }
}
