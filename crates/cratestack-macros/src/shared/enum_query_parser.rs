//! Runtime value parser for an enum-typed query-filter value (issue
//! #928: an enum field previously fell into `query_scalar_parser_tokens`'s
//! catch-all `_ => return None`, silently dropping its whole filter arm —
//! see `super::query_scalar_parser_tokens`'s call site in this module).
//!
//! Reuses the enum's own generated `FromStr` impl
//! (`crate::types::enums::generate_enum_type`/`generate_client_enum_type`)
//! rather than re-deriving a variant match here — that impl already knows
//! every declared variant name and (as of this same issue) already names
//! them in its error text, so wrapping it in the same "invalid value '{}'
//! for {}: {error}" `BadRequest` shape every other scalar parser in
//! `super::query_scalar_parser_tokens` uses gives an enum field the exact
//! same error-message convention as Int/Float/Boolean/Uuid/DateTime/
//! Decimal, with no new variant-matching code to keep in sync. Split out
//! per this crate's 200-LoC file convention.

use cratestack_core::TypeRef;
use quote::quote;

use super::ident;

/// `super::query_scalar_parser_tokens`'s enum branch — the caller has
/// already confirmed `ty.name` names a declared enum before calling this.
pub(crate) fn query_enum_parser_tokens(
    ty: &TypeRef,
    value_expr: proc_macro2::TokenStream,
    field_name: &str,
) -> proc_macro2::TokenStream {
    let enum_ident = ident(&ty.name);
    quote! {
        (#value_expr).parse::<super::#enum_ident>().map_err(|error| {
            CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
        })
    }
}
