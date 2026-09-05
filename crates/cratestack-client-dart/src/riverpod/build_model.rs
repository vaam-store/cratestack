//! Builds one `lib/src/models/<model>.dart` render context per model —
//! the fan-out half of issue #301: the model's own types (itself,
//! `Create<M>Input`, `Update<M>Input`), any nested `type`/`enum` the
//! partition (`crate::riverpod::partition`) assigned exclusively to this
//! model, its `ProjectedX` view, its `XApi` client class, and its
//! `Provider<XApi>` — all relocated verbatim from today's
//! `rest-apis.dart.j2`/`rpc-apis.dart.j2` per-model loop, not redesigned.
//! `Selection`/`IncludeSelection` (REST only) stay in `queries.dart`
//! instead — see `crate::riverpod::views::QueriesFileContext`'s doc for
//! why (a real cross-file Dart privacy bug, not a style choice).
use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{EnumDecl, Model, Schema, TypeDecl};

use crate::builders::{build_data_class, build_enum_view};
use crate::builders_model::{build_model_accessor, build_model_api, build_selection_model};
use crate::find_many_views::{
    build_find_many_data_class, build_order_by_clause_data_class, build_sort_field_enum,
    build_where_data_class,
};
use crate::idents::to_camel_case;
use crate::naming::{
    is_computed_field, is_generated_on_create, is_primary_key, model_name_set, scalar_model_fields,
};
use crate::riverpod::imports::{
    model_file_path, model_file_stem, model_relation_targets, owned_type_decl_model_refs,
    render_import_lines, scalar_type_imports,
};
use crate::riverpod::partition::{Owner, TypePartition};
use crate::riverpod::provider_naming::reserve_operation_symbol;
use crate::riverpod::views::{ModelFileContext, ModelOperationsView};
use crate::views::{DataClassKind, ModelApiView};

/// `build_model_api` is shared verbatim with the `default` preset (its
/// output is a byte-identical contract — see `tests/snapshot.rs`), so an
/// unpaged model's `list()` return type/decode can't be forked in there.
/// Riverpod additionally depends on `fast_immutable_collections`, so its
/// own per-model file gets `IList<Model>` instead of `List<Model>` for
/// this one field, computed on top of the shared view rather than inside
/// it — mirrors `build_pubspec.rs`'s "own builder, not a conditional
/// branch in the shared one" precedent. Paged models are untouched here:
/// `Page<T>.items` becomes `IList<T>` separately, in
/// `shared_types.dart.j2`, since `Page` itself doesn't change name.
fn build_riverpod_model_api(model: &Model) -> ModelApiView {
    let mut view = build_model_api(model);
    if !view.is_paged {
        view.list_return_type = format!("IList<{}>", model.name);
        view.list_decode_expr = format!(
            "cratestackAsValueList(body).map((item) => {}.fromWire(cratestackAsValueMap(item))).toIList()",
            model.name
        );
    }
    view
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_model_file(
    schema: &Schema,
    model: &Model,
    partition: &TypePartition,
    type_by_name: &BTreeMap<&str, &TypeDecl>,
    enum_by_name: &BTreeMap<&str, &EnumDecl>,
    provider_prefix: &str,
    client_class_name: &str,
    is_rest: bool,
    occupied_provider_symbols: &mut BTreeSet<String>,
) -> (String, ModelFileContext) {
    let model_names = model_name_set(&schema.models);
    let enum_names: BTreeSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();

    let model_fields = model.fields.iter().collect::<Vec<_>>();
    let scalar_fields = scalar_model_fields(model, &model_names);

    let mut data_classes = vec![build_data_class(
        &model.name,
        &model_fields,
        DataClassKind::ProjectionModel,
        &enum_names,
        &model_names,
    )];

    // cratestack#743: same reasoning as the `default` preset's
    // `context.rs` — a suppressed `create`/`update` leaves this file's
    // own `create()`/`update()` methods (gated below via
    // `build_riverpod_model_api`'s `allows_create`/`allows_update`)
    // omitted, so the input class would otherwise be unreferenced.
    let internal = cratestack_core::model_internal_actions(model);

    let create_fields = scalar_fields
        .iter()
        .copied()
        // `@computed` fields are resolver-backed and response-time only
        // — never part of a create input (`docs/design/computed-fields.md`).
        .filter(|field| !is_computed_field(field))
        .filter(|field| !is_generated_on_create(field))
        .collect::<Vec<_>>();
    if !internal.contains("create") {
        data_classes.push(build_data_class(
            &format!("Create{}Input", model.name),
            &create_fields,
            DataClassKind::Plain,
            &enum_names,
            &model_names,
        ));
    }

    let update_fields = scalar_fields
        .iter()
        .copied()
        .filter(|field| !is_primary_key(field))
        // `@computed` fields are never part of an update input either —
        // same reasoning as `create_fields` above.
        .filter(|field| !is_computed_field(field))
        .collect::<Vec<_>>();
    if !internal.contains("update") {
        data_classes.push(build_data_class(
            &format!("Update{}Input", model.name),
            &update_fields,
            DataClassKind::Patch,
            &enum_names,
            &model_names,
        ));
    }

    let locus = Owner::Model(model.name.clone());
    let mut owned_type_decls = Vec::new();
    for name in partition.owned_names(&locus) {
        if let Some(type_decl) = type_by_name.get(name) {
            owned_type_decls.push(*type_decl);
            let fields = type_decl.fields.iter().collect::<Vec<_>>();
            data_classes.push(build_data_class(
                &type_decl.name,
                &fields,
                DataClassKind::Plain,
                &enum_names,
                &model_names,
            ));
        }
    }

    let mut enum_types: Vec<_> = partition
        .owned_names(&locus)
        .into_iter()
        .filter_map(|name| enum_by_name.get(name))
        .map(|enum_decl| build_enum_view(enum_decl))
        .collect();

    // `<Model>Where`/`<Model>SortField`/`<Model>OrderByClause`/
    // `<Model>FindMany` are always single-model-owned by construction
    // (never cross-model, unlike the partition-computed types above), so
    // they're generated directly here rather than routed through
    // `TypePartition` — same reasoning as the TypeScript `swr` preset's
    // `find_many_views.rs` usage in `swr/context.rs`.
    let where_class = build_where_data_class(model, &model_names);
    if let Some(where_class) = where_class.clone() {
        data_classes.push(where_class);
    }
    enum_types.push(build_sort_field_enum(model, &model_names));
    data_classes.push(build_order_by_clause_data_class(model));
    data_classes.push(build_find_many_data_class(model, where_class.is_some()));

    let selection = build_selection_model(model, &schema.models, &model_names, &enum_names);
    let model_api = build_riverpod_model_api(model);
    let accessor = build_model_accessor(model, provider_prefix);

    let operations = ModelOperationsView {
        get_function_name: reserve_operation_symbol(
            &to_camel_case(&model.name),
            false,
            provider_prefix,
            occupied_provider_symbols,
        ),
        list_function_name: reserve_operation_symbol(
            &format!("{}List", to_camel_case(&model.name)),
            false,
            provider_prefix,
            occupied_provider_symbols,
        ),
        create_controller_name: reserve_operation_symbol(
            &format!("{}CreateController", model.name),
            true,
            provider_prefix,
            occupied_provider_symbols,
        ),
        update_controller_name: reserve_operation_symbol(
            &format!("{}UpdateController", model.name),
            true,
            provider_prefix,
            occupied_provider_symbols,
        ),
        delete_controller_name: reserve_operation_symbol(
            &format!("{}DeleteController", model.name),
            true,
            provider_prefix,
            occupied_provider_symbols,
        ),
    };

    let mut imports: BTreeSet<String> = BTreeSet::new();
    imports.insert("import 'package:flutter_riverpod/flutter_riverpod.dart';".to_owned());
    imports.insert("import 'package:riverpod_annotation/riverpod_annotation.dart';".to_owned());
    imports.insert("import 'package:dart_mappable/dart_mappable.dart';".to_owned());
    // Issue #668 phase 2: every data class this file emits carries
    // `@CratestackBuilder(...)` (see `enums_and_data_classes.dart.j2`) —
    // unconditional, unlike the imports below that are gated on what this
    // particular model actually uses, because `data_classes` here is
    // never empty (at minimum the model itself, `Create<M>Input` and
    // `Update<M>Input`).
    imports
        .insert("import 'package:cratestack_annotations/cratestack_annotations.dart';".to_owned());
    imports.insert("import '../runtime.dart';".to_owned());
    imports.insert("import '../client.dart';".to_owned());
    // `<Model>ComputedParams.operator ==`/`hashCode`
    // (`computed_params_class.dart.j2`) are wire-equality, built on
    // `jsonEncode(toWire())` — only needed when this model actually emits
    // that class, per this module's "only import what's used" rule.
    if model_api.computed_params_class_name.is_some() {
        imports.insert("import 'dart:convert';".to_owned());
    }
    // cratestack#625/#630: a `Bytes` field maps to `Uint8List`
    // (`dart:typed_data`) and a `Decimal` field to `Decimal`
    // (`package:decimal/decimal.dart`) — neither is in scope by default,
    // so spelling one without its import is an `undefined_class` failure.
    // The `default` preset's monolithic `models.dart.j2:1` and this
    // preset's own `shared_types.dart.j2:5-7` hardcode both lines (a
    // single shared file always has *some* model somewhere), but a
    // per-model file only needs a line when this model's own fields
    // actually use that scalar — otherwise it's a real `unused_import`
    // `flutter analyze --fatal-warnings` failure, per this module's "only
    // import what's used" rule. #625 hand-rolled this for `Bytes` only and
    // missed `Decimal`; `scalar_type_imports` is now the one place that
    // decides. The model's own fields cover every data class this file
    // emits — `<Model>`, `Create<Model>Input`, `Update<Model>Input` and
    // `Projected<Model>` are all projections of them, and `<Model>Where`
    // spells the shared `DecimalFilter`/`BytesFilter` rather than the
    // scalar itself. The `owned_type_decls` term chained below *is*
    // reachable, though only through a `@computed` field: the parser
    // rejects a `type` block as a *stored* model field's storage type
    // (`cratestack-parser`'s `reject_type_decl_as_model_field_type`), but
    // exempts a `@computed` one, so `partition.owned_names` for a
    // `Owner::Model` locus can yield a `TypeDecl` here whenever this
    // model's `@computed` field type isn't shared with anything else.
    // That `TypeDecl`'s own fields can themselves spell `Bytes`/
    // `Decimal`, so this chain is load-bearing, not just symmetry with
    // the adjacent, pre-existing `owned_type_decl_model_refs` call below
    // — the model's own fields alone are no longer guaranteed to cover
    // every scalar this file's data classes use.
    imports.extend(scalar_type_imports(
        model.fields.iter().map(|field| &field.ty).chain(
            owned_type_decls
                .iter()
                .flat_map(|type_decl| type_decl.fields.iter().map(|field| &field.ty)),
        ),
    ));
    if is_rest {
        imports.insert("import '../queries.dart';".to_owned());
    }
    // `shared_types.dart` also carries `Page`/`PageInfo`/`SortDirection`/
    // the shared filter classes (see `build_shared_types`'s doc). Every
    // model unconditionally gets its own `<Model>OrderByClause`, whose
    // `direction` field is always `SortDirection` (see
    // `find_many_views.rs::build_order_by_clause_data_class`), so this
    // import is now unconditional too — unlike before issue #371's
    // `FindMany<Model>` redesign, when only a paged model or one with a
    // partition-shared reference needed it (confirmed empirically: an
    // unpaged, share-free model without this import fails `flutter
    // analyze` with `undefined_class` on `SortDirection`/`NumberFilter`/
    // etc. in its own `<Model>Where`/`<Model>OrderByClause`).
    imports.insert("import 'shared_types.dart';".to_owned());
    // An unpaged model's own `list()`/`listView()` return `IList<...>`
    // (see `build_riverpod_model_api`'s doc), and a list-arity relation
    // getter does too regardless of whether this model itself is paged.
    // Issue #331: the RPC `list` provider's own `input` parameter is
    // always `IMap<String, Object?>` (see `ModelFileContext::is_rest`'s
    // doc), so an RPC model file needs this import unconditionally too,
    // not just when paging/relations already demanded it. Otherwise
    // only import the package when this file actually references it,
    // per this module's "only import what's used" rule (`dart analyze
    // --fatal-warnings` fails on an unused import).
    let has_list_relation = selection.relations.iter().any(|relation| relation.is_list);
    if !model_api.is_paged || has_list_relation || !is_rest {
        imports.insert(
            "import 'package:fast_immutable_collections/fast_immutable_collections.dart';"
                .to_owned(),
        );
    }

    let mut related_models = model_relation_targets(model, &model_names);
    related_models.extend(owned_type_decl_model_refs(
        owned_type_decls.iter().copied(),
        &model_names,
    ));
    for other in related_models {
        imports.insert(format!("import '{}';", model_file_path(&other)));
    }

    let context = ModelFileContext {
        client_class_name: client_class_name.to_owned(),
        provider_prefix: provider_prefix.to_owned(),
        imports: render_import_lines(imports),
        part_file_name: format!("{}.g.dart", model_file_stem(&model.name)),
        mapper_part_file_name: format!("{}.mapper.dart", model_file_stem(&model.name)),
        builder_part_file_name: format!("{}.builder.dart", model_file_stem(&model.name)),
        enum_types,
        data_classes,
        selection,
        model_api,
        accessor,
        operations,
        is_rest,
    };

    (model_file_path(&model.name), context)
}
