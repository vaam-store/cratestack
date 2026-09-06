//! Per-model `<Model>OrderByClause` / `<Model>FindMany` view builders —
//! split out of `find_many_views.rs` per the repo's 200-LoC file
//! convention (that file's own doc comment names this split).

use cratestack_core::Model;

use crate::views::{DataClassView, FieldView};

/// `{ field: PostSortField; direction: SortDirection; }` — a `List` of
/// these on the `FindMany` input preserves multi-key sort order (unlike
/// a field-keyed map, whose iteration order Dart doesn't guarantee to
/// match JSON key insertion order either).
pub(crate) fn build_order_by_clause_data_class(model: &Model) -> DataClassView {
    let order_by_name = format!("{}OrderByClause", model.name);
    let sort_field_name = format!("{}SortField", model.name);
    let fields = vec![
        FieldView::new(
            "field".to_owned(),
            "field".to_owned(),
            sort_field_name.clone(),
            true,
            false,
            false,
            format!(
                "{sort_field_name}.fromWire(cratestackRequireWireValue('{order_by_name}', 'field', value['field']))"
            ),
            "field.toWire()".to_owned(),
        ),
        FieldView::new(
            "direction".to_owned(),
            "direction".to_owned(),
            "SortDirection".to_owned(),
            true,
            false,
            false,
            format!(
                "SortDirection.fromWire(cratestackRequireWireValue('{order_by_name}', 'direction', value['direction']))"
            ),
            "direction.toWire()".to_owned(),
        ),
    ];
    DataClassView {
        name: order_by_name.clone(),
        has_fields: true,
        // Never `Patch`-kind, no touch flags, no relation-valued list
        // fields — see `DataClassView::builder_args`'s doc.
        builder_args: String::new(),
        emit_builder: true,
        fields,
    }
}

/// What a `FindMany<Model>` procedure argument resolves to. `where` is
/// omitted from the class entirely (not just left unset) when the model
/// has no filterable field, matching `has_where`'s caller.
pub(crate) fn build_find_many_data_class(model: &Model, has_where: bool) -> DataClassView {
    let find_many_name = format!("{}FindMany", model.name);
    let where_name = format!("{}Where", model.name);
    let order_by_name = format!("{}OrderByClause", model.name);

    let mut fields = Vec::new();
    if has_where {
        fields.push(FieldView::new(
            "where".to_owned(),
            "where".to_owned(),
            format!("{where_name}?"),
            false,
            false,
            false,
            format!(
                "value['where'] == null ? null : {where_name}.fromWire(cratestackAsValueMap(value['where']))"
            ),
            "where?.toWire()".to_owned(),
        ));
    }
    // `orderBy`'s Dart type is `List<{order_by_name}>?`, structurally
    // identical to any genuine schema list field — but it is
    // framework-synthesized and has no Rust-side counterpart, so it must
    // NOT get issue #661's default-empty-list/`add{Field}` treatment. The
    // old inline template excluded it with an `is_list: false` flag;
    // `package:cratestack_builder` derives list-ness from the emitted Dart
    // (`DartType.isDartCoreList`) and cannot see the distinction, so it is
    // threaded through `nonDefaultingListFields` below instead.
    //
    // Not an acceptable loss, which an earlier revision of this comment
    // claimed: leaving it defaulted changed `<Model>FindMany.orderBy` from
    // `null` to `[]` when unset AND put it on the wire that way, a
    // behaviour break measured at 14/16 against origin/main's 16/16 on an
    // identical parity test.
    fields.push(FieldView::new(
        "orderBy".to_owned(),
        "orderBy".to_owned(),
        format!("List<{order_by_name}>?"),
        false,
        false,
        false,
        format!(
            "value['orderBy'] == null ? null : cratestackAsValueList(value['orderBy']).map((item) => {order_by_name}.fromWire(cratestackAsValueMap(item))).toList(growable: false)"
        ),
        "orderBy?.map((item) => item.toWire()).toList(growable: false)".to_owned(),
    ));

    DataClassView {
        name: find_many_name,
        has_fields: true,
        // Never `Patch`-kind and no touch flags, but `orderBy` is a
        // synthesized list that must keep its `null`-when-unset semantics —
        // see the comment above it. `where` is not list-typed, so it needs
        // nothing here.
        builder_args: "nonDefaultingListFields: {'orderBy'}".to_owned(),
        emit_builder: true,
        fields,
    }
}
