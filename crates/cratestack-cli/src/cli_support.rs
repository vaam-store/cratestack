use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};

/// Renders a `SchemaError` produced by parsing `schema` earlier in the same
/// call. Takes no `schema` argument (cratestack#916 removed it): the error
/// already carries its own file identity and source text from the moment
/// `parse_schema_file` produced it, so there's no longer a second
/// (potentially stale, or simply wrong) disk read to keep in sync with it.
pub(crate) fn render_schema_error(error: &cratestack_parser::SchemaError) -> String {
    error.render()
}

pub(crate) fn json_check_success(schema: &Path) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "schema": schema.display().to_string(),
        "diagnostics": [],
    })
}

pub(crate) fn json_check_failure(
    schema: &Path,
    error: &cratestack_parser::SchemaError,
) -> serde_json::Value {
    let span = error.span();
    serde_json::json!({
        "ok": false,
        "schema": schema.display().to_string(),
        "diagnostics": [
            {
                "message": error.message(),
                // cratestack#916: which file the diagnostic belongs to —
                // always `schema` itself while `check` only ever parses one
                // file, but exposed per-diagnostic (not just the top-level
                // "schema" key) so this shape doesn't have to change again
                // the day `check` can report across several files.
                "file": error.file(),
                "line": error.line(),
                "start": span.start,
                "end": span.end,
            }
        ],
    })
}

pub(crate) fn parse_schema_or_render(schema: &PathBuf) -> Result<cratestack_core::Schema> {
    cratestack_parser::parse_schema_file(schema)
        .map_err(|error| anyhow!(render_schema_error(&error)))
}

/// Hex-encoded SHA-256 of the schema file's raw bytes — the *same*
/// computation `cratestack-macros::include::parse::hash_schema_source`
/// does for `include_server_schema!`/`include_client_schema!` (issue
/// #178), so a Rust server, a Rust client, and a generated Dart/TypeScript
/// client all agree on one hash for one schema file. Deliberately
/// duplicated rather than shared via a dependency: this crate can't
/// depend on `cratestack-macros` (a proc-macro crate), and the
/// computation is five lines, not worth a shared crate for.
pub(crate) fn hash_schema_source(schema: &Path) -> Result<String> {
    let source = std::fs::read_to_string(schema)
        .with_context(|| format!("failed to read '{}'", schema.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    // sha2 0.11 / digest 0.11 return `hybrid_array::Array`, which (unlike
    // digest 0.10's `GenericArray`) implements no `LowerHex`. The
    // byte-wise `{:02x}` fold below is this repo's existing hex idiom
    // (`cratestack-core/src/transport.rs`) and is byte-for-byte what
    // `format!("{:x}", …)` produced — this string is persisted/keyed on,
    // so it must not change shape.
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::hash_schema_source;

    #[test]
    fn matches_the_same_known_sha256_the_macros_crate_test_uses() {
        // Same fixture string and expected digest as
        // `cratestack-macros::include::parse::tests::
        // hash_schema_source_matches_a_known_sha256` — the whole point is
        // that these two independent implementations agree.
        let mut file = NamedTempFile::new().expect("tempfile");
        write!(file, "model Widget {{ id Int @id }}").expect("write fixture");
        let hash = hash_schema_source(file.path()).expect("hash should succeed");
        assert_eq!(
            hash,
            "50fa300ea14f963f4573be7bfff0fb95b58d728f2431afbecb43578370af6e3e"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedFile {
    pub(crate) file_name: String,
    pub(crate) contents: String,
}

pub(crate) trait GeneratedFileLike {
    fn into_generated_file(self) -> GeneratedFile;
}

impl GeneratedFileLike for cratestack_client_dart::GeneratedDartFile {
    fn into_generated_file(self) -> GeneratedFile {
        GeneratedFile {
            file_name: self.file_name,
            contents: self.contents,
        }
    }
}

impl GeneratedFileLike for cratestack_client_typescript::GeneratedTypeScriptFile {
    fn into_generated_file(self) -> GeneratedFile {
        GeneratedFile {
            file_name: self.file_name,
            contents: self.contents,
        }
    }
}

impl GeneratedFileLike for cratestack_mock_wiremock::GeneratedWireMockFile {
    fn into_generated_file(self) -> GeneratedFile {
        GeneratedFile {
            file_name: self.file_name,
            contents: self.contents,
        }
    }
}

pub(crate) fn into_generated_files<T: GeneratedFileLike>(files: Vec<T>) -> Vec<GeneratedFile> {
    files
        .into_iter()
        .map(GeneratedFileLike::into_generated_file)
        .collect()
}

pub(crate) fn write_generated_files(out: &PathBuf, files: Vec<GeneratedFile>) -> Result<()> {
    std::fs::create_dir_all(out)?;
    for file in files {
        let destination = out.join(file.file_name);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, file.contents)?;
    }
    Ok(())
}
