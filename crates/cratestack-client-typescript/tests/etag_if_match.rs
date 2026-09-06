//! Generated-output coverage for issue #610: the generated TS client's
//! `CratestackRuntime.request()` used to discard the `Response`, which
//! made `ETag`/`If-Match` unreachable even though the generated server
//! requires `If-Match` on PATCH (and, since cratestack#519, DELETE) for
//! any `@version` model.
//!
//! `etag_versioned.cstack`'s `Ledger` model declares `@version` — same
//! shape as `crates/cratestack-client/tests/fixtures/versioned.cstack`,
//! the fixture the *Rust* generated client's own `#493` ETag/If-Match
//! coverage (`crates/cratestack-client/tests/generated_client_versioning.rs`)
//! uses.
//!
//! Two halves, mirroring the issue's own split:
//!   * READ  — `ETag` must be reachable from a `get`/detail response.
//!   * WRITE — `update`/`delete` must accept an optional `ifMatch` and
//!     send it as `If-Match`.
//!
//! `etag_generated_output_round_trips_through_a_real_http_stub_server`
//! at the bottom is the real, Node-driven proof (skips, printed, when
//! `node`/`npm` aren't on `PATH` — same convention as
//! `tests/rest_list_query_wire_format.rs` and `tests/swr_runtime.rs`).
//!
//! Both Node-driven tests run their smoke script through `tests/support`'s
//! pre-resolved `tsx` rather than `npx`; see that module for why
//! (cratestack#738).

use std::io::Write as _;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

#[test]
fn versioned_model_update_and_delete_accept_if_match_and_send_the_header() {
    let package = generate_for("etag_versioned", "etag-versioned-client");
    let client = package_file(&package, "src/client.ts");

    assert!(
        client.contains(
            "update(\n    id: number,\n    input: UpdateLedgerInput,\n    options: CratestackWriteRequestConfig = {},\n  ): Promise<Ledger>"
        ),
        "update() must accept an optional ifMatch via CratestackWriteRequestConfig:\n{client}"
    );
    assert!(
        client.contains("headers: withIfMatchHeader(options.headers, options.ifMatch),"),
        "update()/delete() must translate options.ifMatch into an If-Match header via \
         withIfMatchHeader:\n{client}"
    );
    assert!(
        client.contains(
            "delete(id: number, options: CratestackWriteRequestConfig = {}): Promise<void>"
        ),
        "delete() must also accept an optional ifMatch (DELETE on an @version model requires \
         If-Match since cratestack#519):\n{client}"
    );
    // Both call sites (update + delete) must go through the same helper.
    assert_eq!(
        client
            .matches("headers: withIfMatchHeader(options.headers, options.ifMatch),")
            .count(),
        2,
        "both update() and delete() must merge ifMatch into the If-Match header:\n{client}"
    );

    let queries = package_file(&package, "src/queries.ts");
    assert!(
        queries.contains(
            "export interface CratestackWriteRequestConfig extends CratestackRequestConfig {"
        ),
        "queries.ts must define CratestackWriteRequestConfig with an ifMatch field:\n{queries}"
    );
    assert!(
        queries.contains("ifMatch?: string;"),
        "CratestackWriteRequestConfig.ifMatch must be optional:\n{queries}"
    );
    assert!(
        queries.contains("export function withIfMatchHeader("),
        "queries.ts must export the withIfMatchHeader helper:\n{queries}"
    );
}

#[test]
fn versioned_model_get_response_reaches_the_etag_header() {
    let package = generate_for("etag_versioned", "etag-versioned-client");
    let client = package_file(&package, "src/client.ts");
    let runtime = package_file(&package, "src/runtime.ts");

    assert!(
        client.contains(
            "getWithResponse(\n    id: number,\n    options: CratestackQueryRequestConfig = {},\n  ): Promise<CratestackResponseEnvelope<Ledger>>"
        ),
        "getWithResponse() must exist and return the response alongside the record:\n{client}"
    );
    assert!(
        client.contains("return this.runtime.getWithResponse<unknown>("),
        "getWithResponse() must call through to the runtime's response-preserving method:\n{client}"
    );
    assert!(
        client.contains("response: result.response,"),
        "getWithResponse() must surface the raw Response object (so a caller can read \
         response.headers.get(\"etag\")):\n{client}"
    );

    assert!(
        runtime.contains("export interface CratestackResponseEnvelope<T>"),
        "runtime.ts must export CratestackResponseEnvelope:\n{runtime}"
    );
    assert!(
        runtime.contains("async requestWithResponse<T>("),
        "runtime.ts must define requestWithResponse, the response-preserving primitive:\n{runtime}"
    );
    assert!(
        runtime.contains("getWithResponse<T>("),
        "runtime.ts must expose a getWithResponse convenience method:\n{runtime}"
    );
    // request() must still exist and keep its old (body-only) return shape,
    // so this is additive, not a breaking rename.
    assert!(
        runtime.contains("async request<T>(") && runtime.contains("return value;"),
        "request() must remain a thin wrapper that discards nothing callers already relied \
         on — its return type is unchanged:\n{runtime}"
    );
}

/// The `swr` preset's plain per-model functions (`src/swr/*.ts`) get the
/// same WRITE-side `ifMatch` treatment as the default `client.ts` — the
/// two are separate, hand-maintained templates (issue #591's additive
/// `--swr` file set), so this is not implied by the tests above.
#[test]
fn swr_preset_update_and_delete_functions_also_accept_if_match() {
    let package = generate_for_swr("etag_versioned", "etag-versioned-swr-client");
    let model_file = package_file(&package, "src/swr/models/ledger.ts");

    assert!(
        model_file.contains("options: CratestackWriteRequestConfig = {},"),
        "swr's updateLedger/deleteLedger must accept CratestackWriteRequestConfig:\n{model_file}"
    );
    assert!(
        model_file.contains("headers: withIfMatchHeader(options.headers, options.ifMatch),"),
        "swr's updateLedger/deleteLedger must merge ifMatch into an If-Match header:\n{model_file}"
    );
    assert_eq!(
        model_file
            .matches("headers: withIfMatchHeader(options.headers, options.ifMatch),")
            .count(),
        2,
        "both the update and delete plain functions must apply the fix:\n{model_file}"
    );
}

/// Review remediation (round 2): the WRITE half above landed in
/// `swr/models-rest.ts.j2`, but the READ half (reaching the `ETag`) did
/// not — a `--swr` consumer could send `If-Match` but had no per-model
/// way to *obtain* it. Worse, the naive workaround (call
/// `runtime.getWithResponse()` directly) skips this file's own
/// `reviveWireFields(...)` call, silently handing back an unrevived
/// `Decimal` field — exactly the kind of trap issue #610 itself records
/// as the *original* reason a consumer rejected the generated client.
/// This asserts both: the symbol exists, AND it's wired through the
/// same revival `getLedger` uses — presence alone would pass even with
/// the decimal bug still in place.
#[test]
fn swr_preset_get_with_response_exists_and_revives_decimals() {
    let package = generate_for_swr("etag_versioned", "etag-versioned-swr-client");
    let model_file = package_file(&package, "src/swr/models/ledger.ts");

    assert!(
        model_file.contains(
            "export async function getLedgerWithResponse(\n  runtime: CratestackRuntime,\n  id: number,\n  options: CratestackQueryRequestConfig = {},\n): Promise<CratestackResponseEnvelope<Ledger>>"
        ),
        "swr must expose a per-model getLedgerWithResponse returning CratestackResponseEnvelope:\n{model_file}"
    );
    assert!(
        model_file.contains("return runtime.getWithResponse<unknown>("),
        "getLedgerWithResponse must call through to the runtime's response-preserving method:\n{model_file}"
    );
    assert!(
        model_file.contains("response: result.response,"),
        "getLedgerWithResponse must surface the raw Response object:\n{model_file}"
    );
    // The decisive assertion: getLedgerWithResponse's returned `value`
    // must go through the exact same reviveWireFields(...) call
    // getLedger uses — not the raw, unrevived runtime payload.
    assert!(
        model_file.contains(
            "value: reviveWireFields(result.value, 'Ledger') as Ledger,\n    response: result.response,"
        ),
        "getLedgerWithResponse's value must be decimal-revived exactly like getLedger's is — \
         reaching for runtime.getWithResponse() directly instead would skip reviveWireFields \
         and hand back an unrevived (string) Decimal field:\n{model_file}"
    );

    assert!(
        model_file.contains(
            "import type { CratestackRuntime, CratestackResponseEnvelope } from \"../runtime.js\";"
        ),
        "swr's per-model file must import CratestackResponseEnvelope:\n{model_file}"
    );
}

/// Review remediation (round 2): the shared `README.md.j2` "Optimistic
/// concurrency" section documents `client.<accessor>.getWithResponse` —
/// REST-only, `@version`-only API. It must render for a REST schema
/// with a `@version` model, and must NOT render for an RPC schema (even
/// one with the identical `@version` model) or a REST schema with no
/// versioned model at all — otherwise the docs describe methods that
/// don't exist on the generated output (a real, previously-shipped bug:
/// the committed `examples/react-vite-swr/client` — REST, no `@version`
/// model — had this section before this fix).
#[test]
fn readme_optimistic_concurrency_section_is_gated_on_rest_transport_and_a_versioned_model() {
    let versioned_rest = generate_for("etag_versioned", "etag-versioned-client");
    let readme = package_file(&versioned_rest, "README.md");
    assert!(
        readme.contains("### Optimistic concurrency"),
        "a REST schema with a @version model must document the round trip:\n{readme}"
    );
    assert!(readme.contains("getWithResponse"));
    assert!(readme.contains("ifMatch"));

    let versioned_rpc = generate_for("etag_versioned_rpc", "etag-versioned-rpc-client");
    let rpc_readme = package_file(&versioned_rpc, "README.md");
    assert!(
        !rpc_readme.contains("getWithResponse") && !rpc_readme.contains("ifMatch"),
        "an RPC schema must never document getWithResponse/ifMatch — RPC has no per-route \
         If-Match/ETag concept and rpc-client.ts.j2 doesn't generate either symbol:\n{rpc_readme}"
    );
    assert!(
        !rpc_readme.contains("### Optimistic concurrency"),
        "the whole section must be absent for RPC, not just silently wrong:\n{rpc_readme}"
    );

    let unversioned_rest = generate_for("tiny_rest", "tiny-rest-client");
    let plain_readme = package_file(&unversioned_rest, "README.md");
    assert!(
        !plain_readme.contains("getWithResponse") && !plain_readme.contains("ifMatch"),
        "a REST schema with no @version model has nothing to document here:\n{plain_readme}"
    );
}

/// Real, Node-driven proof of the full round trip this issue is about:
/// GET a versioned record through the generated client, read `ETag` off
/// `getWithResponse`'s `response`, PATCH with that value as `ifMatch`,
/// then DELETE with the *next* ETag as `ifMatch` too — and confirm the
/// raw HTTP requests the generated client actually sent carried the
/// real `If-Match` header with the right value at each step. DELETE is
/// included, not just PATCH, because cratestack#519 made `If-Match`
/// mandatory for DELETE on a `@version` model exactly like PATCH — a
/// test that only covered PATCH would miss a regression there.
#[test]
fn etag_generated_output_round_trips_through_a_real_http_stub_server() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping etag_generated_output_round_trips_through_a_real_http_stub_server: \
             `node`/`npm` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let package = generate_for("etag_versioned", "etag-versioned-client");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    let mut install = std::process::Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed:\n{}",
        command_report(&install, &installed)
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub server");
    let port = listener.local_addr().expect("local addr").port();
    let server = std::thread::spawn(move || run_etag_stub_server(listener));

    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ EtagVersionedClientClient }} from "./src/client";

const client = new EtagVersionedClientClient("http://127.0.0.1:{port}", {{ basePath: "/api" }});

const got = await client.ledgers.getWithResponse(4);
const etag = got.response.headers.get("etag");
if (etag === null) {{
  throw new Error("no etag header reached the caller");
}}

const updated = await client.ledgers.update(
  4,
  {{ balance: 5 }},
  {{ ifMatch: etag }},
);
if (updated.balance !== 5) {{
  throw new Error("update did not round-trip the new balance");
}}

// DELETE too — cratestack#519 requires If-Match on DELETE for a
// @version model exactly like PATCH. The stub server doesn't enforce
// freshness, so reusing the same etag the GET returned is enough to
// prove the header is actually sent with the right value.
await client.ledgers.delete(4, {{ ifMatch: etag }});

console.log("ETAG_IF_MATCH_CHECK_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.ts");
    let output = tsx.output().expect("run tsx");

    // THE STATUS CHECK COMES BEFORE THE JOIN, AND THAT ORDER IS THE WHOLE
    // POINT. The stub server thread is parked in `accept()`; if the smoke
    // script died before issuing its request, nothing ever connects and this
    // `join()` never returns. Asserting afterwards leaves the stderr that says
    // WHY it died sitting unread in `output` while the test hangs.
    // Three CI runs sat exactly like that for over three hours each on
    // 2026-08-24 (main `afdcd9ce`, jobs 97452966528 and 97485030107) before
    // being cancelled by hand, and the trigger is STILL unknown because the
    // message was never printed. Assert first, then join.
    assert!(
        output.status.success(),
        "smoke script failed:\n{}",
        command_report(&tsx, &output)
    );

    let captured = server.join().expect("stub server thread");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ETAG_IF_MATCH_CHECK_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );

    assert!(
        captured.get_request_line.starts_with("GET /api/ledgers/4"),
        "expected a GET on the detail route first: {}",
        captured.get_request_line
    );
    assert!(
        captured
            .patch_request_line
            .starts_with("PATCH /api/ledgers/4"),
        "expected a PATCH on the detail route second: {}",
        captured.patch_request_line
    );
    assert_eq!(
        captured.patch_if_match_header.as_deref(),
        Some("\"7\""),
        "the PATCH request must carry the If-Match header the client learned from the GET's \
         ETag — this is the exact round trip issue #610 says the generated client couldn't do"
    );
    assert!(
        captured
            .delete_request_line
            .starts_with("DELETE /api/ledgers/4"),
        "expected a DELETE on the detail route third: {}",
        captured.delete_request_line
    );
    assert_eq!(
        captured.delete_if_match_header.as_deref(),
        Some("\"7\""),
        "the DELETE request must also carry the If-Match header (cratestack#519: DELETE on a \
         @version model requires If-Match exactly like PATCH) — a stub that only checked PATCH \
         would miss a regression here"
    );
}

/// Real, Node-driven proof for the `--swr` preset specifically (review
/// remediation round 2): `getLedgerWithResponse` must both reach the
/// `ETag` AND hand back a real `Decimal` instance for the `amount`
/// field, not the raw JSON string — proving `reviveWireFields` was
/// actually applied on this path, not skipped the way calling
/// `runtime.getWithResponse()` directly would. Then the learned `ETag`
/// is sent back as `ifMatch` on `updateLedger`, same round trip as the
/// default-preset test above, on the `--swr` plain-function surface
/// this time.
#[test]
fn swr_get_with_response_round_trips_through_a_real_http_stub_server_with_decimal_revival() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping swr_get_with_response_round_trips_through_a_real_http_stub_server_with_decimal_revival: \
             `node`/`npm` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let package = generate_for_swr("etag_versioned", "etag-versioned-swr-client");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    let mut install = std::process::Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed:\n{}",
        command_report(&install, &installed)
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub server");
    let port = listener.local_addr().expect("local addr").port();
    let server = std::thread::spawn(move || run_etag_stub_server(listener));

    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ CratestackRuntime }} from "./src/swr/runtime";
import {{ getLedgerWithResponse, updateLedger, deleteLedger }} from "./src/swr/models/ledger";
import {{ Decimal }} from "./src/swr/models/shared";

const runtime = new CratestackRuntime("http://127.0.0.1:{port}", {{ basePath: "/api" }});

const got = await getLedgerWithResponse(runtime, 4);
const etag = got.response.headers.get("etag");
if (etag === null) {{
  throw new Error("no etag header reached the caller");
}}
if (!(got.value.amount instanceof Decimal)) {{
  throw new Error(
    "getLedgerWithResponse's amount field was not revived into a real Decimal — got: " +
      JSON.stringify(got.value.amount),
  );
}}
if (got.value.amount.toString() !== "12.34") {{
  throw new Error("getLedgerWithResponse's amount field has the wrong value: " + got.value.amount.toString());
}}

const updated = await updateLedger(runtime, 4, {{ balance: 5 }}, {{ ifMatch: etag }});
if (updated.balance !== 5) {{
  throw new Error("updateLedger did not round-trip the new balance");
}}

// Completes the 3-request cycle run_etag_stub_server expects (GET,
// PATCH, DELETE) — same shared stub server the default-preset test
// above uses.
await deleteLedger(runtime, 4, {{ ifMatch: etag }});

console.log("SWR_ETAG_DECIMAL_CHECK_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.ts");
    let output = tsx.output().expect("run tsx");

    // THE STATUS CHECK COMES BEFORE THE JOIN, AND THAT ORDER IS THE WHOLE
    // POINT. The stub server thread is parked in `accept()`; if the smoke
    // script died before issuing its request, nothing ever connects and this
    // `join()` never returns. Asserting afterwards leaves the stderr that says
    // WHY it died sitting unread in `output` while the test hangs.
    // Three CI runs sat exactly like that for over three hours each on
    // 2026-08-24 (main `afdcd9ce`, jobs 97452966528 and 97485030107) before
    // being cancelled by hand, and the trigger is STILL unknown because the
    // message was never printed. Assert first, then join.
    assert!(
        output.status.success(),
        "smoke script failed:\n{}",
        command_report(&tsx, &output)
    );

    let captured = server.join().expect("stub server thread");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SWR_ETAG_DECIMAL_CHECK_OK"),
        "smoke script did not print its success marker (this includes the Decimal-revival \
         assertion failing):\n{}",
        command_report(&tsx, &output)
    );
    assert_eq!(
        captured.patch_if_match_header.as_deref(),
        Some("\"7\""),
        "updateLedger must send the If-Match header learned from getLedgerWithResponse's ETag"
    );
}

struct CapturedRequests {
    get_request_line: String,
    patch_request_line: String,
    patch_if_match_header: Option<String>,
    delete_request_line: String,
    delete_if_match_header: Option<String>,
}

/// Accepts exactly three HTTP connections: a GET (replies with a Ledger
/// body and an `ETag: "7"` header), a PATCH (records whatever
/// `If-Match` header the client sent, replies with the updated Ledger),
/// then a DELETE (records its own `If-Match` header too — cratestack#519
/// requires it there exactly like PATCH).
fn run_etag_stub_server(listener: std::net::TcpListener) -> CapturedRequests {
    use std::io::{BufRead, BufReader, Read, Write};

    let get_request_line = handle_one_request(&listener, |request_line, _headers| {
        let body = r#"{"id":4,"label":"gl-4","balance":1,"amount":"12.34","version":7}"#;
        (
            request_line,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"7\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            ),
        )
    });

    let (patch_request_line, patch_if_match) = handle_one_request(
        &listener,
        |request_line, headers| {
            let if_match = if_match_header(&headers);
            let body = r#"{"id":4,"label":"gl-4","balance":5,"amount":"12.34","version":8}"#;
            (
                (request_line, if_match),
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"8\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                ),
            )
        },
    );

    let (delete_request_line, delete_if_match) =
        handle_one_request(&listener, |request_line, headers| {
            let if_match = if_match_header(&headers);
            (
                (request_line, if_match),
                "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_owned(),
            )
        });

    return CapturedRequests {
        get_request_line,
        patch_request_line,
        patch_if_match_header: patch_if_match,
        delete_request_line,
        delete_if_match_header: delete_if_match,
    };

    fn if_match_header(headers: &[(String, String)]) -> Option<String> {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("if-match"))
            .map(|(_, value)| value.clone())
    }

    fn handle_one_request<T>(
        listener: &std::net::TcpListener,
        respond: impl FnOnce(String, Vec<(String, String)>) -> (T, String),
    ) -> T {
        let stream = accept_within(listener, STUB_ACCEPT_TIMEOUT);
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));

        let mut request_line = String::new();
        reader
            .read_line(&mut request_line)
            .expect("read request line");
        let request_line = request_line.trim_end().to_owned();

        let mut headers = Vec::new();
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).expect("read header line");
            if read == 0 || line == "\r\n" {
                break;
            }
            let line = line.trim_end().to_owned();
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_owned();
                let value = value.trim().to_owned();
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.push((name, value));
            }
        }
        if content_length > 0 {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body).expect("read request body");
        }

        let (result, response) = respond(request_line, headers);

        let mut stream = stream;
        stream
            .write_all(response.as_bytes())
            .expect("write stub response");
        stream.flush().expect("flush stub response");

        result
    }
}

fn generate_for(
    fixture_stem: &str,
    package_name: &str,
) -> cratestack_client_typescript::GeneratedTypeScriptPackage {
    generate_with_config(fixture_stem, package_name, false)
}

fn generate_for_swr(
    fixture_stem: &str,
    package_name: &str,
) -> cratestack_client_typescript::GeneratedTypeScriptPackage {
    generate_with_config(fixture_stem, package_name, true)
}

fn generate_with_config(
    fixture_stem: &str,
    package_name: &str,
    swr: bool,
) -> cratestack_client_typescript::GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            swr,
            full_selection: false,
            refine: false,
            tanstack: false,
            schema_sha256: String::new(),
            // This file's assertions are about ETag/If-Match, not the RPC
            // codec (issue #746) — pinned `false` like every other flag
            // here rather than reading the real default, matching this
            // file's existing (pre-#746) convention.
            native_cbor: false,
            // Issue #906: same reasoning, not this file's concern.
            rtk: false,
        },
    )
    .expect("default template should render")
}

fn package_file<'a>(
    package: &'a cratestack_client_typescript::GeneratedTypeScriptPackage,
    file_name: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}

/// Generous next to a healthy run (these round trips take ~5s end to end) and
/// tiny next to the six hours an unbounded `accept()` costs.
const STUB_ACCEPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// `accept()` with a deadline, because a blocking one turns "the client never
/// connected" into a hang with no output at all.
///
/// The status assert above catches the common case — the smoke script failed
/// and said why. This covers the rest: a script that exits 0 without issuing a
/// request, or a runtime that never starts. Neither should cost a CI runner six
/// hours, which is the default job timeout a hang runs into.
fn accept_within(
    listener: &std::net::TcpListener,
    timeout: std::time::Duration,
) -> std::net::TcpStream {
    let deadline = std::time::Instant::now() + timeout;
    listener
        .set_nonblocking(true)
        .expect("set listener nonblocking");
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // An accepted stream can inherit the listener's nonblocking
                // flag, and every reader below this is blocking — so clear it
                // explicitly rather than relying on platform behaviour.
                stream
                    .set_nonblocking(false)
                    .expect("clear stream nonblocking");
                listener
                    .set_nonblocking(false)
                    .expect("restore listener blocking");
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "no client connected to the stub server within {timeout:?} — the smoke \
                     script almost certainly failed or exited before issuing its request"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("accept stub connection: {e}"),
        }
    }
}
