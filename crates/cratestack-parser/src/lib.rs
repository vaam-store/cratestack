mod diagnostics;
mod line_helpers;
mod parse;
mod relation_actions;
mod relation_helpers;
mod validate;

#[cfg(test)]
mod tests_attribute_spacing;
#[cfg(test)]
mod tests_basic;
mod tests_builder_add_setter_collisions;
#[cfg(test)]
mod tests_builder_collisions;
mod tests_builder_collisions_derived;
#[cfg(test)]
mod tests_client_method_collisions;
#[cfg(test)]
mod tests_computed;
#[cfg(test)]
mod tests_computed_params;
mod tests_computed_type_valued;
#[cfg(test)]
mod tests_docs;
#[cfg(test)]
mod tests_enums;
#[cfg(test)]
mod tests_extensions;
#[cfg(test)]
mod tests_field_attrs;
#[cfg(test)]
mod tests_list_arity;
#[cfg(test)]
mod tests_mixins;
#[cfg(test)]
mod tests_model_attrs;
#[cfg(test)]
mod tests_model_index;
#[cfg(test)]
mod tests_model_internal;
#[cfg(test)]
mod tests_model_unique;
#[cfg(test)]
mod tests_multi_error;
mod tests_patch_touch_flag_collisions;
#[cfg(test)]
mod tests_procedure_handler_collisions;
#[cfg(test)]
mod tests_procedures;
#[cfg(test)]
mod tests_queries;
#[cfg(test)]
mod tests_queries_attributes;
#[cfg(test)]
mod tests_queries_rejections;
#[cfg(test)]
mod tests_queries_sql_body;
#[cfg(test)]
mod tests_queries_support;
#[cfg(test)]
mod tests_relation_actions;
#[cfg(test)]
mod tests_relations;
#[cfg(test)]
mod tests_relations_policy;
#[cfg(test)]
mod tests_reserved_keywords;
#[cfg(test)]
mod tests_snake_case_collisions;
#[cfg(test)]
mod tests_spatial;
#[cfg(test)]
mod tests_stream_attribute;
#[cfg(test)]
mod tests_transport;
#[cfg(test)]
mod tests_type_declaration_collisions;
#[cfg(test)]
mod tests_types;
#[cfg(test)]
mod tests_validators;
#[cfg(test)]
mod tests_vector;
#[cfg(test)]
mod tests_version;
#[cfg(test)]
mod tests_views;

use std::path::Path;

pub use diagnostics::SchemaError;

/// Canonical scalar type names built into the `.cstack` language (e.g.
/// `String`, `Int`, `Decimal`, ...), including `Page` (which is valid only
/// as a procedure return type — see `validate::type_names::validate_type_ref`
/// — not as a plain field type).
///
/// This is the same list `cratestack-lsp`'s autocompletion and the
/// `cratestack-pg`/`cratestack-sqlite` emitter/decoder round-trip tests
/// assert against, so a new builtin scalar only has to be added here once —
/// see cratestack#232 for why that matters (this list had already silently
/// drifted from the LSP's hand-copied one before this accessor existed).
pub fn builtin_type_names() -> &'static [&'static str] {
    validate::builtin_type_names()
}

#[cfg(test)]
use relation_helpers::parse_relation_attribute;

pub fn parse_schema(source: &str) -> Result<cratestack_core::Schema, SchemaError> {
    parse_schema_named("<schema>", source)
}

pub fn parse_schema_named(
    path: &str,
    source: &str,
) -> Result<cratestack_core::Schema, SchemaError> {
    let schema = parse::parse_schema_only(source)?;
    validate::validate_schema(path, source, &schema)?;
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
    let schema = match parse::parse_schema_only(source) {
        Ok(schema) => schema,
        Err(error) => return (None, vec![error]),
    };
    let errors = validate::validate_schema_collecting(path, source, &schema);
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
    let source = std::fs::read_to_string(path).map_err(|error| {
        SchemaError::new(
            format!("failed to read schema file {}: {error}", path.display()),
            0..0,
            1,
        )
    })?;
    parse_schema_named(&path.display().to_string(), &source)
}
