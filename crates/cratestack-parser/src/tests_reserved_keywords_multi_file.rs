#![cfg(test)]
//! cratestack#922: reserve the words `part` and `import` as `.cstack`
//! keywords NOW, before the multi-file-schemas feature (epic #910) makes
//! them command words. Any use of either word as an identifier — model,
//! mixin, type, enum or view name; field names; enum variants; procedure
//! names and argument names — must be rejected here, at schema-parse
//! time, with an error that explains *why*: the word is reserved for
//! multi-file schemas (`part "file.cstack"` / `import "file.cstack"`).
//!
//! The check is exact-word only: `partition`, `important`, `imports` and
//! `importer` all contain a reserved word as a substring but must keep
//! parsing (see
//! `substring_matches_of_multi_file_reserved_keywords_are_not_rejected`).

use super::parse_schema;

#[test]
fn rejects_part_and_import_as_a_model_field_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
model KwProbe {{
  id Int @id
  {word} String
}}
"#
        );
        let error =
            parse_schema(&source).expect_err(&format!("a field named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("KwProbe"), "error: {message}");
        assert!(message.contains("model"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_type_block_field_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
type KwProbe {{
  {word} String
}}
"#
        );
        let error = parse_schema(&source).expect_err(&format!(
            "a `type` block field named `{word}` must be rejected"
        ));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("KwProbe"), "error: {message}");
        assert!(message.contains("type"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_mixin_field_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
mixin KwProbe {{
  {word} String
}}
"#
        );
        let error = parse_schema(&source)
            .expect_err(&format!("a mixin field named `{word}` must be rejected"));

        assert!(error.to_string().contains("mixin"), "error: {error}");
        assert!(error.to_string().contains("reserved"), "error: {error}");
        assert!(error.to_string().contains("multi-file"), "error: {error}");
    }
}

// --- top-level and other ident sites ---

#[test]
fn rejects_part_and_import_as_an_enum_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
enum {word} {{
  active
  inactive
}}
"#
        );
        let error =
            parse_schema(&source).expect_err(&format!("an enum named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("enum"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_an_enum_variant_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
enum Status {{
  {word}
  active
}}
"#
        );
        let error = parse_schema(&source)
            .expect_err(&format!("an enum variant named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("Status"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_top_level_model_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
model {word} {{
  id Int @id
}}
"#
        );
        let error =
            parse_schema(&source).expect_err(&format!("a model named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("model"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_top_level_mixin_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
mixin {word} {{
  createdAt DateTime
}}
"#
        );
        let error =
            parse_schema(&source).expect_err(&format!("a mixin named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("mixin"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_top_level_type_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
type {word} {{
  label String
}}
"#
        );
        let error = parse_schema(&source)
            .expect_err(&format!("a `type` block named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("type"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_view_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
datasource db {{
  provider = "postgresql"
}}

model Customer {{
  id Int @id
}}

view {word} from Customer {{
  id Int @id @from(Customer.id)

  @@server_sql("SELECT id FROM customer")
}}
"#
        );
        let error =
            parse_schema(&source).expect_err(&format!("a view named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("view"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_procedure_name() {
    for word in ["part", "import"] {
        let source = format!("procedure {word}(): Int\n");
        let error = parse_schema(&source)
            .expect_err(&format!("a procedure named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("procedure"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_procedure_argument_name() {
    for word in ["part", "import"] {
        let source = format!("procedure getFeed({word}: Int): Int\n");
        let error = parse_schema(&source).expect_err(&format!(
            "a procedure argument named `{word}` must be rejected"
        ));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("procedure argument"), "error: {message}");
        assert!(message.contains("getFeed"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_query_name() {
    // The `query` block is newer than #922's original ident list; upstream
    // added it in #867/#870, and its name is an ident site the Rust-keyword
    // rule already covers (`validate/queries.rs`) — so the reservation must
    // cover it too, or a `query part` would slip through while `model part`
    // is rejected.
    for word in ["part", "import"] {
        let source = format!(
            r#"
type Totals {{
  total Int
}}

query {word}(userId: String): Totals
  @@sql("SELECT $1::text AS total")
"#
        );
        let error =
            parse_schema(&source).expect_err(&format!("a query named `{word}` must be rejected"));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("query"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn rejects_part_and_import_as_a_query_parameter_name() {
    for word in ["part", "import"] {
        let source = format!(
            r#"
type Totals {{
  total Int
}}

query totals({word}: String): Totals
  @@sql("SELECT $1::text AS total")
"#
        );
        let error = parse_schema(&source).expect_err(&format!(
            "a query parameter named `{word}` must be rejected"
        ));

        let message = error.to_string();
        assert!(message.contains(word), "error: {message}");
        assert!(message.contains("query parameter"), "error: {message}");
        assert!(message.contains("reserved"), "error: {message}");
        assert!(message.contains("multi-file"), "error: {message}");
    }
}

#[test]
fn substring_matches_of_multi_file_reserved_keywords_are_not_rejected() {
    // `partition` contains `part` as a substring; `important`, `imports`
    // and `importer` contain `import`. None of them *is* the reserved
    // word, so all must keep parsing fine — the check is exact-match, not
    // substring.
    let schema = parse_schema(
        r#"
model Probe {
  id Int @id
  partition String
  important String
  imports String
  importer String
}
"#,
    )
    .expect("`partition`/`important`/`imports`/`importer` fields are not reserved keywords and must parse");

    assert_eq!(schema.models[0].fields[1].name, "partition");
    assert_eq!(schema.models[0].fields[2].name, "important");
    assert_eq!(schema.models[0].fields[3].name, "imports");
    assert_eq!(schema.models[0].fields[4].name, "importer");
}

#[test]
fn ordinary_top_level_names_are_not_rejected() {
    // A plain schema using only ordinary identifiers for every ident site
    // this module now covers must keep parsing successfully.
    let schema = parse_schema(
        r#"
enum Status {
  active
  inactive
}

model Widget {
  id Int @id
  status Status
}

mixin Timestamps {
  createdAt DateTime
}

type Summary {
  count Int
}

procedure fetchWidget(id: Int): Widget
"#,
    )
    .expect("an ordinary schema should parse and validate fine");

    assert_eq!(schema.enums[0].name, "Status");
    assert_eq!(schema.models[0].name, "Widget");
    assert_eq!(schema.mixins[0].name, "Timestamps");
    assert_eq!(schema.types[0].name, "Summary");
    assert_eq!(schema.procedures[0].name, "fetchWidget");
}
