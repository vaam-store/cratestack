//! `package.json.j2`'s `peerDependencies`/`devDependencies` entry lists.
//!
//! Split out of `context.rs` (issue #617) to keep that file under this
//! repo's ~200-LoC convention as it grew a third optional dependency group
//! (`--tanstack`, alongside `--refine`/`--swr`) — this module is pure list-
//! building with no other context-assembly concerns, so it moves cleanly
//! on its own.
//!
//! Before issue #617, `@tanstack/react-query` was package.json.j2's last
//! *unconditional* peer/dev dependency entry, which gave every optional
//! `{% if refine %}`/`{% if swr %}` block ahead of it a safe place to hang
//! a trailing comma: whatever was on, `@tanstack/react-query` always
//! followed. Gating `--tanstack` too removes that anchor — `peerDependencies`
//! can now have zero, one, two, or three of {refine, swr, tanstack} present,
//! and "does this entry need a trailing comma" depends on which of the
//! *other* optional groups render after it, a combinatorial "join with
//! separator" problem nested `{% if %}` blocks don't solve cleanly. A
//! `{% for %}` loop with `loop.last` in the template solves it generically
//! instead, over the ordered lists this module builds.
//!
//! Issue #746 added [`dependencies_for`], the same loop-over-`{% if %}`
//! rewrite applied to `package.json.j2`'s `dependencies` block: it used to
//! be a hardcoded two-line stanza (`decimal.js` only) because it never had
//! more than one unconditional entry. `@cratestack/cbor` under
//! `native_cbor` (on by default; `--no-native-cbor` opts out — RPC
//! transport only) is the first *conditional* `dependencies` entry, so the
//! same combinatorial trailing-comma problem
//! `peer_dependencies_for`/`dev_dependencies_for` solve applies here too.

use crate::config::TypeScriptGeneratorConfig;
use crate::rtk::deps::{rtk_dev_dependencies, rtk_peer_dependencies};

/// One `"name": "version"` entry in `package.json`'s `peerDependencies` or
/// `devDependencies`.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DependencyEntry {
    name: &'static str,
    version: String,
}

impl DependencyEntry {
    /// `crate::rtk::deps` builds its own entries through this rather than
    /// the struct literal — that literal syntax needs both fields visible
    /// to the caller's module, and this module is the one place that
    /// should decide the entry shape (issue #906 added the first caller
    /// outside this file).
    pub(crate) fn new(name: &'static str, version: String) -> Self {
        Self { name, version }
    }
}

/// `package.json.j2`'s `peerDependencies`, in the same order the object
/// used to render in before issue #617: `--refine`'s two entries, then
/// `--swr`'s two, then `--tanstack`'s one, then `--rtk`'s two-or-three
/// (issue #906). Empty when none of the flags are set — renders a valid
/// empty `"peerDependencies": {}`.
///
/// `rtk_adapter_version_requirement` is only non-empty when `config.rtk`
/// AND the schema is RPC transport (mirrors `native_cbor_version_requirement`'s
/// own transport gate) — see `crate::rtk`'s module doc for why REST never
/// depends on `@cratestack/adapter-rtk`.
pub(crate) fn peer_dependencies_for(
    config: &TypeScriptGeneratorConfig,
    refine_version_requirement: &str,
    rtk_adapter_version_requirement: &str,
) -> Vec<DependencyEntry> {
    let mut deps = Vec::new();
    if config.refine {
        deps.push(DependencyEntry {
            name: "@cratestack/refine",
            version: refine_version_requirement.to_owned(),
        });
        deps.push(DependencyEntry {
            name: "@refinedev/core",
            version: "^5.0.0".to_owned(),
        });
    }
    if config.swr {
        deps.push(DependencyEntry {
            name: "react",
            version: "^18.0.0 || ^19.0.0".to_owned(),
        });
        deps.push(DependencyEntry {
            name: "swr",
            version: "^2.2.0".to_owned(),
        });
    }
    if config.tanstack {
        deps.push(DependencyEntry {
            name: "@tanstack/react-query",
            version: "^5.0.0".to_owned(),
        });
    }
    // `--rtk`'s own entries (issue #906) live in `crate::rtk::deps` rather
    // than inline here — see that module's doc comment for the dependency
    // list and the `react`/`@types/react` de-duplication against `--swr`.
    deps.extend(rtk_peer_dependencies(
        config,
        rtk_adapter_version_requirement,
    ));
    deps
}

/// `package.json.j2`'s `devDependencies` — same flag order as
/// `peer_dependencies_for`, plus the `typescript` entry every generated
/// package needs regardless of flags. `typescript` is appended here
/// (rather than left for the template to render unconditionally after the
/// `{% for %}` loop) so the template has exactly one rendering strategy
/// shared by both objects.
pub(crate) fn dev_dependencies_for(
    config: &TypeScriptGeneratorConfig,
    refine_version_requirement: &str,
    rtk_adapter_version_requirement: &str,
) -> Vec<DependencyEntry> {
    let mut deps = Vec::new();
    if config.refine {
        deps.push(DependencyEntry {
            name: "@cratestack/refine",
            version: refine_version_requirement.to_owned(),
        });
        deps.push(DependencyEntry {
            name: "@refinedev/core",
            version: "^5.0.0".to_owned(),
        });
    }
    if config.swr {
        deps.push(DependencyEntry {
            name: "@types/react",
            version: "^18.0.0 || ^19.0.0".to_owned(),
        });
        deps.push(DependencyEntry {
            name: "react",
            version: "^18.0.0 || ^19.0.0".to_owned(),
        });
        deps.push(DependencyEntry {
            name: "swr",
            version: "^2.2.0".to_owned(),
        });
    }
    if config.tanstack {
        deps.push(DependencyEntry {
            name: "@tanstack/react-query",
            version: "^5.0.0".to_owned(),
        });
    }
    deps.extend(rtk_dev_dependencies(
        config,
        rtk_adapter_version_requirement,
    ));
    deps.push(DependencyEntry {
        name: "typescript",
        version: "^7.0.2".to_owned(),
    });
    deps
}

/// `package.json.j2`'s `dependencies` — `decimal.js` unconditionally (every
/// generated client needs it regardless of flags or transport), plus
/// `@cratestack/cbor` (issue #746) when `native_cbor` is on (the default;
/// `--no-native-cbor` opts out) AND the schema is RPC transport.
/// REST-transport clients never get `@cratestack/cbor` here:
/// `rest-runtime.ts.j2` has no codec seam at all, so the dependency would
/// be dead weight.
pub(crate) fn dependencies_for(
    config: &TypeScriptGeneratorConfig,
    is_rpc_transport: bool,
    native_cbor_version_requirement: &str,
) -> Vec<DependencyEntry> {
    let mut deps = vec![DependencyEntry {
        name: "decimal.js",
        version: "^10.6.0".to_owned(),
    }];
    if config.native_cbor && is_rpc_transport {
        deps.push(DependencyEntry {
            name: "@cratestack/cbor",
            version: native_cbor_version_requirement.to_owned(),
        });
    }
    deps
}
