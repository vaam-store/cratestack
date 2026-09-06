//! Generators for server and client enum types declared in `.cstack`.

use cratestack_core::EnumDecl;
use quote::quote;

use crate::shared::{doc_attrs, ident, schema_lit};

pub(crate) fn generate_enum_type(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    let enum_ident = ident(&enum_decl.name);
    let docs = doc_attrs(&enum_decl.docs);
    let variants = variant_tokens(enum_decl);
    let as_str_arms = as_str_arms(enum_decl);
    let parse_arms = parse_arms(enum_decl);
    let enum_name = schema_lit(&enum_decl.name);
    let accepted_variants = accepted_variants_lit(enum_decl);

    quote! {
        #docs
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub enum #enum_ident {
            #(#variants)*
        }

        impl #enum_ident {
            pub const fn as_str(self) -> &'static str {
                match self {
                    #(#as_str_arms)*
                }
            }
        }

        impl ::core::fmt::Display for #enum_ident {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl ::core::str::FromStr for #enum_ident {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    #(#parse_arms)*
                    _ => Err(format!(
                        "unknown enum variant `{value}` for `{}`; expected one of: {}",
                        #enum_name, #accepted_variants,
                    )),
                }
            }
        }

        impl ::cratestack::IntoSqlValue for #enum_ident {
            fn into_sql_value(self) -> ::cratestack::SqlValue {
                ::cratestack::SqlValue::String(self.to_string())
            }
        }
    }
}

pub(crate) fn generate_client_enum_type(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    let enum_ident = ident(&enum_decl.name);
    let docs = doc_attrs(&enum_decl.docs);
    let variants = variant_tokens(enum_decl);
    let as_str_arms = as_str_arms(enum_decl);
    let parse_arms = parse_arms(enum_decl);
    let enum_name = schema_lit(&enum_decl.name);
    let accepted_variants = accepted_variants_lit(enum_decl);

    quote! {
        #docs
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub enum #enum_ident {
            #(#variants)*
        }

        impl #enum_ident {
            pub const fn as_str(self) -> &'static str {
                match self {
                    #(#as_str_arms)*
                }
            }
        }

        impl ::core::fmt::Display for #enum_ident {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl ::core::str::FromStr for #enum_ident {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    #(#parse_arms)*
                    _ => Err(format!(
                        "unknown enum variant `{value}` for `{}`; expected one of: {}",
                        #enum_name, #accepted_variants,
                    )),
                }
            }
        }
    }
}

fn variant_tokens(enum_decl: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    enum_decl
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let variant_ident = ident(&variant.name);
            let variant_docs = doc_attrs(&variant.docs);
            let name = schema_lit(&variant.name);
            // The first variant is the `Default` — needed so model
            // structs with enum fields can `derive(Default)` for the
            // column-projection `Projection<T>` placeholder values.
            let default_attr = if index == 0 {
                quote! { #[default] }
            } else {
                quote! {}
            };
            quote! {
                #variant_docs
                #[serde(rename = #name)]
                #default_attr
                #variant_ident,
            }
        })
        .collect()
}

fn as_str_arms(enum_decl: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    enum_decl
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = ident(&variant.name);
            let name = schema_lit(&variant.name);
            quote! { Self::#variant_ident => #name, }
        })
        .collect()
}

/// Comma-joined variant names, baked in as one string literal — issue
/// #928's rejection message (and, by extension, the query-filter
/// `BadRequest` that wraps this `FromStr` error, in
/// `shared::types::query_scalar_parser_tokens`) needs to name the
/// accepted values, not just report that the given one didn't match.
fn accepted_variants_lit(enum_decl: &EnumDecl) -> syn::LitStr {
    let known = enum_decl
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    schema_lit(&known)
}

fn parse_arms(enum_decl: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    enum_decl
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = ident(&variant.name);
            let name = schema_lit(&variant.name);
            quote! { #name => Ok(Self::#variant_ident), }
        })
        .collect()
}
