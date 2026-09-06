//! Serializable render contexts for the `riverpod` preset's fan-out
//! templates (`templates/riverpod/*.j2`). Reuses `crate::views`'s
//! sub-views (`EnumView`, `DataClassView`, `SelectionModelView`,
//! `ModelApiView`, `ModelAccessorView`, `ProcedureView`) verbatim — only
//! how they're grouped per output file is new here.
use serde::Serialize;

use crate::views::{
    DataClassView, EnumView, ModelAccessorView, ModelApiView, ProcedureView, SelectionModelView,
};

/// Renders one `lib/src/models/<model>.dart`. `selection` is only used
/// for its `ProjectedX` fields here — the `Selection`/`IncludeSelection`
/// classes render from `QueriesFileContext` instead (see its doc for
/// why).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelFileContext {
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) imports: Vec<String>,
    /// `part '<file_stem>.g.dart';` target (issue #302) — the
    /// `build_runner`-expanded companion this file's `@riverpod`
    /// annotations need. Rendered as a plain string rather than a bool
    /// gate: every riverpod-preset model file carries at least the
    /// `get`/`list` providers, so the directive is unconditional.
    pub(crate) part_file_name: String,
    /// `part '<file_stem>.mapper.dart';` target (issue #325) — the
    /// `dart_mappable_builder`-expanded companion every generated data
    /// class's `@MappableClass()` needs, run in the same `build_runner`
    /// pass as `part_file_name`'s `riverpod_generator` output above.
    pub(crate) mapper_part_file_name: String,
    /// `part '<file_stem>.builder.dart';` target (issue #668 phase 2) —
    /// the `package:cratestack_builder`-generated companion every
    /// `@CratestackBuilder()`-annotated data class in this file needs, run
    /// in the same `build_runner` pass as the two `part`s above.
    /// Unconditional for the same reason `part_file_name`/
    /// `mapper_part_file_name` are: `data_classes` is never empty here.
    pub(crate) builder_part_file_name: String,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    pub(crate) selection: SelectionModelView,
    pub(crate) model_api: ModelApiView,
    pub(crate) accessor: ModelAccessorView,
    /// Issue #302's per-operation `@riverpod` providers, built on top of
    /// `accessor`/`model_api` — see `crate::riverpod::provider_naming`'s
    /// module doc for the naming/collision rule.
    pub(crate) operations: ModelOperationsView,
    /// Issue #331: `model_providers.dart.j2` is `{% include %}`d
    /// verbatim from both `rest_model.dart.j2` and `rpc_model.dart.j2`
    /// (see `build_model_file`'s `is_rest` parameter) — REST's `get`/
    /// `list` providers forward a typed `CratestackFetchQuery`/
    /// `CratestackListQuery` (already imported unconditionally on the
    /// REST path via `../queries.dart`), RPC's `list` provider forwards
    /// an `IMap<String, Object?>` filter/pagination bag instead (no
    /// RPC-side typed query builder exists — see this story's PR body
    /// for why `IMap`, not a bare `Map`: the same missing-value-equality
    /// bug this story's REST fix addresses on `CratestackListQuery`
    /// would otherwise reappear on the RPC `list` provider's own family
    /// argument). One shared template with this flag, not two forked
    /// templates, since every other line (the five providers' shapes,
    /// the write controllers) is identical either way.
    pub(crate) is_rest: bool,
}

/// Collision-checked identifiers (`crate::riverpod::provider_naming`) for
/// one model's five `@riverpod` operation providers — always all five,
/// mirroring `model_api`'s own unconditional list/get/create/update/
/// delete surface (this generator's REST/RPC paths never gate `create`
/// on `@@allow`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelOperationsView {
    /// `@riverpod Future<Model> {get_function_name}(Ref ref, K id)`.
    pub(crate) get_function_name: String,
    /// `@riverpod Future<List<Model>|Page<Model>> {list_function_name}(Ref ref)`.
    pub(crate) list_function_name: String,
    /// `@riverpod class {create_controller_name} extends _$...`.
    pub(crate) create_controller_name: String,
    pub(crate) update_controller_name: String,
    pub(crate) delete_controller_name: String,
}

/// Renders `lib/src/models/shared_types.dart` — always emitted (unlike
/// the per-locus files, it isn't conditional on the partition finding
/// something to share): it also carries the `Page`/`PageInfo` wrapper
/// types, which every `@@paged` model's own file needs regardless of
/// whether anything else is genuinely shared.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SharedTypesFileContext {
    /// Extra imports beyond the always-present `dart:typed_data`/
    /// `../runtime.dart` (hardcoded in the template) — only needed for
    /// the rare case of a shared `type` block directly naming a `model`
    /// (issue #137's `type_references_model.cstack` shape).
    pub(crate) imports: Vec<String>,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    /// `part 'shared_types.builder.dart';` target (issue #668 phase 2/3) —
    /// see `ModelFileContext::builder_part_file_name`'s doc. The *value*
    /// is always `"shared_types.builder.dart"`; unlike
    /// `part 'shared_types.mapper.dart';` above (unconditional — the
    /// hand-written `StringFilter`/`NumberFilter`/etc. filter classes
    /// carry `@MappableClass()` regardless of what the partition assigns
    /// to `Owner::Shared`, so `dart_mappable_builder` always has a target),
    /// those filter classes do NOT carry `@CratestackBuilder()` — only
    /// `data_classes` (the partition-assigned/orphan `type` blocks) does,
    /// via `build_data_class`'s unconditional `emit_builder: true`. A
    /// schema whose partition assigns nothing to `Owner::Shared` (e.g.
    /// `tests/fixtures/no_shared_types.cstack`; NOT `ci_rpc.cstack` any
    /// more — cratestack#928 gave every enum a generated filter class,
    /// which made that fixture's shared file non-empty) has zero
    /// `data_classes` here, and
    /// `package:cratestack_builder`'s `PartBuilder` writes no output file
    /// when its target has zero `@CratestackBuilder()`-annotated classes
    /// — an unconditional directive would be a real `flutter analyze
    /// --fatal-warnings` `uri_has_not_been_generated` failure on that
    /// common case. `shared_types.dart.j2`'s own gate mirrors
    /// `rest_procedures.dart.j2`/`rpc_procedures.dart.j2`'s identical
    /// `data_classes | length > 0` condition for their own builder part.
    pub(crate) builder_part_file_name: String,
}

/// Renders `lib/src/queries.dart` (REST only) — the transport's generic
/// query-builder helpers (unchanged from the `default` preset) plus,
/// still here rather than per-model, every model's `Selection`/
/// `IncludeSelection` pair. Those two classes reference each other's
/// private `_node` field across models with a relation (e.g.
/// `PostSelection.author()` reaches into `AuthorIncludeSelection._node`)
/// — Dart's `_`-prefixed privacy is per-*file*, so splitting them into
/// separate per-model files breaks that cross-reference. `ProjectedX`
/// has no such private cross-reference (`ProjectedPost.author` calls the
/// public `ProjectedAuthor.fromWire` factory), so it stays relocated
/// into the owning model's own file.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueriesFileContext {
    pub(crate) imports: Vec<String>,
    pub(crate) selection_models: Vec<SelectionModelView>,
}

/// Renders `lib/src/procedures.dart` — always emitted (mirrors the
/// `default` preset, which always renders `ProceduresApi` even when the
/// schema declares zero procedures).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProceduresFileContext {
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) imports: Vec<String>,
    /// `part 'procedures.g.dart';` — see `ModelFileContext::part_file_name`.
    /// The *value* is always `"procedures.g.dart"`, but unlike
    /// `ModelFileContext::part_file_name` (every model file carries at
    /// least the `get`/`list` `@riverpod` providers, so it's unconditional
    /// there), `rest_procedures.dart.j2`/`rpc_procedures.dart.j2` only
    /// emit the directive itself when `procedures` is non-empty.
    ///
    /// cratestack#628: this field's doc used to claim an unconditional
    /// directive was safe because "an empty `part` file is a harmless
    /// no-op `build_runner` output" — that was never verified and is
    /// false. `riverpod_generator` writes *no* `.g.dart` file at all for a
    /// source file with zero `@riverpod` declarations (confirmed
    /// empirically by running `build_runner` over a zero-procedure
    /// schema), so an unconditional `part 'procedures.g.dart';` dangles:
    /// `flutter analyze --fatal-warnings` fails with
    /// `uri_has_not_been_generated`, not a no-op. Gated on
    /// `procedures | length > 0` — the same condition
    /// `procedure_providers.dart.j2`'s `@riverpod` loop runs under, since
    /// that loop is the only source of `@riverpod` declarations in this
    /// file.
    pub(crate) part_file_name: String,
    /// `part 'procedures.mapper.dart';` — see
    /// `ModelFileContext::mapper_part_file_name`'s doc. The *value* is
    /// always `"procedures.mapper.dart"`; like `part_file_name` above,
    /// `rest_procedures.dart.j2`/`rpc_procedures.dart.j2` only emit the
    /// directive itself when its own condition holds — here,
    /// `data_classes` is non-empty (issue #325) rather than
    /// `procedures | length > 0` — see those templates' own comment for
    /// why an unconditional directive here would be a real
    /// `flutter analyze` failure on a schema with zero procedures.
    pub(crate) mapper_part_file_name: String,
    /// `part 'procedures.builder.dart';` — see
    /// `ModelFileContext::builder_part_file_name`'s doc. The *value* is
    /// always `"procedures.builder.dart"`; like `mapper_part_file_name`
    /// above (and unlike `part_file_name`), `rest_procedures.dart.j2`/
    /// `rpc_procedures.dart.j2` only emit the directive itself when
    /// `data_classes` is non-empty — `package:cratestack_builder`, like
    /// `dart_mappable_builder`, writes no part file when its target has
    /// zero annotated classes, so an unconditional directive here would be
    /// a real `flutter analyze --fatal-warnings` `uri_has_not_been_generated`
    /// failure on a schema with zero procedures and no procedure-owned
    /// nested `type`s.
    pub(crate) builder_part_file_name: String,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    /// Issue #302: `procedures[i]` and `procedure_operations[i]` are the
    /// same procedure, in the same order — kept as two parallel `Vec`s
    /// rather than folding `ProcedureOperationView` into `ProcedureView`
    /// so `crate::builders_model::build_procedure` (shared with the
    /// `default` preset) never needs to know about riverpod-only naming.
    /// cratestack#627: `rest_procedures.dart.j2`/
    /// `rpc_procedures.dart.j2` gate `ProceduresApi`'s `final _client`
    /// field and constructor param on `procedures | length > 0` directly
    /// — a schema with zero procedures never reads `_client` anywhere (the
    /// `{% for procedure in procedures %}` loop that's its only reader
    /// never runs), which is a real `unused_field` `flutter analyze
    /// --fatal-warnings` failure otherwise. `ClientFileContext::
    /// has_procedures` mirrors this same condition for the call site in
    /// the separate `client.dart` file.
    pub(crate) procedures: Vec<ProcedureView>,
    pub(crate) procedure_operations: Vec<ProcedureOperationView>,
}

/// One procedure's `@riverpod` provider identifier plus which shape it
/// needs — a function (`ProcedureKind::Query`) or a controller class
/// (`ProcedureKind::Mutation`), matching `ProcedureView::kind`'s already-
/// computed `"query"`/`"mutation"` literal so the template can gate on
/// the same field it already has.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureOperationView {
    pub(crate) kind: &'static str,
    /// The function name (query) or class name (mutation) —
    /// collision-checked the same way as `ModelOperationsView`'s fields.
    pub(crate) symbol: String,
    /// `ProcedureView::return_type` with `dart_type(..., force_nullable:
    /// true)` — a mutation controller's `build()` always starts at
    /// `null` (no result yet), so its declared state type must be
    /// nullable even when the procedure's own return type isn't. Can't
    /// just template-append `?` to `return_type`: a procedure whose
    /// schema return type is itself already optional (`Foo?`) already
    /// carries a trailing `?`, and Dart doesn't allow `Foo??`. Only
    /// meaningful for `kind == "mutation"`.
    pub(crate) nullable_return_type: String,
    /// Dart method name for the mutation controller's own action method
    /// — `ProcedureView::method_name` verbatim, *unless* it collides
    /// with a name `riverpod_generator`'s `_$AsyncClassModifier` base
    /// class already declares (`update`, confirmed empirically to
    /// produce a real `invalid_override` `dart analyze` error — see
    /// `templates/riverpod/model_providers.dart.j2`'s `save` rename for
    /// the same collision on the model side, where it's unconditional
    /// rather than schema-dependent). Only meaningful for
    /// `kind == "mutation"`; query providers are top-level functions,
    /// not class methods, so they're never subject to this override
    /// check.
    pub(crate) mutation_method_name: String,
}

/// Renders `lib/src/client.dart` — the package-wide DI surface
/// (`xAdapterProvider`/`xClientProvider`/`{{ client_class_name }}`) that
/// every per-model `Provider<XApi>` watches. Never per-model.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClientFileContext {
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) base_path_literal: String,
    pub(crate) imports: Vec<String>,
    pub(crate) model_accessors: Vec<ModelAccessorView>,
    /// cratestack#627: whether the schema
    /// declares at least one `procedure`. `rest_client.dart.j2`/
    /// `rpc_client.dart.j2` need this to decide whether
    /// `ProceduresApi.new` still takes a client argument — see
    /// `ProceduresFileContext`'s doc for the paired condition on the
    /// `ProceduresApi` class declaration itself. `procedures.dart` is a
    /// separate file from `client.dart`, so this can't just be `procedures
    /// | length > 0` in the template the way `procedures.dart` itself
    /// checks it — `ClientFileContext` never otherwise carries the
    /// procedure list.
    pub(crate) has_procedures: bool,
}

/// Renders `lib/<package_name>.dart`, the library entrypoint.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LibraryFileContext {
    pub(crate) exports: Vec<String>,
}
