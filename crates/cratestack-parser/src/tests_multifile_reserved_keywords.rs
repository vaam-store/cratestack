#![cfg(test)]
//! cratestack#922: `part` and `import` belong to the future multi-file
//! schema grammar, so every place that accepts a `.cstack` identifier must
//! reject them before those declarations ship. Keeping language keywords
//! unavailable in every identifier position also prevents tooling from
//! needing context-specific keyword lists.

use super::parse_schema;

fn assert_reserved(source: &str, keyword: &str, subject: &str) {
    let message = parse_schema(source)
        .expect_err("the multi-file keyword must be rejected")
        .to_string();
    assert!(message.contains(keyword), "error: {message}");
    assert!(message.contains(subject), "error: {message}");
    assert!(message.contains("reserved"), "error: {message}");
    assert!(message.contains("multi-file schemas"), "error: {message}");
}

fn identifier_sites(keyword: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "datasource",
            format!("datasource {keyword} {{\n  provider = \"postgresql\"\n}}"),
        ),
        ("auth block", format!("auth {keyword} {{\n  id String\n}}")),
        ("model", format!("model {keyword} {{\n  id Int @id\n}}")),
        (
            "field",
            format!("model Probe {{\n  id Int @id\n  {keyword} String\n}}"),
        ),
        ("mixin", format!("mixin {keyword} {{\n  value String\n}}")),
        ("field", format!("mixin Probe {{\n  {keyword} String\n}}")),
        ("type", format!("type {keyword} {{\n  value String\n}}")),
        ("field", format!("type Probe {{\n  {keyword} String\n}}")),
        (
            "enum",
            format!("enum {keyword} {{\n  active\n  inactive\n}}"),
        ),
        (
            "variant",
            format!("enum State {{\n  {keyword}\n  active\n}}"),
        ),
        ("field", format!("auth UserAuth {{\n  {keyword} String\n}}")),
        (
            "view",
            format!(
                r#"datasource db {{
  provider = "postgresql"
}}
model Source {{
  id Int @id
}}
view {keyword} from Source {{
  id Int @id @from(Source.id)
  @@server_sql("SELECT id FROM source")
}}"#
            ),
        ),
        (
            "field",
            format!(
                r#"datasource db {{
  provider = "postgresql"
}}
model Source {{
  id Int @id
  value String
}}
view Probe from Source {{
  id Int @id @from(Source.id)
  {keyword} String @from(Source.value)
  @@server_sql("SELECT id, value FROM source")
}}"#
            ),
        ),
        ("procedure", format!("procedure {keyword}(): Int")),
        (
            "procedure argument",
            format!("procedure lookup({keyword}: Int): Int"),
        ),
        (
            "query",
            format!(
                r#"type Result {{
  value Int
}}
query {keyword}(id: Int): Result
  @@sql("SELECT $1 AS value")"#
            ),
        ),
        (
            "query parameter",
            format!(
                r#"type Result {{
  value Int
}}
query lookup({keyword}: Int): Result
  @@sql("SELECT $1 AS value")"#
            ),
        ),
    ]
}

#[test]
fn rejects_multifile_keywords_at_every_identifier_site() {
    for keyword in ["part", "import"] {
        for (subject, source) in identifier_sites(keyword) {
            assert_reserved(&source, keyword, subject);
        }
    }
}

#[test]
fn reserves_exact_case_sensitive_words_without_reserving_of() {
    let schema = parse_schema(
        r#"
model Probe {
  id Int @id
  of String
  partOf String
  important String
  Import String
}
"#,
    )
    .expect("`of`, substrings, and differently cased names stay available");

    let names: Vec<_> = schema.models[0]
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(names, ["id", "of", "partOf", "important", "Import"]);
}
