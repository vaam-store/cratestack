#![cfg(test)]

//! Schema-wide `@computed` rules (`validate::computed`) plus the
//! per-declaration form/placement rules
//! (`validate::fields::validate_computed_field_attribute`). The happy
//! paths for `type` and `model` live in `tests_types`.

use super::parse_schema;

#[test]
fn rejects_computed_with_arguments() {
    let error = parse_schema(
        r#"
type Image {
  thumbnailUrl String @computed(lazy)
}
"#,
    )
    .expect_err("@computed with arguments should fail validation");

    assert!(error.to_string().contains("use bare `@computed`"));
}

#[test]
fn rejects_computed_combined_with_other_attributes() {
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  proxyUrl String @computed @readonly
}
"#,
    )
    .expect_err("@computed combined with another attribute should fail validation");

    assert!(
        error
            .to_string()
            .contains("combines `@computed` with `@readonly`")
    );
}

#[test]
fn rejects_computed_on_mixins_views_and_auth_blocks() {
    for (label, source) in [
        ("mixin", "mixin Timestamps {\n  ago String @computed\n}\n"),
        (
            "view",
            "model Post {\n  id Int @id\n}\n\nview PostSummary {\n  id Int @id\n  ago String @computed\n\n  from Post\n  @@sql(\"SELECT id FROM posts\")\n}\n",
        ),
        (
            "auth block",
            "auth Operator {\n  id Int\n  label String @computed\n}\n",
        ),
    ] {
        let error = parse_schema(source)
            .map(|_| ())
            .expect_err(&format!("@computed on a {label} should fail validation"));
        let message = error.to_string();
        assert!(
            message.contains("cannot use `@computed`"),
            "{label}: {message}"
        );
    }
}

#[test]
fn rejects_computed_field_typed_as_a_model() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

type Card {
  owner User @computed
}
"#,
    )
    .expect_err("a model-typed computed field should fail validation");

    assert!(error.to_string().contains("not a database row"));
}

#[test]
fn rejects_computed_field_typed_as_a_computed_bearing_type() {
    let error = parse_schema(
        r#"
type Image {
  thumbnailUrl String @computed
}

type Card {
  cover Image @computed
}
"#,
    )
    .expect_err("a computed field of computed-bearing type should fail validation");

    assert!(error.to_string().contains("would never be resolved"));
}

#[test]
fn accepts_computed_bearing_type_as_procedure_return() {
    parse_schema(
        r#"
type Image {
  storageKey String
  thumbnailUrl String @computed
}

procedure getImage(id: Int): Image
"#,
    )
    .expect("computed-bearing return types should parse");
}

#[test]
fn rejects_computed_bearing_type_as_procedure_argument() {
    let error = parse_schema(
        r#"
type Image {
  storageKey String
  thumbnailUrl String @computed
}

procedure saveImage(image: Image): Int
"#,
    )
    .expect_err("computed-bearing argument types should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot be used as procedure input")
    );
}

#[test]
fn rejects_computed_bearing_type_nested_inside_a_procedure_argument() {
    let error = parse_schema(
        r#"
type Image {
  thumbnailUrl String @computed
}

type Gallery {
  cover Image
}

procedure saveGallery(gallery: Gallery): Int
"#,
    )
    .expect_err("transitively computed-bearing argument types should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot be used as procedure input")
    );
}

#[test]
fn rejects_computed_bearing_model_as_procedure_argument() {
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  proxyUrl String @computed
}

procedure saveImage(image: Image): Int
"#,
    )
    .expect_err("computed-bearing model argument types should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot be used as procedure input")
    );
}

#[test]
fn rejects_stream_procedures_returning_computed_bearing_items() {
    let error = parse_schema(
        r#"
type Tick {
  label String @computed
}

procedure ticks(): Tick[]
  @stream
"#,
    )
    .expect_err("@stream over computed-bearing items should fail validation");

    assert!(error.to_string().contains("stream encoder"));
}

#[test]
fn rejects_computed_fields_in_composite_constraints() {
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  bucket String
  proxyUrl String @computed

  @@unique([bucket, proxyUrl])
}
"#,
    )
    .expect_err("composite constraints over computed fields should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot participate in database keys")
    );
}

/// Regression coverage for the composite-constraint predicate bug: a
/// guard that only matched bare `@computed` (`raw == "@computed"`)
/// missed the parameterized `@computed(params: <Type>?)` form entirely,
/// so `@@unique`/`@@id`/`@@index` over a parameterized computed field
/// parsed cleanly and then silently dropped the constraint (or narrowed
/// the primary key) at migration time — the field was never persisted
/// in the first place. Covers all three composite attributes, in both
/// the bare and parameterized spellings of `@computed`.
#[test]
fn rejects_computed_fields_in_composite_constraints_parameterized_form() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  bucket String
  proxyUrl String @computed(params: ProxyParams?)

  @@unique([bucket, proxyUrl])
}
"#,
    )
    .expect_err(
        "composite @@unique constraints over parameterized computed fields should fail validation",
    );

    assert!(
        error
            .to_string()
            .contains("cannot participate in database keys")
    );
}

#[test]
fn rejects_computed_field_in_composite_id_bare_form() {
    let error = parse_schema(
        r#"
model Image {
  bucket String
  proxyUrl String @computed

  @@id([bucket, proxyUrl])
}
"#,
    )
    .expect_err("@@id over a bare computed field should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot participate in database keys")
    );
}

#[test]
fn rejects_computed_field_in_composite_id_parameterized_form() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  bucket String
  proxyUrl String @computed(params: ProxyParams?)

  @@id([bucket, proxyUrl])
}
"#,
    )
    .expect_err("@@id over a parameterized computed field should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot participate in database keys")
    );
}

#[test]
fn rejects_computed_field_in_index_bare_form() {
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  proxyUrl String @computed

  @@index([proxyUrl])
}
"#,
    )
    .expect_err("@@index over a bare computed field should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot participate in database keys")
    );
}

#[test]
fn rejects_resolver_method_name_collision_across_owners() {
    // `model Image { setUrl }` and `type ImageSet { url }` both flatten,
    // under `to_snake_case`, to the resolver method `resolve_image_set_url`
    // — a duplicate trait method that would otherwise surface as a raw
    // rustc error at the `include_*_schema!` call site.
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  setUrl String @computed
}

type ImageSet {
  url String @computed
}
"#,
    )
    .expect_err("a resolver method name collision across owners should fail validation");

    let message = error.to_string();
    assert!(
        message.contains("resolve_image_set_url"),
        "message: {message}"
    );
    assert!(message.contains("setUrl"), "message: {message}");
    assert!(message.contains("url"), "message: {message}");
}

#[test]
fn rejects_computed_field_in_index_parameterized_form() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  proxyUrl String @computed(params: ProxyParams?)

  @@index([proxyUrl])
}
"#,
    )
    .expect_err("@@index over a parameterized computed field should fail validation");

    assert!(
        error
            .to_string()
            .contains("cannot participate in database keys")
    );
}
