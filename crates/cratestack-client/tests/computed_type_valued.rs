//! The generated Rust client carries a `type`-valued `@computed` model
//! field as the declared `type`, and encodes it as a nested object.
//!
//! Client-side shapes *include* computed fields — that is the wire shape
//! (`docs/design/computed-fields.md` §"Generated server surface") — so
//! this is the client half of what
//! `crates/cratestack-pg/tests/computed_fields_type_valued.rs` proves the
//! server emits. It is a compile-time assertion as much as a runtime one:
//! the struct literal below would not build if the generator mapped the
//! field to anything but `Option<ClientTvMedia>`.
//!
//! Needs no server and no database — `include_client_schema!` generates
//! types and a reqwest runtime only.

use cratestack::include_client_schema;

include_client_schema!("tests/fixtures/computed_type_valued.cstack");

#[test]
fn a_type_valued_computed_field_is_a_nested_object_on_the_client_model() {
    let product = cratestack_schema::ClientTvProduct {
        id: 1,
        title: "kettle".to_owned(),
        defaultMedia: Some(cratestack_schema::ClientTvMedia {
            assetId: "a1".to_owned(),
            kind: cratestack_schema::ClientTvKind::image,
            url: "https://cdn.example/a1.jpg".to_owned(),
        }),
    };

    let encoded = cratestack::serde_json::to_value(&product).expect("model should serialize");
    assert!(
        encoded["defaultMedia"].is_object(),
        "the computed field must stay a nested object, not be flattened: {encoded}"
    );
    assert_eq!(encoded["defaultMedia"]["assetId"], "a1");
    assert_eq!(
        encoded["defaultMedia"]["kind"], "image",
        "an enum nested inside the computed value keeps its own wire encoding"
    );
}

#[test]
fn an_absent_type_valued_computed_field_round_trips_as_null() {
    let product = cratestack_schema::ClientTvProduct {
        id: 2,
        title: "lamp".to_owned(),
        defaultMedia: None,
    };

    let encoded = cratestack::serde_json::to_value(&product).expect("model should serialize");
    assert!(encoded["defaultMedia"].is_null());

    let decoded: cratestack_schema::ClientTvProduct =
        cratestack::serde_json::from_value(encoded).expect("model should deserialize");
    assert!(decoded.defaultMedia.is_none());
}
