#![cfg(test)]
//! cratestack#922 — the `query` block half of the reservation tests.
//! Sibling to `tests_reserved_keywords_multi_file` and
//! `tests_reserved_keywords_multi_file_entries`; split by concern so each
//! file stays under the 200-LoC ceiling.
//!
//! The `query` block is newer than #922's original ident list — upstream
//! added it in #867/#870 — and its name and parameters are ident sites the
//! Rust-keyword rule already covers (`validate/queries.rs`,
//! `validate/query_signature.rs`), so the reservation must cover them too,
//! or a `query part` would slip through while `model part` is rejected.

use super::parse_schema;

#[test]
fn rejects_part_and_import_as_a_query_name() {
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
