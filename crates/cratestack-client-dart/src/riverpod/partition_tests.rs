//! Unit tests for `super::partition_types` — split out from `partition.rs`
//! to respect the workspace's ~200-LoC file convention (see
//! `CLAUDE.md`'s "200-LoC file ceiling").
//!
//! **Schema-realism note:** `crates/cratestack-parser/src/validate/
//! type_names.rs`'s `reject_type_decl_as_model_field_type` means a
//! `type { ... }` block can never be a *stored* model field's storage
//! type — only a scalar, an `enum`, a `@relation` to another `model`, or
//! (since that function grew a `@computed` exemption) a `type` on a
//! `@computed` field. A nested `type` reached only from procedure args/
//! return types can still only ever land in the (single, combined)
//! `Procedures` locus, or be unreferenced entirely — that half of the
//! constraint (`type_reached_transitively_through_another_type_
//! inherits_procedures_ownership`, below) is unchanged. But a `type`
//! reached through `@computed` fields on two different models *is*
//! genuinely multi-owned, the same as an `enum` (an `enum` *can* be a
//! model field, so two different models can each reference the same
//! `enum` directly) — see `partition.rs`'s own doc comment on `Owner`
//! for that case.
use cratestack_parser::parse_schema;

use super::*;

#[test]
fn enum_used_by_exactly_one_model_is_owned_by_that_model() {
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

model User {
  id Int @id
  role Role
}
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(
        *partition.enum_owner("Role"),
        Owner::Model("User".to_owned())
    );
    assert!(
        partition
            .owned_names(&Owner::Model("User".to_owned()))
            .contains("Role")
    );
}

#[test]
fn enum_used_by_two_models_is_shared() {
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

model User {
  id Int @id
  role Role
}

model Post {
  id Int @id
  editorRole Role
}
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(*partition.enum_owner("Role"), Owner::Shared);
    assert!(
        partition
            .shared_refs(&Owner::Model("User".to_owned()))
            .contains("Role")
    );
    assert!(
        partition
            .shared_refs(&Owner::Model("Post".to_owned()))
            .contains("Role")
    );
}

#[test]
fn enum_referenced_by_a_model_and_by_a_procedure_is_shared() {
    // A second, realistic way to reach ">1 locus": one model field plus
    // one procedure arg/return, not two models.
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

model User {
  id Int @id
  role Role
}

procedure resolveRole(userId: Int): Role
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(*partition.enum_owner("Role"), Owner::Shared);
}

#[test]
fn type_used_only_by_a_procedure_is_owned_by_procedures() {
    let schema = parse_schema(
        r#"
type SearchFilter {
  query String
}

model User {
  id Int @id
  name String
}

procedure search(filter: SearchFilter): User[]
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(*partition.type_owner("SearchFilter"), Owner::Procedures);
    assert!(
        partition
            .owned_names(&Owner::Procedures)
            .contains("SearchFilter")
    );
}

#[test]
fn type_reached_transitively_through_another_type_inherits_procedures_ownership() {
    // No field below carries `@computed`, so no model reaches these
    // types — only the procedure does (see this file's module doc for
    // when a model field *can* reach a `type`). This exercises the BFS
    // traversal through a `type`-to-`type`-to-`enum` chain via the
    // `Procedures` locus rather than a model locus.
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

type Grant {
  role Role
}

type Membership {
  grant Grant
}

model User {
  id Int @id
  name String
}

procedure enroll(membership: Membership): User
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(*partition.type_owner("Membership"), Owner::Procedures);
    assert_eq!(*partition.type_owner("Grant"), Owner::Procedures);
    assert_eq!(*partition.enum_owner("Role"), Owner::Procedures);
}

#[test]
fn relation_fields_do_not_leak_into_the_enum_reachability_graph() {
    // A model-to-model relation must never make one model "own" the
    // other's enums — only direct field references count.
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

model User {
  id Int @id
  role Role
  posts Post[] @relation(fields:[id], references:[authorId])
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId], references:[id])
}
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(
        *partition.enum_owner("Role"),
        Owner::Model("User".to_owned())
    );
    assert!(
        !partition
            .owned_names(&Owner::Model("Post".to_owned()))
            .contains("Role")
    );
}

#[test]
fn orphan_type_declared_but_unused_is_shared_not_dropped() {
    let schema = parse_schema(
        r#"
type Unused {
  note String
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("schema should parse");

    let partition = partition_types(&schema);
    assert_eq!(*partition.type_owner("Unused"), Owner::Shared);
}
