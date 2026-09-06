//! Name resolution for the generated per-enum Dart filter class.
//!
//! Split from `enum_filter_view.rs` to keep both under the 200-line ceiling:
//! resolving a free class name and constructing the class are separate
//! concerns, and only the former needs the collision reasoning below.

use std::collections::BTreeSet;

/// Filter classes hand-written in `templates/models.dart.j2`. These are not
/// in `occupied_type_names` (nothing in the schema declares them), so
/// without reserving them here `enum Number` alone produces a duplicate
/// `class NumberFilter` — no contrived schema required.
pub(crate) const BUILTIN_FILTER_CLASSES: [&str; 6] = [
    "StringFilter",
    "NumberFilter",
    "BooleanFilter",
    "UuidFilter",
    "DateTimeFilter",
    "DecimalFilter",
];

/// Resolve the generated per-enum filter class name.
///
/// Must be a pure function of its inputs: the *declaration* site
/// (`build_enum_filter_data_class`) and the *reference* site
/// (`find_many_views::filter_type_name`) call it independently and must
/// agree, so it cannot carry allocation state between calls.
///
/// Three collisions the earlier fixed three-step ladder allowed, each
/// confirmed by generating a package and counting `class X` declarations:
///
/// * a built-in filter class — `enum Number` → duplicate `NumberFilter`;
/// * two enums racing for one fallback — `enum Kind` + `enum KindEnum`
///   with a schema-authored `type KindFilter`, both landing on
///   `KindEnumFilter`;
/// * every rung taken — the third was returned unconditionally, unchecked.
///
/// Other enums' base names are reserved (excluding this enum's own, which
/// would make every enum fall back), and the ladder ends in an unbounded
/// numeric suffix rather than a fixed rung.
pub(crate) fn enum_filter_class_name(
    enum_name: &str,
    occupied: &BTreeSet<String>,
    all_enum_names: &BTreeSet<&str>,
) -> String {
    let mut taken: BTreeSet<&str> = occupied.iter().map(String::as_str).collect();
    taken.extend(BUILTIN_FILTER_CLASSES);

    let sibling_bases: Vec<String> = all_enum_names
        .iter()
        .filter(|name| **name != enum_name)
        .map(|name| format!("{name}Filter"))
        .collect();
    taken.extend(sibling_bases.iter().map(String::as_str));

    let base = format!("{enum_name}Filter");
    if !taken.contains(base.as_str()) {
        return base;
    }
    let enum_fallback = format!("{enum_name}EnumFilter");
    if !taken.contains(enum_fallback.as_str()) {
        return enum_fallback;
    }
    let value_fallback = format!("{enum_name}ValueFilter");
    if !taken.contains(value_fallback.as_str()) {
        return value_fallback;
    }
    // Unbounded rather than a fourth fixed rung: a fixed ladder is what
    // allowed the unchecked return this replaces.
    (2u32..)
        .map(|n| format!("{enum_name}Filter{n}"))
        .find(|candidate| !taken.contains(candidate.as_str()))
        .expect("an unbounded suffix sequence always yields a free name")
}
