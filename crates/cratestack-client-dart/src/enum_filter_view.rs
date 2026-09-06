//! Per-enum `{EnumName}Filter` data class (cratestack#928) — an
//! enum-typed filterable field's counterpart to the hand-written
//! `StringFilter`/`NumberFilter`/etc. in `models.dart.j2`. Unlike those,
//! this can't be hardcoded once: the field type (and therefore the
//! `.fromWire`/`.toWire()` calls each operator needs) is a schema-authored
//! enum name, so one concrete class is generated per schema enum,
//! alongside that enum's own `EnumView` (`crate::builders::build_enum_view`).
//!
//! Equality/`in`-shaped only (`eq`/`ne`/`in`/`isNull`) — no `lt`/`gt`/
//! `contains`/`startsWith` — matching the server's own
//! `query_scalar_parser_tokens` enum arm and the TypeScript client's
//! `EqualityFilter<V>` (never `ComparableFilter<V>`): declaration order
//! is not a meaningful ordering to expose. Split out per the repo's
//! 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, TypeArity, TypeRef};

use crate::idents::dart_identifier;
use crate::views::{DataClassView, FieldView};
use crate::wire_decode::decode_value_expr;
use crate::wire_encode::encode_value_expr;

fn synthetic_span() -> cratestack_core::SourceSpan {
    cratestack_core::SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

fn enum_type_ref(enum_name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: enum_name.to_owned(),
        name_span: synthetic_span(),
        arity,
        generic_args: Vec::new(),
        int_args: Vec::new(),
        ident_args: Vec::new(),
    }
}

/// The generated class name for an enum's filter — `{EnumName}Filter`,
/// unless that name is already occupied by a schema-authored `type`/
/// `model`/`enum`/`Create`/`Update<Model>Input` (`crate::naming::
/// occupied_type_names`). A real schema does this: a procedure argument
/// hand-declared as `type PostStatusFilter { statuses PostStatus[] }`
/// for an enum named `PostStatus` collides with the unconditional name
/// verbatim (`tests/fixtures/ci_rpc.cstack`, covered by
/// `tests/riverpod_providers.rs`'s
/// `model_and_procedure_files_carry_the_part_directive`). Falls back to
/// `{EnumName}EnumFilter`, then `{EnumName}ValueFilter` — same two-step
/// fallback shape as `crate::naming::procedure_wrapper_name`, which
/// hits the identical class of collision for `<Procedure>Args`.
mod naming;

pub(crate) use naming::enum_filter_class_name;

pub(crate) fn build_enum_filter_data_class(
    enum_decl: &EnumDecl,
    occupied: &BTreeSet<String>,
    all_enum_names: &BTreeSet<&str>,
) -> DataClassView {
    let filter_name = enum_filter_class_name(&enum_decl.name, occupied, all_enum_names);
    // A single-element set is enough: `decode_value_expr`/`encode_value_expr`
    // only ever consult it to ask "is this exact type name an enum", and
    // every field on this class is typed to `enum_decl` itself.
    let enum_names: BTreeSet<&str> = BTreeSet::from([enum_decl.name.as_str()]);
    let optional_ty = enum_type_ref(&enum_decl.name, TypeArity::Optional);
    let list_ty = enum_type_ref(&enum_decl.name, TypeArity::List);
    let in_identifier = dart_identifier("in");

    let fields = vec![
        FieldView::new(
            "eq".to_owned(),
            "eq".to_owned(),
            format!("{}?", enum_decl.name),
            false,
            false,
            true,
            decode_value_expr(
                "value['eq']",
                &optional_ty,
                &enum_names,
                false,
                &filter_name,
                "eq",
            ),
            encode_value_expr("eq", &optional_ty, &enum_names, false),
        ),
        FieldView::new(
            "ne".to_owned(),
            "ne".to_owned(),
            format!("{}?", enum_decl.name),
            false,
            false,
            true,
            decode_value_expr(
                "value['ne']",
                &optional_ty,
                &enum_names,
                false,
                &filter_name,
                "ne",
            ),
            encode_value_expr("ne", &optional_ty, &enum_names, false),
        ),
        FieldView::new(
            in_identifier.clone(),
            "in".to_owned(),
            format!("List<{}>?", enum_decl.name),
            false,
            false,
            true,
            decode_value_expr(
                "value['in']",
                &list_ty,
                &enum_names,
                true,
                &filter_name,
                "in",
            ),
            encode_value_expr(&in_identifier, &list_ty, &enum_names, true),
        ),
        FieldView::new(
            "isNull".to_owned(),
            "isNull".to_owned(),
            "bool?".to_owned(),
            false,
            false,
            true,
            "value['isNull'] as bool?".to_owned(),
            "isNull".to_owned(),
        ),
    ];

    DataClassView {
        name: filter_name,
        has_fields: true,
        // Never `Patch`-kind, no touch flags, no relation-valued list
        // fields — see `DataClassView::builder_args`'s doc.
        builder_args: String::new(),
        emit_builder: true,
        fields,
    }
}

#[cfg(test)]
mod collision_tests;
