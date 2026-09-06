use std::path::PathBuf;

/// Whether a schema with no `--tanstack`/`tanstack: ...` given at all emits
/// `src/react-query.ts`. **Reserved for @stephane-segning (issue #617's
/// Risks section)**: hard-break to `false` in the next release, or a
/// deprecation window where this stays `true` (with a warning) for one more
/// release first. Currently `false`, matching the issue's stated Expected
/// Behavior. This is the one place to change to flip
/// [`TypeScriptGeneratorConfig::default`]'s `tanstack` value — see that
/// field's doc comment for why the CLI's own `--tanstack` flag needs a
/// separate, larger change (not just this constant) if the decision goes
/// the other way: a plain presence/absence `bool` flag can represent
/// "off unless passed", but representing "on unless explicitly turned off"
/// needs an additional negation flag, which this constant alone can't
/// provide.
pub const DEFAULT_TANSTACK: bool = false;

/// Whether a schema with no `--no-native-cbor`/`native_cbor: ...` given at
/// all makes the generated RPC runtime's default codec `@cratestack/cbor`'s
/// `createCborCodec()` instead of the pure-TypeScript `jsonRpcCodec`.
/// Mirrors `cratestack-client-dart`'s `DEFAULT_NATIVE_CBOR` (issue #563) —
/// see [`TypeScriptGeneratorConfig::native_cbor`]'s doc comment for the
/// current reasoning and the one open platform gap.
pub const DEFAULT_NATIVE_CBOR: bool = true;

/// Whether a schema with no `--rtk`/`rtk: ...` given at all emits
/// `src/rtk-api.ts`. `false` — same posture `DEFAULT_TANSTACK` documents:
/// a brand-new opt-in framework binding defaults off, unlike
/// `DEFAULT_NATIVE_CBOR` (an established default this crate is merely
/// mirroring from Dart). See [`TypeScriptGeneratorConfig::rtk`]'s doc
/// comment for what the flag emits.
pub const DEFAULT_RTK: bool = false;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptGeneratorConfig {
    pub package_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
    /// Issue #591 (originally #304/#305 as `--preset swr`, now a
    /// composable flag rather than a mutually-exclusive preset — see
    /// `crate::swr`'s module doc for the full rationale): additionally
    /// emit the file-per-model + SWR-hooks layout under `src/swr/`,
    /// reachable by consumers as `<package_name>/swr` (and
    /// `<package_name>/swr/models/*`, `/swr/procedures`,
    /// `/swr/procedures.hooks`) via a `package.json` `exports` subpath.
    ///
    /// Purely additive — `false` (the default) leaves every other emitted
    /// file byte-identical to before this flag existed, which is what
    /// `tests/snapshot.rs` pins. The default layout at `src/` is always
    /// emitted regardless of this flag; `swr: true` adds the `src/swr/`
    /// subtree alongside it rather than replacing it, so a consumer who
    /// used to run this generator twice (once per preset, into two
    /// directories/packages) gets both layouts from one run into one
    /// package instead.
    pub swr: bool,
    /// Emit model interfaces with every scalar field required (matching the
    /// schema's own nullability) instead of forcing every field optional to
    /// account for partial `fields`/`include` projection. For consumers that
    /// never do partial selection and always fetch full objects.
    pub full_selection: bool,
    /// Issue #571: additionally emit `src/refine.ts`, the
    /// `@cratestack/refine` resource-manifest factory for this schema (see
    /// `crate::refine`'s module doc for what it contains and why it is
    /// generated rather than hand-written).
    ///
    /// Purely additive — `false` (the default) leaves every other emitted
    /// file byte-identical, which is what `tests/snapshot.rs` pins.
    /// Composes freely with `swr: true` — `src/refine.ts` binds to the
    /// default layout's client class, which is always emitted regardless
    /// of `swr`.
    pub refine: bool,
    /// Issue #617: additionally emit `src/react-query.ts` — TanStack Query
    /// (`useQuery`/`useMutation`) hooks over the default layout's client
    /// class — and re-export it from `src/index.ts`, and declare
    /// `@tanstack/react-query` as a peer + dev dependency in
    /// `package.json`. Before this flag existed, all three were emitted
    /// unconditionally for every schema and every transport (REST and RPC
    /// alike); `--tanstack` finishes the convergence `--swr`
    /// (#589) and `--refine` (#571) already went through, where every
    /// framework-specific binding is an additive opt-in and the core typed
    /// client stays framework-free.
    ///
    /// Purely additive with respect to every OTHER emitted file, which stays
    /// byte-identical regardless of this flag's value — that part is not in
    /// question. What IS a reserved maintainer decision (issue #617's Risks
    /// section, not implementation discretion) is which way this defaults
    /// when unset: see [`DEFAULT_TANSTACK`], the single place that decision
    /// lives.
    pub tanstack: bool,
    /// Hex-encoded SHA-256 of the schema file's raw bytes (issue #178) —
    /// computed once by the CLI (`cli_support::hash_schema_source`, the
    /// same computation `cratestack-macros` does for `include_*_schema!`)
    /// and baked into the generated client as `SCHEMA_SHA256`, sent as
    /// `x-cratestack-schema-sha` on every request so a drifted TypeScript
    /// client shows up as a server-side `tracing::warn!`, not a silent
    /// wire mismatch. Empty string when not supplied (e.g. this crate
    /// used as a library directly, or in tests) — the generated client
    /// simply omits the header in that case.
    pub schema_sha256: String,
    /// Issue #746's seam: use the published `@cratestack/cbor` package
    /// (napi-rs on Node via `@cratestack/cbor-node`, wasm-bindgen in the
    /// browser via `@cratestack/cbor-web`) as the generated RPC runtime's
    /// default codec, instead of the pure-TypeScript `jsonRpcCodec` — see
    /// `templates/src/rpc-runtime.ts.j2` and `templates/package.json.j2`.
    /// **REST-only clients ignore this field entirely** —
    /// `rest-runtime.ts.j2` hardcodes JSON and has no codec seam at all
    /// (a separate, larger feature; not this ticket's scope).
    ///
    /// **The default as of this doc (`DEFAULT_NATIVE_CBOR` is `true`).**
    /// TypeScript was the one client language where a first-party,
    /// published CBOR codec existed (`@cratestack/cbor{,-node,-web}`,
    /// cratestack#285-288) and was never wired into the generator — Dart
    /// has defaulted to its own native codec since cratestack#563.
    ///
    /// **Known platform gap.** `@cratestack/cbor-node` ships prebuilt napi
    /// binaries for exactly seven targets: `x86_64-apple-darwin`,
    /// `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
    /// `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
    /// `aarch64-unknown-linux-musl`, and `x86_64-pc-windows-msvc`. The two
    /// musl targets — Alpine, the default base image for a large share of
    /// Node backend containers — were added by cratestack#850; the
    /// remaining gap is **`win32-arm64`**. There the napi dispatcher fails
    /// with a generic *"Cannot find native binding. npm has a bug related
    /// to optional dependencies…"* error that blames npm rather than
    /// naming the real cause (unsupported platform) — nothing like Dart's
    /// explicit `UnsupportedError` on Linux arm64. `--no-native-cbor`
    /// (`native_cbor: false`) is the escape hatch: it falls back to
    /// `jsonRpcCodec`, which has no native dependency and works
    /// everywhere. Closing the remaining win32-arm64 gap in
    /// `@cratestack/cbor-node`'s own napi target matrix is out of this
    /// ticket's scope (a prerequisite ticket, not a generator change).
    ///
    /// Purely additive with respect to every other emitted file for a
    /// REST-transport schema (byte-identical, no `@cratestack/cbor`
    /// dependency ever appears) and for an RPC-transport schema leaves
    /// every file byte-identical apart from `package.json` (the
    /// dependency) and `src/runtime.ts` (the codec resolution) — pinned by
    /// `tests/native_cbor_generator.rs`.
    ///
    /// `@cratestack/cbor`'s `createCborCodec()` is async on both
    /// platforms (Node's underlying codec is actually synchronous — the
    /// `async` there is pure call-site parity with the browser build,
    /// where WASM instantiation genuinely has no sync equivalent — see
    /// `packages/cratestack-cbor/src/node.ts`'s doc comment) while
    /// `jsonRpcCodec` is a plain synchronous object. The runtime bridges
    /// the two via a lazily-created, cached `Promise<CratestackRpcCodec>`
    /// resolved once and awaited at the top of each already-`async`
    /// method, rather than making the `CratestackRpcRuntime` constructor
    /// itself async — that would break every existing consumer's
    /// construction call (swr/tanstack/refine layers, every example).
    pub native_cbor: bool,
    /// Issue #906 (epic #893's `#897`): additionally emit `src/rtk-api.ts` —
    /// an RTK Query `createApi` endpoint set, built on the existing
    /// `@cratestack/adapter-rtk` `createRpcBaseQuery` primitive for an
    /// RPC-transport schema — and re-export it from `src/index.ts`. See
    /// `crate::rtk`'s module doc for the full shape, and why REST and RPC
    /// necessarily dispatch differently even though both stay "no second
    /// transport implementation" (RPC calls the adapter's base query
    /// directly; REST's `queryFn` still calls this same generated
    /// package's own REST client methods, since no REST equivalent of
    /// `@cratestack/adapter-rtk` exists to dispatch through).
    ///
    /// Purely additive, mirroring `tanstack`/`refine`/`swr`: every other
    /// emitted file is byte-identical with and without it. Composes freely
    /// with every other flag and every transport.
    pub rtk: bool,
}

impl Default for TypeScriptGeneratorConfig {
    fn default() -> Self {
        Self {
            package_name: "cratestack-client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            swr: false,
            full_selection: false,
            refine: false,
            tanstack: DEFAULT_TANSTACK,
            schema_sha256: String::new(),
            native_cbor: DEFAULT_NATIVE_CBOR,
            rtk: DEFAULT_RTK,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTypeScriptFile {
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTypeScriptPackage {
    pub files: Vec<GeneratedTypeScriptFile>,
}
