#![cfg(test)]

//! A `@computed` field whose own type is a `type` block — the shape
//! `docs/design/computed-fields.md`'s "Schema surface" describes as "a
//! scalar, enum, or non-computed-bearing `type`", exercised on a
//! **model** owner where `validate::type_names::
//! reject_type_decl_as_model_field_type` used to make it unreachable.
//!
//! Split out of `tests_computed` rather than appended to it: that file is
//! already grandfathered past the 200-line ceiling
//! (`.ci/file-length-allowlist.toml`), and these cases are one coherent
//! concern — the storage-type exemption and the three boundaries that
//! must survive it.

use super::parse_schema;

#[test]
fn accepts_a_type_valued_computed_field_on_a_model() {
    // `docs/design/computed-fields.md`'s "Schema surface" allows a
    // computed field's own type to be "a scalar, enum, or
    // non-computed-bearing `type`", and `validate::computed` enforces
    // exactly that over models and `type`s alike. But
    // `validate::type_names::reject_type_decl_as_model_field_type`
    // (#230/#235) used to reject *every* model field declared with a
    // `type` block, `@computed` ones included, which made the documented
    // shape unreachable on a model. That rule is about **storage** — a
    // `type` has no `CREATE TYPE` behind it, so it cannot back a column —
    // and a `@computed` field is never a column: `cratestack-migrate`'s
    // `convert.rs` skips it before `field_to_column` ever runs.
    let schema = parse_schema(
        r#"
enum MediaKind {
  image
  video
}

type ProductDefaultMedia {
  assetId String
  kind MediaKind
  url String
}

model Product {
  id Int @id
  title String
  defaultMedia ProductDefaultMedia? @computed
}
"#,
    )
    .expect("a `type`-valued `@computed` model field should validate");

    let field = schema.models[0]
        .fields
        .iter()
        .find(|field| field.name == "defaultMedia")
        .expect("the computed field should survive parsing");
    assert_eq!(field.ty.name, "ProductDefaultMedia");
}

#[test]
fn accepts_a_parameterized_type_valued_computed_field_on_a_model() {
    // The exemption is on the field being `@computed`, not on the bare
    // form of the attribute — a parameterized resolver returning a
    // `type` is the same storage-free shape.
    parse_schema(
        r#"
type ProductDefaultMedia {
  url String
}

type MediaParams {
  width Int?
}

model Product {
  id Int @id
  defaultMedia ProductDefaultMedia? @computed(params: MediaParams?)
}
"#,
    )
    .expect("a parameterized `type`-valued `@computed` model field should validate");
}

#[test]
fn still_rejects_a_type_valued_model_field_without_computed() {
    // The exemption must not widen #235: a *stored* model field typed
    // with a `type` block is still a column with no `CREATE TYPE` behind
    // it, and is still rejected. This is the guard that keeps the fix
    // scoped to response-time fields.
    let error = parse_schema(
        r#"
type ProductDefaultMedia {
  url String
}

model Product {
  id Int @id
  defaultMedia ProductDefaultMedia
}
"#,
    )
    .expect_err("a stored `type`-valued model field should still be rejected");

    assert!(
        error
            .to_string()
            .contains("cannot use `type ProductDefaultMedia` as its storage type"),
        "message: {error}"
    );
}

#[test]
fn still_rejects_a_computed_field_typed_as_a_model() {
    // The exemption is scoped to `type` blocks — a `@computed` field
    // typed as a *model* stays rejected. On a model owner the relation
    // validator (`validate_field_relation`) reaches it first, because on
    // a model that spelling is relation syntax; `validate::computed`'s
    // own "never a model" rule is the backstop on a `type` owner. Either
    // way the schema does not parse, which is what this guards.
    let error = parse_schema(
        r#"
model Media {
  id Int @id
}

model Product {
  id Int @id
  defaultMedia Media? @computed
}
"#,
    )
    .expect_err("a model-typed `@computed` field should still be rejected");

    let message = error.to_string();
    assert!(
        message.contains("defaultMedia") && message.contains("Product"),
        "message: {message}"
    );
}

#[test]
fn still_rejects_a_computed_field_typed_as_a_computed_bearing_type() {
    // Same boundary from the other side: exempting `@computed` fields
    // from the storage-type check must not let a computed-bearing `type`
    // through `validate::computed`'s nested-resolution rule.
    let error = parse_schema(
        r#"
type Inner {
  url String @computed
}

model Product {
  id Int @id
  inner Inner? @computed
}
"#,
    )
    .expect_err("a computed-bearing `type` as a computed field's type should still be rejected");

    let message = error.to_string();
    assert!(
        message.contains("itself") && message.contains("contains `@computed` fields"),
        "message: {message}"
    );
}
