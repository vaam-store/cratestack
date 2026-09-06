//! The npm version requirements a generated TypeScript client declares
//! for CrateStack's own published packages (cratestack#779).
//!
//! # The rule
//!
//! > A generated dependency constraint states an **API compatibility
//! > requirement**. It is never derived from `CARGO_PKG_VERSION`, at any
//! > precision.
//!
//! Established by cratestack#754 and implemented there for the Dart
//! generator's two sites — see
//! `crates/cratestack-client-dart/src/package_floors.rs`, whose module
//! doc carries the full reasoning and is deliberately not restated here.
//! This module is the TypeScript half, closing the remaining two sites
//! #754 scoped out.
//!
//! # Why a constant, and not the `^{major}.{minor}.0` this replaced
//!
//! `native_cbor_version_requirement` used to call a
//! `minor_floor_version_requirement(env!("CARGO_PKG_VERSION"))` helper,
//! introduced by #746 as a partial fix. Its own doc comment stated the
//! residual gap honestly: a minor floor is still *derived from the
//! current version*, so it moves to `^0.9.0` at the 0.9.0 bump and names
//! an unpublished package for that whole window. It narrowed the failure
//! from "every bump" to "every minor bump" rather than closing it.
//!
//! A constant does not move at all, so it can never name an unpublished
//! version, at any bump size. The wider consequence is the one that
//! matters: generator output stops being a function of the release
//! version, which is what makes the committed snapshots and example
//! clients stable across a bump.
//!
//! Note this is a no-op for the emitted string *today*: at workspace
//! version `0.8.14`, `minor_floor_version_requirement` already produced
//! `^0.8.0`. The change is that it will keep producing `^0.8.0` at
//! `0.9.0`, instead of following the bump into a version npm cannot yet
//! serve.
//!
//! # The caret ceiling is deliberate
//!
//! npm resolves `^0.8.0` on a `0.x` version as `>=0.8.0 <0.9.0` — the
//! *second* component is pinned pre-1.0, exactly as pub does. So a
//! generated client resolves the newest `0.8.x` on the day the user runs
//! `npm install`, and after `0.9.0` ships it keeps resolving `0.8.x`
//! until the floor is deliberately raised. That is staleness, not
//! breakage, and raising it is the considered act the rule prescribes.
//!
//! It is also why neither floor below sits in the `0.7.x` line even
//! though both APIs predate `0.8.0`: `^0.7.16` would be `>=0.7.16
//! <0.8.0` and would pin every generated client *off* the current
//! release line entirely.
//!
//! # Keeping these honest
//!
//! #754's receipt applies verbatim: a hand-maintained floor there read
//! `^0.8.8`, a version pub.dev never published, and every offline check
//! available was satisfied by it. So neither constant below is justified
//! from a changelog. Both were verified by unpacking the actual
//! published tarballs off the npm registry and grepping the shipped
//! `dist/*.d.ts` for the exact identifiers the templates reference.
//!
//! Unlike the Dart floors, there is no in-repo declared bound to derive
//! these from — `packages/cratestack-refine` and
//! `packages/cratestack-cbor-*` carry the lockstep workspace version,
//! not a floor, so there is no `cratestack_builder`-style pubspec to
//! read a requirement out of. The guards are therefore:
//!
//! 1. `package_floors_tests.rs` asserts each floor is strictly below the
//!    current, not-yet-published workspace version, and that neither is
//!    `CARGO_PKG_VERSION` at any precision. That is what a well-meaning
//!    "keep it in sync with the bump" change would trip.
//! 2. CI's `js (react-vite-swr example)` job installs a generated client
//!    *at these exact versions* and typechecks it, so a floor that is
//!    too low — or names a version npm cannot serve — fails there rather
//!    than at a user's `npm install`.

/// `@cratestack/refine` — the refine data-provider package a generated
/// client lists under both `peerDependencies` and `devDependencies` when
/// `--refine` is on.
///
/// `0.8.0` is the earliest release in the current `0.8.x` line, and it
/// carries everything `refine.ts.j2` references: the `ResourceMap` and
/// `RpcResourceMap` exported types, and the `primaryKey` / `paged` /
/// `versionField` entry fields the generated `cratestackRefineResources()`
/// populates. Verified against the published tarballs for every `0.7.16`
/// through `0.8.14`, not against the changelog. (`0.7.16` is where that
/// surface actually first appears — `0.7.14` has neither `ResourceMap`
/// nor `RpcResourceMap` — but see the module doc for why the floor does
/// not go there.)
pub(crate) const CRATESTACK_REFINE_FLOOR: &str = "0.8.0";

/// `@cratestack/cbor` — the native RPC codec a generated client lists
/// under `dependencies` when `native_cbor` is on (the default) *and* the
/// schema is `transport rpc`. A REST client never gets this dependency
/// at all; `rest-runtime.ts.j2` has no codec seam.
///
/// **`0.8.15`, not `0.8.0`, and the reason is a wire-format fix rather
/// than an API addition** (cratestack#806).
///
/// The one import `rpc-runtime.ts.j2` makes — `createCborCodec()`
/// returning a `Promise<CratestackRpcCodec>`, which `resolveCodec()`'s
/// memoize-and-retry depends on being a promise — is present in every
/// published tarball back to `0.7.10`. So on *API* grounds this floor
/// would be `^0.8.0`, bounded only by the release line.
///
/// It is higher because API compatibility is not the only thing a floor
/// has to guarantee. Up to and including `0.8.14`, the published codec
/// walked a `Uint8Array` as a plain object, so a `Bytes` field reached
/// the wire as a CBOR **map** that no server-side `Vec<u8>` can decode.
/// cratestack#783/#787 fixed it and `0.8.15` is the first release that
/// carries the fix. Measured against the registry rather than a
/// changelog, which is the standard #754 established:
///
/// ```text
/// npm i @cratestack/cbor@0.8.15
/// encode({ b: new Uint8Array([1, 2, 3] ) })  ->  a1 6162 43 010203
///                                                        ^^ major type 2, byte string
/// npm i @cratestack/cbor@0.8.14
/// encode({ b: new Uint8Array([1, 2, 3] ) })  ->  a1 6162 a3 613001 613102 613203
///                                                        ^^ major type 5, a map — broken
/// ```
///
/// **Why this is worth a floor bump rather than a note.** The defect is
/// invisible at the type level: `Uint8Array` typechecks identically
/// against the broken and fixed codecs, so nothing in a consumer's build
/// catches it. It fails at the wire boundary, at runtime, on a field
/// that looked fine in review. A floor is the only mechanism that
/// prevents it.
///
/// This floor equalled the workspace version while both read `0.8.15`,
/// which needed an explicit `PUBLISHED_EQUAL_FLOORS` exemption in
/// `package_floors_tests`. The 0.9.0 bump ended that special case exactly
/// as predicted: the floor is strictly below the workspace version again,
/// the exemption has been deleted, and the ordinary rule covers it.
pub(crate) const CRATESTACK_CBOR_FLOOR: &str = "0.8.15";

/// `@cratestack/adapter-rtk` — the RTK Query `BaseQueryFn` adapter a
/// generated RPC-transport client lists under both `peerDependencies` and
/// `devDependencies` when `--rtk` is on (issue #906). REST-transport
/// clients never get this dependency: `templates/src/rtk-rest.ts.j2` has
/// no base-query seam to dispatch through — see `crate::rtk`'s module doc.
///
/// `0.8.0` — the earliest release in the current `0.8.x` line, same
/// reasoning as `CRATESTACK_REFINE_FLOOR` above. Verified against the
/// published tarball rather than the changelog (the standard #754/#779
/// set): `createRpcBaseQuery`'s exported signature in `0.8.0`'s
/// `dist/index.d.ts` is byte-for-byte what `templates/src/rtk-rpc.ts.j2`
/// references today — `createRpcBaseQuery(client: RpcCaller):
/// BaseQueryFn<RpcBaseQueryArgs, unknown, RpcBaseQueryError>` — so there is
/// no later API addition this floor needs to reach past.
pub(crate) const CRATESTACK_ADAPTER_RTK_FLOOR: &str = "0.8.0";

/// Pairs a floor above with the ceiling derived from **this crate's own
/// version**, which is the workspace version under lockstep publishing.
///
/// Passing the version in rather than reading `CARGO_PKG_VERSION` inside
/// `release_line` keeps the "which version" decision visible where it is
/// made, and lets the derivation be unit-tested against a hand-written
/// table instead of whatever the workspace happens to be at.
pub(crate) fn requirement(floor: &str) -> String {
    crate::release_line::requirement(floor, env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
#[path = "package_floors_tests.rs"]
mod package_floors_tests;
