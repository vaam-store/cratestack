//! Per-scalar-field match arms emitted into the generated
//! `<model>_filter_expr` / `<model>_order_by` switch tables. Each arm
//! pairs `(field_name, operator)` with a `FieldRef<...>` call on the
//! per-field accessor in the field module.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::{
    ident, query_scalar_list_parser_tokens, query_scalar_parser_tokens, supports_comparison,
};

#[cfg(test)]
mod tests;

pub(super) fn generate_query_filter_arm(
    field_module_ident: &syn::Ident,
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> Option<proc_macro2::TokenStream> {
    let field_name = &field.name;
    let field_fn = ident(&field.name);
    let scalar_parser =
        query_scalar_parser_tokens(&field.ty, quote! { value }, field_name, enum_names)?;
    let mut arms = Vec::new();

    if matches!(field.ty.arity, TypeArity::Required | TypeArity::Optional) {
        let list_parser = query_scalar_list_parser_tokens(&field.ty, field_name, enum_names)?;
        arms.push(quote! {
            (#field_name, "eq") => {
                let parsed = (#scalar_parser)?;
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().eq(parsed)))
            }
        });
        arms.push(quote! {
            (#field_name, "ne") => {
                let parsed = (#scalar_parser)?;
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().ne(parsed)))
            }
        });
        arms.push(quote! {
            (#field_name, "in") => {
                let parsed = #list_parser;
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().in_(parsed)))
            }
        });
        if supports_comparison(field) {
            arms.push(quote! {
                (#field_name, "lt") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().lt(parsed)))
                }
            });
            arms.push(quote! {
                (#field_name, "lte") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().lte(parsed)))
                }
            });
            arms.push(quote! {
                (#field_name, "gt") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().gt(parsed)))
                }
            });
            arms.push(quote! {
                (#field_name, "gte") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().gte(parsed)))
                }
            });
        }
    }

    if matches!(field.ty.name.as_str(), "String" | "Cuid")
        && matches!(field.ty.arity, TypeArity::Required | TypeArity::Optional)
    {
        arms.push(quote! {
            (#field_name, "contains") => {
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().contains(value.to_owned())))
            }
        });
        arms.push(quote! {
            (#field_name, "startsWith") => {
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().starts_with(value.to_owned())))
            }
        });
    }

    if field.ty.arity == TypeArity::Optional {
        arms.push(quote! {
            (#field_name, "isNull") => {
                let parsed = value.parse::<bool>().map_err(|error| {
                    CratestackError::BadRequest(format!("invalid value '{}' for {}__isNull: {error}", value, #field_name))
                })?;
                Ok(if parsed {
                    ::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().is_null())
                } else {
                    ::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().is_not_null())
                })
            }
        });
    }

    if arms.is_empty() {
        None
    } else {
        Some(quote! { #(#arms)* })
    }
}

pub(super) fn generate_order_by_arm(
    field_module_ident: &syn::Ident,
    field: &Field,
) -> proc_macro2::TokenStream {
    let field_name = &field.name;
    let field_fn = ident(&field.name);

    quote! {
        #field_name => {
            if descending {
                request.order_by(super::#field_module_ident::#field_fn().desc())
            } else {
                request.order_by(super::#field_module_ident::#field_fn().asc())
            }
        }
    }
}
