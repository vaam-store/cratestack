// Behavioral + snapshot tests for issue #302's per-operation `@riverpod`
// providers. Complements `just verify-dart` (the real `flutter pub get`
// -> `dart run build_runner build` -> `flutter analyze` -> `flutter test`
// pipeline, which is the load-bearing proof these providers actually
// compile and that the override-propagation test actually passes): the
// assertions here are the fast, Rust-side regression guard for the same
// properties — collision-free naming, routing exclusively through the
// existing per-model `Provider<XApi>`, and the riverpod-only pubspec
// additions staying off the `default` preset.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

fn generate(fixture: &str, library_name: &str, preset: DartPreset) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .unwrap_or_else(|error| panic!("{fixture} should generate under {preset:?}: {error}"))
}

fn package_file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("missing generated file {name}\n{:#?}", file_names(package)))
        .contents
        .as_str()
}

fn file_names(package: &GeneratedDartPackage) -> Vec<&str> {
    package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect()
}

#[test]
fn every_model_operation_gets_a_provider_built_on_the_existing_api_provider() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Riverpod);
    let widget = package_file(&package, "lib/src/models/widget.dart");

    // Reads: functions. This fixture is RPC (`tiny_rpc`) — `get` takes
    // no query (RPC has no per-record field-selection contract), but
    // issue #331 gives `list` an `IMap<String, Object?>? input` (not a
    // bare `Map` — see `model_providers.dart.j2`'s own comment for why:
    // `Map`'s identity-based `==` would reintroduce the exact
    // family-provider caching bug this story's REST fix addresses).
    assert!(
        widget.contains("Future<Widget> widget(Ref ref, int id) {\n  return ref.watch(tinyRpcClientWidgetApiProvider).get(id);\n}"),
        "get provider missing or not built on the existing WidgetApi provider:\n{widget}"
    );
    assert!(
        widget.contains("Future<IList<Widget>> widgetList(Ref ref, {\n  IMap<String, Object?>? input,\n}) {\n  return ref.watch(tinyRpcClientWidgetApiProvider).list(input: input?.unlock ?? const <String, Object?>{});\n}"),
        "list provider missing, or not forwarding its IMap input to the existing WidgetApi provider:\n{widget}"
    );

    // Writes: controllers, each reading (not watching) the same existing
    // provider inside their action method. `declared_method` is the
    // controller's own method name; `api_call` is the underlying
    // `WidgetApi` method it calls through to (the update controller's
    // own method is renamed to `save` to avoid colliding with
    // `AsyncNotifier`'s built-in `update(...)` — see
    // `model_providers.dart.j2`'s comment — but it still calls the
    // model API's real `.update(...)` method underneath).
    for (controller, declared_method, api_call) in [
        ("WidgetCreateController", "create", "create"),
        ("WidgetUpdateController", "save", "update"),
        ("WidgetDeleteController", "delete", "delete"),
    ] {
        assert!(
            widget.contains(&format!("class {controller} extends _${controller} {{")),
            "{controller} missing:\n{widget}"
        );
        assert!(
            widget.contains(&format!("Future<Widget> {declared_method}(")),
            "{controller} should declare a `{declared_method}` method:\n{widget}"
        );
        assert!(
            widget.contains(&format!(
                "ref.read(tinyRpcClientWidgetApiProvider).{api_call}("
            )),
            "{controller}'s {declared_method}() should call through tinyRpcClientWidgetApiProvider.{api_call}(...):\n{widget}"
        );
    }

    // Never touches the adapter/client provider directly from one of
    // *this story's new* provider bodies. Scoped to the text after the
    // "Issue #302" marker comment: the pre-existing `Provider<WidgetApi>`
    // above it (relocated by #301) legitimately does
    // `ref.watch(tinyRpcClientClientProvider)` — that's the thing these
    // new providers are supposed to route through instead of
    // reimplementing.
    let new_providers_section = widget
        .split("// Issue #302: one `@riverpod` provider per operation")
        .nth(1)
        .expect("model file should carry the issue #302 provider section");
    assert!(
        !new_providers_section.contains("ref.watch(tinyRpcClientAdapterProvider)")
            && !new_providers_section.contains("ref.read(tinyRpcClientAdapterProvider)"),
        "a generated provider reached the adapter provider directly:\n{new_providers_section}"
    );
    assert!(
        !new_providers_section.contains("ref.watch(tinyRpcClientClientProvider)")
            && !new_providers_section.contains("ref.read(tinyRpcClientClientProvider)"),
        "a generated provider reached the client provider directly:\n{new_providers_section}"
    );
}

#[test]
fn every_procedure_gets_a_provider_shaped_by_its_kind() {
    let package = generate(
        "ci_rpc",
        "dart_verify_riverpod_ci_rpc",
        DartPreset::Riverpod,
    );
    let procedures = package_file(&package, "lib/src/procedures.dart");

    // `searchPosts` is a query procedure -> plain function provider.
    assert!(
        procedures.contains("Future<List<Post>> searchPosts(Ref ref, SearchPostsArgs args) {\n  return ref.watch(dartVerifyRiverpodCiRpcProceduresApiProvider).searchPosts(args);\n}"),
        "query procedure provider missing or not built on the existing ProceduresApi provider:\n{procedures}"
    );

    // `currentStatus` is a `mutation procedure` -> controller class.
    assert!(
        procedures.contains("class CurrentStatusController extends _$CurrentStatusController {"),
        "mutation procedure controller missing:\n{procedures}"
    );
    assert!(
        procedures
            .contains("ref.read(dartVerifyRiverpodCiRpcProceduresApiProvider).currentStatus(args)"),
        "mutation procedure controller should call through the existing ProceduresApi provider:\n{procedures}"
    );
}

#[test]
fn model_and_procedure_files_carry_the_part_directive() {
    let package = generate(
        "ci_rpc",
        "dart_verify_riverpod_ci_rpc",
        DartPreset::Riverpod,
    );

    assert!(package_file(&package, "lib/src/models/author.dart").contains("part 'author.g.dart';"));
    assert!(
        package_file(&package, "lib/src/models/author.dart").contains("part 'author.mapper.dart';")
    );
    assert!(package_file(&package, "lib/src/models/post.dart").contains("part 'post.g.dart';"));
    assert!(
        package_file(&package, "lib/src/models/post.dart").contains("part 'post.mapper.dart';")
    );
    assert!(
        package_file(&package, "lib/src/procedures.dart").contains("part 'procedures.g.dart';")
    );
    assert!(
        package_file(&package, "lib/src/procedures.dart")
            .contains("part 'procedures.mapper.dart';")
    );
    // No `@riverpod` surface lives in these two files, so no
    // `riverpod_generator` `.g.dart` part directive should appear in
    // them — `client.dart` has no `@MappableClass()` surface either (no
    // data classes are declared there), so it stays part-free.
    assert!(!package_file(&package, "lib/src/client.dart").contains("part '"));
    // `shared_types.dart` is different: since issue #371's
    // `FindMany<Model>` redesign, it always hand-declares the
    // `@MappableClass()`-annotated `StringFilter`/`NumberFilter`/etc.
    // filter classes (see `shared_types.dart.j2`), regardless of whether
    // the partition assigned this fixture any actual shared `type`/
    // `enum` — so it always needs the mapper part directive now, unlike
    // before that redesign.
    assert!(
        package_file(&package, "lib/src/models/shared_types.dart")
            .contains("part 'shared_types.mapper.dart';")
    );
    // `ci_rpc.cstack`'s `PostStatus` enum is reached by two loci — model
    // `Post` (the `status` field) AND `Procedures` (`searchPosts`'s
    // `PostStatusFilter` argument type, and `currentStatus`'s return
    // type) — so `TypePartition` assigns it `Owner::Shared` (see this
    // module's own doc). Before cratestack#928, an `Owner::Shared` enum
    // contributed only an `EnumView` (no `@CratestackBuilder()` needed),
    // so `data_classes` stayed empty here and `shared_types.dart` had no
    // builder part directive at all — asserted as the negative case
    // below at the time. cratestack#928 added a generated
    // `{EnumName}Filter` `DataClassView` alongside every enum's
    // `EnumView` (so a `<Model>Where`/`FindMany<Model>` filter on an
    // enum field has a real filter type to reference), which for THIS
    // fixture is the first thing that ever puts a `data_classes` entry
    // in `ci_rpc.cstack`'s `shared_types.dart` — flipping both
    // assertions from absent to present.
    //
    // `PostStatusEnumFilter`, not the unqualified `PostStatusFilter`
    // cratestack#928's naming scheme would produce by default: this
    // fixture already hand-declares `type PostStatusFilter { statuses
    // PostStatus[] }` as `searchPosts`'s argument type, so the
    // generated name resolves through the same collision fallback
    // `procedure_wrapper_name` uses for `<Procedure>Args`
    // (`crate::enum_filter_view::enum_filter_class_name`) rather than
    // silently duplicating the schema-authored class name.
    let shared_types = package_file(&package, "lib/src/models/shared_types.dart");
    assert!(
        shared_types.contains("class PostStatusEnumFilter"),
        "PostStatus is Owner::Shared, so its generated filter class lives in \
         shared_types.dart under its collision-avoiding fallback name:\n{shared_types}"
    );
    assert!(
        shared_types.contains("part 'shared_types.builder.dart';"),
        "ci_rpc's shared PostStatusEnumFilter class needs the builder part directive:\n{shared_types}"
    );
    assert!(
        shared_types
            .contains("import 'package:cratestack_annotations/cratestack_annotations.dart';"),
        "ci_rpc's shared PostStatusEnumFilter class needs the cratestack_annotations import:\n{shared_types}"
    );
}

#[test]
fn shared_types_file_gets_the_mapper_part_directive_when_it_has_data_classes() {
    let package = generate(
        "riverpod_shared_type_orphan",
        "dart_verify_riverpod_shared_type_orphan",
        DartPreset::Riverpod,
    );

    let shared_types = package_file(&package, "lib/src/models/shared_types.dart");
    assert!(
        shared_types.contains("import 'package:dart_mappable/dart_mappable.dart';"),
        "shared_types.dart has a real @MappableClass() data class (Coordinates) in this fixture, \
         so the dart_mappable import must be present:\n{shared_types}"
    );
    assert!(
        shared_types.contains("part 'shared_types.mapper.dart';"),
        "shared_types.dart has a real @MappableClass() data class (Coordinates) in this fixture, \
         so the mapper part directive must be present:\n{shared_types}"
    );
    // `shared_types.dart` gets a builder like every other `build_data_class`
    // call site (issue #668 phase 2/3) — origin/main's inline emission
    // covered this file too, so it is not a deliberate builder-free
    // exception (see `crate::riverpod::build_shared_types_file`'s doc).
    assert!(
        shared_types.contains(
            "@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)\n@CratestackBuilder()\nclass Coordinates with CoordinatesMappable {"
        ),
        "{shared_types}"
    );
    // cratestack#668 regression: `@CratestackBuilder()` alone is not enough
    // — this file also needs the annotation's import and the
    // `dart_builder`-expanded part directive it depends on, or
    // `flutter analyze --fatal-warnings` fails on an undefined
    // `@CratestackBuilder` annotation and `build_runner` never produces a
    // `CoordinatesBuilder` at all (see
    // `SharedTypesFileContext::builder_part_file_name`'s doc). A
    // text-only assertion on the annotation alone previously passed
    // while both were missing — assert them explicitly so that can't
    // recur.
    assert!(
        shared_types
            .contains("import 'package:cratestack_annotations/cratestack_annotations.dart';"),
        "shared_types.dart declares @CratestackBuilder() on Coordinates but is missing the \
         cratestack_annotations import:\n{shared_types}"
    );
    assert!(
        shared_types.contains("part 'shared_types.builder.dart';"),
        "shared_types.dart declares @CratestackBuilder() on Coordinates but is missing the \
         builder part directive:\n{shared_types}"
    );
}

/// The naming collision this fixture deliberately constructs (see the
/// `.cstack` file's own header comment): naive per-operation provider
/// names for `Widget.list`, `WidgetList.get`, and the `widgetCreate`
/// mutation procedure would all collide with an existing model's own
/// symbol. Asserts the escalation in `provider_naming.rs` actually fires
/// and produces distinct, deterministic names — `just verify-dart`
/// separately proves those escalated names still compile and pass
/// `flutter analyze`/`build_runner`.
#[test]
fn colliding_provider_names_escalate_to_distinct_symbols() {
    let package = generate(
        "riverpod_provider_collision",
        "dart_verify_riverpod_collision",
        DartPreset::Riverpod,
    );

    let widget = package_file(&package, "lib/src/models/widget.dart");
    let widget_list = package_file(&package, "lib/src/models/widget_list.dart");
    let procedures = package_file(&package, "lib/src/procedures.dart");

    // Widget claims the naive names first (declared first in the
    // schema). This fixture is REST (default transport), so issue #331
    // gives both providers an optional typed `query` parameter — `get`
    // takes `CratestackFetchQuery?`, `list` takes `CratestackListQuery?`
    // — which is why `id`/`Ref ref` now sit on their own line ahead of
    // the `{...}` optional-parameter block (`dart format`-shaped, not
    // this test's own choice).
    assert!(widget.contains(
        "Future<Widget> widget(\n  Ref ref,\n  int id, {\n  CratestackFetchQuery? query,\n})"
    ));
    assert!(widget.contains(
        "Future<IList<Widget>> widgetList(Ref ref, {\n  CratestackListQuery? query,\n})"
    ));
    assert!(widget.contains("class WidgetCreateController extends _$WidgetCreateController {"));

    // WidgetList's own `get` provider wanted the name `widgetList` too —
    // already taken, so it must have escalated to something else, and
    // that something else must actually appear as a real symbol (not
    // just "not the naive name") — still forwarding its own `query`.
    assert!(
        !widget_list.contains("Future<WidgetList> widgetList(\n  Ref ref,\n  int id, {"),
        "WidgetList's get provider should not have kept the colliding name:\n{widget_list}"
    );
    assert!(
        widget_list.contains("Future<WidgetList>")
            && widget_list.contains(
                "  Ref ref,\n  int id, {\n  CratestackFetchQuery? query,\n}) {\n  return ref.watch(dartVerifyRiverpodCollisionWidgetListApiProvider).get(id, query: query);\n}"
            ),
        "WidgetList's get provider should still exist under an escalated name, still forwarding query:\n{widget_list}"
    );

    // The `widgetCreate` mutation procedure wanted `WidgetCreateController`
    // too — already taken by the Widget model, so it must have escalated.
    assert!(
        !procedures.contains("class WidgetCreateController extends _$WidgetCreateController {"),
        "the widgetCreate procedure's controller should not have kept the colliding class name:\n{procedures}"
    );
    assert!(
        procedures.contains("extends _$") && procedures.contains("WidgetCreateController"),
        "the widgetCreate procedure's controller should still exist under an escalated name:\n{procedures}"
    );
}

#[test]
fn riverpod_pubspec_adds_riverpod_annotation_generator_and_build_runner() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Riverpod);
    let pubspec = package_file(&package, "pubspec.yaml");

    assert!(
        pubspec.contains("flutter_riverpod: ^3.3.1"),
        "flutter_riverpod must stay exactly as the default preset already pins it:\n{pubspec}"
    );
    assert!(
        pubspec.contains("riverpod_annotation: 4.0.3"),
        "riverpod_annotation must be pinned to exactly 4.0.3 — riverpod_generator 4.0.4 (below) \
         itself depends on riverpod_annotation '4.0.3' as an exact pin, not a range:\n{pubspec}"
    );
    assert!(
        pubspec.contains("riverpod_generator: 4.0.4"),
        "riverpod_generator must be pinned to exactly 4.0.4 — the newest release still on \
         analyzer ^12.0.0, which resolves against Flutter stable's meta 1.18.0 pin on the real \
         SDK (Flutter 3.44.8/Dart 3.12.2), unlike newer riverpod_generator/build_runner \
         releases (verified by downloading that exact SDK and reproducing the failure for \
         real, not just reasoning from pub.dev version tables — see the pubspec.yaml.j2 \
         template's own comment for the full chain, including why a bare analyzer version pin \
         or a dependency_overrides resolves `pub get` but genuinely breaks `build_runner build` \
         at codegen time):\n{pubspec}"
    );
    assert!(
        pubspec.contains(r#"build_runner: ">=2.14.0 <2.15.2""#),
        "build_runner must stay capped below 2.15.2 (not 2.15.0 — 2.15.0/2.15.1 still declare \
         analyzer '>=8.0.0 <14.0.0', the same band 2.14.x accepts; only 2.15.2 tightens the \
         floor past what analyzer 12.x, what riverpod_generator 4.0.4 above needs, satisfies; \
         see issue #358):\n{pubspec}"
    );
    // A bare, non-Flutter `riverpod:` package must never be added
    // alongside `flutter_riverpod` — it already re-exports what
    // `@riverpod` needs.
    assert!(
        !pubspec.lines().any(|line| line.trim_start() == "riverpod:"),
        "a bare `riverpod:` dependency line should never be added:\n{pubspec}"
    );

    // Dependencies vs dev_dependencies placement: `riverpod_annotation`
    // must appear before the `dev_dependencies:` marker, and
    // `riverpod_generator`/`build_runner` after it.
    let dev_split = pubspec
        .find("dev_dependencies:")
        .expect("pubspec should have a dev_dependencies section");
    let annotation_index = pubspec
        .find("riverpod_annotation:")
        .expect("riverpod_annotation should be present");
    let generator_index = pubspec
        .find("riverpod_generator:")
        .expect("riverpod_generator should be present");
    let build_runner_index = pubspec
        .find("build_runner:")
        .expect("build_runner should be present");
    assert!(annotation_index < dev_split, "{pubspec}");
    assert!(generator_index > dev_split, "{pubspec}");
    assert!(build_runner_index > dev_split, "{pubspec}");
}

#[test]
fn default_preset_pubspec_stays_untouched_by_the_riverpod_only_additions() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Default);
    let pubspec = package_file(&package, "pubspec.yaml");

    assert!(!pubspec.contains("riverpod_annotation"), "{pubspec}");
    assert!(!pubspec.contains("riverpod_generator"), "{pubspec}");
    // issue #668 phase 2: `build_runner` is now a genuinely shared
    // addition, not riverpod-only — the default preset gained its own
    // (unpinned) `build_runner: ^2.15.0` to expand `part
    // 'models.builder.dart';`. What must stay absent is riverpod's own
    // *pinned* range, which only exists because of the `riverpod_generator`/
    // `dart_mappable_builder` analyzer-version wall this preset doesn't have.
    assert!(
        !pubspec.contains(r#"build_runner: ">=2.14.0 <2.15.2""#),
        "the default preset must not pick up riverpod's pinned build_runner range:\n{pubspec}"
    );
    assert!(
        pubspec.contains("build_runner: ^2.15.0"),
        "the default preset must still gain its own build_runner dependency for \
         cratestack_builder (issue #668 phase 2):\n{pubspec}"
    );
    assert!(pubspec.contains("flutter_riverpod: ^3.3.1"), "{pubspec}");
}

#[test]
fn override_proof_test_file_watches_the_existing_adapter_provider_and_the_new_list_provider() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Riverpod);
    let test_file = package_file(&package, "test/tiny_rpc_client_test.dart");

    assert!(test_file.contains("tinyRpcClientAdapterProvider.overrideWithValue(fakeAdapter)"));
    // Issue #331: `widgetListProvider` now always takes an optional
    // `input`, so `riverpod_generator` emits it as a family — even the
    // zero-argument default has to be called (`widgetListProvider()`),
    // not read bare.
    assert!(test_file.contains("container.read(widgetListProvider().future)"));
    assert!(test_file.contains("class _FakeRpcAdapter implements CratestackRpcAdapter"));
}

// ---- Issue #325: `dart_mappable` adoption. ----

#[test]
fn every_riverpod_data_class_gets_mappable_class_and_mixin() {
    let package = generate(
        "ci_rpc",
        "dart_verify_riverpod_ci_rpc",
        DartPreset::Riverpod,
    );

    // A model class (`Post`), its `Create`/`Update` inputs, a shared
    // `type` (`PostStatusFilter`, owned by `Owner::Shared` since both
    // the procedure and no single model reach it exclusively), and a
    // procedure argument wrapper (`SearchPostsArgs`) — every shape
    // `build_data_class` produces — must all carry the annotation and
    // the generated mixin, not just models. Issue #668 phase 2:
    // `@CratestackBuilder(...)` now sits between `@MappableClass(...)` and
    // the class declaration — `UpdatePostInput` (Patch-kind) gets
    // `listDefaults: false`, every other kind gets the bare form.
    let post = package_file(&package, "lib/src/models/post.dart");
    for (name, cratestack_builder) in [
        ("Post", "@CratestackBuilder()"),
        ("CreatePostInput", "@CratestackBuilder()"),
        ("UpdatePostInput", "@CratestackBuilder(listDefaults: false)"),
    ] {
        assert!(
            post.contains(&format!(
                "@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)\n{cratestack_builder}\nclass {name} with {name}Mappable {{"
            )),
            "{name} should be annotated with @MappableClass()/@CratestackBuilder(...) and carry \
             the generated mixin:\n{post}"
        );
    }

    let procedures = package_file(&package, "lib/src/procedures.dart");
    assert!(
        procedures.contains(
            "@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)\n@CratestackBuilder()\nclass SearchPostsArgs with SearchPostsArgsMappable {"
        ),
        "the searchPosts procedure's generated argument wrapper should be @MappableClass()/\
         @CratestackBuilder()-annotated (this is the exact shape issue #325's bug report \
         reproduced against: a generated class used as a riverpod family provider's argument, \
         e.g. `searchPosts(Ref ref, SearchPostsArgs args)`):\n{procedures}"
    );
    // `PostStatusFilter` is reached only by the `searchPosts` procedure (no
    // model references it), so the partition (`Owner::Procedures`) inlines
    // it into `procedures.dart` rather than `shared_types.dart` — same
    // ownership rule `riverpod_shared_ownership_inlines_procedure_only_types_into_procedures_dart`
    // in `tests/riverpod_generator.rs` exercises, just confirming
    // `@MappableClass()` reaches a procedure-owned nested `type` too, not
    // just the procedure's own top-level args wrapper.
    assert!(
        procedures.contains(
            "@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)\n@CratestackBuilder()\nclass PostStatusFilter with PostStatusFilterMappable {"
        ),
        "a procedure-owned nested `type` must also get @MappableClass()/@CratestackBuilder():\n{procedures}"
    );
}

#[test]
fn dart_mappable_import_and_mapper_part_directive_present_everywhere_data_classes_live() {
    let package = generate(
        "ci_rpc",
        "dart_verify_riverpod_ci_rpc",
        DartPreset::Riverpod,
    );

    // `shared_types.dart` isn't included here — since issue #371's
    // `FindMany<Model>` redesign it always has both the import and the
    // part directive regardless of fixture (see
    // `model_and_procedure_files_carry_the_part_directive`'s comment),
    // so it's covered separately rather than folded into this loop's
    // "present everywhere" assertion;
    // `shared_types_file_gets_the_mapper_part_directive_when_it_has_data_classes`
    // covers it directly with a fixture that also has a real
    // partition-shared `type`.
    for file_name in [
        "lib/src/models/author.dart",
        "lib/src/models/post.dart",
        "lib/src/procedures.dart",
    ] {
        let contents = package_file(&package, file_name);
        assert!(
            contents.contains("import 'package:dart_mappable/dart_mappable.dart';"),
            "{file_name} declares an @MappableClass() but is missing the dart_mappable import:\n{contents}"
        );
    }
}

#[test]
fn default_preset_data_classes_stay_free_of_mappable_class() {
    // Scope guard (issue #325 is `riverpod`-preset-only, per its own
    // "Scope" section): the `default` preset's `models.dart` must never
    // pick up `@MappableClass()`/the generated mixin/the dart_mappable
    // import — `enums_and_data_classes.dart.j2` (riverpod-only) is a
    // completely separate template file from `models.dart.j2` (default),
    // but this guards against the two ever being merged carelessly.
    let package = generate("ci_rpc", "dart_verify_default_ci_rpc", DartPreset::Default);
    let models = package_file(&package, "lib/src/models.dart");

    assert!(!models.contains("@MappableClass"));
    assert!(!models.contains("Mappable {"));
    assert!(!models.contains("dart_mappable"));
}

// ---- Snapshot: the collision fixture's full generated output. ----

#[test]
fn riverpod_collision_snapshot_matches_fixture() {
    let package = generate(
        "riverpod_provider_collision",
        "dart_verify_riverpod_collision",
        DartPreset::Riverpod,
    );
    let snapshot_dir = snapshot_root().join("riverpod_provider_collision");
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

fn write_snapshot(dir: &Path, package: &GeneratedDartPackage) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("snapshot dir should be removable");
    }
    fs::create_dir_all(dir).expect("snapshot dir should be creatable");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("snapshot subdir should be creatable");
        }
        fs::write(&path, file.contents.as_bytes()).expect("snapshot file should write");
    }
}

fn assert_snapshot_matches(dir: &Path, package: &GeneratedDartPackage) {
    assert!(
        dir.exists(),
        "snapshot directory {dir:?} is missing — run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-dart` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing — run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {} — run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}
