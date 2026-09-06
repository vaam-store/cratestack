//! Builds `lib/src/models/shared_types.dart` — always emitted (unlike
//! the other per-locus files, it isn't conditional): it carries the
//! `Page`/`PageInfo` wrapper types every `@@paged` model needs (hardcoded
//! directly in `templates/riverpod/shared_types.dart.j2`, mirroring
//! `models.dart.j2`'s own hardcoded copy), plus every nested `type`/
//! `enum` the partition (`crate::riverpod::partition`) assigned to
//! `Owner::Shared` because more than one locus (or zero) reaches it.
use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::builders::{build_data_class, build_enum_view};
use crate::enum_filter_view::build_enum_filter_data_class;
use crate::naming::{enum_name_set, model_name_set};
use crate::riverpod::imports::{model_file_path, owned_type_decl_model_refs, render_import_lines};
use crate::riverpod::partition::{Owner, TypePartition};
use crate::riverpod::views::SharedTypesFileContext;
use crate::views::DataClassKind;

pub(crate) fn build_shared_types_file(
    schema: &Schema,
    partition: &TypePartition,
) -> SharedTypesFileContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);

    let mut data_classes = Vec::new();
    let mut owned_type_decls = Vec::new();
    for ty in &schema.types {
        if *partition.type_owner(&ty.name) != Owner::Shared {
            continue;
        }
        let fields = ty.fields.iter().collect::<Vec<_>>();
        // A `type` block can be genuinely `Owner::Shared` after all — not
        // just via two-or-more-models sharing it (structurally impossible,
        // see `tests/fixtures/riverpod_shared_ownership.cstack`'s doc), but
        // via an ORPHAN `type` referenced by nothing at all: `Owner::
        // owner_by_name` defaults an unreferenced name to `Owner::Shared`
        // (see `tests/fixtures/riverpod_shared_type_orphan.cstack`).
        // `shared_types.dart` gets a builder here like every other
        // `build_data_class` call site — an earlier revision forced
        // `emit_builder` back to `false` on the premise that this file
        // "emits no builders", which origin/main's inline emission
        // disproves (the orphan fixture's baseline `shared_types.dart`
        // does declare `class CoordinatesBuilder`).
        let data_class = build_data_class(
            &ty.name,
            &fields,
            DataClassKind::Plain,
            &enum_names,
            &model_names,
        );
        data_classes.push(data_class);
        owned_type_decls.push(ty);
    }
    let referenced_models = owned_type_decl_model_refs(owned_type_decls, &model_names);

    let shared_enums = schema
        .enums
        .iter()
        .filter(|decl| *partition.enum_owner(&decl.name) == Owner::Shared)
        .collect::<Vec<_>>();
    let enum_types = shared_enums
        .iter()
        .copied()
        .map(build_enum_view)
        .collect::<Vec<_>>();
    // One `{EnumName}Filter` class per shared-owned enum (cratestack#928)
    // — same per-file pairing `build_model.rs` gives a model-owned enum.
    for enum_decl in &shared_enums {
        data_classes.push(build_enum_filter_data_class(enum_decl));
    }

    let mut imports = referenced_models
        .into_iter()
        .map(|other| format!("import '{}';", model_file_path(&other)))
        .collect::<BTreeSet<_>>();
    // Issue #668 phase 2/3: this file's `@CratestackBuilder(...)`
    // annotations (see `enums_and_data_classes.dart.j2`) only exist when
    // `data_classes` is non-empty — the hand-written filter classes the
    // template hardcodes carry `@MappableClass()` but never
    // `@CratestackBuilder()`, so gating on `data_classes` alone (not
    // "this file always has some class in it", the way
    // `build_model.rs`'s unconditional import reasons) is correct here.
    // An unconditional import would be a real `unused_import`
    // `flutter analyze --fatal-warnings` failure on the common case of a
    // schema whose partition assigns nothing to `Owner::Shared` (e.g.
    // `ci_rpc.cstack`) — see `SharedTypesFileContext::
    // builder_part_file_name`'s doc for the paired part-directive concern.
    if !data_classes.is_empty() {
        imports.insert(
            "import 'package:cratestack_annotations/cratestack_annotations.dart';".to_owned(),
        );
    }

    SharedTypesFileContext {
        imports: render_import_lines(imports),
        enum_types,
        data_classes,
        builder_part_file_name: "shared_types.builder.dart".to_owned(),
    }
}
