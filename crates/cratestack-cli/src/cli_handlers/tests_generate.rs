#![cfg(test)]
//! Integration tests for `--check` (drift-detection) mode on
//! `generate-typescript` and `generate-dart`.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::{handle_generate_dart, handle_generate_typescript};
use crate::cli_types::DartPresetArg;

fn write_schema(dir: &TempDir, source: &str) -> PathBuf {
    let path = dir.path().join("schema.cstack");
    fs::write(&path, source).expect("write schema");
    path
}

const INITIAL_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
}
"#;

const EXTENDED_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
  note String?
}
"#;

fn generate_ts(schema: PathBuf, out: PathBuf, check: bool) -> anyhow::Result<()> {
    generate_ts_with_swr(schema, out, check, false)
}

fn generate_ts_with_swr(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    swr: bool,
) -> anyhow::Result<()> {
    generate_ts_with_swr_and_refine(schema, out, check, swr, false)
}

fn generate_ts_with_swr_and_refine(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    swr: bool,
    refine: bool,
) -> anyhow::Result<()> {
    generate_ts_with_swr_refine_and_tanstack(schema, out, check, swr, refine, false)
}

fn generate_ts_with_tanstack(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    tanstack: bool,
) -> anyhow::Result<()> {
    generate_ts_with_swr_refine_and_tanstack(schema, out, check, false, false, tanstack)
}

fn generate_ts_with_rtk(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    rtk: bool,
) -> anyhow::Result<()> {
    generate_ts_with_all_flags(schema, out, check, false, false, false, rtk)
}

fn generate_ts_with_swr_refine_and_tanstack(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    swr: bool,
    refine: bool,
    tanstack: bool,
) -> anyhow::Result<()> {
    generate_ts_with_all_flags(schema, out, check, swr, refine, tanstack, false)
}

#[allow(clippy::too_many_arguments)]
fn generate_ts_with_all_flags(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    swr: bool,
    refine: bool,
    tanstack: bool,
    rtk: bool,
) -> anyhow::Result<()> {
    handle_generate_typescript(
        schema,
        out,
        "cratestack-client".to_owned(),
        "/api".to_owned(),
        None,
        check,
        false,
        swr,
        refine,
        tanstack,
        // Every fixture in this file is REST transport (see
        // INITIAL_SCHEMA/EXTENDED_SCHEMA above) — `native_cbor` has no
        // effect on REST output (issue #746: RPC-only), so `true`
        // (matching the real CLI default) is as good a value as `false`
        // here. `cratestack-client-typescript`'s own
        // `tests/native_cbor_generator.rs` covers the flag's actual
        // behavior on RPC-transport schemas.
        true,
        rtk,
    )
}

fn generate_dart(schema: PathBuf, out: PathBuf, check: bool) -> anyhow::Result<()> {
    generate_dart_with_preset(schema, out, check, DartPresetArg::Default)
}

fn generate_dart_with_preset(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    preset: DartPresetArg,
) -> anyhow::Result<()> {
    handle_generate_dart(
        schema,
        out,
        "cratestack_client".to_owned(),
        "/api".to_owned(),
        None,
        check,
        preset,
        false,
        false,
    )
}

#[test]
fn typescript_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema.clone(), out.clone(), false).expect("initial generate");
    generate_ts(schema, out, true).expect("check should pass on unmodified output");
}

#[test]
fn typescript_check_fails_and_lists_files_after_schema_change() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema_path.clone(), out.clone(), false).expect("initial generate");

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

    let error =
        generate_ts(schema_path, out, true).expect_err("check should fail after schema change");
    assert!(error.to_string().contains("modified: src/models.ts"));
}

#[test]
fn typescript_check_flags_hand_edited_file_with_no_schema_change() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema.clone(), out.clone(), false).expect("initial generate");

    let models_path = out.join("src/models.ts");
    let original = fs::read_to_string(&models_path).unwrap();
    fs::write(&models_path, format!("{original}\n// hand-edited\n")).unwrap();

    let error = generate_ts(schema, out, true).expect_err("hand-edited file should be flagged");
    assert!(error.to_string().contains("modified: src/models.ts"));
}

#[test]
fn typescript_check_does_not_write_files() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema_path.clone(), out.clone(), false).expect("initial generate");
    let before = fs::read_to_string(out.join("src/models.ts")).unwrap();

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();
    let _ = generate_ts(schema_path, out.clone(), true);

    let after = fs::read_to_string(out.join("src/models.ts")).unwrap();
    assert_eq!(
        before, after,
        "--check must not modify the output directory"
    );
}

// Issue #591: `--check` must be `--swr`-aware — the expected file *set*
// grows (additively — `src/swr/models/<model>.ts` etc. alongside, not
// instead of, `src/models.ts`) when `--swr` is on, and neither direction
// should be treated as spurious drift just because the file lists don't
// match each other.

#[test]
fn typescript_swr_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_swr(schema.clone(), out.clone(), false, true).expect("initial swr generate");
    generate_ts_with_swr(schema, out, true, true)
        .expect("check --swr should pass against its own unmodified output");
}

#[test]
fn typescript_check_flags_missing_swr_files_as_real_drift_when_swr_is_added() {
    // Generate without `--swr`, then run `--check --swr` against the same
    // directory: this must fail with `missing` entries for every
    // `src/swr/**` file — not silently pass just because the default
    // layout (still present in both runs — `--swr` is additive, never a
    // replacement) matches. A drift-check that swallowed this gap would
    // defeat the point of `--check` for anyone turning `--swr` on for an
    // existing generated package.
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_swr(schema.clone(), out.clone(), false, false)
        .expect("initial generate without --swr");

    let error = generate_ts_with_swr(schema, out, true, true)
        .expect_err("check --swr against non-swr output should report drift");
    let message = error.to_string();
    assert!(
        message.contains("missing: src/swr/models/account.ts"),
        "swr's per-model file should be reported missing:\n{message}"
    );
    assert!(
        !message.contains("unexpected: src/models.ts"),
        "the default layout's src/models.ts is unaffected by --swr and must not be reported \
         as drift:\n{message}"
    );
}

#[test]
fn typescript_check_flags_extra_swr_files_as_real_drift_when_swr_is_removed() {
    // The reverse direction: generate WITH `--swr`, then check without it
    // — every `src/swr/**` file must be reported `unexpected`, and the
    // default layout must stay clean.
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_swr(schema.clone(), out.clone(), false, true)
        .expect("initial generate with --swr");

    let error = generate_ts_with_swr(schema, out, true, false)
        .expect_err("check without --swr against swr output should report drift");
    let message = error.to_string();
    assert!(
        message.contains("unexpected: src/swr/models/account.ts"),
        "swr's per-model file should be reported unexpected:\n{message}"
    );
}

// Issue #617: `--check` must be `--tanstack`-aware too, same reasoning as
// the `--swr` pair above — `src/react-query.ts` is additive, not a
// replacement, so turning the flag on/off against previously-generated
// output must show up as real `missing`/`unexpected` drift, not be
// swallowed just because the rest of the file set still matches.

#[test]
fn typescript_tanstack_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_tanstack(schema.clone(), out.clone(), false, true)
        .expect("initial --tanstack generate");
    generate_ts_with_tanstack(schema, out, true, true)
        .expect("check --tanstack should pass against its own unmodified output");
}

#[test]
fn typescript_check_flags_missing_react_query_as_real_drift_when_tanstack_is_added() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_tanstack(schema.clone(), out.clone(), false, false)
        .expect("initial generate without --tanstack");

    let error = generate_ts_with_tanstack(schema, out, true, true)
        .expect_err("check --tanstack against non-tanstack output should report drift");
    let message = error.to_string();
    assert!(
        message.contains("missing: src/react-query.ts"),
        "react-query.ts should be reported missing:\n{message}"
    );
    assert!(
        !message.contains("unexpected: src/models.ts"),
        "the default layout's src/models.ts is unaffected by --tanstack and must not be \
         reported as drift:\n{message}"
    );
}

#[test]
fn typescript_check_flags_extra_react_query_as_real_drift_when_tanstack_is_removed() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_tanstack(schema.clone(), out.clone(), false, true)
        .expect("initial generate with --tanstack");

    let error = generate_ts_with_tanstack(schema, out, true, false)
        .expect_err("check without --tanstack against tanstack output should report drift");
    let message = error.to_string();
    assert!(
        message.contains("unexpected: src/react-query.ts"),
        "react-query.ts should be reported unexpected:\n{message}"
    );
}

// Issue #906: `--check` must be `--rtk`-aware too, same reasoning as the
// `--tanstack` pair above — `src/rtk-api.ts` is additive, not a
// replacement.

#[test]
fn typescript_rtk_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_rtk(schema.clone(), out.clone(), false, true).expect("initial --rtk generate");
    generate_ts_with_rtk(schema, out, true, true)
        .expect("check --rtk should pass against its own unmodified output");
}

#[test]
fn typescript_check_flags_missing_rtk_api_as_real_drift_when_rtk_is_added() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_rtk(schema.clone(), out.clone(), false, false)
        .expect("initial generate without --rtk");

    let error = generate_ts_with_rtk(schema, out, true, true)
        .expect_err("check --rtk against non-rtk output should report drift");
    let message = error.to_string();
    assert!(
        message.contains("missing: src/rtk-api.ts"),
        "rtk-api.ts should be reported missing:\n{message}"
    );
}

#[test]
fn dart_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_dart(schema.clone(), out.clone(), false).expect("initial generate");
    generate_dart(schema, out, true).expect("check should pass on unmodified output");
}

#[test]
fn dart_check_fails_after_schema_change() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_dart(schema_path.clone(), out.clone(), false).expect("initial generate");

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

    generate_dart(schema_path, out, true).expect_err("check should fail after schema change");
}

/// Issue #301, acceptance criterion: `--check` must be preset-aware — a
/// riverpod-generated directory checked against `--preset riverpod`
/// reports no drift (the two file *sets* genuinely differ, so a
/// preset-blind check would report spurious drift on every file).
#[test]
fn dart_riverpod_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_dart_with_preset(schema.clone(), out.clone(), false, DartPresetArg::Riverpod)
        .expect("initial riverpod generate");
    generate_dart_with_preset(schema, out, true, DartPresetArg::Riverpod)
        .expect("check should pass on unmodified riverpod output");
}

/// Same schema, generated once under each preset into sibling
/// directories: each preset's own `--check` against its own directory
/// must stay clean — the file sets differ by design, and drift
/// detection must not conflate them.
#[test]
fn dart_check_does_not_conflate_default_and_riverpod_file_sets() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let default_out = dir.path().join("default_client");
    let riverpod_out = dir.path().join("riverpod_client");

    generate_dart(schema.clone(), default_out.clone(), false).expect("default generate");
    generate_dart_with_preset(
        schema.clone(),
        riverpod_out.clone(),
        false,
        DartPresetArg::Riverpod,
    )
    .expect("riverpod generate");

    generate_dart(schema.clone(), default_out, true).expect("default check should stay clean");
    generate_dart_with_preset(schema, riverpod_out, true, DartPresetArg::Riverpod)
        .expect("riverpod check should stay clean");
}

// Issue #303: `--run-build-runner`.
//
// Deliberately does NOT exercise `handle_generate_dart(..., run_build_runner:
// true)` against the real `dart` binary here: `dart run` auto-fetches
// missing dependencies (a real `pub get`), which means a test that
// actually invokes it would depend on network access and could hang or
// flake in a sandboxed/offline CI runner — exactly the kind of
// unreliability a unit-test suite must not have. There is also no clean,
// `unsafe`-free way to force "no Dart SDK" for a specific test by
// mutating `PATH` (this workspace forbids `unsafe_code`, and mutating
// process-wide env vars from a multi-threaded `cargo test` run is racy
// and requires `unsafe` as of the 2024 edition), so `handle_generate_dart`
// itself is intentionally not the seam for this.
//
// `crate::build_runner`'s own unit tests are the seam instead: `program`
// is injectable there, so those tests spawn a guaranteed-missing binary
// name and a guaranteed-nonzero real process (`false`) to prove
// `DartNotFound`/`Failed` actually fire — hermetically, no network, no
// possibility of hanging. What's left to prove here is just the flag's
// plumbing: that `run_build_runner: false` behaves exactly as before
// (already covered by every other `dart_*` test above, none of which
// pass `run_build_runner: true`), and that clap parses the flag at all
// (see `generate_dart_clap_accepts_run_build_runner_flag` in
// `src/main.rs`).
