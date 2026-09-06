//! The real-compiler proof for issue #906's acceptance criterion "argument
//! and result types line up with generated model types, proven by a
//! type-level test" — a source-level assertion (`tests/rtk_generator.rs`)
//! cannot tell "the generated `.ts` genuinely type-checks against the real
//! `@reduxjs/toolkit`/`@cratestack/adapter-rtk` packages" from "happens to
//! contain the right substrings". This does a real `npm install` (from the
//! generated package's OWN manifest — the whole point is that its
//! `peerDependencies`/`devDependencies` are sufficient on their own) and a
//! real `npm run build` (`tsc -p tsconfig.json`).
//!
//! Follows this crate's established Node-availability skip convention
//! (`tests/tanstack_absent_typechecks.rs`, `tests/swr_paged_model_tsc.rs`):
//! degrades to a printed skip where `node`/`npm` are absent — a *local*
//! Rust-only checkout, not CI, where `ubuntu-latest` ships Node.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

/// A schema with a real `mutation procedure` referencing a model, so the
/// generated `src/rtk-api.ts` exercises both the five model CRUD
/// endpoints AND a procedure endpoint whose `invalidatesTags` is
/// schema-derived — the exact surface a type error would most plausibly
/// hide in (an untyped `any` silently swallowing a real mismatch).
const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Widget {
  id Int @id
  name String
  weight Int?
  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}

mutation procedure retireWidget(widget: Widget): Widget
  @allow(auth() != null)
"#;

#[test]
fn rest_rtk_api_installs_and_typechecks_for_real() {
    run_for_transport(false, "rtk-typecheck-rest");
}

#[test]
fn rpc_rtk_api_installs_and_typechecks_for_real() {
    run_for_transport(true, "rtk-typecheck-rpc");
}

fn run_for_transport(is_rpc: bool, package_name: &str) {
    if !node_npm_available() {
        eprintln!(
            "skipping {package_name}: `node`/`npm` not on PATH (expected only where Node is \
             absent, e.g. a local Rust-only checkout; CI runs this — see this test's module doc)"
        );
        return;
    }

    let source = if is_rpc {
        SCHEMA.replace("datasource db {", "transport rpc\n\ndatasource db {")
    } else {
        SCHEMA.to_owned()
    };
    let schema = cratestack_parser::parse_schema(&source).expect("fixture schema should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            rtk: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--rtk should render for this fixture");

    assert!(
        package
            .files
            .iter()
            .any(|f| f.file_name == "src/rtk-api.ts"),
        "src/rtk-api.ts should be part of the --rtk file set"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    // Real install, straight from the generated `package.json` — no
    // `--no-save` extra-package injection: the whole point is that this
    // package's OWN manifest (peerDependencies + devDependencies) is
    // sufficient for `tsc` to resolve every type this file references,
    // including `@cratestack/adapter-rtk`'s real published `.d.ts` on RPC.
    let install = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path())
        .output()
        .expect("run npm install");
    let mut install_status = install.status;
    let mut install_stdout = install.stdout;
    let mut install_stderr = install.stderr;
    if !install_status.success()
        && String::from_utf8_lossy(&install_stderr).contains("ETARGET")
        && String::from_utf8_lossy(&install_stderr).contains("@cratestack/ts-types@0.11.1")
    {
        // A real, currently-live npm-registry defect this test discovered
        // and verified directly against the registry (not this PR's
        // doing, and not fixable from generator code): the published
        // `@cratestack/adapter-rtk@0.11.1` tarball's OWN `dependencies`
        // pins an EXACT `@cratestack/ts-types@0.11.1` — `npm view
        // @cratestack/ts-types versions` tops out at `0.11.0`; `0.11.1`
        // was never published. `@cratestack/adapter-rtk@0.11.0` (the
        // prior release) correctly depended on `ts-types@0.11.0`, so this
        // is specifically the newest patch's publish, not a longstanding
        // gap. `CRATESTACK_ADAPTER_RTK_FLOOR`'s derived ceiling (`<0.12.0`)
        // resolves the newest matching release, which is this broken one,
        // regardless of what this generator emits — narrowing the ceiling
        // to dodge one broken patch would violate #779's "a constant, not
        // derived from the current version" rule for the same reason a
        // hand-tuned exact pin would.
        //
        // Retries once, pinned to the last known-good `0.11.0` (still
        // inside this generator's own declared `>=0.8.0 <0.12.0` range,
        // so this isn't smuggling in an out-of-range override) via
        // `--no-save` — the same "install an extra package alongside the
        // generated manifest" shape `tests/swr_paged_model_tsc.rs` already
        // uses, not a new pattern. This still proves the real, decisive
        // claim (the generated `.ts` type-checks against a real,
        // published `@cratestack/adapter-rtk`) rather than degrading to a
        // skip that would prove nothing for RPC at all.
        eprintln!(
            "note: the published @cratestack/adapter-rtk@0.11.1 npm package depends on \
             @cratestack/ts-types@0.11.1, which does not exist on the registry (verified via \
             `npm view @cratestack/ts-types versions` — it tops out at 0.11.0). Retrying \
             {package_name}'s install pinned to the last known-good 0.11.0 rather than skipping \
             — this is a pre-existing npm-publish defect in an already-released package, not \
             something a generator change can fix, reported separately rather than silently \
             worked around."
        );
        let retry = Command::new("npm")
            .args([
                "install",
                "--no-audit",
                "--no-fund",
                "--no-save",
                "@cratestack/adapter-rtk@0.11.0",
            ])
            .current_dir(dir.path())
            .output()
            .expect("run npm install retry");
        install_status = retry.status;
        install_stdout = retry.stdout;
        install_stderr = retry.stderr;
    }
    assert!(
        install_status.success(),
        "{package_name}: npm install failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install_stdout),
        String::from_utf8_lossy(&install_stderr)
    );

    let build = Command::new("npm")
        .args(["run", "build"])
        .current_dir(dir.path())
        .output()
        .expect("run npm run build");
    assert!(
        build.status.success(),
        "{package_name}: npm run build (tsc) failed — src/rtk-api.ts's argument/result types \
         must line up with the generated model types and the real @reduxjs/toolkit/\
         @cratestack/adapter-rtk .d.ts files:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
}

fn node_npm_available() -> bool {
    ["node", "npm"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
