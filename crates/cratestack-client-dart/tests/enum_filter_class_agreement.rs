//! Every `{Enum}Filter` a generated Dart package *references* must be a
//! class that package also *declares*.
//!
//! This is the invariant `enum_filter_class_name`'s purity exists to
//! protect: the declaration site (`build_enum_filter_data_class`) and the
//! reference site (`find_many_views::filter_type_name`) resolve the name
//! independently, so if they are ever fed different inputs the generated
//! package references a class nobody emitted — a `dart analyze` failure
//! that `generate_package` happily returns `Ok` for.
//!
//! A unit test calling the resolver twice with identical arguments CANNOT
//! catch that; it was tried, and feeding one declaration site a
//! partition-local enum subset left all 17 test binaries green while the
//! generated package referenced an undeclared `KindValueFilter`. This test
//! works at the package level instead, which is the only level the bug
//! is visible at.

use std::collections::BTreeSet;

/// A generated filter class name.
///
/// Deliberately `contains`, not `ends_with`: the resolver's collision path
/// yields `KindFilter2`, which ends in a digit. An `ends_with("Filter")`
/// test skips exactly the names a collision produces — which is how the
/// first version of this guard stayed green through a real sabotage.
fn is_filter_class(name: &str) -> bool {
    if name.ends_with("Mappable") {
        return false;
    }
    // `KindFilter` — or `KindFilter2`, the resolver's collision form, which
    // ends in a digit. An `ends_with("Filter")` test skips exactly the names
    // a collision produces, which is how the first version of this guard
    // stayed green through a real sabotage.
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
    stem.ends_with("Filter")
}

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

/// Two enums owned by DIFFERENT models (no procedure, so nothing lands in
/// the shared locus), with a schema-authored `type KindFilter` occupying
/// `Kind`'s base name.
///
/// That combination is what makes the declaration/reference agreement
/// observable. With the full enum set, `Kind` must skip `KindEnumFilter`
/// because it is `KindEnum`'s base, and lands elsewhere. With a
/// partition-local set it does not know `KindEnum` exists, takes
/// `KindEnumFilter`, and collides with the class `KindEnum` declares.
///
/// A fixture whose enums are shared-locus, or whose names resolve the same
/// under both sets, cannot catch this — the first version of this test used
/// one and stayed green through a real sabotage.
const HOSTILE: &str = r#"
enum Kind {
  alpha
  beta
}

enum KindEnum {
  gamma
  delta
}

type KindFilter {
  kinds Kind[]
}

model Alpha {
  id Int @id
  kind Kind
}

model Beta {
  id Int @id
  kindEnum KindEnum
}
"#;

fn generate(preset: DartPreset) -> GeneratedDartPackage {
    let schema = cratestack_parser::parse_schema(HOSTILE).expect("hostile fixture should parse");
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "dart_enum_agreement".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: "0".repeat(64),
            native_cbor: false,
        },
    )
    .unwrap_or_else(|error| panic!("hostile fixture should generate under {preset:?}: {error}"))
}

/// Class names the package declares, and filter type names it references.
fn declared_and_referenced(package: &GeneratedDartPackage) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut declared = BTreeSet::new();
    let mut referenced = BTreeSet::new();

    for file in &package.files {
        for line in file.contents.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("class ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if is_filter_class(&name) {
                    declared.insert(name);
                }
            }
            // `  final KindValueFilter? kindValue;` — a field typed to a filter class.
            if let Some(rest) = trimmed.strip_prefix("final ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if is_filter_class(&name) {
                    referenced.insert(name);
                }
            }
        }
    }
    (declared, referenced)
}

#[test]
fn every_referenced_filter_class_is_declared_in_the_same_package() {
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let package = generate(preset);
        let (declared, referenced) = declared_and_referenced(&package);

        assert!(
            !referenced.is_empty(),
            "{preset:?}: fixture must reference at least one filter class, or this test is vacuous"
        );

        let dangling: Vec<_> = referenced.difference(&declared).collect();
        assert!(
            dangling.is_empty(),
            "{preset:?}: these filter classes are referenced but never declared — the \
             declaration and reference sites disagreed: {dangling:?}\ndeclared: {declared:?}"
        );
    }
}

#[test]
fn no_filter_class_is_declared_twice() {
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let package = generate(preset);
        let mut seen: Vec<String> = Vec::new();
        for file in &package.files {
            for line in file.contents.lines() {
                if let Some(rest) = line.trim().strip_prefix("class ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if is_filter_class(&name) {
                        seen.push(name);
                    }
                }
            }
        }
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            seen.len(),
            unique.len(),
            "{preset:?}: a filter class is declared more than once — dart analyze would \
             reject this: {seen:?}"
        );
    }
}
