//! Builds the `swr` preset's template contexts (issue #304) from a
//! `Schema` + the ownership computation (`crate::swr::ownership`). The
//! lower-level import/ownership-splitting helpers live in
//! `context_imports.rs`, and the model-summary builder lives in
//! `model_summary.rs` — both split out to keep this file under this
//! repo's ~200-LoC convention.

use cratestack_core::Schema;

use crate::config::TypeScriptGeneratorConfig;
use crate::find_many_views::{
    build_find_many_interface, build_order_by_clause_interface, build_sort_field_view,
    build_where_interface,
};
use crate::naming::{model_fn_names, procedure_wrapper_name, to_kebab_case};
use crate::procedure_views::build_procedure;
use crate::types::{
    enum_name_set, is_computed_field, is_generated_on_create, is_paged_model, is_primary_key,
    model_allows_create, model_name_set, scalar_model_fields, visible_model_fields,
};
use crate::views::{
    InterfaceKind, build_computed_params_interface, build_interface, build_model_api,
};
use crate::wire_shapes::build_wire_shapes;

use super::context_imports::{
    build_imports, model_refs_in_fields, owned_by, procedure_arg_fields, procedure_model_refs,
    type_decls_model_refs,
};
use super::hook_naming::model_hook_names;
use super::model_summary::build_model_summary;
use super::ownership::{TypeOwner, TypeOwnership};
use super::views::{SwrModelFileContext, SwrProceduresView, SwrSchemaContext};

pub(crate) fn build_shared_context(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
    ownership: &TypeOwnership,
) -> SwrSchemaContext {
    let enum_names = enum_name_set(&schema.enums);
    let model_names = model_name_set(&schema.models);

    let (shared_enums, shared_interfaces) = owned_by(schema, ownership, &enum_names, |owner| {
        matches!(owner, TypeOwner::Shared)
    });
    let shared_model_refs = type_decls_model_refs(schema, ownership, &model_names, |owner| {
        matches!(owner, TypeOwner::Shared)
    });
    let shared = super::views::SwrSharedView {
        enums: shared_enums,
        interfaces: shared_interfaces,
        imports: build_imports(Vec::new(), shared_model_refs, None, "", "./"),
    };

    let models = schema
        .models
        .iter()
        .map(build_model_summary)
        .collect::<Vec<_>>();

    let (procedures_owned_enums, procedures_owned_interfaces) =
        owned_by(schema, ownership, &enum_names, |owner| {
            matches!(owner, TypeOwner::Procedures)
        });
    let mut procedures_shared_names = ownership.shared_imports_for_procedures();
    // `ts_type` inlines a `Page<T>` return type as a literal in the
    // generated function/hook signatures rather than importing it as a
    // named model type (see `crate::types::ts_type`'s `is_page` branch),
    // so the ownership graph never sees it as a consumer edge — add it
    // by hand whenever any procedure's return type actually needs it.
    if schema
        .procedures
        .iter()
        .any(|procedure| procedure.return_type.is_page())
    {
        procedures_shared_names.push("Page".to_owned());
    }
    // Same reasoning, for `PageInput` argument fields: `ts_type` inlines
    // the bare name as a literal rather than the ownership graph seeing a
    // consumer edge (`PageInput` isn't a declared `type`), so a procedure
    // arg wrapper interface referencing it needs the import added by hand
    // too.
    if schema
        .procedures
        .iter()
        .any(|procedure| procedure.args.iter().any(|arg| arg.ty.is_page_input()))
    {
        procedures_shared_names.push("PageInput".to_owned());
    }
    // `FindMany<Model>` argument fields are different again: unlike
    // `Page`/`PageInput`, `ts_type` resolves them to a *per-model*
    // derived name (`PostFindMany`, defined in that model's own file —
    // see `find_many_views.rs`), not a shared literal. `model_refs`
    // below only ever imports a model's own declared interface name
    // (`Post`), never a derived one, so these need their own import
    // entries — built after `procedures_imports` further down.
    let find_many_model_names = schema
        .procedures
        .iter()
        .flat_map(|procedure| procedure.args.iter())
        .filter_map(|arg| arg.ty.find_many_item())
        .map(|item| item.name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut procedures_model_refs = procedure_model_refs(schema, &model_names);
    procedures_model_refs.extend(type_decls_model_refs(
        schema,
        ownership,
        &model_names,
        |owner| matches!(owner, TypeOwner::Procedures),
    ));
    let occupied = crate::naming::occupied_type_names(schema);
    let args_interfaces = schema
        .procedures
        .iter()
        .map(|procedure| {
            let fields = procedure_arg_fields(procedure);
            build_interface(
                &procedure_wrapper_name(procedure, &occupied),
                &fields.iter().collect::<Vec<_>>(),
                InterfaceKind::Plain,
                &enum_names,
            )
        })
        .collect();
    let mut procedures_imports = build_imports(
        procedures_shared_names,
        procedures_model_refs,
        None,
        "./models/shared",
        "./models/",
    );
    for model_name in &find_many_model_names {
        procedures_imports.push(super::views::SwrImport::new(
            format!("./models/{}.js", to_kebab_case(model_name)),
            vec![format!("{model_name}FindMany")],
        ));
    }
    let procedures_file = SwrProceduresView {
        owned_enums: procedures_owned_enums,
        owned_interfaces: procedures_owned_interfaces,
        imports: procedures_imports,
        args_interfaces,
        procedures: schema
            .procedures
            .iter()
            .map(|procedure| build_procedure(procedure, &occupied, &enum_names))
            .collect(),
    };

    SwrSchemaContext {
        package_name: config.package_name.clone(),
        base_path: config.base_path.clone(),
        schema_sha256: config.schema_sha256.clone(),
        shared,
        models,
        procedures_file,
        wire_shapes: build_wire_shapes(schema),
        models_import_path: "../models.js",
        native_cbor: config.native_cbor,
    }
}

pub(crate) fn build_model_file_contexts(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
    ownership: &TypeOwnership,
) -> Vec<SwrModelFileContext> {
    let enum_names = enum_name_set(&schema.enums);
    let model_names = model_name_set(&schema.models);
    let model_interface_kind = if config.full_selection {
        InterfaceKind::Plain
    } else {
        InterfaceKind::Model
    };

    schema
        .models
        .iter()
        .map(|model| {
            let scalar_fields = scalar_model_fields(model, &model_names);
            let model_interface = build_interface(
                &model.name,
                &visible_model_fields(model),
                model_interface_kind,
                &enum_names,
            );
            // cratestack#743: same reasoning as `context.rs`'s (non-swr)
            // interface gating — a suppressed `create`/`update` leaves
            // this model's own `create`/`update` fn+hook
            // (`crate::swr::hooks`, gated on
            // `ModelApiView::allows_create`/`allows_update`) omitted, so
            // the input interface would otherwise be unreferenced.
            let internal = cratestack_core::model_internal_actions(model);
            let create_input =
                (model_allows_create(model) && !internal.contains("create")).then(|| {
                    build_interface(
                        &format!("Create{}Input", model.name),
                        &scalar_fields
                            .iter()
                            .copied()
                            // `@computed` fields are resolver-backed and
                            // response-time only — never part of a create
                            // input (`docs/design/computed-fields.md`).
                            .filter(|field| !is_computed_field(field))
                            .filter(|field| !is_generated_on_create(field))
                            .collect::<Vec<_>>(),
                        InterfaceKind::Plain,
                        &enum_names,
                    )
                });
            let update_input = (!internal.contains("update")).then(|| {
                build_interface(
                    &format!("Update{}Input", model.name),
                    &scalar_fields
                        .iter()
                        .copied()
                        .filter(|field| !is_primary_key(field))
                        // `@computed` fields are never part of an update
                        // input either — same reasoning as the create input
                        // above.
                        .filter(|field| !is_computed_field(field))
                        .collect::<Vec<_>>(),
                    InterfaceKind::Patch,
                    &enum_names,
                )
            });

            let (mut owned_enums, mut owned_interfaces) = owned_by(
                schema,
                ownership,
                &enum_names,
                |owner| matches!(owner, TypeOwner::Model(name) if name == &model.name),
            );

            // `<Model>Where`/`<Model>SortField`/`<Model>OrderByClause`/
            // `<Model>FindMany` are always single-model-owned by
            // construction (never cross-model), so they're inlined here
            // the same way `owned_enums`/`owned_interfaces` already are
            // — never imported.
            let where_interface = build_where_interface(model, &model_names, &enum_names);
            if let Some(where_interface) = where_interface.clone() {
                owned_interfaces.push(where_interface);
            }
            owned_enums.push(build_sort_field_view(model, &model_names));
            owned_interfaces.push(build_order_by_clause_interface(model));
            owned_interfaces.push(build_find_many_interface(model, where_interface.is_some()));
            // `<Model>ComputedParams` (`docs/design/computed-fields.md`) is
            // single-model-owned by construction, same as
            // `<Model>Where`/`<Model>FindMany` above — inlined here, never
            // imported. Its own params-type reference (e.g. `ProxyParams`)
            // is still resolved through the normal owned/shared import
            // machinery: `super::ownership_graph::model_referenced_eligible_names`
            // treats a `@computed(params: <Type>?)` field as reaching that
            // type, so it lands in `owned_interfaces`/`imports` above
            // exactly like any other type this model's fields reference.
            if let Some(computed_params_interface) = build_computed_params_interface(model) {
                owned_interfaces.push(computed_params_interface);
            }

            let is_paged = is_paged_model(model);
            let mut shared_names = ownership.shared_imports_for_model(&model.name);
            // Same reasoning as `procedures_shared_names` above: a paged
            // model's `list_return_type` is `Page<{Model}>` inlined as a
            // literal (`crate::views::build_model_api`), which the
            // ownership graph never sees as a consumer edge.
            if is_paged {
                shared_names.push("Page".to_owned());
            }
            // `<Model>OrderByClause` always references the shared
            // `SortDirection`; `<Model>Where`'s fields reference whichever
            // shared filter interfaces its own field types need — over-
            // importing the full set when there's at least one filterable
            // field is harmless (`import type`, never flagged as unused
            // by a default `tsconfig.json`) and far simpler than tracking
            // exactly which of the 6 filter interfaces this specific
            // model's fields touch.
            shared_names.push("SortDirection".to_owned());
            if where_interface.is_some() {
                shared_names.extend([
                    "EqualityFilter".to_owned(),
                    "ComparableFilter".to_owned(),
                    "StringFilter".to_owned(),
                    "NumberFilter".to_owned(),
                    "BooleanFilter".to_owned(),
                    "UuidFilter".to_owned(),
                    "DateTimeFilter".to_owned(),
                    "DecimalFilter".to_owned(),
                ]);
            }
            let mut model_refs =
                model_refs_in_fields(visible_model_fields(model).into_iter(), &model_names);
            model_refs.extend(type_decls_model_refs(
                schema,
                ownership,
                &model_names,
                |owner| matches!(owner, TypeOwner::Model(name) if name == &model.name),
            ));
            // A relation never needs to import its own model — that type
            // is defined right below in this same file.
            model_refs.remove(&model.name);
            let imports = build_imports(
                shared_names,
                model_refs,
                Some(model.name.as_str()),
                "./shared",
                "./",
            );

            let fns = model_fn_names(&model.name);
            let hooks = model_hook_names(&model.name);
            SwrModelFileContext {
                file_stem: to_kebab_case(&model.name),
                model: build_model_api(model),
                model_interface,
                create_input,
                update_input,
                owned_enums,
                owned_interfaces,
                imports,
                is_paged,
                list_fn: fns.list,
                get_with_response_fn: format!("{}WithResponse", fns.get),
                get_fn: fns.get,
                create_fn: fns.create,
                update_fn: fns.update,
                delete_fn: fns.delete,
                list_hook: hooks.list,
                get_hook: hooks.get,
                create_hook: hooks.create,
                update_hook: hooks.update,
                delete_hook: hooks.delete,
            }
        })
        .collect()
}
