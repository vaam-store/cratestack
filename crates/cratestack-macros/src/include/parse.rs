//! Shared schema file loader for the three top-level include macros.
//! Macro-argument grammars (`db = Postgres`, `decimal = ...`) live in
//! `schema_args.rs`, re-exported below — see that file's doc for why the
//! split.

use std::path::PathBuf;

use proc_macro::TokenStream;
use sha2::{Digest, Sha256};
use syn::LitStr;

pub(super) use super::schema_args::{SchemaPathArgs, ServerDb, ServerSchemaArgs};

pub(super) fn parse_schema_literal(
    schema_path: &LitStr,
) -> Result<(String, PathBuf, cratestack_core::Schema, String), TokenStream> {
    let schema_relative = schema_path.value();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let resolved = PathBuf::from(&manifest_dir).join(&schema_relative);
    let source = std::fs::read_to_string(&resolved).map_err(|error| {
        TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                format!("failed to read schema file {}: {error}", resolved.display()),
            )
            .to_compile_error(),
        )
    })?;

    let schema = cratestack_parser::parse_schema_named(&resolved.display().to_string(), &source)
        .map_err(|error| {
            TokenStream::from(
                syn::Error::new(schema_path.span(), error.render()).to_compile_error(),
            )
        })?;

    reject_composite_primary_keys(schema_path, &schema)?;

    let schema_sha256 = hash_schema_source(&source);

    Ok((schema_relative, resolved, schema, schema_sha256))
}

/// Raw SHA-256 of the schema's source bytes, hex-encoded — deliberately not
/// a canonicalized/semantic hash of the parsed IR. Two byte-identical
/// schema files always agree; two schemas that differ only cosmetically
/// (whitespace, comments) will disagree even though nothing meaningful
/// changed. That's an accepted tradeoff, not an oversight: the value only
/// ever feeds a `tracing::warn!` on the server side (`cratestack-axum`'s
/// schema-fingerprint middleware) — never a rejection — so a false-positive
/// warning on a cosmetic diff costs a stray log line, while the simplicity
/// of "hash the bytes, no parsing required to compare" is worth more than
/// perfect precision here.
pub(super) fn hash_schema_source(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    // sha2 0.11 / digest 0.11 return `hybrid_array::Array`, which (unlike
    // digest 0.10's `GenericArray`) implements no `LowerHex`. The
    // byte-wise `{:02x}` fold below is this repo's existing hex idiom
    // (`cratestack-core/src/transport.rs`) and is byte-for-byte what
    // `format!("{:x}", …)` produced — this string is persisted/keyed on,
    // so it must not change shape.
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `@@id([...])` composite primary keys are parsed and validated by
/// `cratestack-parser`, and `cratestack-migrate` already emits correct
/// composite `PRIMARY KEY` DDL for them — but query builders, axum/RPC
/// routing, and all three client generators still assume exactly one
/// scalar PK column throughout (`ModelDescriptor<M, PK>` and friends).
/// Fail here with one clear message instead of letting a model with
/// `@@id(...)` reach codegen and panic somewhere deep in a `.find(...)
/// .expect(...)` call with no useful context.
///
/// Tracking: <https://github.com/cratestack/cratestack/issues/136>.
fn reject_composite_primary_keys(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if let Some(model) = find_composite_id_model(schema) {
        return Err(TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                ::cratestack_core::composite_id::composite_id_unsupported_message(&model.name),
            )
            .to_compile_error(),
        ));
    }
    Ok(())
}

// Delegates to `cratestack_core::composite_id` so the CLI generators
// (which panicked on exactly these schemas until 2026-08-13, because
// they had no equivalent guard) match this path's behaviour by
// construction rather than by a second hand-copied `starts_with`.
fn find_composite_id_model(schema: &cratestack_core::Schema) -> Option<&cratestack_core::Model> {
    ::cratestack_core::composite_id::find_composite_id_model(schema)
}

#[cfg(test)]
mod tests {
    use super::{find_composite_id_model, hash_schema_source};

    #[test]
    fn hash_schema_source_matches_a_known_sha256() {
        // printf '%s' 'model Widget { id Int @id }' | shasum -a 256
        assert_eq!(
            hash_schema_source("model Widget { id Int @id }"),
            "50fa300ea14f963f4573be7bfff0fb95b58d728f2431afbecb43578370af6e3e"
        );
    }

    #[test]
    fn hash_schema_source_is_deterministic_and_content_sensitive() {
        let a = hash_schema_source("model A { id Int @id }");
        let b = hash_schema_source("model A { id Int @id }");
        let c = hash_schema_source("model B { id Int @id }");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn flags_model_with_composite_id_attribute() {
        let schema = cratestack_parser::parse_schema(
            r#"
model AccountMembership {
  accountId Int
  subject String

  @@id([accountId, subject])
}
"#,
        )
        .expect("schema should parse");

        let flagged = find_composite_id_model(&schema);
        assert_eq!(
            flagged.map(|model| model.name.as_str()),
            Some("AccountMembership")
        );
    }

    #[test]
    fn does_not_flag_single_field_id() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Account {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(find_composite_id_model(&schema).is_none());
    }
}
