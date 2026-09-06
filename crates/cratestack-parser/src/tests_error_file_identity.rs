//! `SchemaError` file identity (cratestack#916).
//!
//! A sibling of `tests_multi_error.rs` (same feature area, distinct concern:
//! that file's point is the *count* and *ordering* of collected errors, not
//! which file each one thinks it came from).
//!
//! Assertions here strip ANSI escapes before matching. ariadne wraps every
//! character of a rendered snippet in its own escape pair, so a plain
//! `contains("identifier")` against a code frame is *always* false — which
//! made an earlier version of these tests pass even with `source_text`
//! hardcoded to the wrong file's text. See `strip_ansi` below.

use crate::parse_schema_diagnostics;

/// Remove ANSI SGR escape sequences so `contains` can see the rendered text.
///
/// Without this, every assertion below is vacuous: ariadne emits each
/// snippet character wrapped in `\x1b[..m` pairs, so the literal identifier
/// never appears as a contiguous substring, and `!rendered.contains("X")`
/// holds no matter which source was resolved.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for escape in chars.by_ref() {
                if escape.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

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
    let plain_a = strip_ansi(&rendered_a);
    let rendered_b = combined[1].render();
    let plain_b = strip_ansi(&rendered_b);

    // Each rendering names its own file and quotes its own offending
    // identifier — proof that rendering resolved each error's *own* source,
    // not one ambient (path, source) pair shared across the whole run.
    assert!(plain_a.contains("a.cstack"), "{plain_a}");
    assert!(plain_a.contains("Rolle"), "{plain_a}");
    assert!(!plain_a.contains("b.cstack"), "{plain_a}");
    assert!(!plain_a.contains("Statuss"), "{plain_a}");

    assert!(plain_b.contains("b.cstack"), "{plain_b}");
    assert!(plain_b.contains("Statuss"), "{plain_b}");
    assert!(!plain_b.contains("a.cstack"), "{plain_b}");
    assert!(!plain_b.contains("Rolle"), "{plain_b}");
}
