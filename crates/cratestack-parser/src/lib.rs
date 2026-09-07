mod diagnostics;
mod entry;
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
mod tests_error_file_identity;
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
mod tests_reserved_keywords_multi_file;
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

pub use diagnostics::SchemaError;
pub use entry::{
    ANONYMOUS_SCHEMA, parse_schema, parse_schema_diagnostics, parse_schema_file,
    parse_schema_named, parse_schema_unvalidated,
};

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
