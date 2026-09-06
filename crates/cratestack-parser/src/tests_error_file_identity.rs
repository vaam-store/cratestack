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

    // The negative assertions carry the weight here: `Statuss` and `Rolle`
    // each appear in the *other* file's source, so finding one in the wrong
    // report proves an ambient (path, source) pair leaked across errors.
    // The positive identifier checks are satisfied by the error message
    // alone and are not, by themselves, evidence of source resolution.
    assert!(plain_a.contains("a.cstack"), "{plain_a}");
    assert!(plain_a.contains("Rolle"), "{plain_a}");
    assert!(!plain_a.contains("b.cstack"), "{plain_a}");
    assert!(!plain_a.contains("Statuss"), "{plain_a}");

    assert!(plain_b.contains("b.cstack"), "{plain_b}");
    assert!(plain_b.contains("Statuss"), "{plain_b}");
    assert!(!plain_b.contains("a.cstack"), "{plain_b}");
    assert!(!plain_b.contains("Rolle"), "{plain_b}");
}

/// Regression guard for the fix's most losable half.
///
/// `parse_schema_unvalidated` is public and takes no path, so before #916 it
/// was the one route to an error with no file and no source: `render()`
/// returned a bare message with no location and no code frame.
///
/// This test exists because that fix is *silently* reversible. A sibling
/// branch relocates this same function, and a plausible merge resolution
/// drops the `.map_err(..with_file(..))` while still compiling and passing
/// every other test. Without an assertion here, that revert is invisible.
#[test]
fn unvalidated_parse_errors_still_render_a_code_frame() {
    let source = "model Post {\n  id Int @id\n  title Titel\n";
    let error = crate::parse_schema_unvalidated(source)
        .expect_err("an unterminated model block must not parse");

    assert_eq!(
        error.file(),
        crate::ANONYMOUS_SCHEMA,
        "a path-less entry point tags the placeholder, not an empty string"
    );

    let plain = strip_ansi(&error.render());
    assert!(
        plain.contains(crate::ANONYMOUS_SCHEMA),
        "render() must name the file it resolved: {plain}"
    );
    // `model Post {` appears in the code frame and nowhere in the message,
    // so it can only be present if the source was actually attached. That is
    // the property under test — not which line the span happens to point at.
    assert!(
        plain.contains("model Post {"),
        "render() must quote the source — a bare message means the source \
         was never attached: {plain}"
    );
}
