#![cfg(test)]
//! cratestack#922 — the "ident site inside a block or on a view/procedure"
//! half of the reservation tests. Sibling to
//! `tests_reserved_keywords_multi_file` (data-model declaration names +
//! exact-match guards) and `tests_reserved_keywords_multi_file_queries`.
//! Split by concern so each file stays under the 200-LoC ceiling.
//!
//! Every ident site a `.cstack` schema can name `part`/`import` must reject
//! with an error that explains why (reserved for multi-file schemas). This
//! file covers field names and the view/procedure/procedure-argument sites.

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
