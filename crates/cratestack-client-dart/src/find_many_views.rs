//! Per-model `<Model>Where` / `<Model>SortField` view builders — the
//! Dart counterpart to `cratestack-macros`'s `model/find_many_where.rs`/
//! `find_many_order_by.rs` and `cratestack-client-typescript`'s
//! `find_many_views.rs`. The shared filter classes (`StringFilter`/
//! `NumberFilter`/etc.) these reference are hand-written directly in
//! `models.dart.j2`/`riverpod/shared_types.dart.j2`, mirroring `Page`/
//! `PageInfo`/`PageInput` — only the per-model shapes below vary by
//! schema, so only they need real codegen. `<Model>OrderByClause`/
//! `<Model>FindMany` live in the sibling `find_many_order.rs`, and the
//! per-enum `{EnumName}Filter` class an enum-typed filterable field
//! needs (cratestack#928) lives in the sibling `enum_filter_view.rs` —
//! both split out per the repo's 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::Field;
use cratestack_core::Model;

use crate::idents::dart_identifier;
use crate::naming::{is_computed_field, scalar_model_fields};
use crate::views::{DataClassView, EnumVariantView, EnumView, FieldView};

/// Same types `cratestack-macros`'s `find_many_where.rs` (and its
/// TypeScript counterpart) filter generated code down to — `Json`/
/// `Bytes`/custom-`type` fields are excluded, matching the untyped REST
/// `?where=` route's own (`query_scalar_parser_tokens`-proven) coverage.
/// Enum fields (cratestack#928) are included: unlike the TypeScript
/// client, Dart's filter classes are hand-written concrete classes with
/// no shared generic base (`models.dart.j2` has no `EqualityFilter<V>`),
/// so an enum field's filter type is a dynamically-generated
/// `{EnumName}Filter` class — see `crate::enum_filter_view`.
fn is_filterable_scalar(field: &Field, enum_names: &BTreeSet<&str>) -> bool {
    matches!(
        field.ty.name.as_str(),
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Uuid" | "DateTime" | "Decimal"
    ) || enum_names.contains(field.ty.name.as_str())
}

/// The shared filter class this field's operators live on — hardcoded
/// once in `models.dart.j2`/`riverpod/shared_types.dart.j2` for every
/// builtin scalar, or (for an enum field) the per-enum `{EnumName}Filter`
/// class `crate::enum_filter_view::build_enum_filter_data_class`
/// generates alongside the enum itself.
fn filter_type_name(field: &Field, enum_names: &BTreeSet<&str>) -> String {
    if enum_names.contains(field.ty.name.as_str()) {
        return format!("{}Filter", field.ty.name);
    }
    match field.ty.name.as_str() {
        "String" | "Cuid" => "StringFilter",
        "Int" | "Float" => "NumberFilter",
        "Boolean" => "BooleanFilter",
        "Uuid" => "UuidFilter",
        "DateTime" => "DateTimeFilter",
        "Decimal" => "DecimalFilter",
        other => unreachable!("{other} is not a filterable scalar — call site must gate first"),
    }
    .to_owned()
}

/// `None` when the model has no filterable field at all — same
/// omit-rather-than-emit-empty convention `Create<Model>Input` follows
/// when a model disallows create.
pub(crate) fn build_where_data_class(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> Option<DataClassView> {
    let fields = scalar_model_fields(model, model_names)
        .into_iter()
        // `@computed` fields are never filterable — resolved at response
        // time, they never live in a column the server's `?where=` route
        // can query (`docs/design/computed-fields.md`).
        .filter(|field| !is_computed_field(field))
        .filter(|field| is_filterable_scalar(field, enum_names))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }
    let where_name = format!("{}Where", model.name);
    let fields: Vec<FieldView> = fields
        .iter()
        .map(|field| {
            let identifier = dart_identifier(&field.name);
            let filter_type = filter_type_name(field, enum_names);
            FieldView::new(
                identifier.clone(),
                field.name.clone(),
                format!("{filter_type}?"),
                false,
                false,
                false,
                format!(
                    "value['{wire}'] == null ? null : {filter_type}.fromWire(cratestackAsValueMap(value['{wire}']))",
                    wire = field.name
                ),
                format!("{identifier}?.toWire()"),
            )
        })
        .collect();
    Some(DataClassView {
        name: where_name,
        has_fields: true,
        // Never `Patch`-kind, no touch flags, no relation-valued list
        // fields — see `DataClassView::builder_args`'s doc.
        builder_args: String::new(),
        emit_builder: true,
        fields,
    })
}

/// A `PostSortField { id, title, ... }` enum — reuses `EnumView`'s
/// existing template rendering rather than a new template block. Every
/// scalar field is sortable (unlike filtering, ordering has no type
/// restriction — see `cratestack-macros`'s `find_many_order_by.rs` doc).
pub(crate) fn build_sort_field_enum(model: &Model, model_names: &BTreeSet<&str>) -> EnumView {
    EnumView {
        name: format!("{}SortField", model.name),
        variants: scalar_model_fields(model, model_names)
            .into_iter()
            // `@computed` fields are never sortable, same reasoning as
            // `build_where_data_class` above.
            .filter(|field| !is_computed_field(field))
            .map(|field| EnumVariantView {
                identifier: dart_identifier(&field.name),
                wire_name: field.name.clone(),
            })
            .collect(),
    }
}
