//! Rename coverage.
//!
//! Rename is the one request here that *writes*. A wrong navigation result is
//! visible and harmless; a wrong rename rewrites source and is easy to miss in
//! a diff. So most of these tests are about what rename refuses to do.

use std::str::FromStr;

use tower_lsp_server::ls_types::{Position, Uri};

use crate::analyze::analyze_document;
use crate::rename::{prepare_rename, rename_ranges};
use crate::rename_error::RenameError;
use crate::state::{DocumentState, next_document_state};
use crate::text::offset_to_position;

const SCHEMA: &str = r#"mixin Timestamps {
  createdAt DateTime
}

model User {
  id Int @id
  @use(Timestamps)
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#;

fn uri() -> Uri {
    Uri::from_str("file:///schema.cstack").expect("uri should parse")
}

fn document(text: &str) -> DocumentState {
    let (schema, _) = analyze_document(&uri(), text);
    next_document_state(None, text.to_owned(), schema)
}

fn valid() -> DocumentState {
    let state = document(SCHEMA);
    assert!(state.resolved().is_some(), "fixture should parse");
    state
}

/// A document whose current text does not parse, so the retained schema
/// predates the buffer.
fn stale() -> DocumentState {
    let broken = SCHEMA.replace("model User {", "mode User {");
    let (schema, _) = analyze_document(&uri(), &broken);
    assert!(schema.is_none(), "the broken fixture must fail to parse");
    let state = next_document_state(Some(valid()), broken, None);
    assert!(state.is_stale());
    state
}

fn position_of(needle: &str, occurrence: usize) -> Position {
    let mut search_from = 0usize;
    let mut found = 0usize;
    for _ in 0..occurrence {
        found = SCHEMA[search_from..]
            .find(needle)
            .map(|index| search_from + index)
            .expect("needle should exist");
        search_from = found + 1;
    }
    offset_to_position(SCHEMA, found)
}

fn rename(needle: &str, occurrence: usize, new_name: &str) -> Result<Vec<String>, RenameError> {
    rename_ranges(&valid(), position_of(needle, occurrence), new_name).map(|ranges| {
        ranges
            .into_iter()
            .map(|range| format!("{}:{}", range.start.line, range.start.character))
            .collect()
    })
}

#[test]
fn renaming_a_model_rewrites_its_declaration_and_every_type_reference() {
    let edits = rename("User", 1, "Account").expect("rename should succeed");
    assert_eq!(edits, vec!["4:6".to_owned(), "12:9".to_owned()]);
}

#[test]
fn renaming_a_field_rewrites_the_relation_attribute_site() {
    let edits = rename("authorId", 1, "writerId").expect("rename should succeed");
    assert_eq!(edits.len(), 2, "declaration plus the `fields:` entry");
}

#[test]
fn renaming_a_mixin_rewrites_its_use_directive() {
    let edits = rename("Timestamps", 1, "Audited").expect("rename should succeed");
    assert_eq!(edits.len(), 2, "declaration plus `@use(...)`");
}

/// The safety-critical guard. With a stale schema every range is measured
/// against text the buffer no longer holds, so applying the edit rewrites the
/// wrong bytes — silently, and in a file the user is mid-edit on.
#[test]
fn rename_refuses_while_the_schema_is_stale() {
    let stale = stale();
    let result = rename_ranges(&stale, position_of("User", 1), "Account");

    assert!(
        matches!(result, Err(RenameError::StaleSchema)),
        "a stale schema must refuse, not compute edits from outdated spans",
    );
    assert!(
        prepare_rename(&stale, position_of("User", 1)).is_none(),
        "the editor should not even offer a rename box",
    );
}

/// `String`/`Int` resolve as type references but nothing declares them.
/// Renaming one would rewrite every occurrence of a builtin across the file.
#[test]
fn rename_refuses_on_a_builtin_type() {
    assert!(matches!(
        rename("Int", 1, "Integer"),
        Err(RenameError::NotRenameable)
    ));
    assert!(
        prepare_rename(&valid(), position_of("Int", 1)).is_none(),
        "prepareRename must decline so no rename box appears over a builtin",
    );
}

#[test]
fn rename_refuses_a_name_that_is_already_declared() {
    assert!(
        matches!(rename("User", 1, "Post"), Err(RenameError::Conflict(name)) if name == "Post")
    );
}

/// Field names only have to be unique within their owner, so the conflict check
/// has to be scoped the same way — `Post.id` must not block renaming a field on
/// `User`, and must block one on `Post`.
#[test]
fn field_conflicts_are_scoped_to_the_owning_declaration() {
    assert!(
        matches!(rename("authorId", 1, "id"), Err(RenameError::Conflict(_))),
        "Post already has an `id`",
    );
    assert!(
        rename("createdAt", 1, "authorId").is_ok(),
        "`authorId` is declared on Post, not on the Timestamps mixin",
    );
}

#[test]
fn rename_refuses_keywords_and_builtin_type_names() {
    assert!(matches!(
        rename("User", 1, "model"),
        Err(RenameError::Reserved(_))
    ));
    assert!(matches!(
        rename("User", 1, "String"),
        Err(RenameError::Reserved(_))
    ));
    for name in cratestack_parser::reserved_multi_file_keywords() {
        assert!(matches!(
            rename("User", 1, name),
            Err(RenameError::Reserved(_))
        ));
    }
}

#[test]
fn rename_refuses_names_that_are_not_identifiers() {
    for bad in ["9lives", "has space", "has-dash", "", "ünicode"] {
        assert!(
            matches!(
                rename("User", 1, bad),
                Err(RenameError::InvalidIdentifier(_))
            ),
            "`{bad}` should be rejected",
        );
    }
}

/// The range the editor highlights must be one of the ranges the rename
/// rewrites, or the preview lies about what is about to change.
#[test]
fn prepare_rename_reports_a_range_the_rename_will_rewrite() {
    let document = valid();
    let position = position_of("Timestamps", 2); // the `@use(...)` reference
    let prepared = prepare_rename(&document, position).expect("should be renameable");
    let ranges = rename_ranges(&document, position, "Audited").expect("rename should succeed");

    assert!(ranges.contains(&prepared));
}
