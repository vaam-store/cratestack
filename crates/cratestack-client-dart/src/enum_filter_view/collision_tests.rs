//! Collision cases for `enum_filter_class_name`, each confirmed against
//! the pre-fix three-rung ladder (cratestack#928 review).

use super::naming::{BUILTIN_FILTER_CLASSES, enum_filter_class_name};
use std::collections::BTreeSet;

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn enums<'a>(items: &[&'a str]) -> BTreeSet<&'a str> {
    items.iter().copied().collect()
}

/// Case (c) from the #928 review — needs no contrived schema at all.
/// `enum Number` produced a second `class NumberFilter` alongside the
/// one hand-written in `models.dart.j2`.
#[test]
fn an_enum_named_after_a_builtin_filter_does_not_duplicate_it() {
    let chosen = enum_filter_class_name("Number", &names(&["Number"]), &enums(&["Number"]));
    assert_ne!(
        chosen, "NumberFilter",
        "must not collide with the built-in NumberFilter in models.dart.j2"
    );
    assert!(!BUILTIN_FILTER_CLASSES.contains(&chosen.as_str()));
}

/// Case (a) — two enums racing for one fallback. `Kind` fell back to
/// `KindEnumFilter`, which is also `KindEnum`'s base name.
#[test]
fn two_enums_cannot_race_for_the_same_fallback() {
    let occupied = names(&["Kind", "KindEnum", "KindFilter"]);
    let all = enums(&["Kind", "KindEnum"]);
    let kind = enum_filter_class_name("Kind", &occupied, &all);
    let kind_enum = enum_filter_class_name("KindEnum", &occupied, &all);
    assert_ne!(kind, kind_enum, "distinct enums must get distinct classes");
}

/// Case (b) — every fixed rung taken. The old ladder returned the third
/// unconditionally, without checking it.
#[test]
fn every_fixed_rung_taken_still_yields_a_free_name() {
    let occupied = names(&["Kind", "KindFilter", "KindEnumFilter", "KindValueFilter"]);
    let chosen = enum_filter_class_name("Kind", &occupied, &enums(&["Kind"]));
    assert!(
        !occupied.contains(&chosen),
        "resolved {chosen} but it is already taken"
    );
}

/// The resolver must stay pure: the declaration site and the reference
/// site call it independently and must agree.
#[test]
fn resolution_is_deterministic_across_calls() {
    let occupied = names(&["Kind", "KindFilter"]);
    let all = enums(&["Kind", "Other"]);
    assert_eq!(
        enum_filter_class_name("Kind", &occupied, &all),
        enum_filter_class_name("Kind", &occupied, &all)
    );
}
