//! `--rtk`'s (issue #906) own `package.json.j2` `peerDependencies`/
//! `devDependencies` entries — split out of `crate::package_deps` (which
//! calls straight into [`rtk_peer_dependencies`]/[`rtk_dev_dependencies`])
//! purely to keep that file under this repo's ~200-LoC convention; there
//! is no other reason this couldn't live inline there, the way `--refine`/
//! `--swr`/`--tanstack`'s own entries do.

use crate::config::TypeScriptGeneratorConfig;
use crate::package_deps::DependencyEntry;

const REACT_RANGE: &str = "^18.0.0 || ^19.0.0";

/// `react`/`react-redux`/`@reduxjs/toolkit`, plus `@cratestack/adapter-rtk`
/// when `rtk_adapter_version_requirement` is non-empty (RPC transport
/// only — see `crate::rtk`'s module doc). Empty when `--rtk` is off.
///
/// `react` is omitted here when `config.swr` is also on: `--swr` already
/// pushes an identical `react` entry, and a second entry of the same name
/// would render as a duplicate JSON *key* — valid JSON, but a landmine
/// (a `Record`-typed reader sees only the second; a human reviewing the
/// diff sees two different-looking promises for the same package).
pub(crate) fn rtk_peer_dependencies(
    config: &TypeScriptGeneratorConfig,
    rtk_adapter_version_requirement: &str,
) -> Vec<DependencyEntry> {
    if !config.rtk {
        return Vec::new();
    }
    let mut deps = Vec::new();
    if !config.swr {
        deps.push(DependencyEntry::new("react", REACT_RANGE.to_owned()));
    }
    deps.push(DependencyEntry::new("react-redux", "^9.0.0".to_owned()));
    deps.push(DependencyEntry::new(
        "@reduxjs/toolkit",
        "^2.0.0".to_owned(),
    ));
    if !rtk_adapter_version_requirement.is_empty() {
        deps.push(DependencyEntry::new(
            "@cratestack/adapter-rtk",
            rtk_adapter_version_requirement.to_owned(),
        ));
    }
    deps
}

/// Same entries as [`rtk_peer_dependencies`] plus `@types/react` — the
/// dev-only type declarations a peer dependency never carries, following
/// `--swr`'s existing split between the two lists for the same package.
pub(crate) fn rtk_dev_dependencies(
    config: &TypeScriptGeneratorConfig,
    rtk_adapter_version_requirement: &str,
) -> Vec<DependencyEntry> {
    if !config.rtk {
        return Vec::new();
    }
    let mut deps = Vec::new();
    if !config.swr {
        deps.push(DependencyEntry::new("@types/react", REACT_RANGE.to_owned()));
        deps.push(DependencyEntry::new("react", REACT_RANGE.to_owned()));
    }
    deps.push(DependencyEntry::new("react-redux", "^9.0.0".to_owned()));
    deps.push(DependencyEntry::new(
        "@reduxjs/toolkit",
        "^2.0.0".to_owned(),
    ));
    if !rtk_adapter_version_requirement.is_empty() {
        deps.push(DependencyEntry::new(
            "@cratestack/adapter-rtk",
            rtk_adapter_version_requirement.to_owned(),
        ));
    }
    deps
}
