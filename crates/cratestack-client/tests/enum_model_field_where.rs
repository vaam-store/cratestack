//! `include_client_schema!` must compile for a schema with an enum-typed
//! model field, and `<Model>Where` must expose that field as a filter.
//!
//! This is a compile-time regression guard as much as a runtime one.
//! cratestack#928 made enum fields filterable by admitting them into
//! `is_filterable_scalar`, which is read by BOTH schema composers — so the
//! client's generated `to_filters()` began calling `FieldRef::eq`, whose
//! `V: IntoSqlValue` bound the client enum generator did not satisfy. Every
//! such schema failed with E0277.
//!
//! No client fixture had an enum-typed model *scalar* field (the one that
//! did nested it inside a `type` block), so the whole client facade broke
//! with CI fully green. This file is that missing shape.

use cratestack::include_client_schema;

include_client_schema!("tests/fixtures/enum_model_field.cstack");

#[test]
fn enum_model_field_is_filterable_from_the_client() {
    use cratestack_schema::types::OrderState;

    // Constructing the filter is the assertion: it does not compile unless
    // the generated enum satisfies `IntoSqlValue`.
    let where_input = cratestack_schema::OrderWhere {
        state: Some(cratestack::FieldFilterInput {
            eq: Some(OrderState::funded),
            ..Default::default()
        }),
        ..Default::default()
    };

    let filters = where_input.to_filters();
    assert!(
        !filters.is_empty(),
        "an enum equality filter must reach the wire, not be silently dropped"
    );
}
