//! Public entry points: `parse_schema*`.
//!
//! Split out of `lib.rs` (cratestack#916) once every entry point that
//! actually knows a path started tagging its returned [`SchemaError`]s with
//! it via `SchemaError::with_file` — the extra couple of lines per function
//! pushed `lib.rs` (already carrying the full module declaration list) past
//! the 200-line ceiling.

use std::path::Path;
use std::sync::Arc;

use crate::{SchemaError, parse, validate};

pub fn parse_schema(source: &str) -> Result<cratestack_core::Schema, SchemaError> {
    parse_schema_named("<schema>", source)
}

pub fn parse_schema_named(
    path: &str,
    source: &str,
) -> Result<cratestack_core::Schema, SchemaError> {
    let file: Arc<str> = Arc::from(path);
    let source_arc: Arc<str> = Arc::from(source);
    let schema =
        parse::parse_schema_only(source).map_err(|error| error.with_file(&file, &source_arc))?;
    validate::validate_schema(path, source, &schema)
        .map_err(|error| error.with_file(&file, &source_arc))?;
    Ok(schema)
}

/// Parse and validate, reporting **every** independent problem rather than
/// only the first.
///
/// [`parse_schema_named`] stops at the first error, which is right for a
/// compiler or a CLI: the build is failing either way, and one clear message
/// beats a cascade. It is wrong for an editor, where stopping early means the
/// author fixes one error, saves, and is handed the next — one round trip per
/// mistake.
///
/// Semantics worth knowing:
///
/// * A **syntax** error still yields exactly one diagnostic. Parsing has no
///   recovery, so there is no second error to report — everything after the
///   failure is unparsed, not valid.
/// * **Validation** errors are collected in stages, and a stage runs only when
///   every earlier stage was clean. Several validators document that they
///   assume an earlier one passed, and running them over already-rejected input
///   produces cascades pointing at the wrong places. Within a stage, every
///   declaration reports independently — three models each naming a type that
///   does not exist produce three diagnostics, not three round trips.
/// * The schema is returned only when there are no errors at all, matching
///   [`parse_schema_named`].
///
/// The first element of the returned `Vec` is always the same error
/// [`parse_schema_named`] would have returned; both go through one set of
/// checks in one order, so they cannot drift apart.
pub fn parse_schema_diagnostics(
    path: &str,
    source: &str,
) -> (Option<cratestack_core::Schema>, Vec<SchemaError>) {
    let file: Arc<str> = Arc::from(path);
    let source_arc: Arc<str> = Arc::from(source);
    let schema = match parse::parse_schema_only(source) {
        Ok(schema) => schema,
        Err(error) => return (None, vec![error.with_file(&file, &source_arc)]),
    };
    let errors: Vec<SchemaError> = validate::validate_schema_collecting(path, source, &schema)
        .into_iter()
        .map(|error| error.with_file(&file, &source_arc))
        .collect();
    if errors.is_empty() {
        (Some(schema), Vec::new())
    } else {
        (None, errors)
    }
}

/// Parse a `.cstack` source into a [`cratestack_core::Schema`] WITHOUT
/// running [`validate::validate_schema`].
///
/// Prefer [`parse_schema`] for any new source — this exists for two
/// legitimate cases where the validated pipeline understates what a
/// `Schema` value can actually be:
///
/// 1. A committed `migrations/*/schema.snapshot.json` can predate a
///    validation rule added later. `cratestack-cli`'s `migrate diff`
///    deserializes that "previous" snapshot directly and never re-runs
///    `validate_schema` on it (only the *new* side, parsed fresh from the
///    `.cstack` source, goes through [`parse_schema_file`]) — so an emitter
///    can still legitimately be handed a shape the current validator would
///    reject at the source level.
/// 2. Tests that deliberately exercise an emitter's rendering logic for
///    such an already-invalid shape, to prove the emitter itself still
///    behaves sanely if that shape arrives via (1) — see
///    `cratestack-migrate`'s `emit::postgres::tests::enums` for an example
///    (a list-valued enum column, rejected by cratestack#229/#236 at parse
///    time, but still real input to the Postgres emitter via a pre-#236
///    snapshot).
pub fn parse_schema_unvalidated(source: &str) -> Result<cratestack_core::Schema, SchemaError> {
    parse::parse_schema_only(source)
}

pub fn parse_schema_file(path: impl AsRef<Path>) -> Result<cratestack_core::Schema, SchemaError> {
    let path = path.as_ref();
    let display_path = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|error| {
        SchemaError::new(
            format!("failed to read schema file {display_path}: {error}"),
            0..0,
            1,
        )
        .with_file(&Arc::from(display_path.as_str()), &Arc::from(""))
    })?;
    parse_schema_named(&display_path, &source)
}
