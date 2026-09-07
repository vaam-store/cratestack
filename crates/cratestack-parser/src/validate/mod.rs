mod builder_collisions;
mod builder_setter_collisions;
mod client_method_collisions;
mod collect;
mod composite_attributes;
mod computed;
mod computed_attribute;
mod computed_params;
mod computed_resolver_names;
mod fields;
mod index_attribute;
mod misspelled_attributes;
mod mixins_types;
mod model_attributes;
mod model_relation;
mod models;
mod no_idempotency;
mod patch_touch_flag_collisions;
mod procedure_handler_collisions;
mod procedure_idents;
mod procedures;
mod queries;
mod query_attributes;
mod query_placeholders;
mod query_signature;
mod removed_attributes;
mod reserved_idents;
mod route_collisions;
mod snake_case_collisions;
mod spatial_type;
mod stream_attribute;
mod type_names;
mod validator_args;
mod validators;
mod views;

use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::diagnostics::{SchemaError, span_error};

use self::builder_collisions::validate_builder_name_collisions;
use self::client_method_collisions::validate_client_method_collisions;
use self::mixins_types::{
    validate_auth, validate_enums_collecting, validate_mixins_collecting, validate_types_collecting,
};
use self::models::validate_models_collecting;
use self::no_idempotency::validate_procedure_no_idempotency_attribute;
use self::procedure_handler_collisions::validate_procedure_model_handler_collisions;
use self::procedure_idents::validate_procedure_idents;
use self::procedures::{
    validate_procedure_api_version_attribute, validate_procedure_deprecated_attribute,
    validate_procedure_isolation_attribute, validate_procedure_no_rate_limit_attribute,
    validate_procedure_status_attribute,
};
use self::snake_case_collisions::validate_type_declaration_collisions;
use self::stream_attribute::validate_procedure_stream_attribute;
use self::type_names::{collect_type_names, validate_type_ref};

/// The canonical scalar type names built into the `.cstack` language today.
///
/// Exposed (via [`crate::builtin_type_names`]) so downstream test suites —
/// notably the emitter/decoder round-trip coverage in `cratestack-pg` and
/// `cratestack-sqlite` (see cratestack#232) — can assert against the same
/// authoritative list the parser validates field types against, instead of
/// hand-maintaining a second copy that can silently drift the way
/// `cratestack-lsp`'s completion list once did.
pub(crate) fn builtin_type_names() -> &'static [&'static str] {
    type_names::BUILTIN_TYPES
}

pub(crate) fn multi_file_schema_keywords() -> &'static [&'static str] {
    reserved_idents::multi_file_schema_keywords()
}

pub(crate) fn validate_schema(
    path: &str,
    source: &str,
    schema: &Schema,
) -> Result<(), SchemaError> {
    collect::first(validate_schema_collecting(path, source, schema))
}

/// Every independent problem in `schema`, in source-validation order.
///
/// Runs in **stages**, and a stage only runs when every earlier stage was
/// clean. That is not caution for its own sake — several validators document
/// that they assume an earlier one already passed (`validate_computed` "may
/// assume every `@computed` attribute is already known to be bare, unique, and
/// on a declaration kind that supports it"), and `collect_type_names` produces
/// the name set the per-declaration stage needs at all. Running a later stage
/// over input an earlier stage already rejected produces cascades of nonsense
/// errors pointing at the wrong places, which is worse than reporting one real
/// one.
///
/// Within a stage, declarations are independent and all of their errors are
/// collected — which is the case that matters while editing, where three models
/// each naming a type that does not exist should not take three round trips.
pub(crate) fn validate_schema_collecting(
    path: &str,
    source: &str,
    schema: &Schema,
) -> Vec<SchemaError> {
    // Stage 1 — the name set everything downstream is checked against. There
    // is nothing to fall back to if this fails, so it is the one hard stop.
    let type_names = match collect_type_names(schema) {
        Ok(names) => names,
        Err(error) => return vec![error],
    };

    // Stage 2 — whole-schema shape: collisions, duplicates, datasource.
    let mut errors = Vec::new();
    collect::record(&mut errors, || validate_type_declaration_collisions(schema));
    collect::record(&mut errors, || validate_builder_name_collisions(schema));
    collect::record(&mut errors, || {
        let mut procedure_names = BTreeSet::new();
        for procedure in &schema.procedures {
            if !procedure_names.insert(procedure.name.clone()) {
                return Err(span_error(
                    format!("duplicate procedure name `{}`", procedure.name),
                    procedure.span,
                ));
            }
        }
        Ok(())
    });
    collect::record(&mut errors, || {
        validate_procedure_model_handler_collisions(schema)
    });
    collect::record(&mut errors, || validate_client_method_collisions(schema));
    collect::record(&mut errors, || validate_datasource(schema));
    collect::record(&mut errors, || {
        validate_no_models_under_datasource_none(schema)
    });
    if !errors.is_empty() {
        return errors;
    }

    let page_item_type_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
        .collect::<BTreeSet<_>>();
    // `FindMany<T>` (unlike `Page<T>`) only ever wraps a model: filtering
    // needs a real table's columns/`allowed_fields()` to validate field
    // names against, which a `type` block has none of.
    let model_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .collect::<BTreeSet<_>>();

    // Stage 3 — per-declaration. These are independent of one another, so every
    // declaration reports.
    validate_models_collecting(
        schema,
        &type_names,
        &page_item_type_names,
        &model_names,
        &mut errors,
    );
    validate_mixins_collecting(
        schema,
        &type_names,
        &page_item_type_names,
        &model_names,
        &mut errors,
    );
    validate_types_collecting(
        schema,
        &type_names,
        &page_item_type_names,
        &model_names,
        &mut errors,
    );
    validate_enums_collecting(schema, &mut errors);
    collect::record(&mut errors, || {
        validate_auth(schema, &type_names, &page_item_type_names, &model_names)
    });
    collect::record(&mut errors, || {
        validate_procedures(schema, &type_names, &page_item_type_names, &model_names)
    });
    self::views::validate_views_collecting(schema, &mut errors);
    self::queries::validate_queries_collecting(schema, &type_names, &mut errors);
    if !errors.is_empty() {
        return errors;
    }

    // Stage 4 — assumes every declaration above is already known good.
    collect::record(&mut errors, || self::computed::validate_computed(schema));
    collect::record(&mut errors, || {
        self::computed_resolver_names::validate_computed_resolver_name_collisions(schema)
    });

    let _ = (path, source);
    errors
}

fn validate_datasource(schema: &Schema) -> Result<(), SchemaError> {
    if let Some(datasource) = &schema.datasource {
        reserved_idents::validate_reserved_identifier(
            &datasource.name,
            datasource.span,
            &format!("datasource `{}`", datasource.name),
        )?;
        let provider = datasource_provider(schema);

        if let Some(provider) = provider
            && provider != "postgresql"
            && provider != "sqlite"
            && provider != "none"
        {
            return Err(span_error(
                format!(
                    "unsupported datasource provider `{provider}`; expected `postgresql`, `sqlite`, or `none`"
                ),
                datasource.span,
            ));
        }
    }
    Ok(())
}

/// The `provider` config entry off `schema.datasource`, with surrounding
/// quotes stripped — `None` when there's no `datasource` block at all, or
/// the block has no `provider` entry. Shared by [`validate_datasource`] and
/// [`validate_no_models_under_datasource_none`] so both read the exact same
/// value.
fn datasource_provider(schema: &Schema) -> Option<&str> {
    schema
        .datasource
        .as_ref()?
        .entries
        .iter()
        .find(|entry| entry.key == "provider")
        .map(|entry| entry.value.trim_matches('"'))
}

/// `datasource { provider = "none" }` declares a no-database, procedures-only
/// schema (cratestack#327): the whole point is that no table-backed `model`
/// exists to accidentally query against a database that was never
/// configured. Zero-model schemas are already valid today (procedures-only
/// or even zero-procedure — see `examples/rpc-procedures/schema.cstack`), so
/// this only ever rejects, never requires, a model list.
fn validate_no_models_under_datasource_none(schema: &Schema) -> Result<(), SchemaError> {
    if datasource_provider(schema) != Some("none") {
        return Ok(());
    }
    if let Some(model) = schema.models.first() {
        return Err(span_error(
            format!(
                "model `{}` is not allowed: schema declares `datasource {{ provider = \"none\" }}`, \
                 which forbids any `model` block (this schema is procedures-only, no database is \
                 configured)",
                model.name
            ),
            model.span,
        ));
    }
    Ok(())
}

fn validate_procedures(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    for procedure in &schema.procedures {
        validate_procedure_idents(procedure)?;
        for arg in &procedure.args {
            validate_type_ref(
                type_names,
                page_item_type_names,
                model_names,
                &schema.declared_extensions,
                &arg.ty,
                procedure.span,
                self::type_names::TypeRefAllow {
                    page_input: true,
                    find_many: true,
                    ..Default::default()
                },
            )?;
        }
        validate_type_ref(
            type_names,
            page_item_type_names,
            model_names,
            &schema.declared_extensions,
            &procedure.return_type,
            procedure.span,
            self::type_names::TypeRefAllow {
                page: true,
                ..Default::default()
            },
        )?;
        validate_procedure_isolation_attribute(procedure)?;
        validate_procedure_api_version_attribute(procedure)?;
        validate_procedure_deprecated_attribute(procedure)?;
        validate_procedure_stream_attribute(procedure)?;
        validate_procedure_no_rate_limit_attribute(procedure, schema)?;
        validate_procedure_no_idempotency_attribute(procedure)?;
        validate_procedure_status_attribute(procedure, schema)?;
    }
    Ok(())
}
