// Unit tests for `compute_type_ownership` (issue #304's Test
// Expectations: "two-models-share-an-enum and shared-nested-type
// cases"). Split into its own file via `#[path]` to keep `ownership.rs`
// itself under this repo's ~200-LoC convention.

use super::*;

fn schema(source: &str) -> Schema {
    cratestack_parser::parse_schema(source).expect("fixture should parse")
}

#[test]
fn enum_used_by_exactly_one_model_is_model_owned() {
    let schema = schema(
        "enum Role {\n  admin\n  member\n}\n\
         model User {\n  id Int @id\n  role Role\n}\n",
    );
    let ownership = compute_type_ownership(&schema);
    assert_eq!(
        ownership.owner_of("Role"),
        Some(&TypeOwner::Model("User".to_owned()))
    );
    assert_eq!(
        ownership.shared_imports_for_model("User"),
        Vec::<String>::new()
    );
}

#[test]
fn enum_used_by_two_models_is_shared() {
    let schema = schema(
        "enum Status {\n  active\n  archived\n}\n\
         model Project {\n  id Int @id\n  status Status\n}\n\
         model Task {\n  id Int @id\n  status Status\n}\n",
    );
    let ownership = compute_type_ownership(&schema);
    assert_eq!(ownership.owner_of("Status"), Some(&TypeOwner::Shared));
    assert_eq!(
        ownership.shared_imports_for_model("Project"),
        vec!["Status".to_owned()]
    );
    assert_eq!(
        ownership.shared_imports_for_model("Task"),
        vec!["Status".to_owned()]
    );
}

#[test]
fn nested_type_used_by_two_procedures_is_shared() {
    // A *stored* model field can never be typed as a `type` block at
    // all (a hard parse error — `cratestack-parser`'s
    // `validate/type_names.rs`, "cannot use `type Address` as its
    // storage type"); only a `@computed` field is exempt. Neither
    // model below has one, so `Address`'s only entry points here are
    // the two procedures' args — this exercises the procedure-side of
    // sharing a `type` block (`ownership.rs`'s module doc covers the
    // `@computed`-field side of the same thing): two procedures, each
    // paired with a different model's create operation, sharing one
    // `type` for its input shape.
    let schema = schema(
        "type Address {\n  street String\n}\n\
         model Project {\n  id Int @id\n  name String\n}\n\
         model Task {\n  id Int @id\n  name String\n}\n\
         procedure relocateProject(id: Int, address: Address): Project\n\
         procedure relocateTask(id: Int, address: Address): Task\n",
    );
    let ownership = compute_type_ownership(&schema);
    assert_eq!(ownership.owner_of("Address"), Some(&TypeOwner::Shared));
    assert_eq!(
        ownership.shared_imports_for_procedures(),
        vec!["Address".to_owned()]
    );
}

#[test]
fn nested_type_referencing_a_shared_enum_makes_the_enum_shared_too() {
    // `Address` is only ever reached through both procedures, and
    // `Country` is only ever reached through `Address` — but the
    // transitive closure still makes `Country` shared, not
    // single-procedure-owned, because both procedures transitively
    // reach it.
    let schema = schema(
        "enum Country {\n  us\n  fr\n}\n\
         type Address {\n  street String\n  country Country\n}\n\
         model Project {\n  id Int @id\n  name String\n}\n\
         model Task {\n  id Int @id\n  name String\n}\n\
         procedure relocateProject(id: Int, address: Address): Project\n\
         procedure relocateTask(id: Int, address: Address): Task\n",
    );
    let ownership = compute_type_ownership(&schema);
    assert_eq!(ownership.owner_of("Address"), Some(&TypeOwner::Shared));
    assert_eq!(ownership.owner_of("Country"), Some(&TypeOwner::Shared));
}

#[test]
fn type_referenced_only_by_a_procedure_is_procedures_owned() {
    let schema = schema(
        "type Filter {\n  role String\n}\n\
         model User {\n  id Int @id\n  name String\n}\n\
         procedure search(filter: Filter): User[]\n",
    );
    let ownership = compute_type_ownership(&schema);
    assert_eq!(ownership.owner_of("Filter"), Some(&TypeOwner::Procedures));
    assert_eq!(
        ownership.shared_imports_for_procedures(),
        Vec::<String>::new()
    );
}

#[test]
fn type_referenced_by_two_different_procedures_is_shared_not_duplicated() {
    // Both procedures still render into the single `src/procedures.ts`
    // (so there was never a duplicate-definition *file* risk here),
    // but this proves the ownership computation classifies it
    // `Shared` rather than crediting it to whichever procedure was
    // seen first — i.e. this isn't passing by accident on a
    // single-consumer default.
    let schema = schema(
        "type Filter {\n  role String\n}\n\
         model User {\n  id Int @id\n  name String\n}\n\
         procedure searchUsers(filter: Filter): User[]\n\
         procedure countUsers(filter: Filter): Int\n",
    );
    let ownership = compute_type_ownership(&schema);
    assert_eq!(ownership.owner_of("Filter"), Some(&TypeOwner::Shared));
    assert_eq!(
        ownership.shared_imports_for_procedures(),
        vec!["Filter".to_owned()]
    );
}

#[test]
fn unused_declared_type_defaults_to_shared() {
    let schema = schema("enum Unused {\n  a\n  b\n}\nmodel User {\n  id Int @id\n}\n");
    let ownership = compute_type_ownership(&schema);
    assert_eq!(ownership.owner_of("Unused"), Some(&TypeOwner::Shared));
}

#[test]
fn page_wrapped_procedure_return_type_unwraps_to_the_item_type() {
    let schema = schema(
        "enum Role {\n  admin\n  member\n}\n\
         model User {\n  id Int @id\n  role Role\n}\n\
         procedure searchUsers(limit: Int?): Page<User>\n",
    );
    let ownership = compute_type_ownership(&schema);
    // `User` isn't eligible (it's a model, not an enum/type), so this
    // just proves `Page<User>` doesn't blow up unwrap_type_name and
    // doesn't spuriously mark anything shared through it.
    assert_eq!(
        ownership.owner_of("Role"),
        Some(&TypeOwner::Model("User".to_owned()))
    );
}
