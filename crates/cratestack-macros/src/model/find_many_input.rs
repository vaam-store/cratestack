//! Per-model `<Model>FindManyInput` — the top-level type `FindMany<Model>`
//! procedure arguments decode into, composing `<Model>Where` and
//! `<Model>OrderByClause` (see the sibling `find_many_where.rs`/
//! `find_many_order_by.rs`), plus the `build_<model>_query_from_find_many`
//! entry point procedure implementations call. Split out per the repo's
//! 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::builder::{BuilderField, generate_builder};
use crate::shared::{generated_doc_attr, ident, is_primary_key, rust_type_tokens, to_snake_case};

use super::find_many_order_by::generate_order_by_types;
use super::find_many_where::generate_where_struct;

/// The `<Model>Where`/`<Model>SortField`/`<Model>OrderByClause`/
/// `<Model>FindManyInput` *types* only — no `build_<model>_query_from_find_many`
/// entry point. Shared by both the server composer
/// ([`generate_find_many_input`], which adds that entry point on top)
/// and the client composer (`include/client.rs`), since a pure HTTP
/// client has no `super::Cratestack`/live query builder to build one
/// against — it only ever serializes a `<Model>FindManyInput` into a
/// request body. `<Model>Where::to_filters()` still resolves for the
/// client too (the `super::<model_snake>::<field>()` `FieldRef`
/// accessors it calls exist in client field modules as well — see
/// `field_module.rs`), it's just dead code there, which is harmless.
pub(crate) fn generate_find_many_types(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let where_types = generate_where_struct(model, model_names, enum_names);
    let order_by_types = generate_order_by_types(model, model_names);

    let find_many_ident = ident(&format!("{}FindManyInput", model.name));
    let where_ident = ident(&format!("{}Where", model.name));
    let order_by_ident = ident(&format!("{}OrderByClause", model.name));

    let docs = generated_doc_attr(format!(
        "What a `FindMany<{}>` procedure argument decodes into.",
        model.name
    ));

    // Both fields optional (`Option<_>` / `Option<Vec<_>>`), so a
    // non-generic builder — same as `{Model}Where`. `ident("where")`
    // raw-escapes to `r#where`, matching the struct field and giving the
    // setter itself the name `r#where`.
    let find_many_builder_fields = vec![
        BuilderField::new(ident("where"), quote! { Option<#where_ident> }, false),
        BuilderField::new(
            ident("order_by"),
            quote! { Option<Vec<#order_by_ident>> },
            false,
        ),
    ];
    let builder = generate_builder(&find_many_ident, &find_many_builder_fields);

    quote! {
        #where_types
        #order_by_types

        #docs
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #find_many_ident {
            pub r#where: Option<#where_ident>,
            pub order_by: Option<Vec<#order_by_ident>>,
        }

        #builder
    }
}

pub(crate) fn generate_find_many_input(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> Result<proc_macro2::TokenStream, String> {
    let types = generate_find_many_types(model, model_names, enum_names);

    let find_many_ident = ident(&format!("{}FindManyInput", model.name));
    let query_fn_ident = ident(&format!(
        "build_{}_query_from_find_many",
        to_snake_case(&model.name)
    ));
    let model_ident = ident(&model.name);
    let accessor_ident = ident(&to_snake_case(&model.name));
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    let query_fn_docs = generated_doc_attr(format!(
        "Converts a decoded `FindMany<{}>` procedure argument into a ready-to-run query \
         builder for `{}` — call `.paginate(PageInput)` or `.run()` on the result. `pub` \
         (unlike most per-model handler internals): procedure implementations live in a \
         separate app crate, not this generated module.",
        model.name, model.name
    ));

    Ok(quote! {
        #types

        #query_fn_docs
        pub fn #query_fn_ident<'a>(
            db: &'a super::Cratestack,
            input: &#find_many_ident,
        ) -> ::cratestack::FindMany<'a, super::models::#model_ident, #primary_key_type> {
            let mut request = db.#accessor_ident().find_many();
            if let Some(where_) = &input.r#where {
                for filter in where_.to_filters() {
                    request = request.where_expr(filter);
                }
            }
            if let Some(order_by) = &input.order_by {
                for clause in order_by {
                    request = request.order_by(clause.to_order_clause());
                }
            }
            request
        }
    })
}
