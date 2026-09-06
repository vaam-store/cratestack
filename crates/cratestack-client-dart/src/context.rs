use cratestack_core::{Field, Schema};

use crate::builders::{build_data_class, build_enum_view};
use crate::builders_model::{
    build_model_accessor, build_model_api, build_procedure, build_selection_group,
    build_selection_model,
};
use crate::config::{DartGeneratorConfig, DartGeneratorError, DartPreset};
use crate::enum_filter_view::build_enum_filter_data_class;
use crate::find_many_order::{build_find_many_data_class, build_order_by_clause_data_class};
use crate::find_many_views::{build_sort_field_enum, build_where_data_class};
use crate::idents::{
    dart_identifier, escape_dart_string, pluralize, to_camel_case, to_pascal_case,
};
use crate::naming::{
    enum_name_set, is_computed_field, is_generated_on_create, is_primary_key, is_relation_field,
    model_name_set, occupied_type_names, procedure_wrapper_name, scalar_model_fields,
};
use crate::package_floors::{
    CRATESTACK_ANNOTATIONS_FLOOR, CRATESTACK_BUILDER_FLOOR, CRATESTACK_CBOR_FLOOR, requirement,
};
use crate::views::{ConstantView, DataClassKind, SampleModelView, TemplateContext};

pub(crate) fn build_template_context(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> Result<TemplateContext, DartGeneratorError> {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied_type_names = occupied_type_names(schema);
    let client_class_name = format!("{}CratestackClient", to_pascal_case(&config.library_name));
    let provider_prefix = to_camel_case(&config.library_name);
    // Issue #178: empty string means "no hash supplied" (library-direct
    // usage or tests bypassing the CLI) — mirrors the Rust client's
    // `Option<&'static str>` rather than ever sending an empty header.
    let schema_sha256 =
        (!config.schema_sha256.is_empty()).then(|| escape_dart_string(&config.schema_sha256));
    let mut enum_types: Vec<_> = schema.enums.iter().map(build_enum_view).collect();

    let mut data_classes = Vec::new();
    // One `{EnumName}Filter` class per schema enum (cratestack#928),
    // generated unconditionally alongside the enum itself — same
    // "generated regardless of use" convention `<Model>Where`/
    // `SortField`/`OrderByClause` below already follow.
    let all_enum_names: std::collections::BTreeSet<&str> =
        schema.enums.iter().map(|e| e.name.as_str()).collect();
    for enum_decl in &schema.enums {
        data_classes.push(build_enum_filter_data_class(
            enum_decl,
            &occupied_type_names,
            &all_enum_names,
        ));
    }
    for ty in &schema.types {
        let fields = ty.fields.iter().collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &ty.name,
            &fields,
            DataClassKind::Plain,
            &enum_names,
            &model_names,
        ));
    }

    for model in &schema.models {
        let model_fields = model.fields.iter().collect::<Vec<_>>();
        let scalar_fields = scalar_model_fields(model, &model_names);
        data_classes.push(build_data_class(
            &model.name,
            &model_fields,
            DataClassKind::ProjectionModel,
            &enum_names,
            &model_names,
        ));

        // cratestack#743: `Create<M>Input`/`Update<M>Input` are only
        // ever referenced from this crate's generated `create`/`update`
        // methods (`rest-apis.dart.j2`/`rpc-apis.dart.j2`/riverpod
        // equivalents), which `build_model_api`'s `allows_create`/
        // `allows_update` already omit when the verb is suppressed —
        // so emitting the input class anyway would be exactly the
        // "unreferenced Create<M>Input" the acceptance criteria forbid.
        // One shared source of truth, consulted once per class here.
        let internal = cratestack_core::model_internal_actions(model);

        let create_name = format!("Create{}Input", model.name);
        let create_fields = scalar_fields
            .iter()
            .copied()
            // `@computed` fields are resolver-backed and response-time
            // only — never part of a create input, since the server
            // struct never carries them either
            // (`docs/design/computed-fields.md`).
            .filter(|field| !is_computed_field(field))
            .filter(|field| !is_generated_on_create(field))
            .collect::<Vec<_>>();
        if !internal.contains("create") {
            data_classes.push(build_data_class(
                &create_name,
                &create_fields,
                DataClassKind::Plain,
                &enum_names,
                &model_names,
            ));
        }

        let update_name = format!("Update{}Input", model.name);
        let update_fields = scalar_fields
            .iter()
            .copied()
            .filter(|field| !is_primary_key(field))
            // `@computed` fields are never part of an update input either
            // — same reasoning as `create_fields` above.
            .filter(|field| !is_computed_field(field))
            .collect::<Vec<_>>();
        if !internal.contains("update") {
            data_classes.push(build_data_class(
                &update_name,
                &update_fields,
                DataClassKind::Patch,
                &enum_names,
                &model_names,
            ));
        }

        // `<Model>Where`/`<Model>SortField`/`<Model>OrderByClause`/
        // `<Model>FindMany` — generated for every model unconditionally,
        // same as `Create`/`Update<Model>Input` above, regardless of
        // whether a procedure actually declares `FindMany<Model>`.
        let where_class =
            build_where_data_class(model, &model_names, &enum_names, &occupied_type_names);
        if let Some(where_class) = where_class.clone() {
            data_classes.push(where_class);
        }
        enum_types.push(build_sort_field_enum(model, &model_names));
        data_classes.push(build_order_by_clause_data_class(model));
        data_classes.push(build_find_many_data_class(model, where_class.is_some()));
    }

    for procedure in &schema.procedures {
        let args_name = procedure_wrapper_name(procedure, &occupied_type_names);
        let fields = procedure
            .args
            .iter()
            .map(|arg| Field {
                docs: arg.docs.clone(),
                name: arg.name.clone(),
                name_span: arg.name_span,
                ty: arg.ty.clone(),
                attributes: Vec::new(),
                span: procedure.span,
            })
            .collect::<Vec<_>>();
        let field_refs = fields.iter().collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &args_name,
            &field_refs,
            DataClassKind::Plain,
            &enum_names,
            &model_names,
        ));
    }

    let selection_groups = schema
        .models
        .iter()
        .map(|model| build_selection_group(model, &model_names))
        .collect();
    let selection_models = schema
        .models
        .iter()
        .map(|model| build_selection_model(model, &schema.models, &model_names, &enum_names))
        .collect();

    let model_accessors = schema
        .models
        .iter()
        .map(|model| build_model_accessor(model, &provider_prefix))
        .collect();

    let model_apis: Vec<_> = schema.models.iter().map(build_model_api).collect();
    let has_computed_params_class = model_apis
        .iter()
        .any(|model_api| model_api.computed_params_class_name.is_some());
    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect();
    let query_procedures = schema
        .procedures
        .iter()
        .filter(|procedure| procedure.kind == cratestack_core::ProcedureKind::Query)
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect();
    let mutation_procedures = schema
        .procedures
        .iter()
        .filter(|procedure| procedure.kind == cratestack_core::ProcedureKind::Mutation)
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect();
    let sample_model = schema.models.first().map(|model| {
        let accessor = pluralize(&to_camel_case(&model.name));
        let field_group_name = format!("{}FieldNames", model.name);
        let include_group_name = format!("{}IncludeNames", model.name);
        let scalar_fields = scalar_model_fields(model, &model_names);
        let first_field = scalar_fields.first().map(|field| ConstantView {
            const_name: dart_identifier(&to_camel_case(&field.name)),
            value: field.name.clone(),
        });
        let relation_fields = model
            .fields
            .iter()
            .filter(|field| is_relation_field(&model_names, field))
            .collect::<Vec<_>>();
        let first_include = relation_fields.first().map(|field| ConstantView {
            const_name: dart_identifier(&to_camel_case(&field.name)),
            value: field.name.clone(),
        });

        SampleModelView {
            model_name: model.name.clone(),
            accessor,
            field_group_name,
            include_group_name,
            first_field,
            first_include,
        }
    });

    // Issue #563: still only computed when the flag is actually set — a
    // `--no-native-cbor` package has no `cratestack_cbor` line to render
    // at all. cratestack#779: but the value is now an API-compatibility
    // floor rather than `^{CARGO_PKG_VERSION}`, so it no longer moves
    // with `just bump` — see `crate::package_floors`.
    let cratestack_cbor_version_requirement = if config.native_cbor {
        requirement(CRATESTACK_CBOR_FLOOR)
    } else {
        String::new()
    };

    // Issue #668 phase 2: unlike `cratestack_cbor_version_requirement`
    // above, never gated — every generated package depends on both
    // unconditionally. cratestack#754: and unlike it, these are API
    // compatibility *floors*, not `^{CARGO_PKG_VERSION}` — see
    // `crate::package_floors` for why deriving them from the release
    // version is what broke `Prepare Release` for 0.8.14.
    let cratestack_annotations_version_requirement = requirement(CRATESTACK_ANNOTATIONS_FLOOR);
    let cratestack_builder_version_requirement = requirement(CRATESTACK_BUILDER_FLOOR);

    Ok(TemplateContext {
        package_name: config.library_name.clone(),
        client_class_name,
        provider_prefix,
        base_path_literal: escape_dart_string(&config.base_path),
        schema_sha256,
        enum_types,
        data_classes,
        selection_groups,
        selection_models,
        model_accessors,
        model_apis,
        procedures,
        query_procedures,
        mutation_procedures,
        sample_model,
        is_riverpod_preset: config.preset == DartPreset::Riverpod,
        native_cbor: config.native_cbor,
        cratestack_cbor_version_requirement,
        has_computed_params_class,
        cratestack_annotations_version_requirement,
        cratestack_builder_version_requirement,
    })
}
