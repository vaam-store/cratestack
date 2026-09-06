//! Regression coverage for cratestack#928: an enum-typed field used to
//! fall into `query_scalar_parser_tokens`'s `_ => return None` catch-all,
//! which `generate_query_filter_arm` propagated with `?` on its very
//! first line — the whole arm silently vanished, and the field was never
//! reachable from the generated `<model>_filter_expr` switch (a real
//! HTTP request against it 400s with "unsupported query filter"; see
//! `crates/cratestack-pg/tests/enum_query_filter.rs` for the full
//! HTTP-level proof against a live Postgres). This module proves the
//! same thing at the token-generation level, which runs without a
//! database.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, Schema};

use super::generate_query_filter_arm;

fn parse_fixture(source: &str) -> Schema {
    cratestack_parser::parse_schema(source).expect("fixture schema should parse and validate")
}

fn order_model(schema: &Schema) -> Model {
    schema
        .models
        .iter()
        .find(|model| model.name == "Order")
        .expect("fixture should declare an Order model")
        .clone()
}

fn state_field(model: &Model) -> Field {
    model
        .fields
        .iter()
        .find(|field| field.name == "state")
        .expect("Order model should declare a state field")
        .clone()
}

const ENUM_FIXTURE: &str = r#"
enum OrderState {
  reserved
  funded
  cancelled
}

model Order {
  id Int @id
  state OrderState

  @@allow("read", true)
}
"#;

#[test]
fn required_enum_field_generates_a_query_filter_arm() {
    let schema = parse_fixture(ENUM_FIXTURE);
    let model = order_model(&schema);
    let field = state_field(&model);
    let enum_names: BTreeSet<&str> = schema
        .enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect();
    let field_module_ident = syn::Ident::new("order", proc_macro2::Span::call_site());

    let arm = generate_query_filter_arm(&field_module_ident, &field, &enum_names);

    assert!(
        arm.is_some(),
        "an enum-typed required field must generate a query-filter arm — \
         cratestack#928's whole bug is that this was `None`"
    );
    let rendered = arm.unwrap().to_string();
    assert!(
        rendered.contains("\"eq\""),
        "expected an eq arm, got: {rendered}"
    );
    assert!(
        rendered.contains("\"ne\""),
        "expected a ne arm, got: {rendered}"
    );
    assert!(
        rendered.contains("\"in\""),
        "expected an in arm, got: {rendered}"
    );
    // Declaration order is not a meaningful ordering to expose (issue
    // #928's explicit ask) — lt/gt/lte/gte must stay absent even though
    // eq/ne/in are now generated.
    assert!(
        !rendered.contains("\"lt\"") && !rendered.contains("\"gt\""),
        "enum fields must not gain comparison operators, got: {rendered}"
    );
}

/// Without an enum in scope at all (the pre-#928 world, modeled here by
/// passing an empty `enum_names` set for a field whose type name isn't
/// one of the eight builtin scalars either), the arm is still correctly
/// absent — proves the fix is additive, not a blanket "always Some" that
/// would silently accept genuinely-unsupported types (`Json`, `Bytes`,
/// custom `type` blocks).
#[test]
fn non_enum_unsupported_type_still_generates_no_arm() {
    let schema = parse_fixture(ENUM_FIXTURE);
    let model = order_model(&schema);
    let field = state_field(&model);
    let field_module_ident = syn::Ident::new("order", proc_macro2::Span::call_site());

    let arm = generate_query_filter_arm(&field_module_ident, &field, &BTreeSet::new());

    assert!(
        arm.is_none(),
        "an unrecognized type name (enum set empty) must still generate no arm"
    );
}
