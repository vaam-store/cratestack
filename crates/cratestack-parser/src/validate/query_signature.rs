//! Argument-type and result-type checks for a `query` block
//! (cratestack#867).
//!
//! Both are narrower than what `procedure` accepts, and deliberately so.
//! A `query`'s arguments are handed straight to `sqlx`'s `.bind(…)` and
//! its result is decoded by column-name `try_get`, so the boundary of
//! what the language accepts here is exactly the boundary of what the
//! generated code can bind and decode. Rejecting the rest *at parse time*
//! is the difference between "this schema names a type a `query`
//! parameter cannot be" and a page of unresolved `sqlx::Encode` trait
//! errors pointing inside a macro expansion.

use std::collections::BTreeSet;

use cratestack_core::{Query, Schema, TypeArity};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::reserved_idents::validate_reserved_identifier;
use crate::validate::reserved_keywords::validate_reserved_keyword;

/// Scalar `.cstack` types a `query` parameter may have in v1.
///
/// The omissions are decisions, not oversights:
/// - `Decimal` — its Rust type depends on the schema's `decimal =` backend
///   choice (cratestack#505), and whether that concrete type implements
///   `sqlx::Encode` depends on the backend crate's own feature set. Adding
///   it needs that matrix pinned down first; a money *result* column is
///   unaffected, since results decode through the same path `view` uses.
/// - `Json`, `Vector`, `Geography`, `Geometry` — each needs an extension or
///   a wrapper type whose bind shape is not obvious from the schema text.
/// - `Page`/`PageInput`/`FindMany` — composite request shapes belonging to
///   the generated read path, meaningless as a bind parameter.
/// - `type`/`enum`/`model` names — a struct has no single column to bind to.
///
/// Widening this list later is additive and breaks nothing.
const BINDABLE_ARG_TYPES: &[&str] = &[
    "String", "Cuid", "Int", "Float", "Boolean", "DateTime", "Uuid", "Bytes",
];

pub(super) fn validate_query_args(query: &Query) -> Result<(), SchemaError> {
    let mut seen = BTreeSet::new();
    for arg in &query.args {
        validate_reserved_identifier(
            &arg.name,
            arg.name_span,
            &format!("query parameter `{}` on query `{}`", arg.name, query.name),
        )?;
        validate_reserved_keyword(
            &arg.name,
            arg.name_span,
            &format!("query parameter `{}` on query `{}`", arg.name, query.name),
        )?;
        if !seen.insert(arg.name.as_str()) {
            return Err(span_error(
                format!(
                    "query `{}` declares parameter `{}` more than once",
                    query.name, arg.name
                ),
                arg.span,
            ));
        }
        if arg.ty.arity != TypeArity::Required {
            return Err(span_error(
                format!(
                    "query `{}` parameter `{}` must be a required scalar — optional (`T?`) and \
                     list (`T[]`) parameters are not supported in v1",
                    query.name, arg.name
                ),
                arg.span,
            ));
        }
        if !BINDABLE_ARG_TYPES.contains(&arg.ty.name.as_str()) {
            return Err(span_error(
                format!(
                    "query `{}` parameter `{}` has type `{}`, which cannot be bound as a SQL \
                     parameter; supported parameter types are: {}",
                    query.name,
                    arg.name,
                    arg.ty.name,
                    BINDABLE_ARG_TYPES.join(", ")
                ),
                arg.span,
            ));
        }
    }
    Ok(())
}

/// The result type must name a declared `type` block, at `Required` arity
/// (one row) or `List` arity (`T[]`, many rows).
///
/// A `model` is rejected even though it would decode: a model's generated
/// struct is tied to a table's columns and its read path carries
/// soft-delete and row-policy filtering that a raw `query` body does *not*
/// get (design §6). Letting a `query` hand back a `Model` would make it
/// look like an ordinary, policy-filtered model read when it is nothing of
/// the kind. A `type` has no such implication, which is why the design
/// picks it.
pub(super) fn validate_query_result_type(
    query: &Query,
    schema: &Schema,
    type_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let name = query.result_type.name.as_str();

    if query.result_type.arity == TypeArity::Optional {
        return Err(span_error(
            format!(
                "query `{}` declares an optional result type `{name}?`; use `{name}` for exactly \
                 one row or `{name}[]` for zero or more",
                query.name
            ),
            query.span,
        ));
    }

    if schema.types.iter().any(|ty| ty.name == name) {
        return Ok(());
    }

    // Distinguish "you named something that isn't a `type`" from "you named
    // nothing that exists" — the fix differs, so the message should too.
    let hint = if type_names.contains(name) {
        format!(
            "`{name}` is not a `type` declaration; a query's result must be a `type` block, \
             because a query's raw SQL gets none of the soft-delete or row-policy filtering a \
             model read does"
        )
    } else {
        format!("no `type {name}` is declared in this schema")
    };
    Err(span_error(
        format!("query `{}` has an unknown result type: {hint}", query.name),
        query.span,
    ))
}
