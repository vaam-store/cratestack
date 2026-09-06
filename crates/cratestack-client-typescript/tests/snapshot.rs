// Golden-file snapshot tests for the TypeScript generator. Two
// fixtures cover both code paths:
//
//   * tiny_rest.cstack — default transport (REST). Generator emits
//     the fetch-based runtime + REST-shaped API classes.
//   * tiny_rpc.cstack  — `transport rpc`. Generator emits the
//     CratestackRpcRuntime + API classes calling
//     `runtime.call('model.<Name>.<verb>', input)`.
//
// To update the snapshots after intentional changes, run with
// `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-typescript`.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

#[test]
fn rest_snapshot_matches_fixture() {
    run_snapshot("tiny_rest", "tiny-rest-client");
}

#[test]
fn rpc_snapshot_matches_fixture() {
    run_snapshot("tiny_rpc", "tiny-rpc-client");
}

/// Issue #746 review finding #4: the fixture above pins `native_cbor:
/// false` (see `generate_for_with_full_config`'s own doc comment for why
/// — it predates the flag and has its own dedicated coverage in
/// `tests/native_cbor_generator.rs`), which means it no longer represents
/// what `cratestack generate-typescript` actually emits by default. This
/// fixture generates with the REAL `TypeScriptGeneratorConfig::default()`
/// (`native_cbor: true`, via `DEFAULT_NATIVE_CBOR`) so the default a user
/// actually gets — `@cratestack/cbor` wired in as the runtime's codec —
/// has byte-reviewed golden coverage too, not just structural assertions.
#[test]
fn rpc_native_default_snapshot_matches_fixture() {
    let package = generate_default_for("tiny_rpc", "tiny-rpc-native-default-client");
    let snapshot_dir = snapshot_root().join("tiny_rpc_native_default");
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

/// Cratestack#765: `--swr` on an RPC schema renders `src/swr/runtime.ts`
/// from the same shared template as `src/runtime.ts`, through a
/// separately-maintained context (`SwrSchemaContext`) that silently missed
/// `native_cbor` until this ticket. No `--swr` + `transport rpc` snapshot
/// fixture existed before this test — that absence is the direct reason
/// the gap shipped unnoticed (see the issue's own "Verification evidence"
/// section). Mirrors `rpc_native_default_snapshot_matches_fixture` above,
/// generating with the REAL `TypeScriptGeneratorConfig::default()` plus
/// `swr: true` so the fixture tracks whatever a bare `cratestack
/// generate-typescript --swr` invocation on an RPC schema actually emits.
#[test]
fn swr_rpc_native_default_snapshot_matches_fixture() {
    let package = generate_default_for_swr("tiny_rpc", "tiny-rpc-swr-native-default-client");
    let snapshot_dir = snapshot_root().join("tiny_rpc_swr_native_default");
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

#[test]
fn rpc_client_invokes_runtime_call_with_dotted_op_ids() {
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let client = package_file(&package, "src/client.ts");
    // Op ids must match the format the server-side macro emits.
    assert!(
        client.contains("\"model.Widget.list\""),
        "client.ts is missing the `model.Widget.list` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.get\""),
        "client.ts is missing the `model.Widget.get` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.create\""),
        "client.ts is missing the `model.Widget.create` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.update\""),
        "client.ts is missing the `model.Widget.update` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.delete\""),
        "client.ts is missing the `model.Widget.delete` op id:\n{client}"
    );
    assert!(
        client.contains("\"procedure.echoName\""),
        "client.ts is missing the `procedure.echoName` op id:\n{client}"
    );
}

/// Issue #333: `list()` must take the typed `CratestackRpcListQuery`
/// (not a bare `Record<string, unknown>`) and forward it through
/// `toRpcListInput()`, and `queries.ts` — the RPC counterpart of REST's
/// `queries.ts` — must actually be generated and exported from the
/// package root.
#[test]
fn rpc_client_uses_typed_list_query_builder() {
    let package = generate_for("tiny_rpc", "tiny-rpc-client");

    let queries = package_file(&package, "src/queries.ts");
    assert!(
        queries.contains("export interface CratestackRpcListQuery"),
        "queries.ts is missing CratestackRpcListQuery:\n{queries}"
    );
    assert!(
        queries.contains("export function toRpcListInput<TComputedParams = never>("),
        "queries.ts is missing toRpcListInput:\n{queries}"
    );

    let client = package_file(&package, "src/client.ts");
    assert!(
        client.contains(
            "import { toRpcListInput, type CratestackRpcListQuery } from \"./queries.js\";"
        ),
        "client.ts does not import the typed list-query builder:\n{client}"
    );
    assert!(
        !client.contains("list(input: Record<string, unknown>"),
        "client.ts's list() is still typed as a bare Record, not CratestackRpcListQuery:\n{client}"
    );
    assert!(
        client.contains(
            "list(query: CratestackRpcListQuery = {}, options: CratestackRpcCallOptions = {})"
        ),
        "client.ts's list() is not typed as CratestackRpcListQuery:\n{client}"
    );
    assert!(
        client.contains("toRpcListInput(query)"),
        "client.ts's list() does not forward its query through toRpcListInput:\n{client}"
    );

    let index = package_file(&package, "src/index.ts");
    assert!(
        index.contains("export * from \"./queries.js\";"),
        "index.ts does not re-export queries.ts:\n{index}"
    );
}

#[test]
fn rpc_runtime_exports_rpc_error_class() {
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("class CratestackRpcError"),
        "runtime.ts must define CratestackRpcError"
    );
    assert!(
        runtime.contains("RpcErrorCode"),
        "runtime.ts must define the RpcErrorCode union"
    );
    assert!(
        runtime.contains("\"not_found\""),
        "runtime.ts must include the `not_found` RPC code"
    );
    assert!(
        runtime.contains("\"unauthenticated\""),
        "runtime.ts must include the `unauthenticated` RPC code"
    );
}

#[test]
fn rpc_runtime_exposes_pluggable_codec_option() {
    // Regression test for #125: the generated RPC runtime used to
    // hardcode "application/json" as both Content-Type and Accept in
    // call()/batch()/stream(), with no way for a consumer whose backend
    // defaults to CBOR to plug in a different codec.
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("export interface CratestackRpcCodec"),
        "runtime.ts must define a CratestackRpcCodec extension point"
    );
    assert!(
        runtime.contains("export const jsonRpcCodec: CratestackRpcCodec"),
        "runtime.ts must export a default jsonRpcCodec"
    );
    assert!(
        runtime.contains("codec?: CratestackRpcCodec;"),
        "CratestackRpcClientOptions must accept a codec override"
    );
    assert!(
        runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "runtime must default to jsonRpcCodec when no codec option is supplied"
    );
    assert!(
        runtime.contains("headers.set(\"Accept\", codec.contentType);")
            && runtime.contains("headers.set(\"Content-Type\", codec.contentType);"),
        // Issue #746's shared-body collapse (review finding #3): `call()`/
        // `batch()`/`stream()`/`readUnaryResponse()` are now one body
        // shared with the native-codec path, gated only on how the local
        // `codec` binding is obtained (`this.codec` vs `await
        // this.resolveCodec()`) — every use downstream of that reads the
        // local `codec`, never `this.codec` directly.
        "call()/batch()/stream() must derive Accept/Content-Type from the resolved codec"
    );
    assert_eq!(
        runtime.matches("\"application/json\"").count(),
        1,
        "\"application/json\" must appear exactly once — as jsonRpcCodec's own \
         contentType literal — not hardcoded again in call()/batch()/stream():\n{runtime}"
    );
}

#[test]
fn generated_rpc_runtime_satisfies_exact_optional_property_types() {
    // Regression test: the generator's own tsconfig.json.j2 sets
    // exactOptionalPropertyTypes, but the generated runtime didn't
    // actually satisfy it — `this.defaultHeaders = options.headers;`
    // and three `signal: options.signal,` fetch-options fields all
    // failed a real `tsc --noEmit` run under that config (verified
    // manually; this repo has no tsc-in-CI harness, so this test
    // pins the source patterns that were confirmed to fix it).
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains(
            "readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;"
        ),
        "defaultHeaders must explicitly include `| undefined` in its type so assigning a \
         possibly-undefined value type-checks under exactOptionalPropertyTypes:\n{runtime}"
    );
    assert!(
        !runtime.contains("signal: options.signal,"),
        "fetch()'s RequestInit.signal is `AbortSignal | null` (no `| undefined`) — passing \
         `options.signal` directly fails under exactOptionalPropertyTypes; must coalesce to null:\n{runtime}"
    );
    assert_eq!(
        runtime.matches("signal: options.signal ?? null,").count(),
        3,
        "call()/batch()/stream() should all coalesce signal to null:\n{runtime}"
    );
    assert!(
        !runtime.contains("idempotencyKey: options.idempotencyKey,"),
        "RpcLinkRequest.idempotencyKey is optional-without-undefined — assigning \
         `options.idempotencyKey` (string | undefined) directly fails under \
         exactOptionalPropertyTypes; the key must be conditionally spread instead \
         (verified with a real `tsc --noEmit` run, issue #182's link-chain plumbing):\n{runtime}"
    );
    assert!(
        runtime.contains(
            "...(options.idempotencyKey !== undefined ? { idempotencyKey: options.idempotencyKey } : {}),"
        ),
        "call()'s RpcLinkRequest must conditionally spread idempotencyKey:\n{runtime}"
    );
    assert!(
        runtime.contains("export const SCHEMA_SHA256: string ="),
        "SCHEMA_SHA256 must be explicitly widened to `string` — otherwise TS infers the \
         literal type of whatever hash was baked in at generation time, and comparing a \
         non-empty literal against \"\" in buildHeaders() fails to type-check (verified with \
         a real `tsc --noEmit` run against a schema with a real, non-empty schema_sha256 — \
         this snapshot's SNAPSHOT_SCHEMA_SHA256 fixture value happens to be exactly that \
         case):\n{runtime}"
    );
}

#[test]
fn generated_rest_runtime_satisfies_exact_optional_property_types() {
    let package = generate_for("tiny_rest", "tiny-rest-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains(
            "readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;"
        ),
        "defaultHeaders must explicitly include `| undefined`:\n{runtime}"
    );
    assert!(
        runtime.contains("body: body ?? null,")
            && runtime.contains("signal: options.signal ?? null,"),
        "fetch()'s RequestInit.body/signal are typed without `| undefined` — must coalesce to null:\n{runtime}"
    );
    assert!(
        runtime.contains("headers?: HeadersInit | undefined;")
            && runtime.contains("query?: Record<string, unknown> | undefined;")
            && runtime.contains("signal?: AbortSignal | undefined;"),
        "CratestackRequestOptions' fields must explicitly include `| undefined` — otherwise \
         generated client.ts methods that spread `{{ headers: options.headers, ... }}` fail to \
         type-check under exactOptionalPropertyTypes, since the source config types \
         (CratestackRequestConfig etc.) are themselves optional-without-undefined:\n{runtime}"
    );
    assert!(
        runtime.contains("export const SCHEMA_SHA256: string ="),
        "SCHEMA_SHA256 must be explicitly widened to `string` — otherwise TS infers the \
         literal type of whatever hash was baked in at generation time, and comparing a \
         non-empty literal against \"\" in request() fails to type-check (verified with \
         a real `tsc --noEmit` run against a schema with a real, non-empty schema_sha256 — \
         this snapshot's SNAPSHOT_SCHEMA_SHA256 fixture value happens to be exactly that \
         case):\n{runtime}"
    );
}

#[test]
fn runtimes_bind_the_default_fetch_to_globalthis() {
    // Real bug found while running the issue #306 example app for real
    // in a browser (Vite dev, Chrome): `this.fetchFn = options.fetch ??
    // fetch;` stores the *unbound* global `fetch` function, and calling
    // it later as `this.fetchFn(...)` invokes it with `this ===
    // <runtime instance>` instead of the global object. Some browsers'
    // `fetch` is spec'd to throw `TypeError: Illegal invocation` for a
    // wrong receiver — reproduced for real (`useBoards()` failed with
    // exactly that message on first render). Node's `fetch` doesn't
    // enforce this, which is why no existing (Node-only) test caught
    // it. All three fetch-based runtimes must bind to `globalThis`.
    let rest = generate_for("tiny_rest", "tiny-rest-client");
    let rest_runtime = package_file(&rest, "src/runtime.ts");
    assert!(
        rest_runtime.contains("options.fetch ?? fetch.bind(globalThis)"),
        "REST runtime must bind the default fetch to globalThis:\n{rest_runtime}"
    );

    let rpc = generate_for("tiny_rpc", "tiny-rpc-client");
    let rpc_runtime = package_file(&rpc, "src/runtime.ts");
    assert!(
        rpc_runtime.contains("options.fetch ?? fetch.bind(globalThis)"),
        "RPC runtime must bind the default fetch to globalThis:\n{rpc_runtime}"
    );
}

#[test]
fn rest_client_keeps_rest_style_methods() {
    let package = generate_for("tiny_rest", "tiny-rest-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("class CratestackRuntime"),
        "REST runtime must keep the existing CratestackRuntime class"
    );
    let client = package_file(&package, "src/client.ts");
    assert!(
        client.contains("this.runtime.get<"),
        "REST client must keep using runtime.get<...>"
    );
    assert!(
        client.contains("this.runtime.post<"),
        "REST client must keep using runtime.post<...>"
    );
    // The REST client should NOT reference the RPC URL space.
    assert!(
        !client.contains("/rpc/"),
        "REST client should not reference /rpc/ URLs"
    );
}

#[test]
fn full_selection_emits_fully_required_model_interface() {
    // tiny_rest.cstack's Widget has a mix of required (`id`, `name`) and
    // nullable (`weight Int?`) scalars — exactly the mix the flag needs to
    // tell apart from projection-driven optionality.
    let package = generate_for_full_selection("tiny_rest", "tiny-rest-full-client");
    let models = package_file(&package, "src/models.ts");
    assert!(
        models.contains("export interface Widget {\n  id: number;\n  name: string;\n  weight?: number | null;\n}"),
        "--full-selection should require id/name (non-nullable in the schema) and keep weight \
         optional (nullable in the schema), rather than forcing every field optional:\n{models}"
    );
}

#[test]
fn full_selection_does_not_change_default_generation() {
    let default_package = generate_for("tiny_rest", "tiny-rest-client");
    let default_models = package_file(&default_package, "src/models.ts");
    assert!(
        default_models.contains("export interface Widget {\n  id?: number;\n  name?: string;\n  weight?: number | null;\n}"),
        "default (no flag) generation must keep every scalar field optional:\n{default_models}"
    );
}

#[test]
fn full_selection_leaves_create_and_update_inputs_unchanged() {
    // The flag targets read-model interfaces only — Create/Update inputs
    // already derive optionality from schema nullability (Create) or are
    // inherently partial by PATCH semantics (Update), so they must be
    // byte-identical with or without the flag.
    let default_package = generate_for("tiny_rest", "tiny-rest-client");
    let full_package = generate_for_full_selection("tiny_rest", "tiny-rest-full-client");
    let default_models = package_file(&default_package, "src/models.ts");
    let full_models = package_file(&full_package, "src/models.ts");

    for interface in ["CreateWidgetInput", "UpdateWidgetInput", "EchoNameArgs"] {
        let default_block = extract_interface_block(default_models, interface);
        let full_block = extract_interface_block(full_models, interface);
        assert_eq!(
            default_block, full_block,
            "{interface} should be unaffected by --full-selection"
        );
    }
}

#[test]
fn schema_sha256_header_is_baked_into_rest_and_rpc_runtimes() {
    // Issue #178, REST/RPC only. Both runtimes get a `SCHEMA_SHA256`
    // constant and set `x-cratestack-schema-sha` from it whenever it's
    // non-empty — REST via its single `request<T>` method (shared by
    // get/post/patch/delete), RPC via its single `buildHeaders` method
    // (shared by call/batch/stream).
    //
    // Real bug found while building the issue #306 example app: this
    // assertion used to expect the REST runtime's constant WITHOUT the
    // `: string` widening `generated_rpc_runtime_satisfies_exact_optional_property_types`
    // (below) already required and explains for RPC — meaning the REST
    // template (`rest-runtime.ts.j2`) shipped without it, and a real
    // `tsc --noEmit` against any REST package generated with a real,
    // non-empty `schema_sha256` (i.e. every real `cratestack
    // generate-typescript` invocation, both presets — this template is
    // shared) failed with TS2367 on `if (SCHEMA_SHA256 !== "")`: a
    // `const` initializer's inferred literal type can never equal `""`,
    // which TypeScript treats as a comparison error. Fixed in
    // `rest-runtime.ts.j2` to match the RPC template; this assertion now
    // locks in the fix instead of the bug.
    let rest = generate_for("tiny_rest", "tiny-rest-client");
    let rest_runtime = package_file(&rest, "src/runtime.ts");
    assert!(
        rest_runtime.contains(&format!(
            "export const SCHEMA_SHA256: string = \"{SNAPSHOT_SCHEMA_SHA256}\";"
        )),
        "REST runtime must bake the configured schema SHA-256, widened to `string`:\n{rest_runtime}"
    );
    assert!(
        rest_runtime.contains("headers.set(SCHEMA_SHA_HEADER, SCHEMA_SHA256);"),
        "REST runtime's request() must set x-cratestack-schema-sha from SCHEMA_SHA256:\n{rest_runtime}"
    );

    let rpc = generate_for("tiny_rpc", "tiny-rpc-client");
    let rpc_runtime = package_file(&rpc, "src/runtime.ts");
    assert!(
        rpc_runtime.contains(&format!(
            "export const SCHEMA_SHA256: string = \"{SNAPSHOT_SCHEMA_SHA256}\";"
        )),
        "RPC runtime must bake the configured schema SHA-256:\n{rpc_runtime}"
    );
    assert!(
        rpc_runtime.contains("headers.set(SCHEMA_SHA_HEADER, SCHEMA_SHA256);"),
        "RPC runtime's buildHeaders() must set x-cratestack-schema-sha from SCHEMA_SHA256:\n{rpc_runtime}"
    );
}

#[test]
fn empty_schema_sha256_bakes_an_empty_constant_that_omits_the_header_at_runtime() {
    // Empty `TypeScriptGeneratorConfig::schema_sha256` (unsupplied — library-
    // direct usage or a test) must not make the generated client send a
    // blank `x-cratestack-schema-sha`. The generator always emits the same
    // `if (SCHEMA_SHA256 !== "")` guard; here it's the baked value that's
    // empty, so the guard is false at runtime and the header is skipped —
    // same "omit, don't send empty" behavior as the Rust client's
    // `Option<&'static str>`.
    let package = generate_for_with_schema_sha("tiny_rest", "tiny-rest-client", "");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("export const SCHEMA_SHA256: string = \"\";"),
        "an unconfigured schema_sha256 must bake an empty SCHEMA_SHA256 constant:\n{runtime}"
    );
    assert!(
        runtime.contains("if (SCHEMA_SHA256 !== \"\") {"),
        "the header must only be sent when SCHEMA_SHA256 is non-empty:\n{runtime}"
    );
}

#[test]
fn rpc_runtime_supports_composable_links_chain() {
    // Issue #182: `call()`/`batch()` run through an ordered `links` chain
    // terminating in the real fetch. No links declared must reduce to
    // the exact terminal call — proven structurally by the
    // `reduceRight` construction, not just documented. `stream()` used
    // to bypass this chain entirely; issue #277 gave it its own
    // separate `streamChain` instead (see
    // `rpc_runtime_supports_composable_stream_links_chain`) — it no
    // longer touches `this.chain` at all, which this test still pins.
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("links?: RpcLink[];"),
        "CratestackRpcClientOptions must accept a links chain:\n{runtime}"
    );
    assert!(
        runtime.contains(
            "import type { RpcLink, RpcLinkNext, RpcLinkRequest, RpcStreamLink, RpcStreamLinkNext } \
             from \"./links.js\";"
        ),
        "runtime.ts must import the link chain types from ./links:\n{runtime}"
    );
    assert!(
        runtime.contains("this.chain = (options.links ?? []).reduceRight<RpcLinkNext>("),
        "runtime must build the chain via reduceRight so an empty array collapses to the \
         terminal link unchanged:\n{runtime}"
    );
    assert!(
        runtime.contains("await this.chain({")
            && runtime.matches("await this.chain({").count() == 2,
        "call() and batch() (exactly two call sites) must invoke the chain instead of \
         fetching directly:\n{runtime}"
    );
    assert!(
        !runtime[runtime.find("async *stream").expect("stream() must exist")..]
            .contains("this.chain"),
        "stream() must never touch the unary/batch `this.chain` — it has its own \
         `this.streamChain` (issue #277):\n{runtime}"
    );

    let links = package_file(&package, "src/links.ts");
    assert!(
        links.contains("export type RpcLink ="),
        "links.ts must export the RpcLink type:\n{links}"
    );
    assert!(
        links.contains("export function createLoggerLink("),
        "links.ts must ship a reference logger link (issue #182 acceptance criteria):\n{links}"
    );

    let index = package_file(&package, "src/index.ts");
    assert!(
        index.contains("export * from \"./links.js\";"),
        "index.ts must re-export the links module:\n{index}"
    );
}

#[test]
fn rpc_runtime_supports_composable_stream_links_chain() {
    // Issue #277: `stream()` runs through its own `streamLinks` chain —
    // same `reduceRight`/no-op-when-empty contract `links` already has,
    // but frame-shaped (`RpcStreamFrame`) instead of `Response`-shaped,
    // and terminating in a real boundary-scan of `application/cbor-seq`
    // bytes instead of a single `Response` read.
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("streamLinks?: RpcStreamLink[];"),
        "CratestackRpcClientOptions must accept a streamLinks chain:\n{runtime}"
    );
    assert!(
        runtime.contains(
            "this.streamChain = (options.streamLinks ?? []).reduceRight<RpcStreamLinkNext>("
        ),
        "runtime must build the stream chain via reduceRight so an empty array collapses to \
         the terminal stream link unchanged:\n{runtime}"
    );
    assert!(
        runtime.contains("import { terminalStreamLink } from \"./stream-terminal.js\";"),
        "runtime.ts must import the stream chain's terminal link from ./stream-terminal:\n{runtime}"
    );
    let stream_body = &runtime[runtime.find("async *stream").expect("stream() must exist")..];
    assert!(
        stream_body.contains("for await (const frame of this.streamChain({"),
        "stream() must consume this.streamChain instead of fetching directly:\n{runtime}"
    );
    assert!(
        stream_body.contains("throw new CratestackRpcStreamError(frame.error);"),
        "stream() must convert a mid-stream `{{ kind: \"error\" }}` frame into a thrown \
         CratestackRpcStreamError, outside the chain, mirroring how call()/batch() already \
         throw CratestackRpcError outside their chain:\n{runtime}"
    );

    let links = package_file(&package, "src/links.ts");
    assert!(
        links.contains("export type RpcStreamLink ="),
        "links.ts must export the RpcStreamLink type:\n{links}"
    );
    assert!(
        links.contains("export type RpcStreamLinkNext ="),
        "links.ts must export the RpcStreamLinkNext type:\n{links}"
    );
    assert!(
        links.contains("export interface RpcStreamLinkRequest"),
        "links.ts must export the RpcStreamLinkRequest type:\n{links}"
    );
    assert!(
        links.contains("export type RpcStreamFrame<O = unknown> ="),
        "links.ts must export the RpcStreamFrame discriminated union:\n{links}"
    );
    assert!(
        links.contains("export function createLoggerStreamLink("),
        "links.ts must ship a reference stream link mirroring createLoggerLink:\n{links}"
    );

    let stream_terminal = package_file(&package, "src/stream-terminal.ts");
    assert!(
        stream_terminal.contains("export const terminalStreamLink: RpcStreamLinkNext ="),
        "stream-terminal.ts must export the terminal stream link:\n{stream_terminal}"
    );

    let cbor_seq = package_file(&package, "src/cbor-seq.ts");
    assert!(
        cbor_seq.contains("export const RPC_STREAM_ERROR_TAG = 48900;"),
        "cbor-seq.ts must pin the RPC_STREAM_ERROR_TAG constant to the same value as \
         cratestack_core::rpc::RPC_STREAM_ERROR_TAG:\n{cbor_seq}"
    );
    assert!(
        cbor_seq.contains("export class CborSeqBoundaryScanner"),
        "cbor-seq.ts must export the boundary scanner:\n{cbor_seq}"
    );
    assert!(
        cbor_seq.contains("export function classifyCborSeqItem"),
        "cbor-seq.ts must export the error-sentinel classification helper:\n{cbor_seq}"
    );

    let index = package_file(&package, "src/index.ts");
    assert!(
        index.contains("export * from \"./cbor-seq.js\";"),
        "index.ts must re-export the public cbor-seq module:\n{index}"
    );
    assert!(
        !index.contains("export * from \"./cbor-item.js\";"),
        "src/cbor-item.ts is an internal implementation detail (the low-level single-item \
         walk) — it must not be re-exported from index.ts:\n{index}"
    );
}

#[test]
fn rest_and_rpc_share_models_ts() {
    let rest = generate_for("tiny_rest", "tiny-rest-client");
    let rpc = generate_for("tiny_rpc", "tiny-rpc-client");
    let rest_models = package_file(&rest, "src/models.ts");
    let rpc_models = package_file(&rpc, "src/models.ts");
    assert_eq!(
        rest_models, rpc_models,
        "models.ts should be identical across transports"
    );
}

fn run_snapshot(fixture_stem: &str, package_name: &str) {
    let package = generate_for(fixture_stem, package_name);
    let snapshot_dir = snapshot_root().join(fixture_stem);
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

fn generate_for(fixture_stem: &str, package_name: &str) -> GeneratedTypeScriptPackage {
    generate_for_with_config(fixture_stem, package_name, false)
}

fn generate_for_full_selection(
    fixture_stem: &str,
    package_name: &str,
) -> GeneratedTypeScriptPackage {
    generate_for_with_config(fixture_stem, package_name, true)
}

fn generate_for_with_config(
    fixture_stem: &str,
    package_name: &str,
    full_selection: bool,
) -> GeneratedTypeScriptPackage {
    generate_for_with_full_config(
        fixture_stem,
        package_name,
        full_selection,
        SNAPSHOT_SCHEMA_SHA256,
    )
}

/// Like [`generate_for`], but with an explicit `schema_sha256` (issue
/// #178) instead of the shared [`SNAPSHOT_SCHEMA_SHA256`] fixture value —
/// used to exercise the empty/omitted-header branch.
fn generate_for_with_schema_sha(
    fixture_stem: &str,
    package_name: &str,
    schema_sha256: &str,
) -> GeneratedTypeScriptPackage {
    generate_for_with_full_config(fixture_stem, package_name, false, schema_sha256)
}

fn generate_for_with_full_config(
    fixture_stem: &str,
    package_name: &str,
    full_selection: bool,
    schema_sha256: &str,
) -> GeneratedTypeScriptPackage {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            swr: false,
            full_selection,
            refine: false,
            tanstack: false,
            schema_sha256: schema_sha256.to_owned(),
            // Issue #746's flag defaults to `true`, but these golden
            // fixtures (and `rpc_runtime_exposes_pluggable_codec_option`'s
            // own `jsonRpcCodec`-specific assertions below) predate the
            // flag and pin the pure-TypeScript codec path — pinned
            // `false` like every other flag this helper hardcodes rather
            // than reading `TypeScriptGeneratorConfig::default()`. The
            // native-on path has its own dedicated coverage in
            // `tests/native_cbor_generator.rs`.
            native_cbor: false,
            // Issue #906: these golden fixtures predate `--rtk` too —
            // pinned `false` like every other flag this helper hardcodes.
            // The on path has its own dedicated coverage in
            // `tests/rtk_generator.rs`.
            rtk: false,
        },
    )
    .expect("default template should render")
}

/// Generates with the REAL `TypeScriptGeneratorConfig::default()` rather
/// than `generate_for_with_full_config`'s hardcoded flag set — used only
/// by `rpc_native_default_snapshot_matches_fixture` above, so that
/// fixture tracks whatever a bare `cratestack generate-typescript`
/// invocation actually emits (including `native_cbor`, and any future
/// flag whose default changes) instead of a snapshot of one fixed,
/// possibly-stale flag combination.
fn generate_default_for(fixture_stem: &str, package_name: &str) -> GeneratedTypeScriptPackage {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            schema_sha256: SNAPSHOT_SCHEMA_SHA256.to_owned(),
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("default template should render")
}

/// Like [`generate_default_for`], but with `swr: true` added on top of the
/// real `TypeScriptGeneratorConfig::default()` — used only by
/// `swr_rpc_native_default_snapshot_matches_fixture` above, so that
/// fixture tracks the `--swr` layout's actual default output rather than a
/// snapshot of one fixed, possibly-stale flag combination.
fn generate_default_for_swr(fixture_stem: &str, package_name: &str) -> GeneratedTypeScriptPackage {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            schema_sha256: SNAPSHOT_SCHEMA_SHA256.to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("default template should render")
}

/// Fixed, deterministic stand-in for a real SHA-256 hex digest (issue
/// #178) — used everywhere the snapshot fixtures need a `schema_sha256` so
/// the golden files exercise the non-empty/header-sent code path instead
/// of always covering only the empty/omitted branch.
const SNAPSHOT_SCHEMA_SHA256: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write_snapshot(dir: &Path, package: &GeneratedTypeScriptPackage) {
    // Wipe the snapshot tree so deleted files don't linger.
    if dir.exists() {
        fs::remove_dir_all(dir).expect("snapshot dir should be removable");
    }
    fs::create_dir_all(dir).expect("snapshot dir should be creatable");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("snapshot subdir should be creatable");
        }
        fs::write(&path, file.contents.as_bytes()).expect("snapshot file should write");
    }
}

fn assert_snapshot_matches(dir: &Path, package: &GeneratedTypeScriptPackage) {
    assert!(
        dir.exists(),
        "snapshot directory {dir:?} is missing — run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-typescript` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing — run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {} — run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn extract_interface_block<'a>(models: &'a str, interface_name: &str) -> &'a str {
    let start_marker = format!("export interface {interface_name} {{");
    let start = models
        .find(&start_marker)
        .unwrap_or_else(|| panic!("missing interface {interface_name} in:\n{models}"));
    let end = models[start..]
        .find("\n}")
        .map(|offset| start + offset + "\n}".len())
        .unwrap_or_else(|| panic!("unterminated interface {interface_name} in:\n{models}"));
    &models[start..end]
}

fn package_file<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}
