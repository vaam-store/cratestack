use cratestack_core::{Field, Schema};
use serde::Serialize;

use crate::config::TypeScriptGeneratorConfig;
use crate::error::TypeScriptGeneratorError;
use crate::find_many_views::{
    build_find_many_interface, build_order_by_clause_interface, build_sort_field_view,
    build_where_interface,
};
use crate::naming::{occupied_type_names, package_class_stem, to_pascal_case};
use crate::package_deps::{
    DependencyEntry, dependencies_for, dev_dependencies_for, peer_dependencies_for,
};
use crate::package_floors::{
    CRATESTACK_ADAPTER_RTK_FLOOR, CRATESTACK_CBOR_FLOOR, CRATESTACK_REFINE_FLOOR, requirement,
};
use crate::procedure_views::{ProcedureView, build_procedure};
use crate::refine::{RefineResourceView, build_refine_resources, refine_resource_map_type};
use crate::rtk::collisions::reject_rtk_endpoint_name_collisions;
use crate::tanstack_collisions::reject_tanstack_hook_name_collisions;
use crate::types::{
    enum_name_set, is_computed_field, is_generated_on_create, is_primary_key, model_allows_create,
    model_name_set, scalar_model_fields, version_field, visible_model_fields,
};
use crate::views::{
    EnumView, InterfaceKind, InterfaceView, ModelApiView, build_computed_params_interface,
    build_enum_view, build_interface, build_model_api, disambiguate_model_api_keys,
};
use crate::wire_shapes::{WireShapeView, build_wire_shapes};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TemplateContext {
    package_name: String,
    client_class_name: String,
    base_path: String,
    /// Issue #178, REST/RPC only — see `TypeScriptGeneratorConfig::schema_sha256`'s
    /// doc comment for the scope decision. Baked into `runtime.ts` as
    /// `SCHEMA_SHA256`; empty when the CLI wasn't given a schema fingerprint.
    schema_sha256: String,
    enums: Vec<EnumView>,
    interfaces: Vec<InterfaceView>,
    models: Vec<ModelApiView>,
    procedures: Vec<ProcedureView>,
    query_procedures: Vec<ProcedureView>,
    mutation_procedures: Vec<ProcedureView>,
    /// One row per model/`type` in the schema — the generated
    /// `wireShapes` registry `models.ts.j2` renders and every decode
    /// call site looks a name up in (`crate::decimal`'s module doc has the
    /// full rationale; cratestack#499 review remediation).
    wire_shapes: Vec<WireShapeView>,
    /// Issue #571 (`--refine`). Mirrors
    /// `TypeScriptGeneratorConfig::refine`, and is read by the two
    /// *unconditional* templates that also change under the flag —
    /// `package.json.j2` (adds the `@cratestack/refine` peer/dev
    /// dependency) and `rest-index.ts.j2` (re-exports `./refine.js`).
    /// `src/refine.ts` itself is gated by spec selection, not by this
    /// field, so it is simply absent from a default run.
    refine: bool,
    /// The semver range the generated `package.json` declares for
    /// `@cratestack/refine` under `--refine`.
    ///
    /// cratestack#779: this is `crate::package_floors::
    /// CRATESTACK_REFINE_FLOOR`, an API-compatibility constant, on the
    /// same terms as `native_cbor_version_requirement` below. It used to
    /// be derived from this crate's own `CARGO_PKG_VERSION`, justified by
    /// `just bump` moving that in lockstep with the npm package — but
    /// lockstep is exactly the problem: the bump lands *before* the tag
    /// that publishes, so the generated client named a version the
    /// registry could not serve for the whole release window.
    ///
    /// Empty when `refine` is off, where no template reads it.
    refine_version_requirement: String,
    /// One entry per model, empty unless `refine` is set. See
    /// `crate::refine`.
    refine_resources: Vec<RefineResourceView>,
    /// The `@cratestack/refine` type `refine.ts.j2`'s
    /// `cratestackRefineResources()` is typed to return — see
    /// `crate::refine::refine_resource_map_type`. Empty when `refine` is
    /// off, where `refine.ts` isn't rendered at all.
    refine_resource_map_type: String,
    /// Issue #591 (`--swr`). Mirrors `TypeScriptGeneratorConfig::swr`, and
    /// is read by `package.json.j2` — the only unconditional template that
    /// changes under the flag, gaining the `"./swr"` `exports` subpaths
    /// and the `swr`/`react` peer/dev dependencies. `src/swr/**` itself is
    /// a wholly separate file set (`crate::swr::generate`), not gated by
    /// this field.
    swr: bool,
    /// Issue #617 (`--tanstack`). Mirrors `TypeScriptGeneratorConfig::tanstack`,
    /// and is read by the three `*-index.ts.j2` templates (gates the
    /// `./react-query.js` re-export) — `package.json.j2` reads
    /// `peer_dependencies`/`dev_dependencies` instead (see those fields'
    /// doc comments for why). `src/react-query.ts` itself is gated by spec
    /// selection (`crate::templates::specs`), not by this field.
    tanstack: bool,
    /// Issue #906 (`--rtk`). Mirrors `TypeScriptGeneratorConfig::rtk`, and
    /// is read by the two `*-index.ts.j2` templates (gates the
    /// `./rtk-api.js` re-export) — `package.json.j2` reads
    /// `peer_dependencies`/`dev_dependencies` instead. `src/rtk-api.ts`
    /// itself is gated by spec selection (`crate::templates::specs`), not
    /// by this field.
    rtk: bool,
    /// The semver range the generated `package.json` declares for
    /// `@cratestack/adapter-rtk` under `--rtk` on an RPC-transport schema.
    /// Empty when `rtk` is off or the schema is REST transport — see
    /// `crate::rtk`'s module doc for why REST never depends on the
    /// adapter package at all.
    rtk_adapter_version_requirement: String,
    /// `package.json.j2`'s `peerDependencies` entries, joined by a
    /// `{% for %}` loop in the template rather than nested `{% if %}`
    /// blocks — see `crate::package_deps`'s module doc for why issue #617
    /// forced that change. Empty when `refine`/`swr`/`tanstack` are all
    /// off, which renders a valid empty `"peerDependencies": {}`.
    peer_dependencies: Vec<DependencyEntry>,
    /// Same shape and rationale as `peer_dependencies`, for
    /// `devDependencies` — see `crate::package_deps::dev_dependencies_for`.
    dev_dependencies: Vec<DependencyEntry>,
    /// `package.json.j2`'s `dependencies` entries — see
    /// `crate::package_deps::dependencies_for`. Always carries
    /// `decimal.js`; additionally carries `@cratestack/cbor` when
    /// `native_cbor` is set on an RPC-transport schema.
    dependencies: Vec<DependencyEntry>,
    /// Issue #746 (on by default; `--no-native-cbor` opts out — see
    /// `TypeScriptGeneratorConfig::native_cbor`'s doc comment). Read
    /// directly by `rpc-runtime.ts.j2` to choose between
    /// `@cratestack/cbor`'s `createCborCodec()` and the plain
    /// `jsonRpcCodec` as the default codec. `rest-runtime.ts.j2` never
    /// reads this field — REST has no codec seam to gate.
    native_cbor: bool,
    /// The semver range `package.json`'s `@cratestack/cbor` dependency is
    /// pinned to under `native_cbor` on an RPC-transport schema.
    ///
    /// cratestack#779: this is `crate::package_floors::
    /// CRATESTACK_CBOR_FLOOR`, an API-compatibility constant. It was a
    /// `^{major}.{minor}.0` floor *derived from* `CARGO_PKG_VERSION`
    /// (#746's partial fix), which still moved at a minor bump — see
    /// `crate::package_floors`' module doc for why only a constant closes
    /// the window, and for the one known gap in the value chosen.
    ///
    /// Empty when `native_cbor` is off or the schema is REST transport,
    /// where no template reads it.
    native_cbor_version_requirement: String,
    /// Issue #610: `README.md.j2`'s "Optimistic concurrency" section
    /// documents `getWithResponse`/`ifMatch`, which only exist on REST
    /// output (RPC has no per-route `If-Match`/`ETag` concept — see
    /// `rest-client.ts.j2`'s doc comment on `getWithResponse` and
    /// `crate::templates::specs`'s module doc on why `rpc-client.ts.j2`
    /// was deliberately left untouched). `true` iff `schema.transport ==
    /// TransportStyle::Rest`.
    is_rest_transport: bool,
    /// Issue #610: whether any model in the schema declares `@version` —
    /// gates the same README section a second way, since the section's
    /// own prose is scoped to "a model with an `@version` field". A
    /// schema with no versioned model has no `If-Match`/`ETag`
    /// requirement to document at all.
    has_versioned_model: bool,
    /// The relative module specifier `rpc-runtime.ts.j2`'s `terminalLink`/
    /// `rpc-stream-terminal.ts.j2`'s `terminalStreamLink` use to import
    /// `encodeWireFields` (cratestack#746 follow-up P1 fix). Both
    /// templates are rendered verbatim against two different output
    /// directories — this (default) layout's `src/runtime.ts` +
    /// `src/stream-terminal.ts`, siblings of `src/models.ts`, so `"./models.js"`
    /// — and `--swr`'s `src/swr/runtime.ts` + `src/swr/stream-terminal.ts`,
    /// one directory below the same `src/models.ts` (never duplicated into
    /// `src/swr/`, see `crate::swr`'s module doc), so `"../models.js"` there
    /// (`crate::swr::views::SwrSchemaContext::models_import_path`) — a
    /// hardcoded `"./models.js"` in the shared template would resolve for
    /// only one of the two.
    models_import_path: &'static str,
}

pub(crate) fn build_template_context(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<TemplateContext, TypeScriptGeneratorError> {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied_type_names = occupied_type_names(schema);
    let wire_shapes = build_wire_shapes(schema);
    let client_class_name = format!(
        "{}Client",
        to_pascal_case(&package_class_stem(&config.package_name))
    );

    let mut enums = schema.enums.iter().map(build_enum_view).collect::<Vec<_>>();
    let mut interfaces = Vec::new();
    for ty in &schema.types {
        interfaces.push(build_interface(
            &ty.name,
            &ty.fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
    }
    // `InterfaceKind::Model` forces every field optional to account for
    // partial `fields`/`include` projection on the wire. `--full-selection`
    // opts a generation run out of that: reuse `Plain`'s arity-driven
    // optionality (the schema's own nullable/required split) so consumers
    // who always fetch full objects get fully-required interfaces instead
    // of hand-rolling a narrowing type on top of the generator's output.
    let model_interface_kind = if config.full_selection {
        InterfaceKind::Plain
    } else {
        InterfaceKind::Model
    };
    for model in &schema.models {
        let scalar_fields = scalar_model_fields(model, &model_names);
        interfaces.push(build_interface(
            &model.name,
            &visible_model_fields(model),
            model_interface_kind,
            &enum_names,
        ));
        // cratestack#743: `Create<M>Input`/`Update<M>Input` are only
        // ever referenced from this model's own generated `create`/
        // `update` client methods (`rest-client.ts.j2`/
        // `rpc-client.ts.j2`), which are correspondingly omitted once
        // `ModelApiView::allows_create`/`allows_update` is `false` — so
        // emitting the interface anyway would be exactly the
        // "unreferenced Create<M>Input" the acceptance criteria forbid.
        // `allows_create` already folds in `model_allows_create`, so
        // this preserves that pre-existing gate unchanged and only adds
        // the new suppression check on top (see `ModelApiView`'s doc).
        let internal = cratestack_core::model_internal_actions(model);
        if model_allows_create(model) && !internal.contains("create") {
            interfaces.push(build_interface(
                &format!("Create{}Input", model.name),
                &scalar_fields
                    .iter()
                    .copied()
                    // `@computed` fields are resolver-backed and
                    // response-time only — never part of a create input,
                    // since the server struct never carries them either
                    // (`docs/design/computed-fields.md`).
                    .filter(|field| !is_computed_field(field))
                    .filter(|field| !is_generated_on_create(field))
                    .collect::<Vec<_>>(),
                InterfaceKind::Plain,
                &enum_names,
            ));
        }
        if !internal.contains("update") {
            interfaces.push(build_interface(
                &format!("Update{}Input", model.name),
                &scalar_fields
                    .iter()
                    .copied()
                    .filter(|field| !is_primary_key(field))
                    // `@computed` fields are never part of an update input
                    // either — same reasoning as the create input above.
                    .filter(|field| !is_computed_field(field))
                    .collect::<Vec<_>>(),
                InterfaceKind::Patch,
                &enum_names,
            ));
        }

        let where_interface = build_where_interface(model, &model_names);
        if let Some(where_interface) = where_interface.clone() {
            interfaces.push(where_interface);
        }
        enums.push(build_sort_field_view(model, &model_names));
        interfaces.push(build_order_by_clause_interface(model));
        interfaces.push(build_find_many_interface(model, where_interface.is_some()));
        // `docs/design/computed-fields.md`'s typed `computedParams` surface
        // (cratestack#stage4): only emitted for a model with at least one
        // *parameterized* computed field — see
        // `crate::views::build_computed_params_interface`'s doc comment.
        if let Some(computed_params_interface) = build_computed_params_interface(model) {
            interfaces.push(computed_params_interface);
        }
    }
    for procedure in &schema.procedures {
        let fields = procedure
            .args
            .iter()
            .map(|arg| Field {
                docs: arg.docs.clone(),
                name: arg.name.clone(),
                name_span: arg.name_span,
                ty: arg.ty.clone(),
                attributes: Vec::new(),
                span: arg.span,
            })
            .collect::<Vec<_>>();
        interfaces.push(build_interface(
            &crate::naming::procedure_wrapper_name(procedure, &occupied_type_names),
            &fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
    }

    let mut models = schema
        .models
        .iter()
        .map(build_model_api)
        .collect::<Vec<_>>();
    disambiguate_model_api_keys(&mut models);
    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| {
            build_procedure(procedure, &occupied_type_names, &enum_names, &model_names)
        })
        .collect::<Vec<_>>();
    // Explicit `::<Vec<_>>` (cratestack#802): these used to infer their
    // type solely from `TemplateContext`'s field, which stops working the
    // moment anything borrows them as a slice before the struct literal.
    let query_procedures = procedures
        .iter()
        .filter(|procedure| procedure.kind == "query")
        .cloned()
        .collect::<Vec<_>>();
    let mutation_procedures = procedures
        .iter()
        .filter(|procedure| procedure.kind == "mutation")
        .cloned()
        .collect::<Vec<_>>();

    // cratestack#779: both are API-compatibility constants now, not
    // values derived from this crate's own version at any precision —
    // see `crate::package_floors`.
    let refine_version_requirement = if config.refine {
        requirement(CRATESTACK_REFINE_FLOOR)
    } else {
        String::new()
    };
    let is_rpc_transport = schema.transport == cratestack_core::TransportStyle::Rpc;
    let native_cbor_version_requirement = if config.native_cbor && is_rpc_transport {
        requirement(CRATESTACK_CBOR_FLOOR)
    } else {
        String::new()
    };
    // `@cratestack/adapter-rtk` is an RPC-only dependency (`crate::rtk`'s
    // module doc) — empty on REST regardless of `config.rtk`.
    let rtk_adapter_version_requirement = if config.rtk && is_rpc_transport {
        requirement(CRATESTACK_ADAPTER_RTK_FLOOR)
    } else {
        String::new()
    };

    // cratestack#802: refuse a schema whose `--tanstack` procedure hook
    // name collides with a derived model hook name, before any file is
    // rendered. Gated on the flag for #317's reason — a schema never
    // generated with `--tanstack` must not be constrained by
    // `--tanstack`'s naming scheme. Placed here rather than in
    // `crate::generator` because the views it compares are built in this
    // function and are not reachable through `TemplateContext`'s private
    // fields.
    if config.tanstack {
        reject_tanstack_hook_name_collisions(&models, &query_procedures, &mutation_procedures)?;
    }
    // cratestack#906: the `--rtk` analogue, same reasoning and placement.
    if config.rtk {
        reject_rtk_endpoint_name_collisions(&models, &procedures)?;
    }

    Ok(TemplateContext {
        package_name: config.package_name.clone(),
        client_class_name,
        base_path: config.base_path.clone(),
        schema_sha256: config.schema_sha256.clone(),
        enums,
        interfaces,
        models,
        procedures,
        query_procedures,
        mutation_procedures,
        wire_shapes,
        refine: config.refine,
        refine_version_requirement: refine_version_requirement.clone(),
        // Built only when the flag is on: a default run has no template
        // that reads this, and walking every model to fill a list nothing
        // renders would be wasted work on the hot path.
        refine_resources: if config.refine {
            build_refine_resources(schema)
        } else {
            Vec::new()
        },
        refine_resource_map_type: if config.refine {
            refine_resource_map_type(schema.transport).to_owned()
        } else {
            String::new()
        },
        swr: config.swr,
        tanstack: config.tanstack,
        rtk: config.rtk,
        rtk_adapter_version_requirement: rtk_adapter_version_requirement.clone(),
        peer_dependencies: peer_dependencies_for(
            config,
            &refine_version_requirement,
            &rtk_adapter_version_requirement,
        ),
        dev_dependencies: dev_dependencies_for(
            config,
            &refine_version_requirement,
            &rtk_adapter_version_requirement,
        ),
        dependencies: dependencies_for(config, is_rpc_transport, &native_cbor_version_requirement),
        native_cbor: config.native_cbor,
        native_cbor_version_requirement,
        is_rest_transport: schema.transport == cratestack_core::TransportStyle::Rest,
        has_versioned_model: schema
            .models
            .iter()
            .any(|model| version_field(model).is_some()),
        models_import_path: "./models.js",
    })
}
