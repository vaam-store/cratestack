//! cratestack#784 evidence: the parser rejection added in
//! `cratestack-parser/src/validate/procedure_handler_collisions.rs` is
//! only worth anything if the collision it describes is real. This module
//! proves it against the **actual** emitters rather than a re-derivation —
//! [`generate_model_axum_handlers`] and [`generate_procedure_axum_handler`]
//! emit their `fn` items into one shared generated module, so a shared
//! `fn` name there is `error[E0428]`, full stop.
//!
//! The schemas below therefore cannot go through `parse_schema` — the
//! parser now refuses them, which is the whole point — so the `Model` /
//! `Procedure` values are built from a non-colliding schema and renamed,
//! which is the only step here that does not run real code.

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure};

use super::{generate_model_axum_handlers, generate_procedure_axum_handler};

/// Every `async fn <name>` item in a generated token stream.
fn emitted_fn_names(generated: &proc_macro2::TokenStream) -> BTreeSet<String> {
    let tokens = generated.to_string();
    let mut names = BTreeSet::new();
    let mut words = tokens.split_whitespace().peekable();
    while let Some(word) = words.next() {
        if word == "fn"
            && let Some(next) = words.peek()
        {
            // `quote!` renders `fn handle_get_order < R , CR ...` — the
            // ident is its own whitespace-delimited token, generics follow.
            names.insert(next.trim_end_matches('<').to_owned());
        }
    }
    names
}

fn order_model(name: &str) -> Model {
    let mut model = cratestack_parser::parse_schema(
        r#"
model Placeholder {
  id String @id
  total Int
}
"#,
    )
    .expect("fixture schema should parse")
    .models
    .remove(0);
    model.name = name.to_owned();
    model
}

fn procedure(name: &str) -> Procedure {
    let mut procedure = cratestack_parser::parse_schema(
        r#"
procedure placeholder(orderId: String): String
"#,
    )
    .expect("fixture schema should parse")
    .procedures
    .remove(0);
    procedure.name = name.to_owned();
    procedure
}

/// The exact pair cratestack#784 reports: `model Order` +
/// `procedure getOrder`.
#[test]
fn model_crud_and_procedure_handlers_share_an_emitted_fn_name() {
    let model = order_model("Order");
    let model_handlers =
        generate_model_axum_handlers(&model, std::slice::from_ref(&model), &BTreeSet::new())
            .expect("model handler emission should succeed");
    let procedure_handler =
        generate_procedure_axum_handler(&procedure("getOrder"), &BTreeSet::new())
            .expect("procedure handler emission should succeed");

    let model_fns = emitted_fn_names(&model_handlers);
    let procedure_fns = emitted_fn_names(&procedure_handler);

    assert!(
        model_fns.contains("handle_get_order"),
        "model handlers emitted: {model_fns:?}"
    );
    let shared = model_fns
        .intersection(&procedure_fns)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        shared,
        vec!["handle_get_order", "handle_get_order_dispatch"],
        "both emitters must land on the same `fn` names — this is the E0428"
    );
}

/// The rename cratestack#784 records as the workaround really does clear
/// the collision, so the parser rejection is not merely unfalsifiable.
#[test]
fn a_renamed_procedure_shares_no_emitted_fn_name() {
    let model = order_model("Order");
    let model_handlers =
        generate_model_axum_handlers(&model, std::slice::from_ref(&model), &BTreeSet::new())
            .expect("model handler emission should succeed");
    let procedure_handler =
        generate_procedure_axum_handler(&procedure("orderDetail"), &BTreeSet::new())
            .expect("procedure handler emission should succeed");

    assert!(emitted_fn_names(&model_handlers).is_disjoint(&emitted_fn_names(&procedure_handler)));
}
