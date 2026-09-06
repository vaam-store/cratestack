//! `SchemaError` file identity (cratestack#916).
//!
//! Split out of `tests_multi_error.rs` (same feature area, but a distinct
//! concern: that file's point is the *count* and *ordering* of collected
//! errors, not which file each one thinks it came from) — also keeps
//! `tests_multi_error.rs` under the 200-line ceiling.

use crate::parse_schema_diagnostics;

const THREE_UNKNOWN_TYPES: &str = r#"datasource db {
  provider = "postgresql"
}

model User {
  id Int @id
  role Rolle
}

model Post {
  id Int @id
  status Statuss
}

model Comment {
  id Int @id
  kind Kindd
}
"#;

/// Every error `parse_schema_diagnostics` collects carries the file identity
/// it was parsed with — the point of the issue is that an error no longer
/// only knows this because the caller happens to remember which file it
/// parsed.
#[test]
fn each_collected_error_exposes_the_file_it_came_from() {
    let (_, errors) = parse_schema_diagnostics("t.cstack", THREE_UNKNOWN_TYPES);

    assert_eq!(errors.len(), 3);
    assert!(errors.iter().all(|error| error.file() == "t.cstack"));
}

/// The multi-*file* case #916 exists to make possible (actually parsing
/// several files together is out of scope here — see #918/#920): errors
/// collected from two independent parses, merged into one `Vec`, each still
/// know their own file and render against their own source — not each
/// other's, and not whichever file happened to be parsed most recently.
#[test]
fn errors_from_two_files_in_one_run_keep_their_own_file_and_source() {
    let source_a = "model User {\n  id Int @id\n  role Rolle\n}\n";
    let source_b = "model Post {\n  id Int @id\n  status Statuss\n}\n";

    let (_, errors_a) = parse_schema_diagnostics("a.cstack", source_a);
    let (_, errors_b) = parse_schema_diagnostics("b.cstack", source_b);

    let mut combined = errors_a;
    combined.extend(errors_b);

    assert_eq!(combined.len(), 2, "{combined:?}");
    assert_eq!(combined[0].file(), "a.cstack");
    assert_eq!(combined[1].file(), "b.cstack");

    let rendered_a = combined[0].render();
    let rendered_b = combined[1].render();

    // Each rendering names its own file and quotes its own offending
    // identifier — proof that rendering resolved each error's *own* source,
    // not one ambient (path, source) pair shared across the whole run.
    assert!(rendered_a.contains("a.cstack"), "{rendered_a}");
    assert!(rendered_a.contains("Rolle"), "{rendered_a}");
    assert!(!rendered_a.contains("b.cstack"), "{rendered_a}");
    assert!(!rendered_a.contains("Statuss"), "{rendered_a}");

    assert!(rendered_b.contains("b.cstack"), "{rendered_b}");
    assert!(rendered_b.contains("Statuss"), "{rendered_b}");
    assert!(!rendered_b.contains("a.cstack"), "{rendered_b}");
    assert!(!rendered_b.contains("Rolle"), "{rendered_b}");
}
