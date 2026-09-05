# Changelog

## Unreleased

### The Rust client's HTTP transport is pluggable, so retries and tracing no longer need a fork

`CratestackClient` hardcoded `reqwest::Client`. Both constructors (`new`,
`with_http_client`) took or built one, and both send paths called `self.http.request(...)`
on it directly, so a consumer wanting `reqwest-middleware` — retries with backoff, an
OpenTelemetry span per request, response caching, a token-refresh hook — had nowhere to put
it. On a Flutter/`flutter_rust_bridge` client, which is exactly where a flaky link makes
retries load-bearing, the only options were forking the crate or reimplementing the
transport.

New `middleware` feature (off by default). `CratestackClient::with_middleware_client(config,
codec, reqwest_middleware::ClientWithMiddleware)` builds a client whose every request runs
the chain; internally the `http` field became a two-variant `HttpClient` enum
(`Plain`/`Middleware`) over a small unified request builder. The existing API is untouched —
`new` and `with_http_client` keep their signatures and their behaviour — and a default build
resolves, compiles, and licence-checks exactly the dependency set it did before:
`reqwest-middleware` is a `dep:`-gated optional dependency. The feature is forwarded by
`cratestack-pg`, `-api`, `-client`, `-sqlite` and `-client-flutter`, so it is reachable from
whichever facade a consumer already picked. `reqwest-middleware` 0.5 is the first line built
against reqwest 0.13; the pairing crates are `reqwest-retry` >= 0.9 and `reqwest-tracing` >= 0.6.

A retry middleware also needs to know *which* requests may be replayed, and that is not a
question the middleware can answer for itself. Every request now carries a
`RequestIdempotency` extension, readable with `extensions.get::<RequestIdempotency>()`, whose
value defaults to the RFC 9110 answer for the method — `GET`/`DELETE` idempotent,
`POST`/`PATCH` not, which is right for every generated REST CRUD route. Two cases the method
cannot express get an explicit override, `CratestackClient::with_idempotency` for REST and
`RpcClient::with_idempotency` for RPC (both take a cheap clone, both land on the same field):
every RPC call is `POST /rpc/{op_id}`, so reads look non-idempotent to the default, and a
REST `@query` procedure is a `POST` whatever it does. The per-op truth already exists as
`OpDescriptor::idempotent_by_default`; passing it from *generated* client code, rather than
by hand at the call site, is follow-up work. Note that the REST-side
`RouteTransportDescriptor` carries no such field today — only the RPC `OpDescriptor` does.

Two smaller things fell out. `ensure_crypto_provider` is now public: `new` installs the #440
`ring` fallback for you, but `with_http_client` and `with_middleware_client` are handed an
already-built `reqwest::Client`, so under `rustls-no-provider` the panic fires in the
caller's code before this crate can do anything — callers on those paths need a way to say
"install it now", and had none. And the extension is deliberately a no-op on the plain
transport rather than a silent half-feature: reqwest 0.13 keeps `Request::extensions_mut()`
`pub(crate)`, so there is no supported way to write one into a bare `reqwest::Request` from
outside that crate, and no middleware chain to read it back out either.

Errors stay where callers already look for them. A transport failure raised *through* a
middleware chain is still `ClientError::Transport` / `RpcClientError::Transport`; only a
failure raised *by* a middleware becomes the new (feature-gated, non-exhaustive-enum)
`Middleware` variant, wrapping the cause opaquely so `anyhow` never reaches this crate's
public signatures. Across the FFI bridge it is reported as the existing
`RuntimeErrorCode::Transport` — that enum's discriminants are a serialized contract every
Dart/Swift/Kotlin consumer switches on, and from the far side "retries exhausted" *is* "the
HTTP call did not complete"; the middleware's own message is preserved verbatim.

Covered by `crates/cratestack-client-rust/tests/middleware.rs` (a counting middleware sees
both REST calls; a GET is replayed on 503 and a POST is not; the override flips a POST; an
RPC call is replayed only with `with_idempotency`; a middleware failure maps to
`ClientError::Middleware` and still chains through `Error::source`) plus a doctest on the new
constructor. `--workspace` resolves default features only, so `just lint` and `just
test-ci-host` each gained one `--features middleware` line — without them the feature would
have shipped never once linted or run in CI.

### The npm publish wrapper retried the wrong things, and a green exit code was not a publish

v0.11.1 (run 33808402763's tag, release run 33808493207) landed during npm's "Intermittent Failures
Impacting npm Publish" incident (opened 21:42 UTC, eighteen minutes before the cbor-node job started
publishing). Every affected attempt failed with `E401 … Failed to generate Web Auth URLs due to
error: BadRequestError: token is invalid` — a registry-side auth failure on an OIDC publish whose
provenance sigstore had already accepted. Four Linux legs cleared it on their second attempt.
`@cratestack/cbor-node-darwin-arm64` got three of them and then `E409 Cannot publish over previously
staged version "0.11.1"`: npm had accepted an earlier attempt and was processing it. And
`darwin-x64` and `win32-x64-msvc` exited 0 with `+ name@0.11.1 … being processed` and stayed absent
from the registry for about an hour before appearing — a window in which the job's green meant
nothing a consumer could act on.

Two defects in `.github/scripts/npm-publish.sh` turned that into a misleading log. Its retry
classifier grepped the *whole* output for `transparency log` — which also matches the informational
notice npm prints on **every** publish ("Provenance statement published to transparency log: …") —
so every failure of any kind was reported as a "sigstore tlog conflict" and retried four times; a
permanent 403 would have been too. And "previously staged" was treated as a give-up failure although
retrying can only repeat the 409.

Now: only `npm error` lines are classified; a sigstore tlog conflict, the `Web Auth URLs` 401, and
5xx/reset/timeout errors are retried; anything else fails on the first attempt and says so; and
"previously staged" exits 0 with a `::warning::`, because npm already holds the tarball. What decides
whether the release is complete is a new step in `publish-npm-cbor-node` that asks the registry —
polls `registry.npmjs.org/<name>/<version>` for the main package and every platform subpackage for
up to six minutes and fails naming the ones that never became visible. It is skipped on rehearsal
(nothing was published, so nothing can be visible) and only ever reads.

`.ci/npm-publish-tests.sh` pins the classification with the real 0.11.1 output lines against a fake
`npm`, wired into CI as `just npm-publish-test`. Its "permanent error with the notice line present"
case fails against the previous wrapper — that is the proof the fix is load-bearing.

What this does *not* fix: 0.11.1 itself. crates.io (all crates, `cratestack-exec` included), pub.dev,
`@cratestack/cli`, the api family, `cbor-web`, `refine` and four Linux cbor-node subpackages are live;
`cbor-node-darwin-x64` and `-win32-x64-msvc` appeared about an hour after the run;
`cbor-node-darwin-arm64` (staged) and `@cratestack/cbor` (skipped as a dependent) are not live, and
cannot be re-run from CI (every publish job is gated on the tag push). `@cratestack/cbor-node@0.11.1`
bundles every `.node` binary, so consumers still resolve a working binding.

### A `@computed` field may be typed with a `type` block on a model, not only on a `type`

`docs/design/computed-fields.md` has said since the feature shipped that "a computed field's own type
must be a scalar, enum, or non-computed-bearing `type`". On a `type` owner that was true. On a `model`
owner it was not: `reject_type_decl_as_model_field_type` (#230, fixed by #235) rejected *every* model
field declared with a `type` block, and it runs per field with no regard for the field's attributes,
so `@computed` never got a say. `check` failed with "cannot use `type X` as its storage type" on a
field that has no storage.

That rule is entirely storage-shaped — its own diagnostic says "not backed by a database column" and
"Postgres has no `CREATE TYPE` emitted for it" — and neither hazard exists for a computed field,
because a computed field never becomes a column at all: `cratestack-migrate`'s `convert.rs` drops it
before `field_to_column` runs. `migrate diff` on such a schema emits a table with the stored columns
and nothing else. So the check now returns early for `@computed` fields, and only for them: a
*stored* model field typed with a `type` block is still rejected, with the same diagnostic, and a
regression test pins that.

The other `@computed` type rules are untouched and still govern. `validate::computed` already
iterated models and `type`s uniformly and already rejected a computed field typed as a `model` (the
resolver would have to fabricate a row) or as a computed-bearing `type` (resolver return values are
serialized as-is, so nested computed fields would ship unresolved) — which is why the fix is an
exemption in one function rather than a new rule anywhere.

Nothing downstream needed changing, which was checked by running it rather than by reading it. The
generated resolver returns the declared type because the macro's return-type mapping never
distinguished scalars from `type` blocks; `ProjectedValue::leaf` takes any `Serialize` value and
nests it as an object through `erased_serde`; and the Dart, TypeScript and Rust client generators all
route model-field type mapping through the same mapper they already use for procedure arguments and
nested `type` fields. Dart emits a `ProductDefaultMedia? defaultMedia` field with a `fromWire` nested
decoder, TypeScript emits the interface plus the `nested` wire-shape entry, and the riverpod/swr
per-file presets place the model-owned type in that model's own file. Several comments in those
generators asserted the old invariant as permanent — including one calling the model-owned-`type`
import path "currently unreachable", which it no longer is — and have been corrected in place.

New coverage, because none existed: every `@computed` field in the test suite was `String`-typed, so
even the `type`-owner case that already passed `check` was untested. A new
`cratestack-parser/src/tests_computed_type_valued.rs` holds the accept cases (bare and
parameterized) and three no-regression guards — its own file rather than more of the already
grandfathered `tests_computed.rs`, which stays at the length the allowlist records — and
`crates/cratestack-pg/tests/computed_fields_type_valued.rs` proves the whole path over RPC against a
real Postgres — the response carries a nested object rather than a scalar, `computedParams` reaches a
`type`-valued resolver, `list` composes per row, and `fields` exclusion still skips the resolver.
`crates/cratestack-client/tests/computed_type_valued.rs` pins the other side of the wire: the
generated client model types the field as the declared `type` (the struct literal would not compile
otherwise) and round-trips it as a nested object, `null` included.

Closes #909.

## 0.11.1 (2026-09-03)

### Procedures and auth providers are plain `async fn` — in every example, and in the trait docs

Every `impl` block an application hands to the generated `router()` — a `ProcedureRegistry`, an
`AuthProvider`, a `ComputedFieldResolver` — implements a trait whose methods are declared as
`fn … -> impl Future<Output = …> + Send`. That trait-side spelling is necessary (an `async fn` in a
trait cannot promise `Send`, and every axum handler needs it). What was *not* necessary is the
impl-side spelling every example in this repo used: `-> impl core::future::Future<Output = …> + Send
{ async move { … } }`, nine lines of signature around a three-line body, eighteen times across eight
examples. Since Rust 1.75 an impl may satisfy such a method with a plain `async fn`, and the compiler
checks the `Send` bound on the concrete future.

All eighteen sites are now `async fn` (`examples/{rpc-procedures, rpc-batch, rpc-batch-debounce,
rpc-streaming, react-vite-swr, microservice-pair, no-database-verification,
no-database-verification-api}`), the generated `ProcedureRegistry` trait and `AuthProvider` gain a doc
comment saying so, and `crates/cratestack-api/tests/async_fn_impls.rs` guards the property against the
real generated `router()` — if the trait ever changed to a shape `async fn` cannot satisfy, that file
stops compiling. No API change; existing long-form impls keep compiling.

Recorded honestly: this PR first built a `#[cratestack::service]` attribute macro to do the rewrite,
and deleted it when its own break-it check (remove the attribute, expect a signature mismatch) passed
`cargo check` instead. `justfile`'s `-A clippy::manual_async_fn` — rationale: "examples/tests return
`impl Future` by hand" — had been muting the lint that says exactly this; the 36 facade test files
that still use the long form, and un-muting that lint, are `docs/design/boot-surface.md` §8.1.

That document is the wider frame: a Spring-Boot-shaped, compile-time-only application surface for
CrateStack (one-line boot, typed config, health, declared cross-cutting concerns, test client,
scaffolder), each piece tested against ADR 0012 and refused where it would need a proxy or a registry.
This is its phase 1; phases 2–3 wait on the document's §8 decisions.

### A beginner on-ramp, and a README that stops describing itself by comparison

The repository had three issue forms, all of them internal planning forms. Epic, User Story and
Development Ticket each demand an intent statement, a linked source of truth, acceptance criteria, a
test plan, verification evidence and a named accountable human — which is the right bar for work we
commit to doing, and an absurd one to put in front of someone who just wants to say "this command in
your README doesn't work". Blank issues are disabled, so there was no lighter path at all.

Four short forms now sit alongside them — bug report, question, documentation problem, and idea —
asking only what a maintainer genuinely cannot work without. They carry `needs-review` and are
explicitly *not* the governance forms: if a report is accepted, a maintainer writes the Development
Ticket and links the report as its source of truth. The governance CI check is PR-only and is
unaffected by any of this.

New: `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SUPPORT.md`,
`docs/contributing/filing-an-issue.md` (which form, and what makes a report actionable — the smallest
reproducing `.cstack` schema, above everything else), and
`docs/contributing/first-contribution.md` (clone to merged PR, assuming no prior knowledge of this
codebase, including the Linux GTK/WebKit prerequisite, the `--exclude embedded_flutter_native` rule,
and why a green `cargo test` does not mean the PG paths ran). `CONTRIBUTING.md` gains a
ways-to-contribute table and the two conventions — the file-length ceiling and REST/RPC parity — that
otherwise cost a review round-trip.

The README is rewritten. It led with a macro inventory and a facade migration note, ended on a
"Current Limits" section whose second bullet was "full ZenStack-style policy and exposure parity",
and told the reader that browser and mobile examples would "land in follow-up PRs" — there are now
about thirty of them, including Next.js, Flutter/Riverpod and Tauri. It now opens with what the tool
does, shows a schema, and gets to an install command inside a screen. The ZenStack comparison is
gone; the limits it was standing in for are stated directly instead.

Three factual corrections came out of the rewrite:

- The examples table said `cargo run --example sqlite_quickstart -p cratestack`. That package is the
  documentation-only vitrine crate and has no examples: `error: no example target named
  sqlite_quickstart in cratestack package`. The four example files carried the same wrong invocation
  in their own header comments. All now name `-p cratestack-sqlite` / `-p cratestack-pg`, and
  `cargo run --example sqlite_quickstart -p cratestack-sqlite` was run to confirm it.
- The "Validation" section prescribed `cargo check --workspace --all-targets --all-features` and
  `cargo test --workspace --all-features`. `--all-features` enables both mutually-exclusive
  `decimal-*` backends and trips a `compile_error!` in `cratestack-core` — the README was handing new
  contributors the one flag `CONTRIBUTING.md` and every `just` recipe deliberately avoid.
- The facade snippets pinned `version = "0.7"` against a 0.11.0 workspace.

### Open VSX publishing is live

`publish-openvsx` is no longer an armed-but-unexercised job. It has published all five platform
targets on both the v0.10.1 and v0.11.0 tags:

```
🚀  Published cratestack.cratestack-vscode-plugin v0.11.0@darwin-arm64
```

The listing is <https://open-vsx.org/extension/cratestack/cratestack-vscode-plugin>, which reaches
VSCodium, Cursor and Windsurf — VS Code still sees only the Marketplace, which is a separate publish
on separate credentials. `docs/tooling/vscode-publishing.md`, `RELEASE.md` and
`release-vscode.yml`'s header all still said no version had been published to either registry and
that the manual `.vsix` download was the only way in; they now describe installing from a registry,
with the `.vsix` demoted to the fallback it is. The extension README — which is also the listing
page on both registries — gains the install section it never had.

One verification step changes shape as a result. `curl -o /dev/null -w '%{http_code}'` against the
Open VSX API was a valid check while a 404 meant "nothing has ever shipped"; now that the listing
exists permanently, a 200 proves only that *some* version did. The documented check reads the
`version` field instead.

### ADR 0015 accepted (amended): the L3 OpExecutor is being built in slices

`docs/design/rpc-transport.md` §4 has specified an `OpExecutor` since 2026-05-15, and ADR 0015
deferred building it behind a gate: L3 gets built when a dispatch path must make an admission
decision from an input that is not an `http::Request`. That gate has fired — not by WebSocket, which
is what everyone expected, but by ADR 0018. Making the in-process invocation path (`invoke_with_db`,
`db.transaction(...)`, `db.queries().<name>().run(...)`) a compatibility-committed dispatch surface
created a second input shape with real, CI-exercised consumers. Alternative (a) — build L3 now — was
rejected in August on the single ground that N = 1 transport shape cannot validate a
transport-neutral design. N = 2 today, so ADR 0015 is now **Accepted (amended)** at alternative (a).

Three facts the original ADR's argument rested on had gone stale in the month it sat open, and the
amendment corrects them rather than editing the Context section in place:

- `rate_limited_by_default` **is** read at runtime. `ratelimit/{rest,rpc}_ops_filter.rs` (#474)
  resolve a request to its descriptor and consult the flag through
  `RateLimitLayer::with_should_rate_limit_fn`, driven end to end by
  `crates/cratestack-pg/tests/rate_limit_runtime.rs`. `@no_rate_limit` is not inert. This
  *strengthens* the case for L3 rather than weakening it — #474 shipped the substance of
  alternative (c) for one of the two flags (a resolver closure over the static table rather than the
  matched descriptor) and it worked, leaving idempotency as the only concern that still cannot see
  the op it is about to run. `extensions.md` §5 said the `rate_limit` Cargo feature "gates the
  dispatch-layer codegen that reads `rate_limited_by_default`"; there is no such codegen reader, the
  readers are runtime and ungated, and that bullet now says so.
- `AuditSink` has a consumer — `SqlxRuntime::with_audit_sink` (#473), dispatched post-commit and
  asserted by `banking_audit.rs`. Alternative (b) is still not adopted, but no longer on the grounds
  that fan-out has no call site.
- The open question "is `mcp` the non-HTTP dispatch path?" is answered: no. `mcp { }` parses as an
  inert config block and neither `cratestack-macros` nor `cratestack-axum` mentions it.

The work lands one concern at a time, each with byte-identical wire behaviour as its acceptance bar
and the existing test suites as the regression oracle — unedited, which is the point. Slice 1
(idempotency, #876) creates `cratestack-exec` at L3 and finally makes `@no_idempotency` do
something after two release cycles of parsing and being ignored. Slice 2 (rate limiting, #877) waits
for #871 so that rewrite is not re-landed against L3. Epic: #875.

No code changes here — this is the decision, its amendment, and the pointers from the four design
documents that had been describing `OpExecutor` as unbuilt.

### `@no_idempotency` works, and idempotency admission moves to a new L3 crate — breaking

`@no_idempotency` has parsed since before it was written down and done nothing at runtime —
`crates/cratestack-axum/src/idempotency/mod.rs` documented the wiring as a follow-up that never
landed, and `idempotency-rate-limit-declarative-surface.md` §6 declared its own ticket unopenable
until `OpExecutor` had a concrete plan. It now works: a procedure carrying the attribute takes no
idempotency reservation, on REST and on RPC.

The mechanism is ADR 0015's first slice. A new crate, `cratestack-exec`, occupies layer 3 — the
slot `docs/adr/layers.toml` recorded as "empty by design" — and owns the admission decision:

```rust
let admission = executor.admit(&OpInput { op, principal, idempotency_key, fingerprint, ctx }).await?;
// Bypass | Reserved { token } | Replay(record) | InFlight | Conflict
```

`IdempotencyLayer` becomes the thin adapter around it that `rpc-transport.md` §4 predicted. It has
two dependencies, `cratestack-core` and `uuid`, because `layering.md` §2's two L3 exclusions hold:
the request fingerprint arrives already computed (method, path and content-type are transport
facts), and no `&mut Transaction` crosses the boundary — audit persistence stays at L2, where its
"the audit row commits with the mutation" guarantee requires it to be.

**Nothing changes for an existing consumer.** Honouring the attribute is opt-in, via a resolver
that reads the generated descriptors, mirroring how `@no_rate_limit` is wired:

```rust
IdempotencyLayer::new(store, ttl)
    .with_op_resolver(build_rpc_op_resolver(cratestack_schema::axum::OPS))
// or, for REST schemas:
    .with_op_resolver(build_rest_op_resolver(cratestack_schema::axum::ROUTE_TRANSPORTS))
```

**A nested router must be told its mount point.** Descriptors record the path the schema
declares (`/$procs/notify`); `MatchedPath` and `Uri::path` report the full path including the
mount, so under `Router::nest("/api", router)` — the example this crate's own README gives —
every lookup misses and the attribute silently does nothing. Use
`build_rpc_op_resolver_with_prefix("/api", OPS)` / `build_rest_op_resolver_with_prefix(...)`.
The prefix is supplied rather than inferred: nothing in a request says which leading segments
were the mount, and guessing would trade a silent no-op for a silent mis-match.

Two related fixes. `@no_idempotency(true)` used to pass `cratestack-cli check` with `schema OK`
and then emit `idempotent_by_default: false` — the argument was accepted and silently did the
opposite of what it said; arguments and duplicates are now parse errors. And a bypassed op no
longer pays the 2 MiB idempotency request-body cap it was skipping the response cap for, so a
`@no_idempotency` POST larger than that succeeds exactly as it would without the header.

That second fix has a visible half worth calling out: an exempt op no longer runs the principal
fingerprint either. If you install a resolver, a `@no_idempotency` request that previously drew
#416's `412 Precondition Failed` — no `Authorization` header and no `ConnectInfo<SocketAddr>`
peer — now succeeds, and stops contributing to that check's once-per-process warning. This is
intended: the fingerprint exists to namespace a reservation, and an exempt op takes none. It
only affects ops the schema marks exempt, and only once a resolver is installed.

Install no resolver and every request looks unresolved, and unresolved **reserves** — so the layer
guards exactly what it guarded before. That fail-closed direction is the inverse of the rate-limit
filters' (a miss there throttles; a miss here reserves), because the two descriptor flags are
polarised opposite ways. Both mean "when in doubt, apply the protection", and both are tested for
the miss rather than only the hit.

`RouteTransportDescriptor` gains `idempotent_by_default`, which RPC's `OpDescriptor` already had.
**Breaking**: it is a required field on a struct that is not `#[non_exhaustive]`, so any
hand-written literal needs the new field; in practice these are emitted by codegen.

The three new types in `cratestack-exec` — `Admission`, `OpAdmission`, `OpInput` — *are*
`#[non_exhaustive]`, which is deliberate and is the opposite call from the one above. Slices 2
and 3 add exactly the variants and fields this would otherwise force a second breaking release
for (a rate-limit refusal, a policy denial, the context slot `OpInput::ctx` reserves), and the
crate is unreleased, so the marker costs nothing today. Build them with `OpInput::new` /
`with_ctx` and `OpAdmission::new` / `unresolved` / the two `From` impls. This follows the
existing house precedent (`CratestackError`, `OpKind`, `cratestack-sql`'s `ConflictTarget`,
`StoreErrorPolicy`) rather than introducing it. One consequence is load-bearing: an external
exhaustive `match` on `Admission` now needs a wildcard arm, and `cratestack-axum`'s is
fail-closed — it refuses the request rather than falling into either arm that would run the
handler. REST needed it because shipping this on RPC alone would
have reproduced #474 — a fix that works on one transport and silently no-ops on the other.

Still at L4 and unchanged: rate limiting (slice 2), audit fan-out, and row-level `@@allow` replay
for subscriptions (slice 3). `invoke_with_db` keeps its signature; an idempotent variant is
additive and separate.

### breaking (#871): the rate-limit bucket keyspace is bounded

`RateLimitLayer` runs before authentication, so the `Authorization` header its default key function
hashes has been validated by nobody. Measured in the #846 security review: 20 requests with a
rotating bearer token created 20 distinct Redis keys, each with a ≥60s TTL, and driving that to
`maxmemory` made every subsequent write fail. #846 stopped that from *disabling* the limiter; this
closes the primitive.

The default key derivation now carries a **cardinality budget** that the store applies atomically
with the token consumption (a separate lookup would race — N concurrent requests each read "under
budget" and each mint a bucket):

| Request carries | Bucket key | Scope | Cap | Charged past the cap |
|---|---|---|---|---|
| `VerifiedPrincipal` extension | `princ:<sha256>` | — | none | — |
| `Authorization` + `ConnectInfo` | `auth:<sha256>` | `peer:<addr>` | **128** | the peer's own `ip:<addr>` bucket |
| `Authorization`, no `ConnectInfo` | `auth:<sha256>` | `global` | **8192** | one `overflow` bucket |
| `ConnectInfo` only | `ip:<addr>` | — | none | — |

`<addr>` is the peer address for IPv4 and its **/64 prefix** for routable IPv6 — in the scope
*and* in every bucket key. A /64 is the smallest block routinely delegated to one subscriber, so
leaving bucket keys un-aggregated let an attacker rotate the source address inside their own
prefix and evade the cap entirely. **The accepted cost: two distinct hosts inside one routable
IPv6 /64 share a throttling bucket.** IPv4 is not aggregated (a /24 under CGNAT is thousands of
unrelated subscribers), and **IPv4-mapped addresses (`::ffff:a.b.c.d`, which is how a dual-stack
`[::]` listener delivers every IPv4 client) are unwrapped to their IPv4 form before any
aggregation** — without that, every IPv4 client in the world shares one `ip:::/64` bucket.

Bucket key *shapes* are unchanged, so no existing bucket moves. Past the cap a caller is collapsed
onto its own peer bucket rather than refused — refusing would hand an attacker a deterministic
outage of every rate-limited route, the failure mode #846 was fought over. Under the cap, distinct
callers still never share (#416).

**Behaviour changes to know about:**

- `InMemoryRateLimitStore` now evicts. Buckets idle for a full TTL (`cratestack_core::
  bucket_ttl_secs`, the same horizon Redis's `EXPIRE` uses) are dropped by an amortised sweep, and
  live buckets are capped at `DEFAULT_MAX_BUCKETS` (**100 000**) — past which a request for a
  *new* identity is refused with `CratestackError::Internal`, a logical class that stays closed
  under every `StoreErrorPolicy`. Existing buckets keep being served. `with_max_buckets(n)` raises
  it; `without_max_buckets()` removes it.
- `RateLimitStore` gains `consume_bounded`. It has a default that delegates to `consume` and
  reports `Charged::Unbounded`, so **third-party stores keep compiling and behaving identically** —
  but they are not bounded, and the layer says so in a throttled `WARN`.
- New: `RateLimitBucketBudget`, `VerifiedPrincipal`, `UnverifiedAuthPolicy`, and the builder methods
  `with_bucket_budget` / `without_bucket_budget` / `with_unverified_auth_policy`.
  `RateLimitService` moved to its own module (the re-export path is unchanged).
- `RedisRateLimitStore` writes one new key kind, `<prefix>:rls:<sha256(scope)>` — a scope's member
  set, a `ZSET` scored by last use. `consume` (no budget) creates none.
- **`window` (default 60s) is a floor on a sliding per-credential slot, not a fixed window.** Each
  admitted credential holds a slot for `max(window, bucket_ttl_secs(config))` after it was **last
  used**; slots expire individually. So a slot always outlives the bucket it admitted (no fresh
  generation can open beneath a live one), an actively-used caller never loses its slot, and a peer
  whose tokens rotate reclaims the slots of credentials it stopped using instead of being capped at
  its first `max_distinct` forever. `cratestack_core::scope_ttl_secs` and `MAX_TTL_SECS` are new and
  public at the crate root; a `Duration::MAX` window is now clamped rather than failing every request.
- **`InMemoryRateLimitStore`'s cap now bounds the scope index too.** Admission is gated on the
  bucket being creatable, so a request refused at the cap no longer interns a scope entry and a
  member key on its way out. New `#[doc(hidden)] _scope_count()` seam alongside `_bucket_count()`.

**Not bounded, stated plainly:** distinct peers (a botnet), stores that do not implement
`consume_bounded`, `with_key_fn` overrides (the layer cannot see the derivation, so bounding it is
the consumer's job), Redis Cluster (three un-hash-tagged keys → `CROSSSLOT`, refused loudly rather
than hash-tagged onto one node), and distinct hosts sharing one routable IPv6 /64.

Full write-up, including why verified principals are opt-in rather than the default and why IPv6 is
aggregated but IPv4 is not: `docs/design/ratelimit-bucket-cardinality.md`.

### `cbor-example-verify`'s headless-Chrome step no longer flakes on a cold CI runner

The `flutter (cratestack_cbor example — linux + web, real builds)` job failed three times in one
day on unrelated PRs and on `main` (runs 33694021063, 33771671818, 33772875028), always with every
real build step green and always the same shape: `verify_web_console.dart` launched headless
Chrome, then ~15s later threw `Bad state: Chrome DevTools Protocol never became ready on 9333` — and
a plain rerun always passed. Three compounding root causes, all in
`dart-packages/cratestack_cbor/example/tool/verify_web_console.dart`:

- The DevTools-readiness poll had a hardcoded 15s deadline (200ms interval) — too tight on a loaded
  runner. It is now 60s by default (250ms interval), overridable per-run via
  `--devtools-ready-timeout-seconds` or `CRATESTACK_CBOR_DEVTOOLS_READY_SECONDS`.
- Chrome's stderr was discarded (`process.stderr...listen((_) {})`), so a failure explained nothing.
  It is now captured (bounded to the last ~4 KB) and included in the thrown error.
- Nothing checked whether the Chrome process had already exited, so a dead Chrome still waited out
  the full deadline before failing. The wait now races the poll against `process.exitCode` and fails
  immediately, with the real exit code, the moment Chrome dies.

A readiness failure now also gets one automatic relaunch (fresh Chrome process, logged loudly, same
port if it frees up in time or the next port otherwise) before giving up; a second failure is fatal
with full diagnostics from both attempts.

`verify_web_console.dart` is split into `verify_web_console/{chrome_launch,chrome_stderr_capture,
devtools_ready,fake_devtools_server,hard_timeout_watchdog,options,self_test,
self_test_subprocess}.dart` to keep every file under this repo's 200-line convention — the script's
contract (exit codes, `CRATESTACK_CBOR_EXAMPLE_RESULT` marker semantics, existing CLI options) is
unchanged.

**This fix's own first landing hung the job it was fixing** — this PR's own CI run sat for the full
45-minute `timeout-minutes` after printing `PASS:`, with the tool's Dart process still alive as an
orphan the runner had to force-kill. `waitForDevtoolsReady` reading `process.exitCode` (needed for
the "did Chrome already exit" check above) opens a native exit-watch handle that keeps the Dart
isolate alive until the process is truly reaped, and a bare `process.kill()` (SIGTERM) doesn't
guarantee that — the old script never touched `process.exitCode` at all, so it always drained
cleanly regardless. Fixed by not relying on the event loop draining at all: every exit path now
tears down deterministically (`ChromeProcess.shutDown`, escalating to SIGKILL if SIGTERM doesn't
reap the process within 5s) and finishes with an explicit `exit(code)`. A new in-process
`HardTimeoutWatchdog` (`--hard-timeout-seconds`, default 180s) is a backstop against any future
regression of the same shape, and `just cbor-example-verify` now also wraps the tool invocation in
`timeout 300` as a second, OS-level line of defence.

`verify_web_console/self_test.dart` (manual, not CI-wired, ~20s) now proves four things: a fake
Chrome exiting with stderr fails fast with that stderr and exit code; a fake DevTools server which
only becomes ready after 20s fails under the old 15s deadline but succeeds under the new 60s
default; a fake Chrome that keeps running after the marker is observed still lets the tool exit
within 5s; and the hard-timeout watchdog fires and exits 2 when the marker never arrives. Removing
the teardown/`exit(code)` reproduces the hang in the third of those (confirmed while writing this
fix — it fails by timing out, not by a wrong assertion).

## 0.11.0 (2026-09-03)

### The Marketplace item page lags a successful publish

After `publish (Marketplace)` succeeded for v0.10.1, the listing page returned 404 for several
minutes — in both publisher casings — while the extension was already fully published with all five
target platforms. That is the same write-path/read-path split that makes `npm view` unreliable
immediately after publishing, and it reads exactly like a failed publish.

The verification section now says so, and gives the `extensionquery` gallery API call to use instead:
it is consistent with the write path and returns `targetPlatform` per version, so it also confirms all
five targets landed, which the item page does not show directly.

Also recorded: `itemName` is case-insensitive. Both `cratestack.` and `Cratestack.` resolve, which
matters because the gallery API reports the publisher as `Cratestack` while `package.json` declares
`cratestack` — enough of a mismatch to look like a problem when it isn't.


### The extension's display name is `CrateStack Schema`

`displayName` moves from `CrateStack`, which the Marketplace rejects as already taken. This is
independent of the extension ID: `cratestack.cratestack-vscode-plugin` was accepted at v0.10.1 and
the publish failed anyway, on the display name alone, after auth and package validation had both
passed.

Nothing public holds the old name — an `extensionquery` for `CrateStack` across the entire gallery,
not just VS Code extensions, returns zero results. Whatever reserves it is unlisted, removed, or
internal, which means gallery search cannot be used to check a candidate name in advance. Only a real
publish attempt answers the question.

Open VSX published `CrateStack` at v0.10.1 without objection, so the two registries genuinely
disagree about this name's availability; v0.10.1 is live there under the old display name.

### `release-vscode.yml` accepts a Marketplace-only manual dispatch

Dispatching the workflow builds all five targets and publishes to the Marketplace alone;
`attach-github-release` and `publish-openvsx` are gated `if: github.event_name == 'push'`, so a manual
run can neither create a Release nor reach Open VSX.

This amends the rule stated in that workflow's own header — *"a throwaway `workflow_dispatch` test run
must never reach either registry"* — and narrows it to Open VSX, which keeps the guarantee absolutely.
The rule was written to stop accidental publishes of throwaway artifacts. The `displayName` collision
above is the case it did not anticipate: a rejection discoverable only at publish time, on a field
baked into the vsix at package time, where a failed release is bumped past rather than re-run. Under
the old rule every candidate name costs a version. A failed manual publish costs nothing, because the
Marketplace only consumes a version on success.

The trade is that a successful probe is a real publish, and can put a version on the Marketplace whose
artifact differs from the one attached to that tag's GitHub Release — which is exactly what happens
when the probe is what fixed the artifact. Cosmetic, resolved at the next real tag, and documented at
the point of use rather than left to be discovered.

### Rate-limit store failures fail open only for transport errors, and every middleware error body is typed — breaking (#846)

A single dropped Redis connection in `RateLimitLayer` took a production RPC call down twice over: the
layer turned the store error into a 500, and the 500's body was a bare `text/plain` string, so the
generated client reported `RPC call returned status 500 with an unrecognized error body` rather than
a code it could branch on.

**`StoreErrorPolicy` — the breaking part.** `RateLimitLayer` gains
`with_store_error_policy(...)`, defaulting to `StoreErrorPolicy::Allow`. `Allow` is
**class-conditional**, not a blanket fail-open: it serves a request unthrottled only when the store
failure is *transport-class* — the socket broke, the server is unreachable — and refuses every other
store failure exactly as `StoreErrorPolicy::Deny` does. Backends signal the transport class with
`CratestackError::Unavailable`; anything else they return stays closed.

That distinction is the whole design, and it is not the one this change originally shipped with. A
blanket fail-open rests on the premise that a store failure is never caller-controlled, and that
premise is false: the default key function hashes an **unvalidated** `Authorization` header (the
layer runs before authentication), so an unauthenticated caller mints one Redis key per request just
by rotating that header. Driven to `maxmemory`, every write then fails with `OOM` — and a blanket
fail-open would serve *every* request through, including from buckets already exhausted. That is a
global limiter bypass reachable by anyone. This change closes the bypass but not the primitive
underneath it — an unauthenticated caller can still mint one Redis key per request, which is tracked
separately as #871. An `OOM`, a `NOPERM`, a poisoned mutex or a malformed reply are all
reachable-and-refusing, do not self-heal, and stay closed. A broken pipe is caused by
nobody, fixable by nobody in the request path, and self-heals — refusing there would convert a
limiter hiccup into a simultaneous outage of every rate-limited route, which is why it is the one
class that degrades to unlimited. Key derivation itself remains fail-closed under both policies
(#416) for exactly the reason the `OOM` case does.

**Set `StoreErrorPolicy::Deny` to keep the previous behaviour** — every store failure, transport
included, refuses the request exactly as before this change. That is what deployments using the
limiter as a security control (a paywall, a brute-force guard) rather than a capacity control want.
`RateLimitConfig` has no env-driven surface, so this is a builder-only knob; there is no environment
variable to set.

**A bounded store lookup, also new.** `with_store_timeout(Duration)` (default 500ms) caps one
`consume` — first attempt *and* any backend-internal retry — as a single budget, and reports an
elapse as a transport-class failure. Without it, "degrade to unlimited" silently meant "hang, then
allow": `redis`'s `ConnectionManager` defaults both its connection and response timeouts to `None`,
so each attempt awaited an unbounded reconnect cycle, measured at 9.46s and doubled to 18.92s by the
retry. Nineteen seconds of blocking is worse for the caller than the refusal it replaced, and is
itself a denial-of-service lever. Both Redis stores now also configure explicit connection/response
timeouts on the `ConnectionManager` (2s each), so a store used outside this layer is bounded too —
including `RedisIdempotencyStore`, which stays fail-closed but now fails promptly.

**Check the 500ms default against your Redis latency.** A store whose p99 `consume` exceeds the
budget is classified transport-unavailable and, under the default `Allow`, served **unthrottled** —
so an under-provisioned or cross-region Redis turns into a silent partial limiter bypass rather than
a slow one. Deployments with a Redis in another region, or a heavily loaded one, should raise the
budget with `with_store_timeout` (or set `Deny`) rather than take the default on faith. The
per-10s `WARN` names the elapse, so the condition is visible, but it is visible only if someone is
reading.

**Typed error bodies, both middleware layers, both transports.** Every response the two tower layers
emit themselves now carries the framework's own codec-negotiated error envelope, encoded through the
same `Accept` negotiation and the same two wire shapes the generated handlers use — the REST
`CratestackErrorResponse` for ordinary paths, `RpcErrorBody` for RPC ones. That covers the rate-limit
layer's throttled `429`, its identity refusal and its refused store error, and — a deliberate scope
extension, same crate and same bug class — the idempotency layer's key conflict, in-flight `409`,
fingerprint refusal and buffer-limit errors. The idempotency layer gets **no** fail-open policy: a
failed idempotency store must keep failing the request, which is the entire point of having one.

Content negotiation never rewrites the status of these responses. `Accept` is caller-controlled, so
passing a negotiation failure through would let any caller downgrade its own throttle — `Accept:
text/html` turned a `429` into a `406`, and a malformed `Accept` into a `400`. An unsatisfiable or
malformed `Accept` now falls back to the default codec and keeps the original status, which RFC 9110
§12.5.1 explicitly permits for a server-originated response.

The `429` needed a code of its own, so `CratestackError` gains `TooManyRequests` (additive, the enum
is `#[non_exhaustive]`) mapping to `TOO_MANY_REQUESTS` over REST and gRPC-style `resource_exhausted`
over RPC. Every client that maps codes back to statuses carries an arm for it: the Rust client, the
TypeScript and Dart RPC runtimes, and `@cratestack/link-batch`'s `errorStatus` — that last one
matters most, because a `/rpc/batch` response is always HTTP 200 and the per-frame status is
synthesized from the code, so a missing arm turned a throttle into a synthetic 500.

Consumers who assert on these bodies as text will see the change: a throttled response is now a CBOR
(or JSON, per `Accept`) `{code, message, details}` map rather than the string `rate limit exceeded`.
`Retry-After` and the `X-RateLimit-*` headers are unchanged.

**Retry-once in the Redis rate-limit store.** `RedisRateLimitStore::consume` re-issues its script
exactly once when the first attempt fails with a transport-class error, keyed on
`RedisError::is_unrecoverable_error` — precisely the set `ConnectionManager` itself reconnects on, so
"the driver considers this connection finished" and "we treat it as transport-class" cannot drift
apart. That set also covers `ErrorKind::Parse`, a half-read reply from a dying socket, which a
narrower connection-dropped test misses. Per `ConnectionManager`'s own contract (see
`docs/design/redis-store-connection-reuse.md`, #174) the command that observes a dropped connection
still fails while the manager reconnects in the background, so a Redis idle-timeout used to cost
exactly one user-visible request; the retry awaits the replacement connection instead. Deliberately
bounded: exactly one retry, never a loop, and never for a deterministic refusal such as `OOM` or
`NOSCRIPT`. `consume` is not idempotent, so a retry after a mid-flight drop can spend a second token;
that is one token out of a bucket that exists for approximate capacity protection, against a failed
user request. The idempotency store gets no such retry, for the opposite reason.

Both store-error `WARN`s are rate-limited (10s and 60s budgets, carrying the count they suppressed),
since an attacker-induced outage must not double as a log-volume amplifier. The mechanism is a new
public module, `cratestack_core::log_throttle` (`LogThrottle`/`ThrottleDecision`) — additive API,
usable by any crate with the same problem, and deliberately not a general-purpose rate limiter: no
token bucket, no configuration, no allocation.


### `@cratestack/cbor-node` ships musl (Alpine) platform packages

`napi.targets` gains `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, so
`@cratestack/cbor-node` builds a binary for Alpine on both architectures and — once the one-time npm
bootstrap described below is done — publishes `@cratestack/cbor-node-linux-x64-musl` and
`@cratestack/cbor-node-linux-arm64-musl` alongside the five existing platform packages
(cratestack#850). Alpine consumers are not gated on that bootstrap: the main package's tarball
bundles every `.node` binary and the loader prefers the bundled file over the subpackage, so a
released `@cratestack/cbor-node` initializes on Alpine either way.

The failure it fixes was not a *fallback* to something slower — it was fatal. The generated
`native.mjs` detects musl and looks only at the `-musl` names; the `-gnu` binary sitting next to it
is never attempted, and the loader ends at *"Cannot find native binding. npm has a bug related to
optional dependencies…"*, which points at npm rather than at the missing platform. Reproduced by
loading the glibc `.node` under `node:22-alpine` before the change, and fixed under the same image
after it.

The `wasm32-wasi` branch further down that loader is dead: nothing publishes a `.wasi.cjs` or a
`@cratestack/cbor-node-wasm32-wasi` package, so it only appends another `Cannot find module` to the
error chain. Left alone deliberately — it is generated output, and the fix for musl is musl binaries.

`build-cbor-node` grows two legs. They cross-compile glibc→musl with zig + `napi build -x`, which is
what napi-rs documents today; the `nodejs-rust:lts-alpine` image it used to recommend is deprecated.
Each leg runs on the runner architecture it targets, so each one then **loads what it just built
under `node:22-alpine`** and asserts a fixed encode/decode vector through the package's own
`native.mjs`. That step is the point: cratestack#850 was a load failure that a build-only pipeline
reported as green for an entire release line, and it is now checked on the arm64 leg as well as x64,
on real hardware rather than emulation.

### `publish-npm-cbor-node` no longer stakes the release on one platform name

Adding a napi target adds an npm package name that has never been published, and npm's Trusted
Publishing categorically cannot create a name (npm/cli#8544) — the first publish of any name is
manual. The old shape turned that into a release-wide failure: `prepublishOnly` ran
`napi prepublish -t npm`, which published each platform subpackage *sequentially from inside* the
root `npm publish`, so the first 404 aborted the hook — earlier platform packages live, later ones
skipped, the main package never published, and the tag's version number spent.

`prepublishOnly` now runs with `--skip-optional-publish` and the job publishes each
`npm/<platform>` package itself, through the same `npm-publish.sh` wrapper every other package here
uses. The loop attempts **every** name, publishes the main package whether or not the loop
succeeded, and only then exits non-zero listing what failed. An un-bootstrapped platform name costs
a red job, not a release.

The same change closes a pre-existing hole that adding targets had made reachable: in rehearsal mode
the root `npm publish --dry-run` does not propagate `--dry-run` into the `prepublishOnly` hook, so
`napi prepublish` was attempting real subpackage publishes during a run whose contract is "writes to
no registry". Each name now goes through the wrapper, which honours `NPM_PUBLISH_REHEARSAL`.

**The two new names needed a manual first publish**, since npm Trusted Publishing cannot create a
name (npm/cli#8544). Both were bootstrapped on 2026-09-02 and are live at 0.10.1
(`npm view @cratestack/cbor-node-linux-x64-musl libc` → `musl`), so nothing is outstanding — but
the next target added to `napi.targets` will need the same treatment.

That bootstrap procedure in `docs/tooling/npm-publishing.md` was rewritten in the same change, and
the old version of it is now actively wrong rather than merely dated: it told you to `npm publish`
at the package root and let the `prepublishOnly` hook create and publish the subpackage as a side
effect, which `--skip-optional-publish` disables. The new procedure publishes `npm/<platform>/`
directly — it is a complete standalone package (own `package.json`, no `scripts` key, so no hook
runs; verified with `npm pack --dry-run` inside it), which also means the all-targets validation is
never reached and the old "temporarily narrow `napi.targets`" workaround is gone. The binaries come
from `gh run download`, so no local Rust or zig toolchain is needed, and each new name still needs
its own Trusted Publisher entry afterwards before CI can publish its next version.

### `napi.targets` and the `build-cbor-node` matrix are checked against each other

`.ci/napi-targets-check.sh` (`just verify-napi-targets`), wired as its own CI job that invokes
that same recipe. The platform list is duplicated between
`packages/cratestack-cbor-node/package.json` and `release-cli.yml`, and `release-cli.yml` cannot be
exercised on a PR — its first execution against any change is a production release. So a mismatch
produced no signal until a tag was already pushed. Both directions are errors: a target with no
matrix leg aborts the publish job, and a matrix leg with no target builds a binary that is silently
dropped, which is the shape of #850 itself — a platform users needed, absent, with CI green.

The `build-cbor-node` legs also now add their cross-target to the *pinned* toolchain rather than to
`stable`. A release rehearsal on the first version of this change failed both musl legs with
`error[E0463]: can't find crate for 'core' … the x86_64-unknown-linux-musl target may not be
installed`: `dtolnay/rust-toolchain@stable`'s `targets:` input installs std for `stable`, while
`rust-toolchain.toml` pins 1.98.0 and cargo selects that inside the checkout. `build-cbor-macos` and
`build-cbor-ios` already carried the same workaround.

Still uncovered: `win32-arm64`. `--no-native-cbor` remains the escape hatch there.


### A `.cstack` schema can declare a parameterized custom SQL query

`query <name>(<arg>: <Type>, ...): <ResultType>` is a new top-level block carrying an
opaque SQL body and a policy:

```cstack
type LoyaltyFeeSummary {
  total     Int
  thisMonth Int
}

query loyaltyFeeSummary(userId: String, cutoff: DateTime): LoyaltyFeeSummary
  @@sql("""
    SELECT
      COALESCE(SUM(discount), 0)::bigint AS "total",
      COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS "thisMonth"
    FROM loyalty_fee_events
    WHERE user_id = $1
  """)
  @allow(auth() != null && auth().subjectId == userId)
```

Call it as `db.queries().loyalty_fee_summary(&args, &ctx).await`. This is the last of epic
#488's five gaps: two aggregates in one round trip, a `FILTER (WHERE …)` clause, a window
function or a CTE had no expression in `.cstack` at all — the generated aggregate builder
handles exactly one column and one aggregate per call — so any service that needed one was a
direct `sqlx` dependent. `examples/declarative-query-verification` proves it isn't any more,
from a `Cargo.toml` with no `sqlx` line and a `src/` with no SQL string in it, against a real
Postgres in CI.

What the framework checks that hand-written `sqlx` cannot:

- **`$N` references, at build time, in both directions.** A `$3` past the declared parameter
  count fails `cargo check`, and so does a declared parameter no `$N` uses. Both are needed:
  typing `$3` where you meant `$2` trips the first, but only the second names the parameter
  that silently went dead.
- **`@allow`/`@deny`, unconditionally, before any SQL runs.** There is one generated entry
  point and no unchecked variant, so there is nothing to bypass by construction. A query that
  declares no `@allow` denies everyone.
- **The result is a declared `type`**, decoded into real Rust fields.

Deliberate limits, so none is discovered as a surprise:

- **A query reads only, and the database enforces it.** The statement runs inside a Postgres
  `READ ONLY` transaction, so `INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` and DDL are refused with
  SQLSTATE `25006` — including inside a data-modifying CTE like
  `WITH ins AS (INSERT … RETURNING …) SELECT …`, which is an ordinary `SELECT` to the driver.
  A write reaching the database this way would bypass `@@audit`, the `@@emit` outbox,
  `@version`, soft-delete, `@@internal` and the target model's own write `@@allow`. Use a
  `procedure` or a model write builder to change data.
- **A query runs on its own pooled connection**, and takes a *second* one while it does. It
  does not observe uncommitted writes made by an enclosing `db.transaction(...)` — read after
  that transaction commits. And on a pool with no free slot, calling a query from inside a
  transaction blocks for `acquire_timeout` and then fails with "pool timed out while waiting
  for an open connection": a deadlock on a small pool, not just a stale read.
- **The policy gates whether the call is permitted, not which rows match.** Nothing injects a
  `deleted_at IS NULL` predicate or a row-level `@allow` filter into a `query` body the way
  every generated read gets one. Query a soft-delete-enabled model's table and deleted rows
  count toward your totals unless you say otherwise. You own every predicate.
- **Postgres only.** A `query` under `include_embedded_schema!` or `db = None` is a compile
  error naming the block. There is no `@@embedded_sql` twin — the escape hatch exists for
  Postgres spellings no portable dialect could translate.
- **No client surface.** No REST route, no RPC op id, no Rust/Dart/TypeScript stub, no
  migration output. A query is reachable only from Rust already running in the process.
- **Column names are the declared field names, verbatim.** A `type` field `thisMonth` decodes
  from a column named `thisMonth`, so alias it `AS "thisMonth"` — quoted, since Postgres folds
  unquoted identifiers to lower case. A mismatch fails loudly at first execution rather than
  resolving to something else.
- **Parameters** may be `String`, `Cuid`, `Int`, `Float`, `Boolean`, `DateTime`, `Uuid` or
  `Bytes`, required arity. `Decimal` is excluded for now because its Rust type depends on the
  schema's `decimal =` backend; a `Decimal` *result* column is unaffected. Widening the list
  later is additive.

`auth().isSystem()` now works in a `procedure` or `query` `@allow` as well as a model's. It
has been available on the model read path since #486, but the procedure policy dialect never
got an arm for it, so `@allow(auth().isSystem())` failed to compile with a message about
string literal arguments. A system-caller reconciliation read is the case this whole feature
exists for, so it could hardly stay unsupported.

**Breaking, for anyone matching `ProcedurePredicate` exhaustively:** that enum gained an
`AuthIsSystem` variant, and is now `#[non_exhaustive]` — the convention
`cratestack_core::CratestackError` and `TransportStyle` already follow. Adding the attribute in
the same release as the variant costs nothing extra (the variant already breaks such a match)
and stops the next variant doing it again. Add a `_ => …` arm. Constructing variants is
unaffected, which is what generated code in every consumer crate does.

Two shared fixes fall out of the same work and apply to `view` as well as `query`, since both
read their SQL through one extractor: a `@@sql`/`@@server_sql` argument that is not a quoted
string is now a schema error naming the accepted forms (it used to yield an empty body — for a
query that meant `SQL = ""` with every `$N` check skipped; for a view, silent treatment as
embedded-only), and `\"` inside a single-line body is now unescaped rather than passed to
Postgres literally.

Purely additive: no existing schema changes behaviour, and `query` was not previously a valid
top-level keyword.

Design: `docs/design/declarative-custom-query.md`, accepted in #488's 2026-09-02 decision
comment. That decision also settled #488's open question 2 — using CrateStack as an ORM is a
supported posture — now recorded as `docs/adr/0018-orm-posture.md`.

Closes #867. Refs #488, #515.


## 0.10.1 (2026-09-01)

### `azure/login` needs `allow-no-subscriptions` for Marketplace publishing

The `cratestack-vsce-publish` managed identity holds no RBAC role on the subscription, and needs
none: publishing authenticates to Azure DevOps, not to ARM. `azure/login` nonetheless enumerates
subscriptions by default, finds zero, and aborts with `No subscriptions found for ***` — *after* a
successful OIDC exchange, so it reads like an authentication failure while the federation is in fact
working correctly.

Found by the first real run of the `Marketplace Identity ID` workflow, which failed exactly here.
`publish-marketplace` in `release-vscode.yml` made the identical call, so the first genuine
Marketplace publish would have failed the same way on a release tag rather than on a throwaway
dispatch. Both now pass `allow-no-subscriptions: true`.

The flag alone is not enough: with `subscription-id` still passed, `azure/login` stops enumerating
and starts *selecting*, failing `The subscription of '***' doesn't exist in cloud 'AzureCloud'`
instead. Both call sites now omit it too, so `AZURE_SUBSCRIPTION_ID` is no longer read by any
workflow — kept as a secret only because a future ARM-touching job would want it.

Both failures land after a successful OIDC exchange, and both advise `Double check if the
'auth-type' is correct`, which is the one thing that was never wrong in either case.

Fixed this way rather than by granting the identity a subscription role, which would also work and
would leave a standing ARM permission nothing in this pipeline uses.


### The VS Code extension is renamed `cratestack-vscode-plugin`

`packages/cratestack-vscode/package.json`'s `name` moves from `cratestack-vscode` to
`cratestack-vscode-plugin`. The `publisher` (`cratestack`) is unchanged, so the published extension
ID becomes `cratestack.cratestack-vscode-plugin` and the release assets become
`cratestack-vscode-plugin-<target>-<version>.vsix`.

Done now specifically because **nothing has been published to either registry yet** — Open VSX's
`cratestack` namespace reports `{"extensions":{}}` and there is no Marketplace item. An extension ID
is immutable once published: renaming afterwards orphans the old listing permanently, with no
redirect and no carry-over of install counts or reviews. This was the last moment the rename was
free.

Deliberately scoped to the manifest name. The directory stays `packages/cratestack-vscode`, so every
`working-directory:`, the `pnpm-workspace.yaml` glob, `repository.directory`, and `homepage` are
untouched. Two consequences worth recording, both verified rather than assumed:

* Every `turbo` filter in CI is path-based (`--filter='./packages/...'`), not name-based, so no
  workflow needed editing to keep matching the package.
* `pnpm-lock.yaml` keys workspace importers by path (`packages/cratestack-vscode:`), not by name, so
  no lockfile refresh was required — confirmed by a passing `pnpm install --frozen-lockfile`, which
  is what CI runs.

`test/vscode-suite.js` needed no change either: it derives the extension ID as
`${pkg.publisher}.${pkg.name}`, which is exactly the fix made after the earlier `vaam-store` →
`cratestack` publisher rename left a hardcoded ID that silently resolved to `undefined`. That
mechanism paid for itself here.

Historical CHANGELOG entries, the two path references in `release-vscode.yml`'s comments, and the
v0.7.16 `Invalid pattern` error quoted verbatim in that workflow all keep the old name on purpose —
they describe what things were called at the time, and rewriting quoted evidence would falsify it.

Also corrected in passing: `ci.yml`'s `js` job comment claimed the extension "has no
build/test/lint scripts, so turbo simply skips" it. That stopped being true when the package was
brought into CI; it defines all three and participates in that job. Only `@cratestack/cli` (a
`postinstall`-only package) is actually skipped.

### Publishing status docs match reality again

Both registry publish jobs are now configured and fire on the next tag. Open VSX has `OVSX_PAT` and
the `cratestack` namespace; the Marketplace has its publisher, the `cratestack-vsce-publish` managed
identity, a federated credential scoped to the `vscode-marketplace` environment, and the three
`AZURE_*` secrets. Neither has ever actually run a publish.

That second one changes the failure mode of a release. While `AZURE_CLIENT_ID` was absent,
`publish-marketplace` exited 0 and a tag stayed green no matter what; it can now turn a release red.
The step most likely to be missing is authorizing the managed identity **on the Marketplace
publisher**, which is the only one that succeeds silently at setup and fails at publish time with
`InvalidAccessException`. Both the doc and the workflow header now say so at the point of use.

`docs/tooling/vscode-publishing.md`, `RELEASE.md`, and `release-vscode.yml`'s header all described
both registries as "dormant" and the Marketplace as blocked on a Microsoft-account 2FA loop. All
three carried that same claim, so fixing one would have left two contradicting it. Each now states
the per-registry status, and the doc leads with a channel table.

Two things the old text never said, both of which are the actual source of confusion here:

* **Open VSX does not reach VS Code.** It serves Cursor, Windsurf, and VSCodium; Microsoft's VS Code
  build can only see the Marketplace. VS Code users stay on the manual `.vsix` download, without
  auto-updates, until the Marketplace path is finished — so the Open VSX work does not close that
  gap, and `product.json` `extensionsGallery` overrides are a per-user hack no publisher can ship.
* **A green `publish-openvsx` is not evidence of a publish**, because the job exits 0 when the
  secret is absent. The verification section now gives a `curl` against the Open VSX API, and flags
  that no publish has landed there yet.

The Marketplace setup's step 1 is also corrected: `az identity create` needs an explicit
`--location` (it fails `InvalidArgumentValue: Missing required field: --location` without one, even
though the resource group already has a region), and it does not create the resource group, so
`az group create` is now a real command in the block rather than an aside. Both were hit in order
while following the doc as written.

## 0.10.0 (2026-08-31)

### Generated client version ceilings follow the release line automatically

All five emitted floors — Dart's `cratestack_annotations`/`cratestack_builder`/`cratestack_cbor` and
npm's `@cratestack/refine`/`@cratestack/cbor` — are now ranges whose **upper** bound is derived from
the release line, while the **lower** bound stays a hand-verified constant.

The two halves answer different questions and only one of them is mechanical. A floor says "this
release is the first that carries what the generator emits" — a fact about published archives that no
arithmetic can derive. A ceiling exists to stop a client resolving across a pre-1.0 minor, and the
boundary it wants is always "the line after the one this generator was built from". A caret conflated
them: `^0.8.15` means `>=0.8.15 <0.9.0`, so the moment 0.9.0 shipped every generated client refused
the only releases a user could still install. That is #838, and closing it by hand means editing five
constants across two crates at every minor bump forever — a step #838 proves gets missed.

What changes today, at workspace 0.9.4:

| Package | Before | After |
|---|---|---|
| `cratestack_annotations` | `>=0.8.10 <0.10.0` | unchanged |
| `cratestack_builder` | `^0.9.3` | `>=0.9.3 <0.10.0` |
| `cratestack_cbor` | `^0.9.3` | `>=0.9.3 <0.10.0` |
| `@cratestack/refine` | `^0.8.0` | `>=0.8.0 <0.10.0` |
| `@cratestack/cbor` | `^0.8.15` | `>=0.8.15 <0.10.0` |

The two npm ones genuinely widen — they were capped below `0.9.0` and can now resolve the 0.9.x
releases the registry has carried since the 0.9.0 bump. Checked against npm rather than assumed: both
publish 0.9.1 through 0.9.4.

**A fully version-derived floor was considered and rejected on measurement, not principle.** Deriving
*both* bounds (at 0.9 → `>=0.8.0 <0.10.0`) fails twice. `0.8.0` was never published for
`cratestack_annotations` or `cratestack_builder` — pub.dev's 0.8.x line starts at 0.8.5 — so the floor
would name a version that does not exist. And pinned to 0.8.5, the generated client does not compile:

```text
error • The named parameter 'nonDefaultingListFields' isn't defined
        lib/src/models/board.dart:282:20 • undefined_named_parameter
```

Worth noting how that surfaced: `dart run build_runner build` exited 0 and wrote 13 outputs. Only
`flutter analyze` caught it. Codegen succeeding is not floor validation.

**What this costs.** Generator output is a function of the release version again in one component, so
`just bump` no longer leaves the committed snapshots and `examples/flutter-riverpod/client`
byte-identical across a **minor** bump (it still does across a patch bump). #754 decoupled these
deliberately, so this is a partial reversal of that decision rather than an oversight. The failure
#754 cared about does not return: that was a *floor* naming an unservable version, whereas a ceiling
is an exclusive upper bound that is supposed not to exist yet.

The arithmetic lives in a new `release_line` module per client crate, unit-tested against a
hand-written table (two-digit minors, tens boundaries, the 0.0.x line, pre-release suffixes). It is
duplicated rather than shared because the only crate both generators depend on is `cratestack-core`,
which holds runtime metadata, not codegen concerns — ADR 0014's layer direction is CI-enforced.

The #779/#849 tripwire literals survive unchanged in spirit: `native_cbor_generator.rs` still
hand-writes the **floor** and still turns red when the real constant moves, but composes the ceiling,
which by design names a version that does not exist yet and would otherwise redden every minor bump
for no defect.

### The generated Dart annotations floor is a range, and every emitted floor is now quoted

`CRATESTACK_ANNOTATIONS_FLOOR` moves from `^0.9.3` to `>=0.8.10 <0.10.0`.

`^0.9.3` was not an API-compatibility statement. It arrived with the lockstep 0.9.3 release rather
than because the generator started reading anything new off `cratestack_annotations`, which left the
constant contradicting its own doc comment — the justification said 0.8.10, the value said 0.9.3. By
this module's stated rule (raise a floor only when the generator emits an annotation argument the
floor release lacks) 0.8.10 is still the honest lower bound.

It has to be a range rather than a caret, for the same reason `cratestack_builder`'s own constraint
is one: `^0.8.10` means `>=0.8.10 <0.9.0` and forbids the 0.9.x release a generated client resolves
today, while `^0.9.3` has an **empty intersection** with the `^0.8.10` that every already-generated
client still declares. Only a range satisfies both. `<0.10.0` keeps the pre-1.0 ceiling a caret would
have given — deliberately bounded, since an open-ended `>0.9.0` would let pub resolve 1.0.0, a
forward-compatibility promise across a major that nothing here backs.

**All three emitted floors are now quoted in both pubspec templates**, not just the one that needs it
today. Unquoted, a leading `>` is YAML's folded-block-scalar indicator and `>=` is not a valid
header, so a bare range is a hard `ScannerError` at the consumer's `pub get` rather than a value that
parses wrongly. Quoting a caret is a no-op, so the next floor to become a range does not have to
rediscover this. Verified by unquoting the committed example client on purpose and confirming
`flutter pub get` fails in `Scanner._scanBlockScalar`, then confirming it resolves
(`cratestack_annotations 0.9.3`) once quoted.

The quoting rationale lives in non-rendering `{#` template comments rather than YAML ones, so it is
not shipped into every user's generated pubspec.

### Revert #845: the floor literal was a tripwire, and removing it was the wrong fix

#845 replaced `native_cbor_generator.rs`'s hand-written `CRATESTACK_CBOR_FLOOR` literal with a value
derived from `src/package_floors.rs`, on the grounds that the copy had drifted. Reverted, because the
drift *was the guard working*: it had caught an incomplete revert of the real constant during the
0.9.3 floor work. Removing it traded a working guard for a quieter test run.

The literal is deliberate on two counts, only one of which #845 accounted for. It is a regression
guard — a derived expectation would agree with the generator by construction and could not observe a
floor that wrongly tracks the release version (#779). It is also a **process** guard: raising the
real floor turns this test red, forcing the second edit to be a deliberate act with a reason
attached. The TypeScript twin says so outright — *"do not 'fix' it by deriving"* — and records the
tripwire working for #806. That instruction was there before #845 and should have settled it.

The real defect was legibility, and that is what is fixed instead. A disagreement used to surface as
three unrelated "pubspec must depend on cratestack_cbor" assertions dumping whole generated files,
which is what made it read as noise. Both crates now carry a `literal_matches_the_real_floor` test
that states it in one line and says what to do:

```
this file's CRATESTACK_CBOR_FLOOR literal (^0.9.3) disagrees with src/package_floors.rs (^9.9.9).
This is the tripwire, not a bug: raising the real floor is meant to force a deliberate second
edit here. Confirm ^9.9.9 names a version pub.dev actually serves, then update the literal.
```

Verified by forcing the real constant to `^9.9.9` in each crate and confirming the new test fails
with that message — and that the expectation itself is still the hand-written literal, so the
tripwire is intact rather than automated away.

A sweep for the same pattern found no other copies: the Dart `ANNOTATIONS`/`BUILDER` floors and the
TypeScript `REFINE` floor have no duplicates, as constants or bare literals. `CRATESTACK_CBOR_FLOOR`
is duplicated in both crates by design, and both are now guarded.

#845's own entry has been removed from this section rather than left standing. It described the
floor as "now derived, not hand-synced", which stopped being true in the same unreleased window —
both landed after 0.9.4, so nothing shipped either state. Leaving it would have made these notes
announce a change and its reversal as two independent features.

### PostGIS spatial columns are declarable (#842)

`cratestack-sql` has shipped `ST_Covers`/`ST_DWithin` filters since 0.6, and `cratestack-migrate`
has emitted `USING gist` indexes — but there was no way to *declare* the column they operate on.
`BUILTIN_TYPES` had no geospatial entry, so every PostGIS-backed model needed a hand-authored
migration stacked on the generated one, plus a duplicate "input" column in the `.cstack` to carry
the value a trigger derived from. `migrate diff` then reported `no changes` forever, so the
committed snapshot and the real table permanently disagreed about the table's columns.

A schema can now say what it means:

```cstack
extension postgis {
}

model DeliveryZone {
  id          Int    @id
  serviceArea Geography(Polygon, 4326)
  pickupPoint Geography(Point, 4326)?
  @@index([serviceArea], using: gist)
}
```

which emits `CREATE EXTENSION IF NOT EXISTS postgis;` and a real
`service_area geography(Polygon,4326) NOT NULL` column. `Geometry(...)` is accepted alongside
`Geography(...)`, the SRID is optional (`Geography(Point)` defers to PostGIS's own default), and a
bare `Geography` is a legal unmodified column. Subtype names are validated against PostGIS's
vocabulary — including the `Z`/`M`/`ZM` dimensionality suffixes — so `Geography(Polygone, 4326)` is
now a schema error rather than a runtime SQL error. Casing is normalised into the snapshot, so
re-casing a subtype doesn't read as a column change.

Alongside the column type:

- **`extension postgis { }`** joins the closed extension list, gated by a `postgis` Cargo feature
  forwarded from `cratestack-pg`/`cratestack-client` down to `cratestack-macros`/`cratestack-sqlx`,
  exactly like `pgvector`. `include_embedded_schema!` rejects it unconditionally — rusqlite ships no
  SpatiaLite, so no feature could make it valid there.
- **Generated `FieldRef`s** mean `covers_geography`/`dwithin_geography` stop being string-keyed. A
  typo in a column name is a compile error instead of a runtime failure.
- **`ST_Distance` ordering** via `FieldRef::order_by_distance_to(point)` — the ordering half of the
  pair whose filtering half is `dwithin_geography`, so "closest N within X metres" no longer needs
  the distance recomputed in application code after the radius filter returns.

Verified end-to-end against `postgis/postgis:16-3.4`: the DDL `cratestack migrate diff` generates
applies cleanly and produces columns Postgres reports as `geography(Polygon,4326)`, with a real
GIST index.

Two things #842 reported that turned out not to be bugs, recorded so they aren't re-filed:

- **An unrecognised scalar was already a parse error.** `validate_type_ref` has rejected unknown
  type names with ``unknown type `X` `` since #69, including in the 0.9.1 release the issue was
  measured against, and `migrate diff` goes through the validating parse path. `Geography` did not
  silently become `TEXT` — it failed to parse.
- **`scalar_to_postgres`'s fallback comment was wrong**, which is what made the above look like a
  bug. It claimed unknown scalars were "passed through unquoted — the developer is responsible",
  while the arm returns a literal `"TEXT"` and discards the name (it returns `&'static str`; it
  *cannot* pass the name through). The comment now describes what the code does and why the arm is
  unreachable from a `.cstack` file.

Also fixed, both surfaced by the new grammar: the field-line tokenizer split a type on the first
whitespace, and the procedure-argument splitter split on every comma — so any parametric type
containing a space or comma was silently truncated before reaching the type parser.

#### Breaking: the geospatial query surface is now behind the `postgis` feature

`SpatialFilter`, `SpatialPoint`, `point()`, `FilterExpr::Spatial` and the
`FieldRef::covers_geography` / `dwithin_geography` accessors have shipped unconditionally since
0.6. They now live behind `cratestack-sql`'s `postgis` feature, forwarded from each facade's own
`postgis` feature, so a build with no spatial columns doesn't carry the surface. Enabling it is one
line:

```toml
cratestack = { package = "cratestack-pg", features = ["postgis"] }
```

Off by default, matching every other extension feature. Note this is an API-surface gate, not a
dependency-weight one — PostGIS's wire format is EWKB, i.e. bytes, so the feature pulls in no
third-party crate at all.

### `cratestack migrate diff` no longer panics on a `pgvector` schema

Independent of the above, and a real bug found while wiring #842: `cratestack-cli` enabled neither
`pgvector` nor `postgis` on its `cratestack-migrate` dependency, but the emitter's gate assumes any
crate that can be handed such a schema was built with the feature on. The shipped binary can be
handed any schema, so `migrate diff` on a schema declaring `extension pgvector { }` hit a deliberate
`unreachable!`:

```text
internal error: entered unreachable code: ColumnType::Vector(3) reached the Postgres emitter
without the `pgvector` Cargo feature enabled on cratestack-migrate
```

Both features are now enabled unconditionally on that edge, for the same reason
`postgres-introspect` already was: DDL emission is a runtime capability of the shipped binary, not a
build variant.

### `up.pre.sql` is a real mechanism (#843)

**Breaking:** `cratestack_sqlx::Migration` gains an `up_pre: Option<String>` field. Code that
constructs it with a struct literal must add `up_pre: None`; the type now derives `Default`, so
`..Default::default()` works too. Nothing else about the type changed.

`migrate diff` used to prepend this to a blocking `up.sql`:

```
-- will fail on a non-empty table unless an `up.pre.sql` backfills
```

No such mechanism existed. Nothing wrote the file and nothing read it, so an operator who followed
the instruction literally got a file that was silently ignored and a deploy that failed with the
exact NOT NULL violation the warning was about — and only against a table with rows, which the
fresh scratch database CI migrates never has.

It exists now. `migrate diff` scaffolds `up.pre.sql` whenever it emits a blocking operation,
`cratestack_sqlx::apply_pending` runs it immediately before `up.sql` **inside the same
transaction**, and `Migration::checksum` covers it, so editing a pre-script after it has been
applied is drift like any other edit. `cratestack_service::migrations_from_dir` loads it.

Two details worth knowing:

- **Existing checksums do not change.** `up_pre` is mixed into the digest only when `Some`, and a
  scaffold left as comments is normalised to `None`. A migration with no pre-script hashes exactly
  as it did before this field existed — pinned by a test, because getting it wrong would turn every
  applied migration in every deployment into a `ChecksumMismatch` on upgrade.
- **SQLite gets no scaffold.** cratestack ships no migration runner for the embedded backend, so a
  file there would be exactly the phantom this change removes. Its guidance moved into `up.sql`,
  where a NOT NULL promotion means the 12-step table rebuild — SQLite has no `ALTER COLUMN`, and a
  pre-script could not help regardless.

The warning text was also wrong about *which* operation blocked: it always said "a required column
was added without a default", including for the `Optional → Required` promotion in the report,
where no column is added at all. Warnings are now generated per-op from the same list that drives
the scaffold, so the two cannot disagree. Where a pre-script genuinely cannot help — a newly added
required column does not exist yet when `up.pre.sql` runs — it says so instead of offering an
`UPDATE` that would fail.

### `@default(dbgenerated())` no longer drops the default it asserts exists (#843)

`ALTER COLUMN … DROP DEFAULT` is no longer emitted when a column switches **to**
`@default(dbgenerated())`, nor when it switches away from it. That marker means "a database-level
default exists, supplied by something cratestack cannot see" — a trigger, `GENERATED … AS
IDENTITY`, hand-authored DDL. Dropping it left a column the schema says has a default with none at
all, so a hand-written `INSERT` omitting it began failing on NOT NULL the moment the migration
landed. A comment recording the transition is emitted instead.

The rule is now symmetric: cratestack sets and drops defaults it can express, and leaves alone the
ones it cannot. That symmetry is load-bearing — `down.sql` is generated by swapping `from`/`to` and
re-running the emitter, so an unconditional drop on the reverse would have destroyed the external
default anyway.

## 0.9.4 (2026-08-30)

### The generated Dart client floors move to `^0.9.3` (#838)

Step 2, and the last one. A generated client emitted `cratestack_cbor: ^0.8.0` — `>=0.8.0 <0.9.0` —
so an app depending on `cratestack_cbor` **directly** at any 0.9.x could not resolve alongside it:

```
So, because <app> depends on cratestack_cbor ^0.9.1, version solving failed.
```

That is the user-facing bug #838 opened for, and this closes it. `cratestack_builder` and
`cratestack_cbor` now emit `^0.9.3`. `cratestack_annotations` moved to `^0.9.3` here as well, and
then to the range `>=0.8.10 <0.10.0` later in this same unreleased window — see the annotations-floor
entry above for why a caret cannot express that one. The net state for this release is two carets and
one range.

It took two releases because the floors could not move until a *published* `cratestack_builder`
accepted `cratestack_annotations` 0.9.x — 0.9.1 and 0.9.2 both declared `^0.8.10`, which excludes it.
0.9.3 published the range `>=0.8.10 <0.10.0`, so pinning the floors at `^0.9.3` now resolves. Verified
by running the gate that blocked it twice: `flutter pub get` on the committed example client with all
three pinned to the published 0.9.3 releases.

Every floor names a release that is live on pub.dev, checked against the registry rather than
assumed — which is the rule `package_floors.rs` exists to enforce, and why this could not simply
follow `just bump`.

## 0.9.3 (2026-08-30)

### `cratestack_builder` accepts `cratestack_annotations` 0.8.x AND 0.9.x (#838)

Step 1 of moving the generated-client Dart floors off `^0.8.10`. The floors do **not** move in this
release, and that is the finding rather than an omission.

`cratestack_builder` declared `cratestack_annotations: ^0.8.10` — `>=0.8.10 <0.9.0` — so once
annotations published at 0.9.x the builder *forbade* it:

```
Because cratestack_builder >=0.8.14 depends on cratestack_annotations ^0.8.10
and <client> depends on cratestack_annotations ^0.9.1,
cratestack_builder >=0.8.14 is forbidden.
```

Raising it to `^0.9.1` breaks the mirror case: `>=0.9.1 <0.10.0` has an **empty** intersection with
the `^0.8.10` floor every already-generated client declares, so a raise breaks all of them. Both
directions were measured against the published packages, not reasoned about.

So the constraint becomes a **range**, `>=0.8.10 <0.10.0`, and the floors wait. They cannot move in
the same release: the `flutter (flutter-riverpod example)` job pins the *published* builder, and
neither 0.9.1 nor 0.9.2 accepts annotations 0.9.x. This publishes one that accepts both; the floors
move next release against it. Same shape as the analyzer 12/13 range in 0.9.2, one layer down.

`package_floors_tests.rs`'s requirement parser now understands a two-sided range and compares its
lower bound. The assertion is unchanged and still bites — verified by raising the builder's
requirement above the emitted floor and confirming the test fails.

Four Dart test harnesses also gain a `pubspec_overrides.yaml` pointing at this repo's own
`dart-packages/`, so they verify working-tree constraints rather than the last release's.
`cratestack_cbor` is deliberately **not** overridden — it still resolves from pub.dev, which keeps
proving the emitted floor names a really-published release.

## 0.9.2 (2026-08-30)

### `cratestack_builder` now supports analyzer 12 AND 13, which unblocks the migration (#828)

The `analyzer <13.0.0` ceiling was stale — `riverpod_generator` moved to `^13.0.0` in 4.0.6 and
4.0.8 requires it, so the ceiling had become the *cause* of the incompatibility it was written to
prevent. It could not be fixed by flipping to `>=13 <14`, and v0.9.0 proved that the hard way.

Two CI gates constrain this package from opposite directions:

* `just verify-dart` resolves `cratestack_builder` from the **working tree**, so the generated-client
  templates must match the in-tree constraint.
* the `flutter (flutter-riverpod example)` job pins the floors from `package_floors.rs` and resolves
  from **pub.dev**, so those same templates must match the *published* builder.

While in-tree and published sat on different majors those were mutually exclusive — no single
release satisfied both, and the floor gate correctly refused a generated client no user could have
resolved.

A range spanning both majors dissolves the deadlock with **no gate disabled and no compatibility
promise narrowed**: `analyzer: '>=12.0.0 <14.0.0'`. This release publishes a builder that satisfies
analyzer 12 and 13 while the templates stay on their analyzer-12 pins, so both gates stay green.
Only then can the templates and `CRATESTACK_BUILDER_FLOOR` move to analyzer 13, against a published
builder that already accepts it — that is the follow-up, not this release.

`sdk:` deliberately stays `^3.5.0`. analyzer 13.0.0 needs `^3.9.0` and 13.1+ needs `^3.11.0`, so on
an older SDK pub simply resolves analyzer 12 from the range. The earlier attempt at this migration
forced `^3.11.0` and narrowed a published package's compatibility promise; this one does not.

One source change makes the range possible: `param.isInitializingFormal` becomes
`param is FieldFormalParameterElement`. Analyzer 13 deprecates the getter, and this package analyzes
with `--fatal-infos`, so the old form was a build failure there. The type test is semantically
identical and valid on both majors — measured, not assumed: `dart analyze --fatal-infos` reports
"No issues found!" and `dart test` passes 12/12 against a coherently-resolved analyzer 12.1.0 graph
(test_core 0.6.18) and against 13.3.0 alike.

### Dependabot no longer proposes TypeScript 7 for the webpack example

`examples/embedded-browser-webpack/web` is held at TypeScript `^6` because TS 7 is the Go-native
compiler and no longer exports the classic API (`ts.sys`, `ts.findConfigFile`, `ts.createProgram` are
undefined at runtime), which `ts-loader@9` is built on — `tsc --noEmit` passes while `webpack` dies.

The ignore needed its own update entry rather than a line in the shared npm one: `ignore` applies to
a whole entry, and the shared entry spans ten directories, so scoping it there would have silently
stopped TypeScript updates for the 28 manifests that are correctly on `^7`.

## 0.9.1 (2026-08-29)

### Release rehearsal was broken on any branch with a slash in its name (#652)

`rehearsal: true` is documented as "safe on any branch", but it could not complete on the branches
this repo actually creates. With no `tag` input the resolver falls back to `GITHUB_REF_NAME` — a
**branch** name — and that value was interpolated straight into the CLI asset filename. On
`claude/release-0.9.1` the slash is a path separator, so `tar` was asked to write
`cratestack-cli-x86_64-unknown-linux-gnu-claude/release-0.9.1.tar.gz` into a directory that does not
exist. All five `build` jobs failed there, *after compiling successfully*.

The consequence was worse than a broken filename: `preflight` needs every `build` job, so a
rehearsal could never reach the pre-flight — the single thing it exists to exercise, and the thing
that had just blocked a release. Found by rehearsing rather than by reading.

`prepare` now emits `asset_slug` alongside `tag`, with `/` replaced by `-`, and only the asset
filename uses it. For a real release tag the two are identical (`v0.9.1`), so nothing about a tagged
run changes; `ref:` checkouts and the GitHub Release `tag_name` keep using `tag` unchanged.

### crates.io now publishes before the other registries, because no probe can prove publish scope (#651)

The pre-flight's stated purpose is that "publish-crates would fail after other channels had already
published". It approximated that with an HTTP probe, because all eleven publish jobs ran in
parallel behind it. Reading crates.io's own `src/auth.rs` shows the approximation cannot be made
complete:

* `AuthCheck::default()` is `allow_token: true, endpoint_scope: None`.
* `endpoint_scope_matches` returns **false** for `(Some(scopes), None)` — a *scoped* token is
  rejected by any endpoint that declares no scope.
* So a scoped token only passes endpoints declaring a matching `PublishNew`/`PublishUpdate`
  scope — and every one of those mutates. **There is no read-only endpoint that proves publish
  authorization.**

The probe is still correct for what it *can* prove. `/api/v1/me` is `only_cookie`, so it rejects at
the `allow_token` check (`"this action can only be performed on the crates.io website"`) *before*
reaching the scope check — which makes it behave identically for legacy and scoped tokens, and
makes that response positive proof of authentication. It stays, with its limits documented.

Authorization is now gated the only faithful way: `publish-crates` runs first and the nine npm and
pub.dev publish jobs depend on it. A crates.io failure of any kind — bad token, wrong scope, a crate
rejected on content — now stops the other registries before they write anything immutable, which is
what the pre-flight was reaching for.

### The crates.io pre-flight probed a website-only endpoint, so it could never pass (#651)

Follow-up to #835, which fixed the missing `User-Agent` on this check. That fix was necessary and
revealed the real defect underneath: **`/api/v1/me` is a website-session endpoint that no API token
can complete**, so the check's `HTTP 200` success condition was unreachable by construction. It had
never passed, which matches the record — every `release-cli` run before v0.9.0 had no pre-flight
job at all, so v0.9.0 was its first execution.

crates.io evaluates authentication first and the endpoint's session requirement second, which makes
the two failures ordered and separable:

```
bad/unknown token -> {"errors":[{"detail":"authentication failed"}]}
VALID token       -> {"errors":[{"detail":"this action can only be performed on the crates.io website"}]}
```

The second message is only ever emitted **after** authentication succeeds, so reaching it is
positive proof the credential is good — which is how the v0.9.0 token was cleared without rotating
it. The check now treats that response as success, `authentication failed` as the only
rotate-worthy outcome, and an empty body as an edge rejection that says nothing about the
credential.

### Released as 0.9.1, not 0.9.0

v0.9.0 was tagged and its GitHub Release cut, but **nothing reached any registry**: the publish
pre-flight (#651) failed on its first ever real run and gated all ten channels before any of them
executed. crates.io, npm and pub.dev all remained on 0.8.15 throughout, so no version was
half-published — which is precisely what that gate exists to prevent.

The pre-flight failure itself was a bug in the check, not a bad credential (see the entry below),
and is fixed. The version number moves to 0.9.1 as a maintainer decision: `v0.9.0` remains as a tag
and a GitHub Release that shipped nothing to any package registry, and should not be mistaken for a
real release.

### The release pre-flight failed a valid crates.io token because it sent no User-Agent (#651)

The publish pre-flight added by #651 ran for the first time on v0.9.0 and blocked the release,
reporting `crates.io rejected CARGO_REGISTRY_TOKEN (HTTP 403) — the token is set but not valid`
and advising a rotation. The token was fine: it had published successfully hours earlier, and it
was never evaluated.

crates.io rejects curl's default `curl/X.Y.Z` User-Agent at the edge, **before** reading the
`Authorization` header, and answers `403` with a **zero-byte body**. The check sent no
`--user-agent`, so it hit that path every time. Reproduced directly:

```
curl -o /dev/null -w '%{http_code}' -H 'Authorization: <anything>' https://crates.io/api/v1/me
  -> 403, body 0 bytes                       # no UA: token never read
curl ... --user-agent 'x' -H 'Authorization: <bad>' ...
  -> 403, {"errors":[{"detail":"authentication failed"}]}
```

crates.io does not use `401` for any of these, so the status code alone cannot separate "blocked
before auth" from "bad credential" — only the body can, which is exactly why the check's own
`head -c 400` diagnostic printed nothing and the failure looked like a dead token.

Fixed by sending a real User-Agent, and by splitting the error message on the body: an empty body
now says explicitly *do not rotate on this signal*, and names the reproduction command; a body
containing `authentication failed` is the only case that advises rotating. The gate's behaviour is
otherwise unchanged — it still refuses to let any channel publish, which is what kept v0.9.0 from
going out half-published across ten registries.

### Dependency and toolchain audit — 2026-08

A full sweep of GitHub Actions, Cargo, container images, npm and pub.dev. Every "latest"
figure was checked against a live registry/API rather than recalled, and each item below was
verified by running the thing it claims to fix.

**Deadline-driven (GitHub Actions).** Six actions still declared `runs.using: node20`, which
GitHub removes from runners on **2026-09-23**; the `ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION`
opt-out was not set anywhere. `setup-node`→v7, `download-artifact`→v7 (deliberately **not** v8
— that release makes digest mismatches a hard error and stops blanket-unzipping, which deserves
its own change), `upload-artifact`→v7, `pnpm/action-setup`→v6, `action-gh-release`→v3,
`cache`→v6, and `codeql-action/upload-sarif`→v4.37.9 (v3 is formally deprecated). Every action
reference in `.github/` was then resolved back to its own `action.yml`: all node24 or composite,
zero node20. `checkout`→v7 (its only breaking change restricts `pull_request_target`/
`workflow_run`, neither of which this repo uses). `node-version: 20` → `24` at all ten sites —
Node 20 has been EOL since 2026-04-30, and `prepare-release.yml` already carried a comment
documenting a *reproduced* release failure caused by that exact pin.

**Security.** Four HIGH advisories cleared, all inside existing caret ranges: `brace-expansion`,
`nanoid`, `fast-uri`, `js-yaml`. The `brace-expansion` one was the only advisory with real
end-user exposure — `cratestack-vscode` bundles with `bundle: true`, so it was inlined into the
shipped `dist/extension.js`; the fix is verified in the rebuilt bundle, not merely in the source
tree. A yanked `chacha20 0.10.0` reached the published `cratestack-auth` via `cuid2` and is
cleared. `.github/dependabot.yml` added covering github-actions, cargo, and all nine pnpm
lockfile roots — the eight example roots are independent workspaces and had nothing refreshing
them, which is why the backlog accumulated.

**CI was testing against EOL databases.** `testcontainers-modules` hardcodes `postgres:11-alpine`
and `redis:5.0`, and no call site overrode them — so `just test-pg-tc`, which is what CI runs, was
exercising **PostgreSQL 11** (EOL 2023-11-09) while `just test-pg` used PostgreSQL 18. Seven majors
apart, on a framework whose job is generating SQL. All eight sites now pin explicitly (`18-alpine`
/ `7.4`), verified by `SELECT version()` against a live container rather than by a green suite.
A crate bump would not have fixed this: upstream still hardcodes the same defaults.

**Toolchain and dependencies.** Rust 1.95.0 → **1.98.0** (`rust-version` moves in lockstep, as
`ci.yml`'s three-way equality assertion requires — note this raises the *published* MSRV).
`sqlx` 0.8.6 → **0.9.0**: not cosmetic, it is the durable fix for `pgvector` re-resolving onto
sqlx 0.9 and putting two `sqlx_core` majors in one graph, a break that was invisible to a default
`cargo check` because `pgvector` is off by default. Also `tower-http` 0.7, `base64` 0.23, `toml` 1,
`rusqlite` 0.40.2, `minicbor` 2.3 + `minicbor-serde` 0.7.1, `syn` 3, and the
`rand`/`sha2`/`hmac`/`ed25519-dalek` cluster as one atomic change.

Two of those needed proof rather than argument. The **minicbor** bump touches the default wire
format, so 32 representative payloads were hex-dumped through the real codec before and after:
byte-identical, and the harness was itself shown to detect drift. **sha2 0.11** returns a type
with no `LowerHex`, breaking `format!("{:x}", ..)` at five sites that produce *persisted* strings
(migration checksums, Redis keys, schema hashes) — the replacement is gated by a test that fails
if the hex casing changes.

Three bare `"0"` requirements (`axum`, `chumsky`, `chrono`) were tightened to `"0.8"`/`"0.13"`/
`"0.4"`. Under Cargo's 0.x rules every 0.x minor is a semver-major, so `"0"` opted the workspace
into every future breaking release on any `cargo update`. Zero resolution change; two design docs
already reasoned specifically against "axum 0.8.9, this repo's pinned version".

**Container image.** `crates/cratestack-mock-wiremock/docker/Dockerfile` no longer git-clones and
Gradle-builds `wiremock-state-extension` from source. Its comment claimed the shaded artifact "is
never published anywhere" — that has been false since release 0.9.x. The `-standalone` classifier
on Maven Central is the `shadowJar` output (572 relocated Handlebars entries, zero unrelocated),
now fetched with a build-time sha256 check. The JDK stage, the git clone and the Gradle-vs-Java
version risk all go away.

**Node support floor.** `engines.node` moves to `">=24"` across the published packages (Node 18
has been EOL since 2025-04-30, and `vitest@4` cannot run on it, so the old floor was a promise
nothing exercised), and pnpm to 11. pnpm 11 stops reading pnpm settings from `.npmrc`, where this
repo's `link-workspace-packages=true` lived — left unaddressed, the first install silently
re-pointed every workspace importer at the last *published* tarballs instead of the working tree.
`linkWorkspacePackages: true` now lives in `pnpm-workspace.yaml`.

**Convention enforcement.** `just verify-file-length` added, asserting the ~200-line-per-file
ceiling `CLAUDE.md` declares over `crates/*/src`. Nothing enforced it before and it had drifted to
116 files, the largest at 553 lines — the same silent way `[workspace.lints]` drifted before
cratestack#523. A dated allowlist grandfathers the backlog so the ceiling binds for new code
immediately, and a stale entry is a hard failure so the list can only shrink. `crates/cratestack-auth`
is deliberately absent from it: it was the worst offender and was split in the same change, from
6 files (largest 1381 lines, all tests inline) to 65 (largest 193, zero inline test modules), with
its public API proven unchanged.

**Not in this release: the Dart analyzer-13 migration.** It was attempted and reverted, and the
reason is worth recording. `cratestack_builder`'s `analyzer <13.0.0` ceiling is stale —
`riverpod_generator` moved to `^13.0.0` in 4.0.6, so the ceiling now causes the incompatibility it
was written to prevent — but it cannot be fixed in one release. `just verify-dart` resolves
`cratestack_builder` from the working tree, while the floor-pinning step in the `flutter
(flutter-riverpod example)` job resolves the *published* floor, and both are CI gates. With the
in-tree and published builders on different `analyzer` majors those constraints are mutually
exclusive, so it has to be sequenced across two releases. The floor gate refused the attempt
rather than letting a generated client ship that no user could resolve — which is exactly what it
exists to do. Tracked in #828.

Two now-stale `deny.toml` ignores (RUSTSEC-2026-0194/0195) were deleted — their own recorded
revisit condition, "when tauri ships a plist >=1.10", is met, and `cargo-deny` reports both as
not-detected.

### Linux arm64: corrected from "half open" to blocked upstream, both halves (#823)

Every doc that described this gap drew a distinction that does not exist. The claim — inherited from
#563 and repeated in the package README, the library doc, `native_cbor_codec.dart`'s header and its
`UnsupportedError` message, `cratestack-cli`'s `--no-native-cbor` help, and
`docs/tooling/cratestack-cbor-development.md` — was that *Flutter* on arm64 Linux is blocked upstream
but plain `dart test`/`dart run` "needs no Flutter bundling at all" and so remained separately
reachable and separately fixable.

Measured on a clean `dart:stable` container with no Flutter on `PATH`, and it fails identically for a
pub.dev dependency, a `path:` dependency, and the package in place:

```text
Because cratestack_cbor requires the Flutter SDK, version solving failed.
```

`dart-packages/cratestack_cbor/pubspec.yaml` declares `flutter.plugin.platforms`, which obliges
`environment.flutter` — pub validates the one against the other — so the Flutter SDK is required to
**resolve** the package, not merely to bundle it. That reproduces on x86_64, so it was never an
architecture question. With no published Flutter SDK for arm64 Linux (re-verified: 734 entries in
`releases_linux.json`, zero containing `arm` or `aarch`), an arm64 Linux user fails at `pub get`
before `createCborCodec()` runs, and a vendored `blobs/linux-arm64/` would be unreachable.

#823 was filed to add that blob and is closed unimplemented on this evidence. `--no-native-cbor`
remains the answer for that target. Docs only — no behaviour change.

### The generated Dart `pubspec.yaml` no longer claims `cratestack_cbor` is version-locked (#563, #823)

`crates/cratestack-client-dart/templates/pubspec.yaml.j2`'s comment above the `cratestack_cbor`
dependency said the constraint was "version-locked to this generator's own crate version, the same
lockstep convention `just bump` already applies". That stopped being true with #779, which moved
every generated dependency constraint to a constant API floor: `context.rs` emits
`CRATESTACK_CBOR_FLOOR`, and it deliberately does *not* move with `just bump` — which is the whole
point, since deriving it from the release version is what broke `Prepare Release` for 0.8.14 (#754).
The correct account was three lines below in the same file, for the sibling constraints. The comment
now points at `package_floors.rs` instead, and at #823 for the Linux arm64 gap it mentions (#563,
which used to track that gap, closed on 2026-08-29).

Comment-only. The emitted `cratestack_cbor:` constraint is byte-identical.

### `.cstack` files carry the CrateStack mark in the VS Code explorer

`.cstack` files rendered with whatever generic glyph the active icon theme falls back to, so a
schema was visually indistinguishable from any unrecognised file in the tree. The extension now
contributes `icons/cstack-light.svg` and `icons/cstack-dark.svg` through
`contributes.languages[].icon`.

Worth being precise about what this does, because the mechanism is a fallback rather than an
override: VS Code shows a language icon only when the active file icon theme has no icon of its own
for that language or extension, and does not set `"showLanguageModeIcons": false`. Under Seti (the
default) `.cstack` matches nothing, so the mark renders; a theme that already ships a `.cstack` glyph
still wins, and one that opts out still shows nothing. An extension cannot force an icon into a theme
the user chose — the only alternative is shipping an entire icon theme, which would make users
abandon Seti or Material Icon Theme to see one file type.

The artwork is the approved extension mark redrawn as geometry rather than traced, so it stays crisp
at the 16x16 the explorer actually renders, with the palette sampled exact from `icon.png`
(`#F7B270`/`#E88A3A`/`#BF6A26`). The gallery tile's `#1E222E` background is deliberately dropped: a
file icon sits on the explorer's own background, where an opaque plate would render as a dark box on
every theme that isn't that navy. The light variant deepens the palette one step for legibility
against a near-white tree; the hue is unchanged.

Requires VS Code 1.64+ (microsoft/vscode#14662, implemented January 2022). `engines.vscode` is
already `^1.91.0`, so no floor change — but `test/language-icon.test.js` now asserts that floor stays
above 1.64, because below it the contribution is parsed and silently ignored, and lowering the floor
would un-ship the icon for exactly the users a lower floor was meant to reach. The same test guards
that both variants are declared, resolve to real files, and are SVG. All four assertions were proven
by breaking them independently.

Both SVGs were confirmed inside a built `.vsix` by sha256 against the working tree, not inferred from
`.vscodeignore` being a denylist. What no automated check here covers is whether the icon *looks*
right at 16x16 in a real explorer.

### `Bytes` now survives `transport rpc` — two independent defects, one symptom (#820, #806)

A `Bytes` field on an RPC schema did not reach the wire in any shape a server-side `Vec<u8>` could
decode, on **either** codec. Two separate causes had to be fixed for it to work, and each was
invisible while the other was present:

**#820 — the generated client destroyed the value before any codec ran.** `encodeWireFields` had no
`Uint8Array` branch. A typed array is not `Array.isArray`, so it fell into the generic object case and
was rebuilt through `Object.entries` into `{"0":1,"1":2,"2":3}`. `terminalLink` calls that function
unconditionally, so it broke both codecs at once: the native codec emitted a CBOR map where a byte
string belongs, and on the JSON path `encodeBinaryAsJson` never saw a real `Uint8Array`, so its
`Array.from` never fired. Fixed by passing `Uint8Array` through untouched — which is what
`encodeBinaryAsJson`'s own doc comment already said the native codec required.

**#806 — no published codec encoded it correctly either.** `CRATESTACK_CBOR_FLOOR` is raised from
`^0.8.0` to `^0.8.15`, the first published `@cratestack/cbor` whose codec encodes a `Uint8Array` as a
CBOR byte string (major type 2). Verified against the registry rather than a changelog, which is the
standard #754 established.

Both were needed. With only the floor raised, the client still shipped map-encoded bytes; with only
the client fixed, any consumer resolving `^0.8.0` still got the broken codec.

**REST was never affected** and needs no change — `rest-runtime.ts.j2` calls `encodeBinaryAsJson`
directly without `encodeWireFields`, which is why `tests/bytes_round_trip.rs` passed throughout. The
asymmetry is the defect's shape, not an omission.

The new test asserts **wire bytes**, deliberately. The defect is invisible at the type level
(`Uint8Array` typechecks against both) and invisible to a decode-side round trip, because the same
broken walk reads its own output back. Only the encoded bytes distinguish them.

Also in `package_floors_tests.rs`: a floor may now **equal** the workspace version when its release
has already published, listed in `PUBLISHED_EQUAL_FLOORS` with the evidence. The previous strict `<`
was a conservative proxy for "the floor names a shipped release", correct on a bump PR and
over-strict in the window between a release publishing and the next bump — which is exactly the
window #806 landed in. A second test deletes the exemption once it is no longer needed, so it cannot
rot into a blanket hole.

### The iOS round-trip check detects from the log store, not the live subscription (#723)

`cbor-example-verify-ios` decided pass/fail by grepping a live `log stream` subscription, falling back
to the log store only after the full 90-second budget expired. #723 was opened to hold that open until
the capture defect could be observed once rather than theorized. It has now been observed, repeatedly,
and the verdict is recorded on the issue.

**The decisive measurement** — job 97444502262: the live capture delivered **0 Runner-attributed lines
/ 95 bytes** while the log store held the marker. The healthy baseline is ~1050 lines / 270,521 bytes.
Not a slow subscription; nothing at all. The round trip itself was fine.

The poll loop now queries the log store (every 2s, bounding the simctl round trips) and the live stream
is demoted to diagnostics. Consequences:

- A run with a dead subscription passes in **~2s instead of ~103s**. The old shape recovered the same
  marker, but only after burning the whole budget — which is why job 97444502262 reported the marker
  "103s after launch, -13s of margin". That was the recovery timestamp, not the app's.
- Capture health is now reported on **every** run rather than gathered by a bespoke watch workflow. An
  unreported signal that no longer causes failures is indistinguishable from one that was fixed.
- The live stream is still subscribed, captured, counted and dumped on failure. It is a far richer
  failure dump than the marker-filtered store query, and it is the measurement that keeps a future
  regression in the subscription visible.

Positive evidence from the stream is still accepted. "Detect from the store" means detection must not
*depend* on the live stream, not that a marker it demonstrably captured should be thrown away — so the
inverse anomaly (store empty, stream has it) passes and is reported rather than failing on a
technicality about which channel won.

### Releases can be rehearsed without consuming a version number (#652)

`release-cli.yml` gained a `rehearsal` input. Trigger it from the Actions tab, on any branch, with the
tag input left empty: every artifact is built, every content gate runs, and every irreversible write
is skipped. No registry is touched and no version number is spent.

Eleven PRs (#629, #631–#638, #640, #641) fixed release-pipeline defects between v0.8.0 and v0.8.3.
Not one was found by CI. Every one was found by cutting a real release and watching it fail — or
watching it *succeed while shipping nothing*. The reason is structural: this workflow only ran on a
tag push, so its first execution against any change to it **was** a production release.

**It is a flag on the real pipeline, never a second copy.** That is the ticket's first Risk, and a
diverging rehearsal stops representing reality within one release. Concretely: the seven npm
publishes all route through the one wrapper they already shared, `npm-publish.sh`, which switches to
`npm publish --dry-run` on `NPM_PUBLISH_REHEARSAL=1`; crates.io uses `just release-publish dry`, the
same recipe in its existing dry mode; the pub.dev jobs run every vendor step, every cross-host
artifact download and the archive-contents gate, and stop before `dart pub publish`.

**The rehearsal cannot publish, and that is now checked rather than asserted.** The ticket asks for
it "verified by inspection of every publish step's guard, not by trusting a flag", so the inspection
is a script — `.ci/release-rehearsal-guard-check.py` — wired into CI as its own job. It fails if any
irreversible write loses its `github.event_name == 'push'` guard, if a step delegating to the
rehearsal-aware wrapper stops wiring the signal through, and if its own pattern ever matches nothing
(a check that silently stops checking is the failure mode it exists to prevent).

**What a rehearsal does NOT cover, reported rather than implied.** pub.dev's OIDC trust is configured
per-tag-pattern, so no non-tag run can mint a token it would accept (#641). The pre-flight reports
that channel as NOT COVERED and the manifest repeats it. A green rehearsal is never rendered as LIVE
either — a successful publish job in rehearsal mode reads "rehearsed — gates passed, nothing
published", because rendering it as LIVE would recreate the false-green both #651 and #652 exist to
kill, one level up.

The existing `workflow_dispatch` path that rebuilds binaries for an existing tag is unchanged; the
tag input is now optional only so a rehearsal can run without one.

### Releases gate every publish behind one pre-flight, and end with a channel manifest (#651)

Maintainer decision on #651 (2026-08-29): options **(b)** and **(c)** combined, recorded in a policy
comment at the head of `release-cli.yml` so the next person does not re-derive it.

Previously the publish jobs fanned out in parallel from `prepare`, each individually correct and
collectively unsafe: during v0.8.1–v0.8.3 the pub.dev leg failed three times — a hang, then a false
green, then an OIDC error — while crates.io and every npm package had *already* published
irreversibly. Three version numbers were spent recovering from a failure in the least-established
channel.

**(b) The pre-flight.** A new `preflight` job waits on every cross-host `build-*` job and validates
each channel's credentials; all ten publish jobs now carry it in their `needs`. A missing artifact or
a bad credential in *any* channel therefore blocks *every* publish, rather than being discovered
after eight of them are live.

The crates.io check asks crates.io — `GET /api/v1/me` — rather than testing the secret for
non-emptiness. That distinction is the point: an expired token is present and non-empty, and a `-z`
check waves it through into a release that has already spent every other channel.

**(c) The manifest.** A `release-manifest` job runs with `if: always()` and reports every channel as
LIVE (irreversible) / FAILED / not attempted / cancelled at the release version, with a recovery note
saying plainly that a mixed outcome needs a new version number rather than a re-run. `skipped` is
reported as "not attempted" and never folded in with a failure.

**pub.dev is reported as NOT COVERED, never as passed**, on any trigger other than a tag push. Its
OIDC trust is configured per-tag-pattern, so a `workflow_dispatch` run cannot mint a token pub.dev
would accept (#641). A pre-flight that quietly green-lit an unverifiable credential would recreate
the exact false-green this came from.

Nothing existing was weakened: the pub.dev archive-contents gate and the post-publish "did pub.dev
actually receive this version" poll are untouched. And what the pre-flight *cannot* cover is stated
in the workflow rather than implied — `publish-pubdev-cbor` vendors its Linux/web/Android artifacts
inside the publish job, so those specific artifacts are not behind the barrier.

### `cbor-example-verify-windows` polls for the marker instead of waiting a fixed 20s (#803)

The Windows CBOR round-trip verification ran the built `.exe` under `timeout 20 ... || true` and
grepped its captured stdout exactly once, after the full 20 seconds had unconditionally elapsed. Two
independent defects, byte-for-byte the pair #753 documented on Linux:

- A cold start slower than 20s failed the job even though the app would have printed moments later.
- `|| true` discarded the exit status, so a crash, a non-zero exit and a slow start all reached the
  same grep and produced the same "did not print the expected round-trip marker" message — true in
  every case, diagnostic in none.

It now polls to a 45s deadline and stops the instant the marker appears, so a healthy run finishes
*faster* than the old fixed wait rather than slower. A timeout is reported as a timeout; an early
exit names its status; a marker followed by a non-zero exit is a pass, decided explicitly and
documented inline (the same answer #800 gave for Linux, plus a Windows-specific reason — the poll
loop's own `kill` races the app's normal exit, so a non-zero status in that branch may be nothing but
our own signal).

This was the last platform in the family: iOS was fixed in #720/#722, Linux in #753/#800, and macOS
and Android already polled. `expected_hex` is untouched and still shared across all five.

Where the Linux fix could not simply be transplanted, that is said rather than assumed. On Linux
`timeout` is load-bearing for signal delivery because it wraps `xvfb-run`; Windows launches the
`.exe` directly, so `timeout` is only the outer deadline there. Whether `kill` on the MSYS `timeout`
pid reaches a native Win32 grandchild is unverified from a Linux dev machine — and if it does not,
the recipe still reports the correct verdict and merely waits out the deadline on an
already-successful run, which is no worse than the fixed wait it replaces.

### BREAKING (`--template-dir` only): TypeScript templates render under `UndefinedBehavior::Strict` (#774)

The TypeScript generator's minijinja environment no longer treats an undefined name as falsy. A
template that branches on — or interpolates — a field the render context does not provide now fails
generation with `failed to render template '<name>'` instead of quietly taking the else-branch.

**Why.** minijinja's default is `Lenient`, and two shipped defects came from it within one week:
`native_cbor` (#765) and `models_import_path` (#764), each a field present on `TemplateContext` and
absent from `SwrSchemaContext` while both render the same template. In both cases the generator
emitted TypeScript that compiled, looked right, and was wrong — one spoke `application/json` where
the rest of the same package spoke `application/cbor`. A build failure would have been strictly
better than either.

**Who this affects.** Only `--template-dir` users. Every template this project ships already renders
clean under Strict — the full `cratestack-client-typescript` suite and `just regen-examples --check`
both pass unchanged. If you override a template and it references a name the contexts do not define,
generation now stops and names the template. Delete the reference, or guard it with
`{% if name is defined %}`.

`SemiStrict` was not chosen, and not on the ticket's say-so — measured. Under `SemiStrict` an
undefined `{{ interpolation }}` does fail, but an undefined `{% if %}` condition is still silently
false, which is exactly the case that produced #765 and #764. Only full `Strict` closes it.

Not extended to the Dart generator: its two pipelines share zero template files, so the
one-template-two-contexts class this fixes cannot arise there.

### Read policies gained `in` / `not in` against a set of literals (#666)

`@@allow`/`@@deny` read-policy comparisons now accept set membership:

```
model Asset {
  id Int @id
  purpose AssetPurpose

  @@allow("read", purpose in [product_image, product_thumbnail])
  @@deny("read", purpose not in [product_image, product_thumbnail, product_video])
}
```

This is the second half of #666. The first half — comparing a required enum field against a single
variant — shipped earlier; `in` was scoped out then because it needed a genuinely new multi-value
predicate shape rather than another literal arm, and this closes it.

`in` is not enum-only. It reuses the same literal parser as `==`, so it accepts every type that arm
already accepted: required `Boolean`, `Int`, `String`, and enum fields. Optional and list fields stay
rejected, for the same reason as before — `column IN (...)` and `column NOT IN (...)` both evaluate to
NULL when the column is NULL, so a nullable column would silently fall out of *both* branches.

**It is not a desugaring.** `purpose == a || purpose == b` still works and still lowers to an `Or`
tree of single comparisons; `purpose in [a, b]` lowers to one flat `ReadPredicate::FieldInLiterals`
and renders as a single `purpose IN ($1, $2)`, one bind slot per element.

Rejected at compile time rather than accepted quietly:

- `field in []` — an empty set is a constant `FALSE` wearing a policy's clothes, and SQL has no valid
  `IN ()` form to render it to.
- A trailing or doubled comma, naming the position.
- An unknown enum variant *anywhere* in the list, naming the offending variant — a typo among
  otherwise-valid variants silently narrows a policy, which is the failure this whole issue is about.
- A bracketed term with no `in` keyword (`join [...]`). Without this guard the trailing `in` is
  stripped off the *field name*, and if the shortened name is also a real column the malformed term
  compiles into a working policy that gates on the wrong data.

Commas inside a quoted string literal do not split the list, so `status in ["a,b", "c"]` is two
elements.

### The `ovsx` publish CLI is pinned instead of fetched at publish time (#811)

`packages/cratestack-vscode`'s `publish:open-vsx` script and the `publish (Open VSX)` job in
`release-vscode.yml` both invoke the Open VSX CLI as `npx ovsx`, but `ovsx` was declared in neither
`devDependencies` nor `pnpm-lock.yaml`. `npx` resolves a locally installed binary when one exists and
otherwise downloads the package from the registry at run time — so that step executed whatever npm
served that moment, unpinned and unreviewed, in the one job that holds `OVSX_PAT`. `@vscode/vsce`
sitting beside it was pinned and resolved locally, so the two publish paths were asymmetric for no
reason anyone had chosen.

Nothing was failing, and that is the point: the job would have gone green either way. Open VSX
publishing is dormant today (#811 covers turning it on), so the fix lands *before* the first real
publish rather than after — an Open VSX publish cannot be cleanly deleted and retried.

`ovsx` is now a pinned `devDependency` and present in the lockfile, so `pnpm install
--frozen-lockfile` is what puts it on disk. Verified by the resolution flipping: on `main`,
`npx --offline ovsx --version` fails with `ENOTCACHED` and a request to `registry.npmjs.org`, proving
the network fetch; with the fix it prints `1.1.1` from `node_modules/.bin`.

`test/publish-tooling.test.js` guards the general rule rather than the one package that broke it —
every tool a script reaches for via `npx` must be a declared dependency — plus a second assertion that
the scan still matches the publish scripts, so the guard cannot pass vacuously if a script is reworded
or `npx` is swapped for `pnpm exec`. Both were proven by breaking them independently: undeclaring
`ovsx` fails the first and not the second, and rewriting `npx ovsx` to `pnpm exec ovsx` fails the
second and not the first.

The Open VSX setup doc now runs its one-time `create-namespace` step from the package directory, so
that command uses the pinned CLI too.

## 0.8.15 (2026-08-28)

### A misspelled field attribute is now a parse error, not a silent no-op (#679)

`.cstack` attributes parse generically and an unrecognised one is simply inert, so a typo'd
`@raedonly` reported `schema OK` while quietly leaving the field ordinary and writable. The author
got positive confirmation that a protection was in place when it was not — failing in the unsafe
direction. (`@allow`/`@deny` at field position were already rejected; this is the typo half #679
also reports.)

**Maintainer decision: option (b), near-miss detection, not a closed attribute set.** An unknown
attribute is rejected only when it is a near-miss of a name the language knows. `@raedonly` now fails
and names `@readonly`; `@totallyBogusAttribute` stays inert exactly as before. Option (a) — reject
anything not on an allowlist — matches the ticket's first criterion literally but commits the
language to a closed set with real blast radius: the supported set has to be reconstructed from
scattered comparisons with no in-repo spec, it must be correct for all five field-bearing
declarations, and a too-narrow list breaks users' schemas on upgrade. That is a worse failure than
the no-op it replaces.

Distance is optimal string alignment — Levenshtein plus transposition as one edit. That is
load-bearing, not a refinement: `raedonly` -> `readonly` is a transposition, costing 2 under plain
Levenshtein, so without it the canonical case would need a looser threshold and far more noise.
Names under three characters never produce a suggestion, since at that length almost anything is one
edit from something.

The reference set deliberately lists every attribute the language knows at *any* position, not just
field-valid ones — under option (b) an extra name only reduces detections and can never cause a
false rejection, while a missing one is the only real hazard. It includes the names
`removed_attributes` rejects outright, so an exact `@allow` still gets that module's specific
guidance rather than a generic suggestion.

Verified against every committed schema, not just new fixtures: all 195 `.cstack` files in the repo
were parsed before and after, and the result is byte-identical (189 parse, 6 are negative fixtures).
The rejection test was observed failing with the check disabled, and both "must still parse" controls
stay green in that state — so "reject everything" cannot satisfy the suite. All five field-bearing
declarations (`model`, `view`, `mixin`, `type`, `auth`) are covered, because a missed call site fails
silently, which is the exact bug.

### `--tanstack` rejects a procedure hook that collides with a model hook (#802)

`--tanstack` emits per-model hooks (`use<Model>ListQuery`, `useCreate<Model>Mutation`, …) and
procedure hooks (`use<Procedure>Query`/`Mutation`) into the **same** `src/react-query.ts`, and
nothing checked whether the two families derived the same identifier. A procedure named `create_post`
alongside a `model Post` produced two `export function useCreatePostMutation` declarations in one
file — a package that cannot compile, discovered at the consumer's build.

#777 fixed this class for `--swr` and explicitly scoped `--tanstack` out as "structurally identical
but not currently triggered". This is that half. Generation now fails up front, naming the procedure,
the model, the operation, and the shared identifier.

The check is gated on the flag, per decision spike #317: a schema never generated with `--tanstack`
must not be constrained by `--tanstack`'s naming scheme. A test asserts the default and `--swr`
layouts still accept the same fixtures, so "reject everything" cannot satisfy the suite. Both
transports are covered — `rest-react-query.ts.j2` and `rpc-react-query.ts.j2` are separate templates
emitting the same two families, so a REST-only fix would have left the hazard live on RPC.

The three rejection tests were observed failing against the pre-fix generator, not merely passing
after.

**Correction to the ticket:** #802 predicted `tsc` would report TS2300. It does not. Generating the
collision fixture with the check disabled and running real `tsc` reports **TS2393** (duplicate
function implementation) and **TS2323** (cannot redeclare exported variable). The error message names
the codes it actually produces, so a user grepping their build output finds it.

### `cbor-example-verify`'s Linux marker capture polls for readiness instead of sleeping a fixed 15s window (#753)

The Linux half of `cbor-example-verify` ran the built example under `xvfb-run` with `timeout 15
... || true`, checking for the round-trip marker exactly once, after a fixed 15s had already
elapsed — so a cold GTK/EGL start slower than that (observed in CI: a DRI3 device-lookup failure
forcing a software EGL fallback) failed the job even though the app would have printed moments
later. `|| true` also discarded the binary's exit status, so a crash, a non-zero exit, and a slow
start all produced the identical "did not print the expected round-trip marker" message.

This is the Linux analogue of the iOS fix in #720/#722: the recipe now backgrounds the run under
`timeout` and polls the captured log for the marker up to a 45s deadline, ending the moment it
appears rather than always paying the full window. The binary's real exit status is captured via
`wait` and reported distinctly — a timeout reads `timeout`'s own well-known status 124 and is
reported *as* a timeout, while a process that exited on its own without printing the marker names
its exit status instead of only saying "missing marker". Verified locally that `timeout`, not a
plain `kill` on `xvfb-run`'s own pid, is required to reach the whole process tree `xvfb-run` starts
without `exec`ing away, so early-exit kills reliably clean up the wrapped Flutter binary rather than
orphaning it.

Decided explicitly, per the ticket's open question: marker-printed-then-non-zero-exit is treated as
a **pass** — under `xvfb-run` the app can exit untidily on teardown (AT-SPI/DBus noise), and the
round trip this recipe exists to prove has already completed by the time the marker is captured.

Windows and Android verify recipes are out of scope for this ticket and unchanged, even though the
Windows recipe has the same fixed-timeout-plus-`|| true` shape.

### The changelog tooling stops arming its own next false alarm, and starts checking where entries land (#739, #740)

Two related fixes to `.ci/`'s changelog tooling, sharing the test harness in `changelog-seed-tests.sh`.

`changelog-seed.sh`'s no-op auto-fill scope (`CHANGELOG_NOOP_SCOPES`) included a package's own
`CHANGELOG.md` in the range it counted commits over, so a docs-only edit to that file — including the
hand-written fix for the *previous* occurrence of this exact defect — counted as a "functional change"
to the package and armed the placeholder seed on the next bump (`#728` → hand-fixed by `#731` → caused
`#736` → hand-fixed by `3de442b8` → 0.8.14 armed). The scope now excludes the changelog file itself via
a `:(exclude)` pathspec derived from the declared map key, so it cannot go stale as packages are added.
Verified against this repo's own `v0.8.9..v0.8.10` and `v0.8.12..v0.8.13` ranges: a changelog-only
commit is now correctly excluded, and a real source change in the same range is still counted (#740).

`changelog-check.sh` only ever grepped for the unedited-seed TODO marker; it never examined WHERE an
entry landed, so an entry misfiled under an already-released `## X.Y.Z (date)` section instead of
`## Unreleased` passed clean (`#672`, `#680`, `#686`, and most recently `#737`, four releases behind
where its feature actually shipped). The check now diffs a PR's changes against its base ref and flags
any newly-added `### ` entry whose nearest preceding `## ` heading is a dated section that already
existed before the diff — naming the file, the line, and the offending section. Scoped to lines the PR
itself adds (historical misfilings already on `main` do not retroactively fail future PRs), and a
release bump's legitimate promotion of `## Unreleased` into a freshly-created dated section is not a
violation, whether or not that promotion also adds entries under the same, newly-created heading (#739).
### `cratestack-vscode` ships an extension icon (#782)

The extension declared no `icon`, so the Marketplace, Open VSX, and the in-editor Extensions sidebar
after a manual `.vsix` install all rendered the generic grey placeholder. For a pre-1.0 framework
asking people to trust its codegen, an unbranded listing reads as abandoned or unofficial — and it is
cheap to fix now, awkward once a listing is live and indexed.

`packages/cratestack-vscode/icon.png` is a 256x256 PNG (Marketplace rejects SVG and enforces a
128x128 floor; 256 is the safer HiDPI source), paired with a `galleryBanner`. There was no CrateStack
mark to derive from, so the artwork was approved by the maintainer rather than defaulted by the
implementation — the ticket called that out specifically, to avoid a placeholder becoming permanent
by being first. The mark is a stack of three isometric crates.

Verified inside the built archive rather than in the source tree, since `.vscodeignore` is a denylist
and a future entry could exclude a file that exists on disk: `unzip -l ./*.vsix` lists
`extension/icon.png`, and its sha256 matches the committed file. The field is platform-independent,
so all five `vsce_target` builds carry it.

`test/icon.test.js` guards the manifest half offline — the field exists, resolves to a real file, and
that file is a square PNG of at least 128x128 (dimensions read from the PNG IHDR chunk, no image
dependency). Proven by breaking it both ways the ticket names: removing the `icon` field fails three
assertions, and renaming the file on disk while leaving the field in place fails two. Neither break
disturbs the build, the lint, or `vsce package` — which is exactly why the test exists.

### Every generated dependency constraint is an API floor, not the workspace version (#779)

#754 established the rule — *a generated dependency constraint states an API compatibility
requirement; it is never derived from `CARGO_PKG_VERSION`, at any precision* — and applied it to two
of the six sites that had the defect. The remaining four are fixed now: `cratestack_cbor` in both the
default and riverpod Dart pubspecs, and `@cratestack/refine` and `@cratestack/cbor` in the generated
`package.json`.

Each one independently reopened the release-cycle window #754 closed. `just bump` moves the workspace
version *before* the tag that publishes the packages, so anything derived from it names a version the
registry cannot serve for the whole window. The `@cratestack/cbor` site is the subtle one: it already
used a `^{major}.{minor}.0` floor rather than an exact pin, which survives a patch bump — but a minor
floor is still derived from the current version, so it would move to `^0.9.0` at the 0.9.0 bump and
name an unpublished package until that release landed. Narrowed from "every bump" to "every minor
bump", not closed. All four are now constants, so generator output is no longer a function of the
release version at all.

The payoff beyond correctness: **`just regen-examples` no longer passes `--no-native-cbor`**, and
`examples/flutter-riverpod/client` demonstrates the default native codec instead of the fallback.
That flag was hardcoded specifically to dodge this defect (#707) — a committed example pinning the
workspace version drifts on every bump and cannot resolve during the release window — and the
justfile's own comment called the trade "real and accepted" only because the pin format forced it.
With a constant, neither failure is reachable.

The floors were chosen against the registry, not the changelog, because #754's receipt is that a
hand-written floor read `^0.8.8` — a version pub.dev never published. Every candidate was checked by
unpacking the published archives and grepping the shipped `lib/`/`dist/*.d.ts` for the exact
identifiers the templates reference.

Two guards, both demonstrated failing rather than merely passing:

- The tests that assert the emitted constraint used to *recompute* the expected value from
  `CARGO_PKG_VERSION`, so they agreed with the generator by construction and stayed green while the
  emitted range walked off the registry. They now assert a literal, which disagrees the moment the
  generator starts moving with `just bump` again — on the bump PR, which is where the damage lands.
- A new `just verify-typescript-floors` (called by CI, so the recipe *is* the check) generates a
  client and typechecks it against the **exact** floor versions. A plain `npm install` resolves the
  caret to the newest match and proves only the ceiling; pinning is what tests the floor. Confirmed
  by deliberately lowering `@cratestack/refine` to `0.7.14`, which predates the `ResourceMap` type
  the generated `refine.ts` imports: `tsc` exits 2 with `TS2307`. The Flutter job's existing
  resolve-at-the-floor step gained `cratestack_cbor` on the same terms.

**One known gap, stated rather than absorbed.** No *published* `@cratestack/cbor` encodes a
`Uint8Array` as a CBOR byte string — as of `0.8.14` a `Bytes` field still reaches the wire as a CBOR
map that no server-side `Vec<u8>` can decode. #783/#787 fixed that, but the fix has not shipped
(`0.8.14` published the day before #787 merged). The honest floor for a `Bytes`-carrying schema is
therefore the first release containing #787, and the constant must be raised once it publishes. It is
deliberately not raised pre-emptively: naming an unpublished version is the exact defect this ticket
removes, and it would break every `npm install` today. The gap is unchanged by this PR — the
`^{major}.{minor}.0` being replaced already emitted `^0.8.0`.

### The generated Dart client caches its CBOR codec for successes only (#798)

Both generated Dart runtimes resolved `cratestack_cbor`'s `createCborCodec()` through a plain
`_cratestackCborCodecFuture ??= createCborCodec()`. That never re-evaluates once the field holds a
settled *rejected* future, so a single transient failure — a wasm asset that 404s on web, a vendored
library that was not there yet — bricked every later request in the isolate, replaying the same
error instead of retrying. Nothing about the failure was permanent; only the cache made it so.

Both now clear the cached future on failure and rethrow with the original stack trace, so the next
request gets a fresh attempt while a *successful* resolution stays cached exactly as before. This
brings the Dart runtimes in line with `@cratestack/cbor-web`'s `ensureInitialized()`, the generated
TypeScript RPC runtime's `resolveCodec()`, and `cratestack_cbor`'s own `createCborCodec()` (#794),
which had all already been fixed for this.

REST and RPC changed together, as the transport-parity rule requires — the cached accessor is
generated twice, once in `rest-runtime.dart.j2` and once in `rpc_runtime/types.dart.j2`, so fixing
one would have proven nothing about the other.

The new test is behavioral, not a source-text match: it generates a real package per transport,
points it at a stub `cratestack_cbor` whose factory fails on demand, and runs it under `flutter
test`. Asserting the fixed *expression* appears in the rendered template would repeat the mistake
the TypeScript suite already made once — a string match on the buggy line kept passing with the bug
fully present.

### `cratestack_cbor`'s `createCborCodec()` no longer throws on a second call (#794)

An app that uses CBOR directly and also has a generated `transport rpc` Dart client ends up with
two independent initialisers: its own `createCborCodec()` call, and the one the generated client's
RPC codec makes from inside its own runtime, against `package:cratestack_cbor` directly. The second
threw `Bad state: Should not initialize flutter_rust_bridge twice`. Neither call site is wrong,
neither is reachable from the other, and the generated client cannot be told to reuse an existing
codec. It surfaced only against a live service — everything compiled and every offline suite passed.

`createCborCodec()` is now idempotent by two independent mechanisms, because the `bool` flag it
replaced provided neither. The returned `Future` is memoized, so concurrent callers share one
initialization rather than both seeing "not initialized" and both calling `init`. And the `init`
itself is guarded on flutter_rust_bridge's own state rather than on a flag private to the library,
which covers what memoization structurally cannot: a consumer that already bootstrapped the bridge
through its own path. Only a *successful* initialization is memoized — the same rule, for the same
reason, as the generated TypeScript RPC runtime's `resolveCodec()`.

A new `isCborRuntimeInitialized` is exported next to `createCborCodec` on every platform, so a
consumer with its own bootstrap can ask instead of guess.

Separately, and the reason such bootstraps get written at all: under `flutter test` the package
could not resolve its vendored library, because `flutter_tester` does not implement
`Isolate.resolvePackageUriSync`. That did not fail softly — it threw `Unsupported operation`, and
the only workaround was `CRATESTACK_CBOR_NATIVE_LIB`, an env var a Dart process cannot set for
itself. Resolution now falls back to reading `.dart_tool/package_config.json` directly, walking up
from the working directory so nested directories and pub workspaces resolve too. The two problems
compounded: the workaround for the resolution gap is what created the double-init. The package's
own suite now runs under `flutter test` as well as `dart test` in CI, with the env var explicitly
unset, so neither half can regress unnoticed.

### The parser rejects a schema name that collides with the generated client's own methods (#784 follow-up)

Found while checking a claim made when #784 was closed, which turned out to be wrong. That comment
said the generated Rust client had an equivalent, unfixed collision between a model accessor and a
procedure method — `model Order` alongside `procedure orders`. It does not: accessors live on
`Client` and procedure methods on `ProceduresClient`, so the two never share a namespace, confirmed
by `cargo check` rather than by re-reading the templates. The claim is retracted on the issue.

Checking it did turn up two real collisions, on the same surface and of the same class #784 closed —
a schema-derived method landing on one the client defines for itself:

- `model Procedure` derives the accessor `pluralize(to_snake_case("Procedure"))` = `procedures()`,
  which `Client` already defines.
- `procedure new` derives `to_snake_case("new")` = `new()`, which `ProceduresClient` already
  defines.

Both compiled to `error[E0592]: duplicate definitions with name ...`, naming neither the model nor
the procedure that caused it. `cratestack-parser`'s `validate::client_method_collisions` now rejects
both up front, naming the declaration, the derived method, the built-in it hits, the rustc error it
would have produced, and the remedy.

The check covers `new`/`runtime`/`procedures`/`rpc`/`batch` — the union across both transports —
even though `pluralize` only appends, so `procedures` is the only one a model accessor can actually
reach today. Checking the whole list means a built-in added to `Client` later is guarded without
anyone re-deriving that argument.

Deliberately not rejected, with tests pinning each: a model accessor matching a *procedure* method
name (different types, compiles); an all-caps `model PROCEDURE`, which `to_snake_case` normalizes to
`p_r_o_c_e_d_u_r_e` and so does not collide; and `procedure runtime`/`procedures`/`batch`/`rpc`,
since those built-ins are on `Client`, not `ProceduresClient`. Model-vs-model accessor collisions
were already rejected by `route_collisions` — an accessor name *is* the REST route segment.

This affects both roles: the client module is emitted by `include_server_schema!` (for the server's
own peer-calling client) as well as by `include_client_schema!`.

### `@cratestack/refine` converts every method's errors the same way, and stops destroying them (#786)

The data provider handled a thrown error two different ways depending on which method threw.
`getList` and `getMany` had no `catch` at all and rethrew the original untouched; `getOne`,
`create`, `update` and `deleteOne` each ran it through `toRefineError`, which returned a **plain
object literal** for anything that was not a `CratestackHttpError` — discarding the value's class,
`name`, `cause` and every own property, leaving only `message`.

A consumer throwing a typed error from a custom `fetch` transport (a `DeviceNotEnrolledError`
raised before the request ever leaves the browser) and classifying it with `instanceof` therefore
got **correct behaviour on list screens and silently wrong behaviour on every detail/create/edit
screen** — a bug that shipped, because the retry classifier was validated against the one list-only
resource, which is the single path where `instanceof` held. The workaround it forced was
string-matching exported message constants, the only field that survived flattening.

Both halves are fixed:

1. **All seven methods** — the six named in the report plus `custom`, which had the same gap —
   now route thrown values through `toRefineError`. Leaving `custom` out would have recreated the
   defect one method over.
2. **`toRefineError` annotates the thrown value in place and returns it**, rather than building a
   bare object literal. `message` and `statusCode` (the two fields refine renders) are set on the
   original object, so `instanceof` keeps working, `name`/`cause`/own properties survive, and a
   `CratestackHttpError`'s `status`/`payload`/`response` stay readable on the same value. Mutating
   rather than cloning is deliberate: an `Object.create`-based copy preserves the prototype but
   silently drops private class fields, which is the state a typed error's own methods read.

When the thrown value cannot be annotated — a thrown primitive, a frozen or sealed object, a class
exposing `message` as a getter with no setter — the result is `{ message, statusCode, cause }` with
the original under `cause`, the report's stated minimum.

Unchanged: the `412 Precondition Failed` conflict message and the promotion of a
`CratestackHttpError` envelope's `payload.message`. `message` remains the one field conversion
rewrites, so it remains the wrong field to classify on; the README's new "Errors" section says so.

Behavioural note for existing consumers: `getList`/`getMany` errors now carry `statusCode` (they
previously reached refine with none), and a 412 surfaced from a list call now carries the conflict
message rather than the raw one.
### **Breaking:** generated TypeScript clients type `Bytes` as `Uint8Array` (#783 follow-up)

A schema `Bytes` field is now a real `Uint8Array` on **both** sides of a generated TypeScript
client, not a `number[]`. This is what the Dart client has always done (`Bytes` → `Uint8List`,
converted at the wire boundary in `wire_encode.rs`/`wire_decode.rs`), so the two clients now agree.

**What breaks.** Reading a `Bytes` field as `number[]` no longer compiles:

```ts
const digest: number[] = blob.digest;        // was fine, now a type error
const digest: Uint8Array = blob.digest;      // the replacement
Array.from(blob.digest);                     // if you genuinely want a number[]
```

Writing one gets easier, which was the point — `client.seal({ payload: bytes })` instead of
`client.seal({ payload: Array.from(bytes) })`. Node `Buffer` works too (it is a `Uint8Array`
subclass).

**The wire is unchanged.** A `Bytes` field still travels as an array of integers in both
directions, so this is a client-side type change only — no server, Dart, or Rust client is
affected, and a mixed-version fleet is fine. The conversion happens in the generated runtime:
`encodeBinaryAsJson` on the way out, and the `bytesKeys`/`bytesListKeys` arms of the shape walk on
the way back.

Two details worth knowing:

- **Why not `Uint8Array | number[]` on inputs only.** A union has to be narrowed by every *reader*,
  and it cannot be applied consistently anyway: a `type` block is a single generated interface that
  can sit in an argument position, a return position, or both (`procedure seal(env: Envelope):
  Envelope`), so there is no input-only place to widen. Model interfaces and `Create`/`Update`
  inputs *are* cleanly split, but `type` blocks are not — one type in both directions has no such
  ambiguity.
- **Why the runtime converts rather than the codec.** `JSON.stringify` turns a `Uint8Array` into an
  index-keyed object (`{"0":1,"1":2}`) that no server-side `Vec<u8>` can decode — the same defect
  #783 fixed for CBOR, in a different disguise. `encodeBinaryAsJson` runs on the JSON paths only
  (`jsonRpcCodec` and the REST request body); the native `@cratestack/cbor` codec keeps receiving
  the real `Uint8Array` so it can emit a compact byte string. It is a pre-walk rather than a
  `JSON.stringify` replacer because Node's `Buffer` defines its own `toJSON`, which
  `JSON.stringify` applies *before* any replacer, yielding `{"type":"Buffer","data":[...]}`.

**Renamed generated exports.** The decode-side registry now carries `Bytes` as well as `Decimal`,
so its names no longer say "decimal": `decimalShapes` → `wireShapes`, `DecimalShape` → `WireShape`,
`reviveDecimalFields` → `reviveWireFields`, `revivePagedDecimalFields` → `revivePagedWireFields`,
`reviveDecimalScalar` → `reviveWireScalar` (now taking a second `kind` argument),
`encodeDecimalFields` → `encodeWireFields`. These are generated-client internals; application code
rarely imports them, but a client with a customised template will need the rename.

`Bytes` keys are recorded per arity (`bytesKeys` vs `bytesListKeys`) because the wire form is not
self-identifying at every value: a populated `Bytes` is `number[]` and a populated `Bytes[]` is
`number[][]`, but `[]` is both. The schema knows which; the runtime cannot.

### `generate-dart` stops emitting dead imports for a schema with no models (#785)

`cratestack generate-dart` on a schema with zero `model` blocks emitted `import 'queries.dart';`
into `lib/src/apis.dart` and `import 'models.dart';` into `lib/src/queries.dart` unconditionally,
with nothing on either side to reference them. `flutter analyze` reports each as `unused_import` —
a warning, and `--fatal-warnings` (Dart's own default, and what `just verify-dart` runs) makes a
warning a failed build. The reporting consumer had to add a `knownAnalyzeFailures` allowlist entry
to work around it, which silently downgrades a future real regression to a warning.

Both are now gated on the loop that actually consumes them: `apis.dart`'s `queries.dart` import on
`model_apis`, and `queries.dart`'s `models.dart` import on `selection_models`. Same one-line
mechanism as #629's `{% if procedures | length > 0 %}`, which fixed this defect one level up (a
class body rather than an import line) and does not reach import statements.

A third, unreported case is closed with it: `apis.dart`'s own `models.dart` import is live for a
procedure-only schema (every procedure gets a generated `{Procedure}Args` wrapper there) but dead
for a schema with **neither** models nor procedures, which is valid today. That gate is applied on
both transports — `rpc-apis.dart.j2` carries the same import and the same condition.

The riverpod preset was already correct: its `queries.dart` never imported `models.dart`.

Verified end-to-end rather than by inspection. A `procedures_only_rest` fixture joins
`just verify-dart`'s default-preset list, where `verify_pkg` runs real `flutter pub get` →
`build_runner` → `flutter analyze --fatal-warnings`. Nothing else in that list has a zero-model
shape, which is why this shipped: a dead import is invisible to text-level generator tests and only
a real analyzer fails on one. Confirmed the fixture reports the two warnings on the pre-fix
templates and `No issues found!` after — and likewise for the zero-model/zero-procedure schemas on
both transports (three warnings before, clean after).
### The parser rejects a procedure colliding with a model's generated CRUD handler (#784)

`model Order` alongside `procedure getOrder` generated the Rust item `handle_get_order` twice into
the same axum module — once from `axum/model/prep.rs`'s per-model CRUD handlers, once from
`axum/procedure.rs`'s per-procedure handler — and the same for the `_dispatch` twins the RPC
transport dispatches through. `cratestack check` reported `schema OK`; the only diagnostic was a raw
`error[E0428]: the name 'handle_get_order' is defined multiple times`, which names neither the
procedure, nor the model, nor the fix. It cost two rounds of guess-the-cause in production porting
work (`deleteBuyerAddress` vs `model BuyerAddress`, `getOrder`/`getSubOrder` vs `model
Order`/`SubOrder`).

`cratestack-parser` now refuses such a schema, naming the procedure, the model, the operation, the
shared identifier, and the remedy — a fifth validator in the mould of `snake_case_collisions`,
`route_collisions`, `builder_collisions` and `procedure_idents`. Detection runs on the
`to_snake_case`-normalized form, so `procedure get_order` is caught identically, and it covers both
the handler and its `_dispatch` twin, so `getOrderDispatch` is caught too. `list`/`create` are
matched against the *pluralized* stem (`handle_list_orders`) and `get`/`update`/`delete` against the
singular one, mirroring the macro exactly.

`@@internal(...)` is deliberately not an exemption: route suppression omits the `.route(...)`
registration, not the handler function, so the ident collides either way.

The collision is proved against the real emitters rather than a re-derivation —
`cratestack-macros/src/axum/handler_collision_tests.rs` runs `generate_model_axum_handlers` and
`generate_procedure_axum_handler` for the reported pair and asserts the emitted `fn` names
intersect on exactly `handle_get_order` and `handle_get_order_dispatch`, and that the issue's
recorded workaround rename clears it.

Four in-repo fixtures were themselves this defect and are renamed (`listPosts` → `searchPosts`,
`getWidget` → `widgetSummary`, `listUsers` → `searchUsers`, `listOrders` → `searchOrders`). One of
them is #777's `--swr` collision fixture, which now exercises `create` rather than `list`: `create`
is the only one of the five operations whose `--swr` free function (`createPost`) and generated
handler (`handle_create_posts`) disagree on plurality, and so the only one that is a
`--swr`-specific collision rather than an `E0428` the parser now catches first. #777's
generator-level check is unchanged and still owns the cases this one cannot see.
### `Bytes` fields round-trip a JS `Uint8Array` (#783)

`@cratestack/cbor` serialised a JS `Uint8Array` as a CBOR **map** of index→value
(`{"0":1,"1":2,…}`) rather than a byte string, because both its builds funnelled every JS value
through `serde_json::Value` — a type with no byte-string variant, and one whose napi conversion
classifies a typed array as a plain object. A generated Rust `Bytes` field is a `Vec<u8>` whose
blanket `Deserialize` accepts only a sequence, so such a request failed at the codec with
`400 invalid_argument` and never reached the handler. Callers had to write `Array.from(bytes)` at
every call site — a workaround that is easy to "optimise" back into a break, and that costs ~2x the
wire bytes of the byte string it stands in for (as measured in the issue, on a random-filled 16 KiB
payload: 31,180 bytes as `number[]`, ~16,400 as a byte string, and 118,374 in the broken map form).

Both codec builds now bridge through `cratestack_core::Value`, the framework's own canonical wire
value, which has a `Bytes` variant:

- **`encode`** — `Uint8Array` (including Node `Buffer`) and `ArrayBuffer` become a CBOR byte string
  (RFC 8949 major type 2). A subarray contributes its own window, not the whole backing buffer.
  Everything else — `Uint8ClampedArray`, `DataView`, `Int32Array`, … — keeps its previous behaviour
  rather than being silently reinterpreted; pass
  `new Uint8Array(view.buffer, view.byteOffset, view.byteLength)` for those. That set is identical
  in the node and web builds on purpose: a TypeScript client has to put the same payload on the wire
  whichever runtime it loads in, so the node build accepts no more than the web build can.
- **`decode`** — a CBOR byte string comes back as a `Uint8Array`.
- A plain `number[]` is unchanged in both directions. An untyped value carries no schema, so nothing
  at the codec layer guesses that an integer array "meant" bytes.

For that to work end to end, the server had to accept the shape. A schema `Bytes` field — on a
model, a CRUD input, a `type` block, or a procedure argument — now deserializes from **either** a
CBOR byte string or the integer array every already-deployed Rust/Dart/TypeScript client sends
(`cratestack_core::lenient_bytes`, attached by `cratestack-macros`' `shared::bytes_serde`). That
keeps `application/json`, where the integer array is the only expressible shape, working untouched,
and it makes this an additive change rather than a wire break: **nothing about the outbound shape
changed.** A `Bytes` field still serializes as an integer array on both transports and in all three
client languages, so every existing decoder — the Dart client's `cratestackAsValueList`, the
TypeScript client's `number[]` — keeps working. Flipping the outbound shape would be a real break
and is deliberately not bundled here.

Two knock-on effects of the bridge-type change, both edge cases of `Value`'s number model (already
the framework's wire contract for `Json` fields, procedure `Json` arguments and RPC error details):
an integer above `i64::MAX` (a JS `BigInt` past 9223372036854775807) now degrades to a float instead
of staying an exact unsigned integer, and a non-finite float (`NaN`, `±Infinity`) now survives as
itself rather than decoding as `null`. Every byte-free payload is otherwise byte-identical, pinned
by the cross-language fixtures both the Rust and vitest suites assert.

Two limits worth naming: `POST /rpc/batch` carries each frame's input as an opaque
`serde_json::Value`, so a byte string inside a *batched* frame still fails at the envelope — send
`number[]` there, or use unary RPC. And the generated TypeScript client still types `Bytes` as
`number[]`, so passing a `Uint8Array` needs a cast until that type is widened.

## 0.8.14 (2026-08-27)

### Generated Dart clients declare an API floor, not the workspace version (#754)

A generated Dart client used to declare `cratestack_annotations: ^{workspace version}` and
`cratestack_builder: ^{workspace version}`. Because pub.dev publishing runs off a tag pushed *after*
the version-bump PR merges, that meant every generated client spent each release cycle naming a
version pub.dev could not serve yet. It took down `Prepare Release` for 0.8.14: five snapshot
fixtures, three `flutter pub get` tests, and `just regen-examples --check`, from one cause.

Both requirements are now **API-compatibility floors** — constants in
`cratestack-client-dart/src/package_floors.rs`, used by the default and riverpod presets alike, that
name the earliest release whose annotation surface the generated code actually needs (`^0.8.10`).
A constant never moves with the release, so it can never name an unpublished version at any bump
size — unlike a "minor floor" (`^{major}.{minor}.0`), which still moves at a minor bump. The wider
effect is that generator output stops being a function of the release version at all, so the
committed snapshots and example clients survive a bump instead of being invalidated by it.

**Behavioural change for consumers:** a regenerated Dart client now asks for `cratestack_annotations`
/ `cratestack_builder` `>=0.8.10 <0.9.0` rather than pinning the current release. Pub resolves that
to the newest 0.8.x, so the packages a client actually gets are unchanged today. After 0.9.0 ships,
generated clients keep resolving 0.8.x until the floor is deliberately raised — staleness rather than
breakage, and raising it is the considered act the rule prescribes.

**A correction that came out of this.** `dart-packages/cratestack_builder/pubspec.yaml` declared
`cratestack_annotations: ^0.8.8`, justified in `docs/tooling/dart-publishing.md` as "the first
release with `touchFlagFields`/`nonDefaultingListFields`". Checked against pub.dev's API and the
published archives, both halves were wrong: **0.8.8 was never published** (0.8.8/0.8.9 were skipped
releases, so versions run 0.8.7 → 0.8.10) and **0.8.7 contains neither identifier** — 0.8.10 is the
first that does. It was harmless only because a caret resolves upward. The declaration and the doc
paragraph are corrected, and because that is a hand-maintained floor rotting before anything relied
on it, the new floors are backed by checks rather than by comments:

- Unit tests assert the emitted floor is at least the floor `cratestack_builder`'s own pubspec
  declares (read from that file, so raising one flags the other), and that both sit strictly below
  the current, not-yet-published workspace version.
- CI's `flutter (flutter-riverpod example)` job now resolves the committed client at the *exact*
  floor via a generated `pubspec_overrides.yaml` and re-analyzes it. Verified to fail as intended:
  pinned to 0.8.7, `flutter analyze` reports `undefined_named_parameter` on the emitted
  `@CratestackBuilder(nonDefaultingListFields: …)` call sites. Note `build_runner` alone does *not*
  catch it — an older builder silently ignores an argument it does not know, exits 0, and reports
  outputs written; that is why the check is `flutter analyze`.

Unchanged: what the packages themselves publish as. `just bump` still moves every
`dart-packages/*/pubspec.yaml` `version:` in lockstep with the Cargo workspace, which pub.dev's
`v{{version}}` tag pattern requires. Also unchanged, and tracked separately: `cratestack_cbor` (Dart)
and `@cratestack/refine` / `@cratestack/cbor` (npm) still derive their emitted requirements from the
release version.

### `--swr` rejects a procedure whose name collides with a generated model function (#777)

`--swr` is the only TypeScript layout that exports a model's CRUD operations as top-level free
functions (`listPosts`/`getPost`/`createPost`/…, derived from the model name), and its
`src/swr/index.ts` barrel `export *`s both `./models/<model>.js` and `./procedures.js`. A schema
declaring `model Post` alongside `procedure listPosts` therefore put two bindings of the same name
into that barrel, and the generated package failed to compile — `tsc` reported
`TS2308: Module "./models/post.js" has already exported a member named 'listPosts'`. Generation
itself exited 0 with no warning, so the failure only surfaced at the consumer's own build.

`generate-typescript --swr` now refuses such a schema up front, before any file is written, naming
the procedure, the model, the operation and the shared identifier. The collision is detected on the
`to_camel_case`-normalized form, so `procedure list_posts` is caught too, not only an
already-camelCase spelling. Suppressed operations are exempt: a name that `@@internal` or a missing
`create` rule keeps out of the generated file cannot collide, and `get<Model>WithResponse` is
checked for REST schemas only, since the RPC template never emits it.

This mirrors #344's precedent (two models whose kebab-case file names collide) in both placement and
stance: the check lives in the generator that owns the naming scheme rather than in
`cratestack-parser` — per decision spike #317 — so a schema that never passes `--swr` is unaffected,
and it fails loudly rather than silently picking which of two public function names to rename.

The default (non-`--swr`) layout was never exposed: its model operations are methods on per-model
client classes (`client.post.list(...)`), leaving nothing for a top-level procedure function to
collide with. `--refine` adds no comparable surface. `--tanstack` has the same category of hazard
structurally (model and procedure hooks share `src/react-query.ts`) but is not addressed here and no
committed fixture triggers it — see #777's "Out of Scope".

The `ci_rest.cstack`/`ci_rpc.cstack` verification fixtures rename their `listPosts` procedure to
`searchPosts` accordingly; this is why `just verify-typescript`'s `--swr` + RPC leg, added in #776,
is green again.

### `--swr` + `transport rpc` now honours `native_cbor` too — breaking (#765)

`cratestack generate-typescript --swr` on an RPC-transport schema emits `src/swr/runtime.ts` from
the same `rpc-runtime.ts.j2` template as the default layout's `src/runtime.ts`, but the two were
rendered through independently-maintained context structs. `SwrSchemaContext` had no `native_cbor`
field, so every `{% if native_cbor %}` site in the shared template silently evaluated falsy
(minijinja's `UndefinedBehavior::Lenient` treats an undefined condition as false rather than
erroring) regardless of the actual flag — `src/swr/runtime.ts` always emitted the plain
`jsonRpcCodec` fallback, even though `@cratestack/cbor` became the default RPC codec for the
default layout in #746/#752. One generated package therefore shipped two runtimes that disagreed
about the wire codec: `src/runtime.ts` spoke `application/cbor`, `src/swr/runtime.ts` spoke
`application/json`, for the same models.

`SwrSchemaContext` now carries `native_cbor` (mirrored from `TypeScriptGeneratorConfig::
native_cbor`, same as the default layout's `TemplateContext`), so `src/swr/runtime.ts` now resolves
`@cratestack/cbor`'s `createCborCodec()` by default exactly like `src/runtime.ts` does, and
`--no-native-cbor` turns both off identically. No template changes were needed — `rpc-runtime.ts.j2`
already branched on the field name correctly; it was only ever missing from one of its two callers'
contexts.

**This is the same breaking change #752 already made and changelogged for the default layout,
applied here for real:** regenerating a `--swr` + `transport rpc` client's wire codec changes from
`application/json` to `application/cbor`. A server built against a JSON-only `CodecSet` will now
reject a regenerated `--swr` client's requests with `406 Not Acceptable` / `415 Unsupported Media
Type`, same as it already does for the default layout — `--no-native-cbor` (or an explicit `codec:
jsonRpcCodec` passed to the constructor) restores the JSON-only behavior exactly. Anyone relying on
the previous divergence was relying on a bug: the `--swr` and default layouts now agree by
construction rather than by coincidence.

A new `--swr` + `transport rpc` snapshot fixture (`tiny_rpc_swr_native_default`) closes the gap that
let this ship unnoticed in #746/#752 — no such fixture existed before this ticket.

### Schema validation reports every independent error, not just the first

`cratestack-parser` gains `parse_schema_diagnostics`, which returns all independent
problems in a schema instead of stopping at the first. `cratestack-lsp` uses it, so an
editor now shows three squiggles for three unknown field types rather than handing them
over one save at a time.

Validation runs in **stages**, and a stage runs only when every earlier stage was clean.
That is not caution for its own sake: several validators document that they assume an
earlier one passed (`validate_computed` "may assume every `@computed` attribute is already
known to be bare, unique, and on a declaration kind that supports it"), and
`collect_type_names` produces the very name set the per-declaration stage checks against.
Running a later stage over already-rejected input produces cascades of nonsense pointing at
the wrong places, which is worse than one real error. Within a stage, declarations are
independent and all of them report.

Two properties are pinned by tests because they are what make this safe:

* **The first collected error is exactly the error `parse_schema_named` returns.** Both go
  through one set of checks in one order — the fail-fast entry point is now literally the
  head of the collected list — so the two paths cannot drift into disagreeing about what is
  wrong with a schema. The crate's existing 303 tests are the regression guard for that and
  all still pass unchanged.
* **A syntax error still yields exactly one diagnostic.** Parsing has no recovery, so
  everything after the failure is unparsed rather than valid, and inventing further errors
  from it would be guessing.

**No behaviour change for existing consumers.** `parse_schema`, `parse_schema_named` and
`parse_schema_file` keep their signatures and their first-error semantics; the macros, CLI
and migrate paths are untouched.
### `.cstack` rename (F2)

`cratestack-lsp` now answers `textDocument/rename` and `textDocument/prepareRename`.
Renaming a model, type, enum, mixin, procedure, field or enum variant rewrites its
declaration and every reference in one edit — including `@relation(fields:/references:)`
columns and `@use(Mixin)` directives.

It reuses the reference index that already answers find-all-references rather than growing
a second notion of "everywhere this appears": a rename that disagreed with Shift+F12 would
be a rename that misses call sites.

Rename is the first request here that *writes*, so it is held to a stricter standard than
navigation. A go-to-definition that lands a line off is an annoyance; a rename computed
from the wrong offsets rewrites the wrong text and is easy to miss in a diff. It therefore
refuses rather than guesses:

* **While the file has a syntax error.** This is the one that matters most. Since #767 the
  server retains the last schema that parsed, and navigation happily works from it — but
  its spans describe text the buffer no longer holds, so an edit computed from them would
  land at the wrong positions. `prepareRename` declines too, so no rename box appears.
* **On builtin types.** `String` and `Int` resolve as type references, but nothing declares
  them; rewriting every `String` in a file is not a rename.
* **On names that are not identifiers**, and on keywords or builtin type names — either
  changes how the file parses rather than what it calls something.
* **On a name already taken in that scope.** Scoped the way the language scopes it: field
  names only have to be unique within their owner, so `Post.id` does not block renaming a
  field on `User`.

All four refusals carry a message explaining why, rather than returning an empty edit —
a rename that silently does nothing is worse than one that says no.

### `.cstack` editor features no longer blink off on every syntax error

A failed parse used to drop the schema entirely, and every feature that needs one —
go-to-definition, find-references, hover, document symbols, semantic tokens — returned
nothing until the file was valid again. While someone is typing, a file spends most of its
time invalid, so in practice these features flickered on and off keystroke by keystroke.

The server now retains the last schema that parsed, together with **the exact text it was
parsed from**. That pairing is the correctness argument rather than an implementation
detail: spans index into the text that produced them, so resolving a retained span against
the current buffer would read bytes the parser never saw and silently land in the wrong
place — which looks like working navigation, just wrong. Providers take both halves from
one accessor (`DocumentState::resolved`) so the two can never be mixed.

Measured against the running server, opening a valid file, breaking it, then fixing it:

```text
                       before                        after
1. valid               definition=L0 tokens=8        definition=L0 tokens=8
2. mid-edit (broken)   definition=NONE tokens=0      definition=L0 tokens=8
3. fixed again         definition=L0 tokens=8        definition=L0 tokens=8
```

Two limits are deliberate. Diagnostics always describe the *current* text, so a retained
schema never suppresses a live error. And a document that has never parsed keeps nothing —
there is no schema to fall back to, and inventing one would be worse than staying quiet.

Because results can now legitimately predate what is on screen, hover marks them: a stale
popup carries a one-line note rather than presenting itself as current.

### `.cstack` semantic tokens — identifiers coloured by what they resolve to

`cratestack-lsp` now answers `textDocument/semanticTokens/full`. This is what closes the
gap the TextMate grammar structurally cannot: `String` (a builtin scalar), `User` (a model),
`Role` (an enum) and `Timestamps` (a mixin) are four bare capitalised words to a regex and
four different things to a resolved schema. Models colour as `struct`, enums as `enum`,
mixins as `interface`, builtins as `type`, fields and `@relation` columns as `property`,
enum variants as `enumMember`, procedures as `function` and their arguments as `parameter`.

The tokens **supplement** the grammar rather than replace it. VS Code has no tree-sitter API
for third-party languages, so the grammar keeps doing what regexes do well — keywords,
strings, comments, available instantly before the server starts — and the server re-colours
identifiers on top. Only an attribute's `@name` head is a decorator, so the columns named
inside `@relation(fields: [...], references: [...])` keep colouring as the properties they
are rather than being swallowed into one attribute-coloured run.

One non-obvious case is pinned by a test: `expand_model_mixins` clones each mixin field into
every consuming model *keeping the mixin's spans*, so the same span is collected twice and
would emit a duplicate zero-width-delta token if it were not de-duplicated.

### Fixed: generated RPC clients crashed encoding a `Decimal` under the native `@cratestack/cbor` codec

Every RPC call whose request body carried a `Decimal` field — `create`/`update` model
inputs, procedure arguments, and each frame of a `batch()` payload — threw `JS functions
cannot be represented as a serde_json::Value` before the request was ever sent, once
`@cratestack/cbor` became the default codec (#746). `decimal.js`'s `Decimal` (the class
generated clients represent a `Decimal` field as) assigns `constructor` as an own
enumerable property pointing at a function; `jsonRpcCodec`'s `JSON.stringify` tolerated
this via `Decimal.prototype.toJSON`, but `@cratestack/cbor` walks a value's own
enumerable properties on its way to a `serde_json::Value` and never calls `toJSON`, so it
choked on that property instead.

Generated TypeScript RPC clients now export `encodeDecimalFields` (`src/models.ts`) — the
encode-side counterpart to the existing `reviveDecimalFields`/`reviveDecimalScalar`
decode helpers — and apply it to every outbound request body (`src/runtime.ts`'s
`terminalLink`/`src/stream-terminal.ts`'s `terminalStreamLink`), unconditionally rather
than gated on the codec choice: converting a `Decimal` to its `.toString()` form before
`JSON.stringify` is a byte-identical no-op for the JSON codec, so both codecs now share
one code path. Regenerate any RPC client committed before this fix (`just
regen-examples`, or your own `generate-typescript` invocation) to pick it up.

### `.cstack` navigation: enum/mixin go-to-definition, and find-all-references

`cratestack-lsp` now answers `textDocument/references` and
`textDocument/documentHighlight`, and go-to-definition covers the reference sites it
previously missed.

Go-to-definition already resolved model/type field types and both halves of
`@relation(fields: [...], references: [...])` — verified against the running server, not
just its unit tests. What did not resolve, and now does:

* **`enum` declarations.** A `role Role` field type resolved to nothing, because
  `declaration_span` walked models, types and procedures but never `schema.enums`.
* **`mixin` declarations and their fields**, including field types declared inside a
  `mixin` body.
* **`@use(Timestamps)`**, which needs recovering from source text: `expand_model_mixins`
  inlines a mixin's fields into each consuming model and then drops the `@use(...)`
  attribute from `Model::attributes`, so the reference site does not exist in the IR by
  the time the language server sees the schema.

Find-all-references works from either end of a relation — asking for references of
`User.id` surfaces the `references: [id]` site on `Post` — and treats `@use(Mixin)` as a
reference to the mixin. Field references are qualified by their owning declaration
rather than matched by name, so `User.id` and `Post.id` stay distinct symbols; a
regression test pins that. Mixin fields resolve to the mixin that declares them rather
than to whichever model inlined them, since `expand_model_mixins` clones those fields
into every consumer while keeping the mixin's spans, making both match by position.

Two related limitations are unchanged and now written down in the extension README:
diagnostics stop at the first parse error, and navigation goes quiet entirely while a
file does not parse, because a failed parse drops the schema with no last-known-good
fallback.

### Fixed: the published VS Code extension could not activate at all

Every `.vsix` attached to a GitHub Release failed activation with `Cannot find module
'vscode-languageclient/node'`. `main` pointed at an unbundled `extension.js`, but `.vscodeignore`
excludes `node_modules/**` and the packaging scripts pass `vsce --no-dependencies` (pnpm's symlinked
layout defeats vsce's npm-style dependency discovery), so the extension's only runtime dependency was
never in the package. The declarative contributions still loaded, so installs looked partly healthy:
users got TextMate syntax highlighting and silently no language server — no diagnostics, hover,
completion, go-to-definition or document symbols.

`main` is now `dist/extension.js`, an esbuild bundle built by `scripts/build.mjs` and produced during
packaging via `vscode:prepublish`, so `--no-dependencies` is a true statement. The release workflow
needs no change. Alongside it:

* `activate` no longer pushes `client.start()` into `context.subscriptions`. That returns
  `Promise<void>` in vscode-languageclient 7+, not the `Disposable` it returned in 6.x, so the
  extension registered a Promise for later disposal and left a rejected `start()` unhandled — a
  missing or unexecutable server binary failed with nothing in the UI. The client itself is now the
  registered `Disposable`, and a failed start reports the resolved command path.
* `engines.vscode` moves to `^1.91.0`, the floor `vscode-languageclient@10` actually requires
  (`^1.90.0` was declared).
* The package's stale nested `pnpm-lock.yaml` is deleted. It was git-tracked, unused (the workspace
  root lock governs), and pinned `vscode-languageclient ^9.0.1` against a `^10.1.0` manifest.

### Fixed: the VS Code extension package is now covered by CI

None of the above was caught because `packages/cratestack-vscode` declared no `build`/`test`/`lint`
scripts, so `turbo run build test lint` skipped it entirely — the `js` job's own comment recorded this
as expected. It now defines all three and participates in that job. Its VS Code integration suite was
also dead on arrival independently: it looked up the pre-rename `vaam-store.cratestack-vscode`
publisher ID, so `getExtension` always returned undefined; the ID is now derived from `package.json`.

The new `test/bundle.test.js` reproduces the packaged-VSIX environment without needing vsce or a
network — it loads the built entry point from a directory containing nothing but a `vscode` stub — and
was confirmed to fail with the original `MODULE_NOT_FOUND` when `main` is pointed back at the
unbundled source.

### `.upsert(..).run(..)` no longer reports `Created` for an update it lost a race on (#745)

The `ON CONFLICT ... DO UPDATE` upsert decided Created-vs-Updated from a `SELECT ... FOR UPDATE`
probe taken *before* the statement ran, and never reconciled that decision against what the database
actually did. When the probe found no row and a concurrent transaction committed a conflicting row in
the gap, Postgres serialized on the unique index and performed a genuine **UPDATE** — but the runtime
still enqueued a `Created` model event, wrote `AuditOperation::Create` with a `null` before-snapshot,
and skipped the update-policy gate entirely. The row handed back was correct; the audit trail and the
event stream described something that never happened. On the money-flow call sites this primitive
exists for, that is the record of what the service did.

The insert branch now runs `INSERT ... ON CONFLICT (<target>) DO NOTHING RETURNING ...` — the same
statement `.do_nothing()` already relies on — so the database itself answers "did I insert?". A
returned row means a genuine insert (unchanged: `Created`, `AuditOperation::Create`, no
before-snapshot, one statement, no extra round trip). No returned row means the race was lost *and the
winning row has not been touched yet*, so the runtime re-enters the update branch properly: it locks
the winner, runs the update policy gate, captures a real before-snapshot, and only then issues the
`DO UPDATE`. A lost race now emits `Updated` / `AuditOperation::Update` with the winner's row as its
`before`.

Nothing changes off the race path: the uncontended insert and the probe-predicted update emit exactly
the events, audit operations and snapshots they emitted before. `.upsert(..).do_nothing()` is
untouched — it already resolved its outcome from the database — and `UpsertOutcome`'s public shape is
unchanged. Deliberately **not** implemented with `RETURNING (xmax = 0)`: that classifies correctly but
only *after* the prior row has already been overwritten and is unrecoverable, and it is a
storage-layer implementation detail with no counterpart in any other engine, where
`ON CONFLICT DO NOTHING ... RETURNING` is documented behaviour that SQLite mirrors verbatim.

Known adjacent gap, unchanged and out of scope here: with `@@soft_delete`, a tombstone sitting at the
conflict target is invisible to the probe, so a `DO UPDATE` upsert still revives it and reports a
create. `upsert_resolve.rs` records this at the branch where it surfaces.

Regression coverage is `crates/cratestack-pg/tests/upsert_do_update_race.rs`, which reproduces the
race deterministically — a second session holds an uncommitted conflicting row (invisible to the
probe, but something the loser's `INSERT` provably *must* block on), and the harness only commits it
once that block is observed in `pg_stat_activity`, failing loudly rather than passing if the ordering
never happens.

### The generated-TypeScript smoke tests no longer race on npm's shared `_npx` cache (#738)

Five test files in `cratestack-client-typescript` ran their Node smoke scripts through
`npx --yes tsx@4.23.12 <script>`. npm derives the `~/.npm/_npx/<hash>` directory from the package spec
alone, so all seven of those call sites resolved to one directory,
`~/.npm/_npx/95c8da6ffd4052b6`, and each one installed into it and (on any failure) rolled its install
back out of it. `etag_if_match.rs`'s two smoke tests run as concurrent libtest *threads in the same
process*, so they contend on that directory directly; the other four files add the same exposure
against anything else on the machine resolving the same spec. One shared mutable directory, three CI
signatures from a single cause: `ERR_MODULE_NOT_FOUND` on `tsx/dist/loader.mjs`,
`npm warn cleanup ENOTEMPTY`, and `npm error code ENOENT / syscall spawn sh`. It turned `main` red on
`e0f92cc4` and cost four manual reruns in one session.

(Correcting a claim made in the issue's own comments and repeated in an earlier draft of this entry:
`cargo test` runs test *binaries* **sequentially**, not in parallel — visible in any CI log's ordering.
The concurrency that caused this is between threads inside one binary. The fix does not depend on
which it was, but the record should be right.)

`tsx` is now resolved **once**, by `tests/support`, into an immutable tree published under
`CARGO_TARGET_TMPDIR` by an atomic `rename` of a fully-installed staging directory; tests invoke
`node <cli.mjs> <script>` — exactly what npm's own `.bin/tsx` shim execs. This takes the issue's
second Expected Behavior direction (remove `npx` from the concurrent path) rather than the first
(give each test its own cache), because a per-test cache leaves N racers racing more politely while
paying a cold download each, whereas nothing here writes to a shared directory at all any more. A
reader now only ever sees the published path absent or complete, never half-written and never being
rolled back. Verified structurally rather than by repetition: a full-suite run against a wiped,
private npm cache creates **no `_npx` directory whatsoever** (it previously created two —
`95c8da6ffd4052b6` for `tsx@4.23.12`, plus `fd45a72a545557e9` for a second, *unpinned* `npx --yes tsx`
call site in `rest_list_query_wire_format.rs` that the issue had not catalogued; it now uses the pin
like the rest).

Independently, a failed smoke script is now attributable. The panic used to read `smoke script failed:`
followed by two empty streams — on a red `main` run, one of the two failing tests printed nothing at
all, making an npm tooling death indistinguishable from a genuine assertion failure in generated
TypeScript. Every subprocess assertion in these tests now reports the command line, its working
directory, and its exit status (naming a signal death rather than rendering it as a bare `-1`)
alongside the streams.

Publishing tolerates a destination that already exists in **any** state — absent, empty, complete,
gutted, or concurrently written. This is not defensive padding: on CI a gutted destination is the
*expected* input, and assuming otherwise turned `main` red on `248fc7ee`.
`Swatinem/rust-cache`'s `cleanTargetDir` (`src/cleanup.ts` at the pinned `6323deb`) treats any
directory under `target/` with no `build`/`.fingerprint`/`deps` child as a nested target directory and
recurses into it, deleting every non-directory entry it meets — so the cache-save step walks
`target/tmp/tsx-4.23.12/node_modules/tsx/dist/` and unlinks `cli.mjs` along with every other regular
file, keeping the directory skeleton. That skeleton is saved and restored by the *next* run, which is
why the first CI run after this landed was green and the one after it was not. A tree is now
considered usable only if the artifact actually executed is present **and non-empty** (rejecting both
the skeleton and a zero-byte file from a truncated restore); an unusable destination is swapped out
under a private name and re-checked in private before being destroyed, so a concurrent publisher's
good tree can never be deleted out from under a reader about to exec from it. Every mutation is a
`rename`, which is what makes "absent or complete" a property rather than an aspiration.

No production code changed — this is test-harness only. Both smoke tests still exercise a real HTTP
stub round trip, `#726`'s exit-status assertion is untouched, and the Node-absent skip path still
fires (it now probes `node`/`npm`, the two binaries the harness actually invokes, rather than
`node`/`npm`/`npx`).

### `CRATESTACK_REQUIRE_DB` now fails when *no* database backend is configured (#747)

`CRATESTACK_REQUIRE_DB` exists to turn a silent skip into a loud failure, so that a green run can be
trusted as evidence. It did not do that in the case that matters most. `cratestack-pg`'s
`connect_or_skip()` threaded `require` into every *connection failure* but ignored it entirely on the
fall-through where neither `CRATESTACK_TEST_DATABASE_URL` nor `CRATESTACK_USE_TESTCONTAINERS` is set —
the single most likely CI misconfiguration. The result: the whole `cratestack-pg` PG-backed suite
(`banking_*`, `policy_db_*`, `generated_client_rust*`, `upsert_*`, `internal_suppression`, …) printed
`ok` in 0.00s having touched no database, *with the guard explicitly enabled*, and three separate
reviews accepted that green as proof that DB coverage had run.

Both affected crates now panic on that path, naming both variables. **Auditing the family found a
second broken copy the issue did not name: `cratestack-outbox`**, whose five transactional-outbox tests
(atomic persist/rollback, cursor-ordered drain, GC sweep) were skippable in exactly the same silence.

The decision is now a pure `pick_backend(has_url, use_testcontainers, require) -> Backend` in
`tests/support/require_db.rs` in each of those two crates, adopting the shape
`crates/cratestack-redis/tests/support/redis.rs` already used for `CRATESTACK_REQUIRE_REDIS`. Purity is
the point: it lets `tests/require_guard.rs` prove the guard is *able to fail* with a `#[should_panic]`
test, instead of the guard only ever being observed passing — which is precisely how this defect
survived. Driving `connect_or_skip` with real env vars could not do that without mutating
process-global state shared with every other test thread in the binary.

Behaviour with `CRATESTACK_REQUIRE_DB` **unset** is unchanged: a silent skip remains the deliberate
local-dev default, so a contributor without Docker still gets a quiet pass. No CI job changes state —
all five places that set `CRATESTACK_REQUIRE_DB=1` (`.github/workflows/ci.yml`) either pair it with
`CRATESTACK_USE_TESTCONTAINERS=1` or exclude the affected crates.

The other four copies of the guard (`cratestack-studio`, `cratestack-migrate`, `cratestack-cli`, and
`cratestack-redis`'s two) were audited and are correct; each now carries a comment pointing at
`crates/cratestack-pg/tests/support/require_db.rs`, which holds the registry of all seven.
`examples/db-transaction-verification` reads the variable but has no such fall-through — it always uses
testcontainers.

**A shared helper crate was considered and deliberately not built.** The pure decision is ~15 lines; the
I/O half genuinely differs per site (three different `sqlx` import paths plus Redis) and is not
shareable. Collapsing the decision into one crate would mean adding a workspace member under `crates/`,
which ADR 0014 requires be assigned a layer in `docs/adr/layers.toml` — and test-only scaffolding has no
honest place in the L0–L5 model, `tools`, or `vitrine`. Putting it anywhere else means inventing a new
top-level directory. Both are architecture decisions larger than this bug and are left to the
maintainer; the `#[should_panic]` regression tests are what actually stop the copies from diverging
again silently.
### Documented: Studio's `[target.db]` write path enforces no schema-declared write constraint (#744)

**No behaviour change** — this records a maintainer decision (#744, option 3) about what
`mode = "rw"` already grants, because an operator granting it needs to know which of Studio's two
channels they are granting. A `[target.db]` target (`PostgresSource`/`SqliteSource`) is a direct SQL
connection that sits *beneath* the schema the way `psql` does: it enforces **no** schema-declared
write constraint — not `@@allow`/`@@deny` (on reads or writes, and never has), and not
`@@internal(...)` route suppression (#743). A model whose `create` is suppressed with
`@@internal("create")`, or denied by an `@@allow` rule, is still creatable through
`POST /api/targets/{key}/models/{model}/records` on an `rw` `[target.db]` target. The only pre-flight
checks remain `TargetMode::Rw` and `@version`/`@@emit` write routability plus the `allow_unsafe_writes`
opt-in (#507, #516). Making Studio enforce `@@internal` was considered and rejected as the arbitrary
half of the change while `@@allow` stayed unenforced (#744's option 1).

A **`[target.api]`-only** target is the opposite and gets both for free: `ApiSource` issues ordinary
HTTP requests against the deployed service's macro-generated REST routes — the same surface the
TypeScript and Dart clients consume — so a suppressed verb has no route to call (`405`, or `404` when
every verb on the path is suppressed) and policies are evaluated server-side against the identity in
`[target.api].auth`. Note the precedence, newly written down: the workspace loader takes `[target.db]`
whenever the target declares one, so a target declaring **both** blocks is a `[target.db]` target for
every read and write — adding `[target.api]` alongside a `[target.db]` buys no enforcement.

Written down in `cratestack-studio`'s crate rustdoc (new "Granting `rw`" section), `TargetMode`'s doc
comment, `require_writable`'s doc comment in `api/records/guards.rs`, `cratestack-studio`'s README (new
"What `mode = \"rw\"` grants" table), both starter `studio.toml` templates, and
`docs/design/route-suppression.md` §8b. A new drift-guard test pins the warning in both starter
templates, matching the one that already pins the #507 unsafe-write warning.

### `@@internal("action")` route suppression — REST, RPC and every generated client (#743, implementing #514's accepted design)

A model action can now be marked `@@internal("list" | "detail" | "read" | "create" | "update" |
"delete" | "all")` to declare it must never be reachable from the wire — no REST route, no RPC dispatch
arm, and no client stub in any generated SDK, on either transport. Suppression is implemented as
*emitting nothing*: a suppressed verb on a REST path with surviving verbs gets axum's own `405 Method
Not Allowed`; a model that suppresses every verb on a path never registers that path at all (axum's own
`404`); a suppressed RPC op id falls into the pre-existing unknown-op-id arm and returns the exact same
`CratestackError::NotFound` a genuinely unknown op id gets, including per-frame inside `/rpc/batch`
(a suppressed op in one frame does not poison sibling frames). The canonical case this unblocks: a
schema declaring `@@allow("create", auth().isSystem())` — fail-closed and correct, but until now still
generated a `POST` route and a `.create()` client method that could only ever 403. `@@internal("create")`
now removes both.

**Breaking, per the pre-1.0 lockstep convention, and opt-in per action**: adding `@@internal` to an
action a generated client already calls removes that client method (`Widget.create()` in Rust, Dart and
TypeScript; the corresponding REST/RPC/riverpod/`--swr` hook/controller in Dart and TypeScript) — a
compile error at the call site on regeneration, not a runtime `403` discovered later. It is opt-in per
action, so nothing breaks until a schema author adds it. `Create<Model>Input`/`Update<Model>Input` are
also omitted from every generated **client** SDK when the corresponding verb is suppressed (the
server's own ORM-facing input types are unaffected — a suppressed action's policy still compiles and
still gates any in-process caller, e.g. a custom procedure calling `db.create()` directly). A model with
no `@@internal` attribute at all generates byte-identical output to before this change.

The shared source of truth is `cratestack_core::model_internal_actions(&Model) -> BTreeSet<&str>`,
consulted once per surface: `cratestack-macros`'s REST route assembly
(`axum/model/routes.rs::generate_model_axum_routes`, now emitting per-verb `MethodRouter`s merged with
`.merge()` rather than one fused `.get(..).post(..)` chain), RPC dispatch-arm/op-descriptor collection
(`transport/rpc.rs`, `transport/op_descriptors.rs`), the generated Rust client
(`client/rest/model.rs`, `client/rpc/model.rs`), `cratestack-client-dart` (both presets, REST and RPC),
and `cratestack-client-typescript` (default, `--swr`, REST and RPC — extending the pre-existing,
`create`-only, presence-based `model_allows_create` gate to all five verbs). An invalid action name in
`@@internal(...)` is a compile error naming the model and the bad action
(`cratestack-parser`'s `validate_model_attributes`).

**Follow-up fixes from post-merge review (#743):**

- **`generate-wiremock` now suppresses stub mappings too.** `cratestack-mock-wiremock`'s
  `model_mapping::build_model_mappings` (both the stateful REST path and the static RPC path) consults
  `model_internal_actions` before emitting a mapping — previously a suppressed verb still got a stub
  advertising a working response (e.g. a stateful `201 Created` for a `create` the real server
  suppresses), handing a mock consumer a contract the real server doesn't honor.
- **`cratestack diff` now gates on `@@internal`.** Adding `@@internal(...)` to a model action is
  classified `Severity::Breaking` (previously fell to the generic "no tracked wire-shape effect"
  branch) — a PR that suppresses an action with live consumers now fails the diff gate. Removing
  `@@internal(...)` is `Severity::Additive` for what a schema-only diff can observe (it cannot see a
  consumer's hand-written replacement handler colliding with a regenerated route at the same path).
  Each `@@internal("action")` declaration is now keyed independently in the diff (mirroring `@@unique`'s
  existing per-instance keying) — a model suppressing more than one action used to collapse every
  declaration but the last onto one `BTreeMap` entry, silently under-reporting (in the worst case,
  reporting *zero* changes for a diff that actually restored a suppressed live action). `@@internal`
  itself takes exactly one action per declaration, enforced by the parser (`@@internal("create",
  "update")` is a compile error) — suppressing more than one action means writing more than one
  `@@internal("action")` line.
- **`ROUTE_TRANSPORTS` now omits suppressed verbs.** `generate_model_transport_constants`/
  `generate_model_transport_entries` (`cratestack-macros/src/transport/rest.rs`) consult
  `model_internal_actions` — this `pub const` registry in the generated crate's public API no longer
  lists a verb the schema author explicitly suppressed, even though its one runtime reader
  (`cratestack-axum`'s rate-limit filter) already failed closed on a miss.
- **`@@internal` added to `cratestack-lsp` completions**, with a detail string distinguishing it from a
  policy attribute.
- **New test**: an in-process `.create()` call against a model carrying both `@@allow("create", ...)`
  and `@@internal("create")` succeeds for an authenticated caller and is still denied for an
  anonymous one — pinning that suppression is purely generation-time and leaves policy evaluation
  untouched (design doc §9's non-goal).
- **Known-incomplete surface documented, not silently left**: the TypeScript `swr` preset's
  `list`/`get`/`update`/`delete` cache-key factories (not `create`'s, which is correctly gated) are
  still emitted for a suppressed verb — confirmed inert (no generated hook ever calls through one) and
  documented in `docs/design/route-suppression.md` §8a rather than fixed, since gating them has no
  caller-visible effect today.
### `@@unique`/`@@index` gain a `where: "<sql predicate>"` argument — partial index DDL — breaking for `cratestack-core` API consumers (#742)

`@@unique([...], where: "...")` and `@@index([...], where: "...")` declare a **partial** index,
following the same keyword-argument shape and verbatim-passthrough posture `using`/`opclass` already
use (#156): the predicate is never parsed or validated, only carried through to a trailing
`WHERE <predicate>` on the emitted `CREATE [UNIQUE] INDEX`, and left for the database to accept or
reject. `@@unique` previously required at least two fields ("use a field-level `@unique` instead");
that rule is relaxed for the single-field case specifically when `where:` is present, since
field-level `@unique` has no room for a keyword argument at all — this is the motivating shape (ADR
0038's deferred B3): a single, genuinely-optional column that must be unique only when present, e.g.
`@@unique([idempotencyKey], where: "idempotency_key IS NOT NULL")`. An empty field list
(`@@unique([], where: "...")`) is still rejected regardless of `where:`, matching `@@index`'s existing
unconditional "at least one field" check — the `where:`-relaxed floor only lowers the minimum from two
to one, it never drops it to zero.

`AddIndex` gains a `where_predicate: Option<String>` field (`#[serde(default)]`, so previously
serialized migration IR keeps deserializing); `None` renders byte-identical DDL to before this field
existed on both backends. SQLite renders the same `WHERE` syntax (it has supported partial indexes
since 3.8.0) — the one real divergence is what a predicate may legally *reference*, not the syntax
(see `emit::sqlite::indexes`'s doc for SQLite's restrictions).

**Breaking for `cratestack-core` API consumers:** `cratestack_core::schema::composite_unique::parse_composite_unique_attribute`
(public, re-exported) changed its return type from `Result<Vec<String>, String>` to
`Result<ParsedCompositeUnique, String>` to carry the new `where_predicate` field alongside the field
list. Both in-repo call sites were updated; any external consumer calling this function directly needs
to switch to `ParsedCompositeUnique.fields`.

The load-bearing part is introspection round-tripping without churn: Postgres exposes a stored
predicate via `pg_get_expr(indpred, indrelid)`, and that text is **normalized** — verified empirically
against a live Postgres 18 rather than assumed — wrapped in exactly one pair of parentheses with
whitespace collapsed, so `idempotency_key IS NOT NULL` reads back as
`(idempotency_key IS NOT NULL)`. A naive comparison against the schema's literal text would make every
`migrate` run diff the index as changed. The diff engine (`crate::diff::indexes`) now compares matched
indexes' predicates through that same normalization before deciding whether to no-op or drop+recreate,
proved against a real database (not a hand-written IR fixture, which can't exhibit the normalization)
in `crates/cratestack-migrate/tests/postgres_introspect.rs`.

Landed initially without also tolerating the explicit `::type` cast Postgres inserts onto *every*
literal comparison — `status = 'active'` reads back as `(status = 'active'::text)`, `amount > 100`
against a floating-point column as `(amount > (100)::double precision)` — which meant any predicate
comparing a column to a literal (i.e. almost any partial index anyone would actually write) never
compared equal to its introspected form and churned a drop+recreate on *every* `migrate` run: the
ticket's load-bearing "no churn" requirement, silently unmet for exactly the shapes that matter. Caught
in post-merge review and fixed in the same PR before release, in two rounds:

Round 1 independently stripped the `::type` cast from each side of the comparison before a plain string
equality check. That closed the churn bug but opened a worse one, caught in a second review round: since
the type name was discarded, two predicates casting the *same* literal to two *genuinely different*
types compared as equal — `email = 'x'::citext` (case-insensitive) vs. `email = 'x'::text`
(case-sensitive) got **no migration**, silently leaving the database enforcing the old, wrong uniqueness
rule. An unnecessary drop+recreate is a noticed operational annoyance; a missed one is a wrong constraint
nobody notices until it lets a duplicate through on a money path — the worse failure direction.

Round 2 replaced independent normalization with a joint, type-aware comparison
(`crate::diff::indexes::predicate` + `predicate::casts`, the latter tokenizing each predicate into
literal-vs-everything-else segments rather than rewriting a string): a literal on one side lacking a
cast the other side has is still forgiven (presumed Postgres-inserted), but when **both** sides carry an
explicit `::type`, the type names must match — mismatched (e.g. changing `citext` to `text`) now
correctly triggers a drop+recreate. The type-name grammar itself was also hardened past a plain
lowercase-letters-only word run (which silently corrupted literals containing digits — `100::int4` lost
its trailing `4` and it leaked back onto the literal being compared, turning it into `1004`) into one
recognizing schema-qualified (`pg_catalog.int4`), double-quoted, and array (`text[]`) type-name shapes
without corruption. Genuinely ambiguous shapes (schema-qualified vs. bare spellings of what might be the
same catalog type, e.g. `public.citext` vs. `citext`; double casts; doubled parens) fail toward churn,
never toward silent equality — this module has no catalog access to confirm two spellings really name
the same type, and guessing risks the exact false-equality class it exists to close. It still doesn't
replicate Postgres's remaining normalization — identifier/keyword case-folding — since that needs a real
SQL expression parser, out of scope per the ticket; a predicate that differs from its stored form only by
identifier case will still show as changed and get dropped/recreated, a documented limitation (pinned by
a test), not a silent gap.

Round 3 closed one more gap in round 2's exact-string type-name comparison: Postgres normalizes a handful
of type-name aliases on deparse — an author-written `::int` reads back as `::integer`, `::int8` as
`::bigint` — and since round 2 requires an exact match once both sides carry an explicit cast, an author
who happened to write an aliased spelling got a spurious drop+recreate on *every* single `migrate` run,
forever: round 1's exact failure, resurfacing for the narrower population that writes explicit casts.
Fixed with `predicate::casts::type_name::alias`, a small, closed table (`int`/`int4` → `integer`,
`int2` → `smallint`, `int8` → `bigint`, `float4` → `real`, `float8` → `double precision`,
`varchar` → `character varying`, `char` → `character`, `bool` → `boolean`, `decimal` → `numeric`,
`timestamptz` → `timestamp with time zone`, `timetz` → `time with time zone`) applied only to a bare,
unqualified, unquoted, undecorated type name — never to a double-quoted user-defined type (`"int"` is a
real, different type named `int`, not a spelling of `integer`), never to a schema-qualified spelling
(still not equated with a bare one, unchanged from round 2), and never guessed for an unrecognized name,
which still fails toward churn on mismatch. `serial`/`bigserial`/`smallserial` are deliberately absent —
they aren't real column types in this sense (see `alias`'s own doc).

**The alias table only fixes the type-*name* mismatch, not every structural shape a cast can deparse
into**: `int8`/`bigint` is proven end-to-end (a genuinely clean single-cast round-trip, see below), but
`varchar` compared against a `text` column deparses with an *additional*, nested implicit cast
(`pg_get_expr` — verified empirically — returns `(email = ('x'::character varying)::text)` for a schema's
`email = 'x'::varchar`), a structural difference no alias table can resolve; an author writing that
spelling still gets churn, not corruption or a silent wrong result (the safe direction), and it's pinned
as a known limitation rather than left to be rediscovered — see `alias`'s own doc and
`tests::alias::varchar_on_a_text_column_still_churns_due_to_the_extra_implicit_cast`.

Proved against a live database: `partial_index_with_text_literal_predicate_round_trips_without_churn`,
`partial_index_with_numeric_literal_predicate_round_trips_without_churn`,
`partial_index_cast_type_change_is_detected_as_drop_and_recreate` (using the real `citext` extension to
reproduce the money-relevant case end-to-end), and
`partial_index_with_aliased_cast_type_round_trips_without_churn` (an author-written `::int8` against a
`bigint` column, round 3's decisive case) in `crates/cratestack-migrate/tests/postgres_introspect.rs` —
not just the `IS NOT NULL` shape that needs no cast and can't exercise any of this.

**Adoption note — widened blast radius:** introspecting partial indexes at all required dropping the
prior `AND i.indpred IS NULL` exclusion in `introspect::postgres::indexes`, which used to make every
partial index invisible to `migrate` regardless of origin. A hand-made partial index that no `.cstack`
schema declares — including one created outside Cratestack entirely, this ticket's own motivating
scenario — is now visible to introspection like any other index, and an index present in the database
but absent from the schema is a `DROP INDEX` candidate on the very next `migrate` run (no `CASCADE`,
same as any other unmanaged index — not a new rule, just a newly-reachable one for partial indexes
specifically). **If you're upgrading and have a hand-made partial index no schema declares, the next
`migrate` run will drop it** — declare it via `@@unique`/`@@index([...], where: "...")` first to keep
it. Pinned by `undeclared_partial_index_is_dropped_by_diff` in
`crates/cratestack-migrate/tests/postgres_introspect.rs`.
### `ConflictTarget` can target a partial unique index, and the upsert conflict probe honors it — breaking for exhaustive external matches (#741)

An upsert can now target a **partial** unique index (`CREATE UNIQUE INDEX ... WHERE <predicate>`),
which Postgres refuses to infer from an unpredicated `ON CONFLICT (<cols>)` — motivated by an
optional-idempotency-key shape (`UNIQUE (key) WHERE key IS NOT NULL`) that previously had no route
onto the framework's upsert primitive at all.

`ConflictTarget` (`cratestack-sql`) stays the enum it always was: `ConflictTarget::PrimaryKey` and
`ConflictTarget::Columns(&'static [&'static str])` are unchanged, and a new
`ConflictTarget::columns(&[...]).where_index("<predicate>")` attaches the partial-index predicate as a
compile-time `&'static str` — the same no-runtime-value-path precedent `@@index`'s `using`/`opclass`
already set — riding along on two purely additive variants
(`ColumnsWithPredicate`/`PrimaryKeyWithPredicate`) that `.where_index` produces rather than being
constructed directly. An earlier draft of this change collapsed the two original variants into a
`{ kind, predicate }` struct, which broke every direct enum-variant construction/match in the process;
a repo-wide grep found that in-repo usage is 100% construction, zero pattern matches, so that break
bought nothing and was reworked additively before landing — every known construction site (in-repo,
across both backends and all tests) is unaffected and needed no changes. **Still breaking**, but only
for external code that pattern-matches `ConflictTarget` **exhaustively without a wildcard arm**: the
variant count grows from two to four, so such a match stops compiling (adding enum variants is
semver-breaking in Rust regardless of how additive the rest of the change is). Pairing a predicate with
`ConflictTarget::PrimaryKey`/`ConflictTarget::PRIMARY_KEY` is a clear
`CratestackError::Validation`/`RusqliteError::Validation` (the PK index is never partial), not a
silently dropped predicate — the invalid combination is deliberately kept representable
(`PrimaryKeyWithPredicate`) so this is a runtime rejection, not something the type system prevents you
from writing at all.

`ConflictTarget` is also now `#[non_exhaustive]`. Construction is unaffected — every existing variant,
including the two additive predicate-carrying ones, stays constructible from outside `cratestack-sql`
exactly as before; only external *exhaustive* `match`es (already broken by the two-to-four variant
growth above) additionally need a wildcard arm going forward. This costs nothing on top of the break
those callers are already absorbing this release, and it makes every *future* variant addition
non-breaking instead of requiring a second, separate break later.

Both backends emit `ON CONFLICT (<cols>) WHERE <predicate> DO UPDATE|DO NOTHING`; SQLite accepts the
identical inference syntax (confirmed against the vendored libsqlite3-sys 0.37.0 / SQLite 3.51.3).
Unpredicated targets emit byte-identical SQL to before this change.

The non-obvious half: `cratestack-sqlx`'s conflict probe (`SELECT ... FOR UPDATE`, used to decide
`Inserted`/`Existing`/`Created` vs `Updated` ahead of the real `ON CONFLICT` statement) now applies the
same predicate two ways — filtering candidate existing rows by it (so a row outside the partial index
isn't mistaken for a conflict), and short-circuiting to "no possible conflict" when the *incoming* row
itself doesn't satisfy the predicate (mirroring Postgres's own partial-index semantics: a row is only
ever added to a partial index's B-tree, hence only ever able to conflict via it, if that row itself
satisfies the predicate). Skipping either half lets the emitted SQL be correct while the caller is
still handed the wrong `Inserted`/`Existing` verdict — the acceptance test for this
(`crates/cratestack-pg/tests/upsert_partial_index.rs`) uses a predicate that is deliberately not a
`col IS NOT NULL` test, since that shape happens to "work" even against a predicate-unaware probe.
`cratestack-rusqlite` needs no equivalent probe fix — the embedded backend has no probe at all; SQLite
resolves the conflict natively in the same statement.

Two correctness fixes to the probe, found in review before this shipped:

- SQL predicates are three-valued, not boolean — `status = 'active'` evaluates to `NULL`, not `false`,
  whenever `status` is `NULL`. The incoming-row predicate check now decodes `Option<bool>` and treats
  `NULL` the same as `false` (outside the index's domain), instead of failing the whole upsert with an
  opaque 500 (`sqlx::Error::ColumnDecode`) the moment the predicate touched a `NULL` column.
- A predicate referencing a column excluded from the insert set by `@default(...)` (the database's own
  column `DEFAULT` fills it, so this crate never learns its value client-side) made the incoming-row
  check's synthetic one-row `SELECT` fail with Postgres `42703 column "..." does not exist` —
  unconditionally 500ing every `.do_nothing()` upsert on that conflict target, no matter what the
  caller passed. `.do_nothing()` now falls back to skipping the pre-probe when the incoming-row check
  can't be evaluated for that specific reason (Postgres `42703`, and only `42703` — every other probe
  failure, e.g. a connection loss or a genuinely malformed predicate, still propagates rather than being
  silently absorbed and possibly re-raised later as a different, more confusing error from a different
  statement; see `upsert_predicate_probe_error.rs`'s module doc comment), going straight to the
  authoritative `ON CONFLICT ... DO NOTHING RETURNING` statement, which is always correct there (see
  `upsert_do_nothing_probe.rs`'s doc comment) — the plain `.upsert(...).run(...)` (`DO UPDATE`) path
  cannot safely use the same fallback (its
  Created-vs-Updated classification and audit before-snapshot have no equivalent authoritative source)
  and still surfaces this as an error; closing that gap for real needs either backfilling literal
  `@default(...)` values into the insert set at codegen time or basing Created-vs-Updated on the real
  statement's own result, both larger than this fix. That remaining `DO UPDATE` error is now
  actionable rather than an opaque 500, though: the incoming-row probe narrowly maps Postgres `42703`
  from its own query into a `CratestackError::Validation` naming the offending predicate and
  explaining the likely cause (a `@default(...)` column) and the workaround — every other SQLSTATE,
  and every other call site, still gets the ordinary `cratestack_error_from_sqlx` mapping unchanged.

### `@cratestack/cbor` is now the default codec for generated TypeScript RPC clients — breaking (#746)

`generate-typescript` on an RPC-transport schema now emits a client whose `CratestackRpcRuntime`
resolves `@cratestack/cbor`'s `createCborCodec()` by default, instead of the pure-TypeScript
`jsonRpcCodec` — closing a cross-language asymmetry `cratestack-client-dart` closed for its own native
codec back in #563. `--no-native-cbor` (`TypeScriptGeneratorConfig::native_cbor: false`) falls back to
today's `jsonRpcCodec` behavior. **REST-transport schemas are unaffected either way** —
`rest-runtime.ts.j2` has no codec seam at all; this is a deliberate, documented scope boundary, not an
oversight (see the `CratestackRpcClientOptions`/`TypeScriptGeneratorConfig::native_cbor` doc comments).

**Breaking at the wire level, not at the construction API:** the constructor stays synchronous — every
existing consumer (swr/tanstack/refine layers, examples) keeps constructing a client the same way. But
the default codec's `contentType` changes from `application/json` to whatever `@cratestack/cbor`
reports (`application/cbor`), which is sent as both `Content-Type` and `Accept` on every request. **A
server built against a `CodecSet` that only understands JSON will now reject a regenerated default
client's requests with `406 Not Acceptable` / `415 Unsupported Media Type`** — regenerating this
client without also either updating the server's `CodecSet` to serve CBOR or passing
`--no-native-cbor` is a functional break, not just a type-level one. `--no-native-cbor` (or an explicit
`codec: jsonRpcCodec` passed to the constructor) restores today's JSON-only behavior exactly.

What also changes on the native-cbor path: `CratestackRpcRuntime`'s public `readonly codec:
CratestackRpcCodec` field is **removed** — it cannot stay synchronous once the default codec requires
an async `createCborCodec()` resolution (true on both Node and the browser — see `@cratestack/cbor`'s
own doc comment on why Node's is `async` too, purely for call-site parity). It is replaced by a
private, lazily-created, memoized `Promise<CratestackRpcCodec>` and a `resolveCodec()` accessor that
every already-`async` call site (`call()`, `batch()`, `stream()`, `readUnaryResponse()`) awaits —
`createCborCodec()` itself runs at most once per runtime instance, never once per request, and (fixed
during review) a *rejected* resolution is never memoized, so a transient failure gets retried on the
next call rather than bricking the runtime instance forever. Nothing outside this file read the public
`codec` field directly (verified against `packages/cratestack-link-batch`, the zod/yup validators, and
`rpc-stream-terminal.ts.j2`, all of which only touch the per-request `RpcLinkRequest.codec`, whose
synchronous shape is unchanged), so this has no other call sites to update. On the `--no-native-cbor`
path, the field survives, but `call()`/`batch()`/`stream()`/`readUnaryResponse()` now read it into a
local `codec` binding first (a review-driven de-duplication of what used to be near-identical logic
duplicated per flag) rather than referencing `this.codec` inline — a mechanical rename with no
behavioral effect on that path.

The generated `package.json`'s `dependencies` block (previously a hardcoded `decimal.js`-only stanza)
is now a `{% for %}` loop over a new `dependencies_for()` builder (`cratestack-client-typescript`'s
`package_deps.rs`), matching the `peerDependencies`/`devDependencies` loops issue #617 already
introduced; `@cratestack/cbor` is added there when the flag is on and the schema is RPC transport,
pinned to `^{major}.{minor}.0` of this crate's own version — a *floor*, not the exact
`CARGO_PKG_VERSION` `refine_version_requirement` uses, specifically so a version-bump PR (which moves
the workspace version before the corresponding npm packages publish) doesn't ship a caret range that
resolves to nothing on the registry — the same incident already recorded on the Dart side for the
v0.8.9 release (#707). This narrows but does not close that class of gap: a *minor* bump still has a
window between the version moving and the npm packages publishing.

**Known platform gap** (documented on `TypeScriptGeneratorConfig::native_cbor`'s doc comment, the
`--no-native-cbor` CLI flag, and `packages/cratestack-cbor-node/README.md`'s "Scope" section, which
previously — and as of this default flip, incorrectly — said `jsonRpcCodec` remains the default):
`@cratestack/cbor-node` ships prebuilt napi binaries for macOS (x86_64/aarch64), glibc Linux
(x86_64/aarch64), and Windows x64 only. There is no musl (Alpine) build and no `win32-arm64`; on
either, the napi loader fails with a generic "Cannot find native binding…" error. `--no-native-cbor`
is the escape hatch for both, falling back to the dependency-free `jsonRpcCodec`.

### Generated Dart builders move to `package:cratestack_builder` — breaking for build tooling (#668, phase 2/3)

`cratestack-client-dart` no longer emits `{Class}Builder` classes inline. Every generated data class
(models, `Create{Model}Input`, `Update{Model}Input`, `{Model}Where`/`{Model}OrderByClause`/
`{Model}FindMany`, `type` blocks, per-procedure argument classes) instead carries a runtime-only
`@CratestackBuilder(...)` annotation from `package:cratestack_annotations`, and its containing file
gains a `part '<stem>.builder.dart';` directive that `package:cratestack_builder`'s `build_runner` step
expands. #668 phase 1 (0.8.6) added the two packages without touching generated output; this closes
that gap — supersedes the 0.8.6 entry below's "the Rust generator still emits builder classes inline".

**Breaking for the build step, not the generated API surface**: `{Class}Builder`'s public shape
(setters, `add{Field}`, `build()`) is unchanged, but every regenerated client now needs
`dart run build_runner build` (or `cratestack generate-dart --run-build-runner`) before it analyzes —
including the **default** preset, which previously needed no build step at all. The generated
`pubspec.yaml` gains `cratestack_annotations` (`dependencies:`) and `cratestack_builder` +
`build_runner` (`dev_dependencies:`) accordingly; the riverpod preset already depended on
`build_runner` for its own `@riverpod`/`dart_mappable` codegen, so this is additive there.

Builder generation exits `cratestack generate-dart --check`'s coverage (it only sees the annotation +
`part` directive now, not the expanded builder itself) — replaced by `just verify-dart` running a real
`build_runner build` + `flutter analyze --fatal-warnings` pass for both presets, plus
`dart-packages/cratestack_builder`'s own generator-level test suite.

### `lib/src/models/shared_types.dart` under riverpod's `--preset riverpod` was missing its builder import/part (related to #668)

The phase 2/3 rollout above missed one call site: a `type` block the partition assigns to
`Owner::Shared` (most commonly an orphan `type` referenced by nothing else — see
`tests/fixtures/riverpod_shared_type_orphan.cstack`) rendered into `shared_types.dart` with a real
`@CratestackBuilder(...)` annotation on it, but that file never gained the
`cratestack_annotations` import or the `part 'shared_types.builder.dart';` directive every other
data-class-carrying riverpod file gets — `flutter analyze --fatal-warnings` failed on the undefined
annotation, and `build_runner` produced no `{Type}Builder` at all, a real regression against
`origin/main`'s previous inline emission. Both are now gated on `data_classes | length > 0`
(mirroring `rest_procedures.dart.j2`/`rpc_procedures.dart.j2`'s identical gate), not unconditional —
this file also hand-declares the `StringFilter`/`NumberFilter`/etc. filter classes, which carry
`@MappableClass()` but never `@CratestackBuilder()`, so the common case of a schema with nothing
partitioned to `Owner::Shared` must stay builder-part-free.
`just verify-dart`'s riverpod fixture loop now includes `riverpod_shared_type_orphan`, closing the
gap that let this regression ship without a real `flutter analyze`/`build_runner` run ever exercising
this file with a genuine shared data class in it.

## 0.8.13 (2026-08-26)

### The changelog no-op scope keeps its shape, but its stated reason was wrong

`.ci/changelog-files.sh` widened `cratestack_cbor`'s no-op scope beyond its own directory in
cratestack#713, and justified it with v0.8.6 as a "load-bearing counter-example, not a hypothetical
one": a real CBOR fix (cratestack#675) landed in `crates/cratestack-codec-cbor` in `v0.8.5..v0.8.6`
while `dart-packages/cratestack_cbor/` carried zero commits, so — the argument went — the vendored
binaries changed and a directory-only proxy would have written "No functional changes" onto a release
whose bytes moved.

The git facts were right. The conclusion was not, because it stopped one step short of the call site.
That fix was **encode-only**, and both vendoring sources wrap values in `EncodableValue` before
encoding — which maps `Value::Null` through `serialize_none()` at every position in the tree and had
already shipped in v0.8.5 (`52b50cea`, cratestack#580). The wrapper bypassed the fixed branch, so the
bytes did not change. cratestack#675's own commit message says as much: the wrappers are "kept as
intentional defense-in-depth ... not as the only thing preventing the bug". Its edits to
`crates/cratestack-cbor-wasm` in the same range were module-doc and test-only.

So v0.8.6 was a genuine no-op for that package, and the "No functional changes" wording on `main` for
that section is correct — the correction originally proposed here would have replaced a true statement
with a false one.

**The scope itself is unchanged and stays.** What justifies including `crates/cratestack-codec-cbor`
is the *decode* path, which is a mechanism rather than an incident: both
`crates/cratestack-client-flutter/src/cbor/mod.rs` and `crates/cratestack-cbor-wasm/src/wasm.rs` call
`CborCodec.decode(bytes)` bare, with nothing in between, so a decode-side change reaches both vendored
binaries directly. cratestack#675 merely happened to be encode-only; the next one need not be.

Comment-only: `CHANGELOG_NOOP_SCOPES` is byte-identical, Test 28's body is untouched (only its
narrative header cited the false example), and the seed suite passes 73/73 as before. Test 28 was
re-confirmed decisive — narrowing the fixture scope back to the package directory fails three of its
assertions.

The generalisable trap, now recorded in the comment itself: **a dependency edge is not a behaviour
path.** `cratestack-client-flutter` depends on `cratestack-codec-cbor` — true, trivial to verify, and
insufficient. Whether a dependency change is observable depends on how the consumer *calls* it.

## 0.8.12 (2026-08-24)

### RPC `get` gains the selection surface REST already had

0.8.11 shipped `@computed` resolver params over both transports, but left RPC `get` deliberately
narrower than REST's: it carried `computedParams` and nothing else. The 0.8.11 entry below calls that
"an intentional scope limit, not a gap". This release closes it, and `docs/design/rpc-transport.md`
§3.1a — which is where that limit was written down — is rewritten accordingly.

`RpcGetInput` now carries `fields`, `include`, and `include_fields` alongside `computedParams`, and
`synthesize_get_query` routes all four through REST's `parse_model_fetch_query` byte-for-byte, so both
transports run the same parse, validation, and projection path rather than two implementations that
have to be kept in agreement. `include_fields` is snake_case on the wire, matching `RpcListInput`.
`RpcGetInput` also gained `#[derive(Default)]`, so callers can use `..Default::default()`.

Every new field is `#[serde(default)]` and additive: an old `{"id": 1}` frame decodes unchanged, and a
client that sets no selection emits byte-identical frames — so no RPC snapshot-format-version bump was
needed.

**The pre-fix behaviour is worth knowing if you have RPC `get` calls in flight: a `fields` key on a get
frame was silently dropped by serde and the server returned the full record.** No error, no signal —
a request that looked like it was projecting simply wasn't. Two consequences of the fix follow from
that. The "`computedParams` for a `?fields=`-excluded field" 422, previously unreachable over RPC, now
fires. And `/rpc/batch` applies selection per frame, resolved and projected independently, with
in-frame selection signed by construction since the frame bytes are the canonical request body.

The `unexpected =>` arm of `parse_model_fetch_query` still rejects unknown keys on both transports.
`crates/cratestack-pg/tests/rpc_get_projection.rs` is the parity proof.

### Client projection surfaces

Rust RPC clients gain `get_view<P: ProjectionDecoder>(id, projection)`, the twin of REST's. Plain
`get` is byte-identical and still decodes into the full model type.

TypeScript's per-model RPC `get` options bag now carries `fields`, `include`, and `includeFields`,
emitted for every model.

Dart RPC clients are unchanged, deliberately: Dart has no projection surface for `list` either, so
adding one for `get` alone would trade a cross-transport asymmetry for an intra-client one. Tracked as
a follow-up rather than treated as done.

### `<Model>ComputedParams` gains the standard builder

It was the one generated object still without the builder that every other generated object has had
since cratestack#656. Rust gets the typestate builder —
`<Model>ComputedParams::builder().<field>(Some(..)).build()` — non-generic, because every field is
optional, which is the same shape `{Model}Where` gets. Dart gets the standard fluent
`<Model>ComputedParamsBuilder`.

TypeScript is excluded because no builder convention exists anywhere in its generated output. The
hand-written `RpcGetInput`/`RpcListInput` are excluded too: `generate_builder` is codegen-only, and
`..Default::default()` is those structs' idiom.

### A caveat before you add your first parameterized computed field (Rust clients)

The Rust client's `computed_params` parameter is **positional**. Adding a model's first parameterized
computed field turns `get(id, headers)` into `get(id, computed_params, headers)` and breaks existing
call sites. Dart (named optional) and TypeScript (options bag) are additive here and unaffected.

The builder above does **not** fix this — it changes how the argument is constructed, not the parameter
list. A Rust options-bag entry point is the real fix and is a tracked follow-up. This is the first time
that caveat has been written down.

### New convention: transport parity — REST and RPC ship together, never REST first

Recorded in `CLAUDE.md` and `AGENTS.md`. Any feature touching the request/response surface — query
parameters, projections, per-request arguments, new response shapes, client call surfaces — lands on
both transports in the same PR: server dispatch, the `RpcListInput`/`RpcGetInput` frame slots, and
every generated client.

The rule exists because the `@computed` params surface shipped REST-only and took three follow-up PRs
to close, this release included. What makes it cheap to follow is that RPC dispatch synthesizes a real
URL query string and re-enters the REST parsing path, so the server side is usually one frame field and
one `pairs.push`. A genuinely excluded transport is now a design-doc'd, changelog'd decision rather
than an omission discovered later.

## 0.8.11 (2026-08-24)

### A failing TypeScript smoke test now fails in a second instead of hanging for hours

Four tests in `cratestack-client-typescript` run generated client code under `npx tsx` against a real
TCP stub server. Each asserted the script's exit status **after** joining the server thread — and that
thread parks in a blocking `accept()`. So when the script died before issuing its request, nothing
ever connected, `join()` never returned, and the stderr saying why sat unread in `output`.

On 2026-08-24 three CI runs sat in exactly that state for over three hours each (main `afdcd9ce`,
jobs `97452966528` and `97485030107`) before being cancelled by hand, consuming roughly seven runner
hours between them. The trigger is still unknown, because the message that would have named it was
never printed: the same test binary (`etag_if_match-2c9818e999eb07df`) passed in five seconds at
12:12 and hung at 13:44 with no code change in between, and the tests pass locally.

Four changes, none of which alters what the tests assert:

- The status assert moves **above** the join, at all four sites. A broken client now fails in ~1s with
  its stderr, verified by pointing the runner at a nonexistent version.
- `accept()` is bounded (120s) rather than blocking forever, so a script that exits 0 without
  connecting also cannot hang.
- `npx --yes tsx` is pinned to `tsx@4.23.12`. An unpinned tool inside CI changes version without a
  commit here.
- Every job in `ci.yml` gains `timeout-minutes: 45`. There were none, so a hang ran to GitHub's 6h
  default.

### The iOS CBOR job's stream-readiness probe: `logger` emitter, and a counter that was lying

Two corrections to cratestack#720's probe.

**The readiness counter was counting `log stream`'s own header.** That header is
`Filtering the log data using "<predicate>"`, and the predicate contains the probe tag, so a plain
`grep -c` matched it as though it were a delivered record. Jobs `97436514543` and `97456642501` both
reported exactly 1, and in both it was the header. The conclusion drawn from the first of those — that
the stream had delivered its own invocation record, and that delivery therefore worked — was wrong and
is retracted. With the header excluded both runs read **0**, and nothing has yet been established about
delivery in either direction.

**The emitter is now `logger`, with `log show` as the fallback.** `logger` writes an ordinary log event
as itself, which a live subscription should carry, where `log show`'s self-record demonstrably does not
reach one. Job `97456642501` then reported that `logger` is not present in the simulator runtime at
all, so on current runner images the fallback is what actually runs — the recipe is no worse than
before, and the attempt is there for any image where that changes.

The stream's predicate constrains the probe clause to `process == "logger"`, which closes the
self-match trap structurally rather than by convention: `log stream`'s own record is process `log` and
cannot match. The nonce stays, now guarding against a probe from an earlier attach in the same booted
simulator counting as this attach's proof.

An unproven probe still degrades to the same fixed wait and never fails the job.

Part of cratestack#704, which stays open.

### A scheduled job now watches for the iOS capture defect, which no longer fails a build

Review found the watch had already gone stale before it ran once: its `PROBE DEAD` signal matched the
literal `0 tag-bearing`, and cratestack#722 had reworded that line to `0 probe record(s) delivered`
hours earlier. So the workflow now **asserts its own signal literals against `justfile` and fails its
own job** when one has moved — a monitoring job going red, never a build gate. Cancelled jobs are also
no longer reported as failures (`= failure`, not `!= success`); this repo's concurrency group cancels
superseded runs constantly, which would have put a false red on the ticket most days.


`cratestack#718` made a dropped `log stream` subscription survivable — the marker is recovered from
the log store and the run goes green with a `WARNING`. Right for the build, wrong for visibility: the
defect stopped producing a red job, and the WARNING existed only to be found by someone who already
knew to look. `cratestack#720`'s readiness probe has the same property, reporting a tag-bearing line
count on every run whether or not anything is wrong.

`.github/workflows/watch-ios-capture-defect.yml` runs daily and reports to `cratestack#723` when a
recent iOS job carries one of three signals: the marker was recovered from the store, the job went
red, or the readiness probe delivered zero tag-bearing lines. It reports each job once, reopens the
ticket if a signal arrives after it was closed, and changes nothing about CI itself — it only makes an
already-emitted line reach a human.


### flutter_rust_bridge moves to 2.13.0 (breaking for consumers on 2.12.0)

Every flutter_rust_bridge pin in the workspace moves from `2.12.0` to `2.13.0`:
`cratestack-client-flutter` and `examples/embedded-flutter/native` (Cargo), `cratestack_cbor`,
`cratestack-client-flutter/dart` and `examples/embedded-flutter` (pub), plus the pinned
`flutter_rust_bridge_codegen` installs in the `justfile` and both workflows.

This matters because the pub-side pin decides who can use `cratestack_cbor` **at all**. A bare
version is an exact pin in pub's grammar, so a Flutter app already depending on a different
flutter_rust_bridge version — for unrelated native functionality of its own — cannot add the package;
`pub get` fails during version solving. Since #702 made native CBOR the `generate-dart` default, an
app hits this simply by upgrading the CLI and regenerating. Reported as #716.

**Anyone on 2.12.0 is now blocked instead**, which is the unavoidable shape of this change: the
constraint cannot be widened, so the pin can only ever point at one release. Apps pinned to a 2.13.0
*prerelease* (`2.13.0-beta.6`) are still blocked and must move to stable — pub excludes prereleases
from ranges, so no constraint admits both.

Verified end to end rather than by editing version strings: glue regenerated with codegen 2.13.0,
`cargo build --features frb-glue`, `just frb-verify-client-flutter`'s Dart round-trip harness, and
`cratestack_cbor`'s own `dart test` (7 tests) all pass, with the cross-binding CBOR fixtures matching
byte for byte — the wire format is unchanged. Upstream's Windows codegen bug is **not** fixed in
2.13.0, so the Linux-only `cbor-vendor-glue` arrangement and its comments stand.

### The pin is documented as install-blocking, and one earlier claim is retracted

The constraint is now stated where someone hits it: a README section ahead of the quickstart, and a
pubspec comment aimed at the next maintainer tempted to widen it. flutter_rust_bridge requires
codegen, Dart runtime, and Rust runtime to be exactly equal, enforces that with `==` on a `String` in
generated code, and closed
[the request to make minor versions compatible](https://github.com/fzyzcjy/flutter_rust_bridge/issues/2694)
without acting on it. A range is not an option for a different reason than first documented: it
resolves to the *newest* match while the shipped glue is fixed at one version, so it would work today
and start handing consumers 2.14.0 against 2.13.0 glue the day upstream publishes it — breaking on
upstream's release schedule, with our CI still green.

**Correction.** The first draft claimed flutter_rust_bridge's codegen "rejects a ranged constraint
outright". That was wrong. The `bail!("unexpected version range")` cited applies to `ffigen`, and
reaches `flutter_rust_bridge` only through an `.is_ok()` in `auto_upgrade.rs` that discards it —
measured: `just cbor-vendor-glue` completes with a ranged constraint in `pubspec.yaml`. The claim was
written from a code search without confirming the call path, and is retracted here, in #717, and on
#716 where it was first stated.

An affected app's option that needs no version negotiation remains `cratestack generate-dart
--no-native-cbor`, which selects the pure-Dart codec and drops the flutter_rust_bridge dependency
entirely.

Also fixed a stale claim in `docs/tooling/dart-publishing.md`, spotted in the same report: a
narrative about the 0.8.0 first-publish rejection described the package as shipping no `ios/` folder
in the present tense. It has shipped one since 0.8.7.

### The iOS CBOR job can finish its pre-launch wait early, and reports what its log subscription did

`just cbor-example-verify-ios` attached a `log stream` subscription, slept 2 seconds, then launched the
app. Spawning `log stream` returns as soon as the *process* exists; delivery begins some unbounded time
later. On a contended runner that gap exceeds two seconds, and the app prints its marker into a window
nothing is listening to — which is what job `97199199670` recorded: 13 captured lines, every one of
them from the last ~15% of a launch, and none of the ~900 before them.

The sleep becomes an 8-second ceiling that ends early if a probe proves delivery. `log show` writes its
own invocation — command line included — into the unified log, so invoking it with the probe string as
its predicate both emits the probe and needs no extra binary in the simulator runtime. When that
message comes back out of the live capture, the subscription is demonstrably delivering.

The stream's predicate carries only a fixed *tag* while readiness requires tag + pid + timestamp. That
split is the whole trick: `log stream` logs its own invocation too, and its command line contains this
recipe's predicate verbatim, so a probe that greps for the same string the predicate contains reports
"live" instantly while delivering nothing. Verified by running both forms against a capture holding
only the stream's self-record — the shipped form rejects it, the naive form accepts it.

**The probe does not yet work on a real runner, and this is deliberately shipped anyway.** On job
`97423719675` it never round-tripped in 20 seconds, and the capture then took 1044 Runner-attributed
lines normally. So the ceiling is 8 seconds rather than 20 — a fixed wait with a chance of finishing
early, not a guarantee — and both paths now report how many tag-bearing lines reached the capture. That
count separates the two possible causes: above zero means the stream delivers such records and the
emitter or nonce is at fault; zero means nothing from that predicate clause ever arrived and the
mechanism needs replacing rather than tuning. A failed probe never fails the job, and cratestack#718's
log-store fallback still covers a marker the live capture misses.

Part of cratestack#704, which stays open.

### `@computed` — resolver-backed response-time fields, replacing `@custom` (`docs/design/computed-fields.md`)

A schema author can now declare a field that is derived at response time by hand-written Rust rather
than stored — a signed `proxyUrl` on an `Image`, computed while the framework composes the response:

```
model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed
}

type Thumbnail {
  url String @computed(params: ProxyParams?)
}
```

`@computed` (bare) and `@computed(params: <Type>?)` (parameterized — `<Type>` a declared `type`, the
trailing `?` required in v1) are accepted on `type` and `model` fields only; resolvers are invoked on
every model HTTP response (get, list, create, update, delete, relation includes) and on any procedure
output that reaches a computed-bearing `type`/`model`, over both REST and RPC transport.
`include_embedded_schema!` rejects any schema declaring a computed field at macro-expansion time — the
embedded backend has no response-composition boundary to run a resolver in. Model reads gain a
`?computedParams=<url-encoded JSON object>` query parameter (root model only) to pass per-field
resolver arguments — on REST this is the URL query string; on RPC, `RpcListInput` gains a
`computedParams` field (raw JSON-object text, so `serde_json::Value` round-tripping through `/rpc/batch`
can't corrupt an `Option::None` inside a params payload) and `model.<X>.get` gets a new `RpcGetInput`
input type carrying the same field (kept separate from `RpcPkInput`, which `delete` still uses
unmodified, so `delete` never gains a silently-ignored field). Both are additive and
`#[serde(default)]`: an old `{"id": 1}` get frame decodes unchanged and a client that never sets
`computedParams` emits byte-identical frames, so no RPC snapshot-format-version bump was needed.
`/rpc/batch` frames carry per-frame `computedParams` inside each frame's `input`, and in-frame params
are signed by construction (the frame bytes are the canonical request body). RPC `get` has no
`fields`/`include` slot (an intentional scope limit, not a gap — see `docs/design/rpc-transport.md`
§3.1a): it always decodes into the full model type, which can't represent a partial payload.
*(That limit was lifted in 0.8.12 — see above. This paragraph describes 0.8.11 as released; §3.1a now
documents the closed state.)*

**BREAKING:** the generated `router()`, `rpc_router()`, `model_router()`, and `procedure_router()`
functions gain a new `resolvers` parameter: `router(db, registry, resolvers, codec, auth_provider,
body_limit_bytes)`. Pass `()` for any schema with no computed fields — a generated
`impl ComputedFieldResolver for ()` covers that case with no extra caller-side wiring.

**BREAKING:** `@custom` is removed. It generated a `CustomFieldResolver` trait that nothing ever
invoked (the field stayed a plain struct member the caller had to fill by hand) — `@computed` replaces
it with a trait the framework actually calls. A schema still carrying `@custom` now fails to parse,
pointing at `@computed` as the replacement.

Downstream generators: `cratestack-migrate` excludes computed fields from DDL/diff; the wiremock
generator fabricates them like ordinary response fields; the LSP adds `@computed` to attribute
completion. All three generated clients (Rust, Dart, TypeScript) emit computed fields in response
classes only (excluded from create/update inputs, filters, and sorts) and, over both REST and RPC, a
**typed** per-model `<Model>ComputedParams` surface on `get`/`list`, gated the same way in every
language: offered only when the model has at least one *parameterized* `@computed(params: <Type>?)`
field, never for a bare-`@computed`-only model. Rust's `<Model>ComputedParams` struct
(`include_client_schema!` and the server's own embedded self-client) is new — there was no
`computedParams` surface at all before this feature.

**BREAKING vs. `main`:** Dart's and TypeScript's `computedParams` parameters were already present as
untyped v1 escape hatches and are now typed, which is a breaking shape change for existing callers.
Dart's untyped `Map<String, Object?>?` parameter is now a generated `<Model>ComputedParams` class
(const constructor, one declared-type field per parameterized computed field, `toWire()`, value
`==`/`hashCode` for riverpod family-provider cache safety). TypeScript's untyped
`Record<string, unknown>` is now a generic `CratestackFetchQuery<TComputedParams = never>` /
`CratestackRpcListQuery<TComputedParams = never>`, with a generated `<Model>ComputedParams` interface
substituted in on a gated model and `never` (unassignable, `tsc`-enforced) everywhere else.

TypeScript's swr RPC `get` cache keys now incorporate `computedParams` too, fixing a collision where
two reads of the same id with different resolver params shared one cache entry; known pre-existing
limitation, unchanged by this fix: `update`/`delete` invalidation still only targets the params-less
`get` cache key (mirrors REST's own `get`-invalidation behavior, which has never threaded
`computedParams` through either).

Two bugs surfaced and fixed alongside this feature: the parser silently dropped a
parenthesized/bracketed attribute-argument group separated from its attribute by whitespace — so
`@computed (params: ProxyParams?)` parsed as bare `@computed` with no diagnostic — and now raises a
spanned parse error naming the attribute instead. And the server's own embedded self/peer-calling
client used to decode model and procedure responses into the server-side struct shape (computed fields
excluded by design), so a server calling its own or a peer's API silently lost every resolved computed
value; it now decodes computed-bearing responses into a dedicated wire-shape struct set instead, so
computed fields are visible there too.

### `just cbor-example-verify-ios` no longer fails when the live log capture drops the marker

Marker detection depended on a **live** `log stream` subscription having been listening at the moment
the app printed. It now falls back to `log show` — a retrospective query against the log store, which
does not care whether anything was subscribed — before declaring a failure.

This is the mechanism cratestack#704 points at. In job `97199199670` every one of the 13 captured
lines maps to the last ~15% of a normal launch (`nw_activity` at line 899 of 1069 in a healthy
capture, `BSBlockSentinel:FBSScene` at 911, `KeyboardArbiter` at 927): the tail of a launch with none
of the ~900 lines before it, which is the shape of a subscription that started delivering late, not of
an app that failed to start. A recovered marker still has to pass the payload check, and the run says
loudly that the live capture missed it — a flake that stops failing without being counted is
indistinguishable from one that was fixed.

The `--console-pty` channel could never have covered this. Flutter's `print` does not reach fd 1: it
goes through `Logger_PrintString` → `UIDartState::LogMessage` → `syslog(LOG_ALERT)` into the unified
log, while the branch of that function named "Stdout" emits a VM-service event for DevTools. Verified
on a real simulator with a probe printing the same string four ways — `print` reached the unified log
only, `stdout.writeln`/`stderr.writeln` reached the pty only. The two "independent" channels were
never independent for this marker.

The query excludes the `log` process, which is not cosmetic: `log show` logs its own invocation
including its command line, and the command line contains the marker, so the query matches itself.
Without the exclusion, a run where the app printed nothing would recover that self-match, fail the
payload check, and report a "genuine round-trip failure" about an app that never printed at all.

The `--console-pty` capture is no longer searched for the marker. It is still captured and still
printed on failure — it carries native `NSLog`, dyld and crash output, and anything the app writes to
stderr on its way down, none of which the unified log shows — but it is labelled as diagnostics, and
the recipe no longer implies a marker could appear there. The comment claiming two independent
channels "so the marker is found if EITHER works" is corrected in place: that claim is what made the
cratestack#704 failure read as "the app printed nothing" on the strength of the marker being absent
from "both".

### The `cratestack_cbor` example's verification marker no longer depends on a widget building

Every headless verification of that example — `just cbor-example-verify` and its `-android`,
`-windows`, `-macos`, `-ios` siblings — greps process or console output for a marker the app prints.
That marker was produced by a round trip hanging off `late final _future = runRoundTrip()` on a
`State` object whose only read was inside `build()`, making the assertion everything downstream
depends on a side effect of the app constructing its widget tree.

It now starts in `main()`, before `runApp`, and the widget is handed the already-running future.

Scope, stated precisely: `runApp` schedules `attachRootWidget` on a bare `Timer.run` and
`attachToBuildOwner` inflates the tree synchronously, so the old code ran one event-loop turn after
`runApp` — no frame and no platform scene required. This change removes a dependency on the widget
tree building, **not** on rendering. It is not a fix for cratestack#704, which stays open: on iOS the
engine is launched from `FlutterViewController.viewDidLoad`, upstream of any Dart code, in both the
old and new shape.

Also in `just cbor-example-verify-ios`: the failure path printed `--- device state ---` and `--- is
the app installed? ---` twice (cratestack#705 added a second copy rather than moving the first), and a
passing run now reports what it previously kept to itself — the poll margin, the install duration and
launch time, and how much the app actually logged (capture bytes plus Runner-attributed line count).

That last pair is the point. A green iOS job used to print one tick and nothing else, with 281 seconds
of silence before it (job 97215919059), so a failure had no healthy run to be compared against: when
job 97199199670 captured 2335 bytes holding 13 Runner-attributed lines and then went quiet for 94
seconds, nothing on record said whether 13 was low, normal or high for this app. The same two summary
lines are now printed on both the passing and failing paths, in the same order, so the two can be
diffed directly. They also settle a question that had to be reconstructed by hand from GitHub's line
timestamps: that failing run's install took 111s, *less* than the green run's, so install contention
alone does not predict the flake.

### `just cbor-example-verify` now runs the example's `flutter test`

`dart-packages/cratestack_cbor/example/test/widget_test.dart` existed but ran nowhere in CI and failed
on a clean checkout with `Unsupported operation: Isolate.resolvePackageUriSync` — `flutter test`'s test
VM does not support the synchronous package-URI resolution the native backend's dev-mode fallback
tries when no built app bundle exists yet at that point in the recipe. The pre-existing
`CRATESTACK_CBOR_NATIVE_LIB` override (checked before either resolution strategy) sidesteps this
entirely by pointing straight at the vendored Linux blob, so the recipe now runs the test with that set
rather than leaving it permanently unexercised.

Investigated for cratestack#704 but out of scope for that issue: it does not address the iOS flake.
No test on this repo's Linux-only toolchain could be made to discriminate that failure mode — see
cratestack#715 for the attempts and why they were discarded rather than shipped green.

### The changelog seeder writes its own "no functional changes" no-op for Dart packages (cratestack#713)

Three of the last four `cratestack_cbor` releases needed a hand-written, byte-identical two-line entry
in `dart-packages/cratestack_cbor/CHANGELOG.md` — `changelog-seed.sh` correctly fell back to its
placeholder (nothing under that package's own `## Unreleased` to carry forward), the
`changelog (no unedited seeds)` gate correctly failed on the release PR, and a human rewrote it into
the same wording every time. The seeder wasn't wrong; for this package "nothing to carry" is the normal
case, not the rare one.

`changelog-seed.sh` now writes the standard wording itself — `- No functional changes. Version kept in
lockstep with the CrateStack workspace, which every published CrateStack artifact shares.` — instead of
the marker+commit-list placeholder, whenever a declared package's no-op scope (`.ci/changelog-files.sh`'s
new `CHANGELOG_NOOP_SCOPES`) has zero non-bump commits since the last release tag. No manual edit is
needed, and the gate passes without a human ever touching the file.

The scope checked for `cratestack_cbor` is deliberately **not** just `dart-packages/cratestack_cbor/`.
That package vendors prebuilt binaries built at release time from Rust crates that live outside its own
directory (`crates/cratestack-client-flutter`, `crates/cratestack-cbor-wasm`) — and v0.8.6 is the
load-bearing proof that scoping to the package directory alone is unsafe: `crates/cratestack-codec-cbor`
(depended on directly by both vendoring crates) carried a real CBOR-encoding fix (cratestack#675) in
that release's commit range, confirmed shipped in its vendored binaries (`git merge-base
--is-ancestor` says the fix commit is an ancestor of the `v0.8.6` tag), while
`dart-packages/cratestack_cbor/` itself carried zero commits in that same range. What `v0.8.6` actually
shipped to pub.dev was the raw, unedited seed placeholder — TODO marker and all — because `main` had no
required status checks to block the merge; the "No functional changes" wording seen for that section
today was written afterward, in cratestack#687, using exactly the directory-only proxy this scope
replaces, and per the finding above that retroactive claim is itself wrong. The declared scope now also
covers `crates/cratestack-client-flutter`, `crates/cratestack-cbor-wasm`, and
`crates/cratestack-codec-cbor` — verified against all four of the last four releases' real history,
including the v0.8.6 case, which the widened scope now correctly flags as needing a real entry rather
than auto-filling one.

`dart-packages/cratestack_annotations` and `dart-packages/cratestack_builder` are covered too, scoped to
their own directory each — checked, not assumed: `release-cli.yml`'s publish jobs for both are pure
`dart pub publish` from each package's own checkout with no build step and nothing vendored, and neither
directory contains any binary artifact, so unlike `cratestack_cbor` their own directory is the complete
scope. Both have genuine non-bump history in-directory too (cratestack#699, cratestack#710), so the
inverse (placeholder still fires) matters for them exactly as much as it does for `cratestack_cbor`.

A package that genuinely changed is still caught: any non-bump commit anywhere in a declared scope still
writes the ordinary placeholder, and the gate still fails until a human writes prose — unchanged, and
covered by a decisive test (`.ci/changelog-seed-tests.sh` Test 28) that seeds a range with a real change
reaching the scope only through one of the extra directories, never the package's own, and asserts the
gate still goes red. The root `CHANGELOG.md` is unaffected — it's named in the new
`CHANGELOG_NOOP_EXEMPT` list rather than given a scope, and always takes the existing "carry forward
`## Unreleased` prose" path.

`changelog-seed.sh` now also refuses to run (before writing anything) if any file declared in
`CHANGELOG_FILES_DEFAULT` has neither a `CHANGELOG_NOOP_SCOPES` entry nor a `CHANGELOG_NOOP_EXEMPT` name
— the coverage gap that silently affected `cratestack_annotations` and `cratestack_builder` after they
were added to the declared set in cratestack#714 (no failure, just no benefit from this mechanism until
someone noticed). Proven by a decisive test (Test 31) that adds an uncovered fourth fixture file, confirms
the guard refuses and names it, then confirms the same declaration with the gap closed does not.

## 0.8.10 (2026-08-23)

### `just bump` no longer silently skips a Dart package that has drifted

The bump's `dart-packages/*/pubspec.yaml` rewrite was anchored to the *old* workspace version, so it
only touched a package whose version already matched. Any package that had diverged was skipped
without a word — and skipped again on every later bump, because the thing that was broken was the
match itself.

That is what broke the 0.8.9 release. `cratestack_annotations` and `cratestack_builder` were
deliberately at 0.8.8 while the workspace was 0.8.7 (they needed an annotation-surface release of
their own), so the 0.8.7 → 0.8.9 bump matched only `cratestack_cbor` and left the other two behind.
pub.dev's automated publishing rejects a tag that disagrees with the pubspec:

```
this token has 'refs/tags/v0.8.9' ref for which publishing is not allowed.
Expected tag 'v0.8.8'.
```

crates.io and npm had already published by that point, so the release was half-out and 0.8.9 could
not be re-cut.

The rewrite is now unconditional, and a post-condition asserts every `dart-packages/*/pubspec.yaml`
ended at the new version, failing the bump loudly if one did not. A failed bump costs a re-run; a
silent skip costs a burned version.

`cratestack_annotations` and `cratestack_builder` are brought back to the workspace version here.
`cratestack_builder`'s `cratestack_annotations: ^0.8.8` constraint is deliberately untouched — for
`0.x` releases pub's caret already spans the whole `0.8.x` series, and that floor states an API
requirement (the first release carrying `touchFlagFields`/`nonDefaultingListFields`), not a version
relationship.

## 0.8.9 (2026-08-23)

### `changelog-seed-tests.sh` Test 5 now exercises a self-contained git fixture instead of the ambient repo's tags/HEAD (#670)

Test 5 asserted "at least one `#### ` conventional-commit type grouping appears in the seed" against
`changelog-seed.sh`'s real, ambient `git log "${last_tag}..HEAD"` range — so what it actually measured
depended entirely on the checkout it ran in. Locally, on a full clone with `HEAD` exactly at the newest
release tag (immediately after a release-bump merge), that range is empty, so the assertion went red. In
CI, `actions/checkout`'s default shallow, tagless clone finds no tags at all, so `last_tag` resolves empty
and `range` silently degrades to plain `HEAD` — the assertion passed against a one-commit log for a reason
that had nothing to do with the range logic being correct, and would have kept passing even if that logic
were completely broken. `.github/workflows/prepare-release.yml` deliberately checks out with
`fetch-depth: 0` because the production seeder genuinely depends on the tag-range computation being right;
this test is what is supposed to guard it, and in CI it guarded nothing.

Test 5 now builds a disposable git repository of its own — a known commit before a known `v*` tag, and
known commits after it — and points `changelog-seed.sh`'s git calls at that fixture via
`GIT_DIR`/`GIT_WORK_TREE` (the script always `cd`s to the real project root itself, so the existing
`CHANGELOG_FILE` sandbox seam alone can't relocate which repository `git log`/`git tag`/`git rev-parse`
read from). The decisive assertion is negative: a commit made *before* the fixture's tag must not appear
in the seed, which is what actually exercises the `last_tag`/range computation rather than merely "some
grouping appeared." The test now passes deterministically regardless of the ambient repo's tags, history,
clone depth, or `HEAD` position, and fails if the range computation regresses.

### `` ```ignore `` doctest fences converted to `` ```text ``; `tests-ignored-report`'s doctest blind spot documented and guarded (#683)

On edition-2024 crates, `cargo test --doc -- --ignored` merges doctests and reports every
`` ```ignore ``-fenced block as passing **without compiling it**, so `test-ci-ignored-report`
(and the non-blocking `tests-ignored-report` CI job that runs it) was structurally blind to
anything fenced `` ```ignore ``. #683 proposed forcing `--merge-doctests=no` to make them
actually compile; that was rejected after triage — every `` ```ignore `` doctest in `crates/`
(10 of them, across `cratestack-sql`, `cratestack-pg`'s test suite, `cratestack-api`,
`cratestack-core`, `cratestack-axum` x2, `cratestack-auth` x2, and `cratestack-client`) turned
out to be illustrative pseudocode never meant to compile — elided struct bodies, free variables
with no scope, a JSON environment-variable value, a reference to a schema file that doesn't
exist. Forcing compilation would make the job permanently red for zero real defects.

Instead, every one of those 10 is now fenced `` ```text `` — rustdoc never schedules a `text`
block as a doctest under any flag or merge mode, so the ignored-sweep has nothing left to be
blind about for them. This generalizes #611's fix (`8fb373d`) for the single macro-generated
`invoke_with_db` example to every hand-written one. One `` ```ignore `` fence remains, in
`cratestack-rusqlite/src/opfs.rs`: it is genuine, would-compile Rust, but only under
`--target wasm32-unknown-unknown`, which no doctest sweep in this repo's CI runs against — a
real gap, tracked rather than silently converted or force-fixed.

`.ci/ignore-doctest-fence-check.sh` (`just verify-ignore-doctest-fences`, wired as CI's
`ignore-doctest-fence` job) is the regression guard: a newly reintroduced `` ```ignore `` fence
around illustrative content fails the check, with an explicit, maintained exception list for
the one documented real-but-skipped case.

### `cratestack-client-rust`'s RPC batch client silently dropped explicit nullable-column clears (#677)

`BatchableCall::new` (the constructor every macro-generated batched CRUD/procedure call goes through)
stripped `null`-valued object entries out of the input before handing it to the codec, recursing into
nested objects. That strip was a workaround for a CBOR mis-encoding bug fixed at the root by #657/#675
(`CborCodec::encode` now sets `serialize_unit_as_null`, so `serde_json::Value::Null` correctly encodes
as RFC 8949 CBOR null `0xf6` instead of the empty-array marker `0x80`).

While the strip remained, it recursed into a batched `model.<Model>.update`'s nested `patch` object and
removed an explicit `null` there exactly like an untouched field — indistinguishable from "the caller
never mentioned this field" by the time the server decoded it. Since absent means "leave the column
alone" and explicit null means "clear it" for nullable-column patch inputs, **a batched update that
explicitly cleared a nullable column silently left it unchanged**, with no error surfaced anywhere.
Non-batched (`.await`ed) RPC calls and the REST client were never affected — only calls queued via
`.queue(&mut batch)` went through this code path.

The strip is removed; the Rust batch client now puts explicit `null` on the wire as CBOR null (`0xf6`),
matching every other client (Dart, TypeScript, the server's own encoder) and the root-cause codec fix.
An untouched field continues to stay off the wire entirely (`skip_serializing_if`, #663) — that contract
is unaffected and is covered by a decode-level regression test asserting on raw encoded bytes, not
decoded values, decisive against a re-introduced strip.

### `transport grpc`/`@pb` rejection messages and docs now say 0.8.5, not v0.9 (#654)

Protobuf/gRPC removal (#655) was planned as a v0.9 breaking change and every reference said so, but it
actually shipped in 0.8.5 — there was never a v0.9 release, and the workspace has since moved past
0.8.7. A schema author on a current release who writes `transport grpc` or `@pb(N)` was told the
feature "was removed in v0.9", a version that does not exist.

The two user-facing parser error strings (`transport grpc` rejection, `@pb` field-attribute rejection)
now name 0.8.5, along with the tests that pin them and the internal comments/design docs that stated
the same wrong version as fact. ADR 0017 (the removal decision itself) keeps its original "effective
v0.9" decision-time wording intact, with a short dated correction note added under Status pointing at
the real 0.8.5 release — the decision record isn't rewritten, just annotated.

### `generate-dart` defaults to the native `cratestack_cbor` codec; `--native-cbor` is replaced by `--no-native-cbor` — breaking (cratestack#563 follow-up)

`cratestack_cbor` 0.8.7 is published on pub.dev with Windows, macOS and iOS support verified there, so
the two reasons the codec choice was opt-in — a platform-support gap, and the published package lagging
the repo — are both closed. Linux arm64 is the one remaining unsupported target.

`generate-dart`'s `--native-cbor` flag is **removed**, not merely defaulted differently: a bare
`bool` flag cannot express "on by default", so the flag is replaced by `--no-native-cbor`, which
defaults to off (native on). **Any existing `--native-cbor` invocation — CLI, script, or CI job — is
now an unknown-argument error**, not a no-op; update call sites to drop the flag (native is now the
default) or, for Linux arm64 / any consumer that wants the dependency-free pure-Dart codec, to pass
`--no-native-cbor` instead. `DartGeneratorConfig::DEFAULT_NATIVE_CBOR` (the library-level default for
callers who construct `DartGeneratorConfig` directly rather than going through the CLI) flips from
`false` to `true` for the same reason.

Every other emitted file is unaffected — this only changes which of `pubspec.yaml`'s CBOR dependency
and `lib/src/runtime.dart`'s codec import a freshly generated (or regenerated, unflagged) client picks
by default.

## 0.8.7 (2026-08-23)

### `changelog-seed.sh` re-seeds a fresh `## Unreleased` heading after every release (#688)

`changelog-seed.sh`'s conversion branch turned `## Unreleased` into the new dated `## X.Y.Z (date)`
section without leaving a fresh, empty `## Unreleased` behind. Every release therefore reset the file
to a state with no obvious place for the next contributor to file an entry, and three PRs in one day
(#672, #680, #686) ended up misfiled under an already-released section as a result — which in turn
meant the following bump found nothing under `## Unreleased` and fell through to the placeholder-seed
fallback, merged unedited into v0.8.6.

The seed script now emits a fresh, empty `## Unreleased` heading immediately above the newest dated
section on all three paths (prose present, present-but-empty, absent entirely), for every changelog in
`.ci/changelog-files.sh`, not just the root one. See `CONTRIBUTING.md` for the contributor-facing
convention this cements.

### Field-level `@allow`/`@deny` is rejected at parse time — breaking (#679)

A field-level `@allow(...)` parsed, reported `schema OK`, was retained in the IR, and was then read by
nothing. It is an annotation that reads as access control and enforces none: the field reached every
caller the model-level read policy admitted, exactly as if the annotation were absent.

It now fails to parse, on all five field-bearing declaration kinds — `model`, `view`, `mixin`, `type`
and the `auth` block — naming the offending field and pointing at what does work: model/view-level
`@@allow`/`@@deny` for row visibility, `@readonly` to keep a field out of generated inputs, and
`@server_only` to keep it out of client responses.

Breaking for any schema currently carrying one, though only in the sense that a line which never did
anything now says so. Procedure-level `@allow`/`@deny` and model/view-level `@@allow`/`@@deny` are
untouched — this targets the field-position, single-`@` case only.

This is half of #679. The other half — that unknown field attributes are accepted generally, so a
misspelled `@raedonly` silently drops `@readonly` and leaves a field writable — is deliberately not
addressed, because catching it needs the generic unknown-attribute pass that
`validate/removed_attributes.rs` documents as an intentional non-choice. #679 stays open for it.

### `cratestack_cbor` gains macOS, Windows and iOS (#563, via #685 and #690)

The pub.dev package shipped Linux x64, web and Android; it now also ships **macOS** (arm64 + x86_64
as one universal xcframework), **Windows** x64, and **iOS** (device `ios-arm64` plus a universal
simulator slice). Every one of them is a prebuilt vendored artifact — the maintainer decision that no
consumer needs a Rust toolchain or a network fetch at build time still holds, and no consumer build
invokes cargo or cargokit. CI builds a real app on each platform, asserts the artifact is inside the
built bundle, and runs the binary: the same CBOR fixture now round-trips byte-identically on six
targets. iOS and Linux arm64 were the remaining gap; only Linux arm64 is left.

The macOS artifact ships **zipped**, and that is load-bearing rather than a packaging preference.
`dart pub publish` dereferences symlinks when it builds its archive, and a macOS framework is a
versioned bundle whose three symlinks are structural — so shipping the directory delivered a
framework that failed `codesign` outright, taking a consumer's `flutter build macos` down with
`Command CodeSign failed with a nonzero exit code`. That was measured on real hardware, not inferred,
and no symlink-free layout avoids it (each alternative fails elsewhere: `bundle format is ambiguous`,
`unsealed contents present in the root directory`, `did not contain an Info.plist`, `does not use
shallow bundles`). So `just cbor-vendor-macos` also emits an `.xcframework.zip`, the package's
`.pubignore` keeps the unpacked directory out of the archive, and the podspec's `prepare_command`
unpacks it at pod-install time. No broken artifact was ever published — the defect was found and
fixed before any release carried macOS at all.

iOS needs none of that: shallow bundles have no symlinks to lose, so it ships unpacked. That is
asserted rather than assumed — `just cbor-vendor-ios` counts symlinks and fails loudly, naming the
macOS zip mechanism as the remedy, if the assumption ever breaks.

The verification recipes now exercise the **published** shape, not the built one:
`just cbor-example-verify-macos` deletes the unpacked xcframework before building, forcing
`prepare_command` to reconstruct it. The original defect passed a fully green CI run precisely
because nothing tested what a consumer actually receives.


## 0.8.6 (2026-08-21)

### `cratestack_annotations` + `cratestack_builder` Dart packages (#668, phase 1)

Two new hand-written Dart packages, groundwork for moving builder emission out of the Rust generator
and into the Dart ecosystem. Nothing generated changes yet — no `.cstack` schema, generated client,
or example is affected, and the Rust generator still emits builder classes inline.

`cratestack_annotations` carries `@CratestackBuilder` and has **no dependencies at all**.
`cratestack_builder` is the `source_gen`/`build_runner` generator that turns an annotated class into
a `part '<file>.builder.dart'`, and is a **dev** dependency. The split is deliberate: a generated
client lists the annotation package under `dependencies:`, and pub resolves a package's own
dependencies transitively into the consumer's graph — folding the two together would put `analyzer`,
`build` and `source_gen` into the runtime graph of every Flutter app consuming a generated client.
Same split as `json_annotation`/`json_serializable`.

Emitted builders match today's inline ones: required-field `StateError` (with required-ness read from
`isRequiredNamed`, so a required *nullable* field is still enforced), the `build` -> `setBuild` shim,
a copy-not-mutate `add{Field}` append setter, and no static `Class.builder()` factory.

`@CratestackBuilder(listDefaults: false)` is the one piece of information the generator cannot derive
for itself. A projection model's list field and a patch input's list field emit byte-identical Dart,
yet must build differently — `[]` versus `null` — so the distinction has to be supplied by whoever
applies the annotation. Inferring it from nullability, the obvious shortcut, produces builders that
are self-consistent and quietly disagree with the schema.

`cratestack_builder` pins `analyzer >=12.0.0 <13.0.0`. The upper bound is load-bearing rather than
defensive: under the riverpod preset this builder will share a `build_runner` pass with
`riverpod_generator`, whose own constraint is `analyzer ^12.0.0`, and allowing 13.x makes `pub get`
fail there before codegen is reached.

Verified on Dart 3.12.1 and 3.13.1 — testing a single SDK is exactly what let the previously
evaluated `lean_builder` through (it passes on 3.12.1 and dies on 3.13.1; see
https://github.com/Milad-Akarie/lean_builder/issues/25).

Three follow-ups landed alongside: both packages were brought into version lockstep with the rest of
the workspace (#674), `cratestack_builder` now resolves against the published `cratestack_annotations`
rather than a path dependency (#678), and the release workflow publishes both to pub.dev on tag push
(#682).

### Untouched update-input fields no longer reach the wire, at any arity — breaking for Dart consumers (#663)

An update input built without touching a field used to serialize that field as an explicit `null` for
`Required`-arity (non-nullable-column) fields, because only the `Optional` arity carried
`skip_serializing_if`. A server cannot distinguish that from a deliberate write, so an untouched field
silently clobbered its column. `PATCH {"name": "x"}` against a model with a second non-nullable field
wrote `null` over it.

Both clients now omit an untouched field at every arity. The Rust side gains the missing
`TypeArity::Required` arm in `struct_only.rs`.

**Dart consumers: the generated API changed.** Dart data classes are flat, with no analogue of Rust's
`Option<Option<T>>`, so a nullable-column patch field now carries a sibling boolean —
`weight` gains `weightIsSet` as a constructor parameter, public field, and builder flag. `false`
means untouched (omitted), `true` with a null value means an explicit clear (serialized as `null`),
and a non-null value is always sent. Existing named-argument call sites still compile, since the new
parameter is optional with a default.

The explicit-clear guarantee from 0.7.x is unchanged: clearing a nullable column still puts `null`
on the wire, verified at the raw CBOR byte level rather than through decoded values.

A schema that declares both a nullable `foo` and a field literally named `fooIsSet` would generate
uncompilable Dart, so that collision is now rejected at parse time with a specific error, alongside
the existing `build`/`set_build` and `add_{field}` guards. A schema with `fooIsSet` and no nullable
`foo` still parses.

TypeScript is unaffected — its plain object-literal model never had this bug.

### `POST /rpc/batch` encodes `null` as CBOR null, not an empty array — wire-format change (#657)

Every `null` crossing `/rpc/batch` was silently corrupted into the CBOR empty-array marker (`0x80`)
instead of RFC 8949 null (`0xf6`), in both directions and for every type, whenever the CBOR codec was
in use. A server decoding the corresponding `Option<T>` failed with "expected text, got array".

The cause is that batch envelope frames are deliberately opaque `serde_json::Value`, and
`serde_json::Value::Null` serializes through `serialize_unit()` rather than `serialize_none()` —
which `minicbor-serde` encoded as an empty array. `CborCodec::encode` now enables
`serialize_unit_as_null`, fixing every shape at once rather than per-call-site.

Bytes on this path change. Decoders were never the broken half — `0xf6` always decoded correctly
across the Rust, napi, wasm and JS paths — so old-client/new-server and new-client/old-server both
remain safe. `cratestack-client-rust`'s batch path still strips nulls before the codec, a workaround
predating this fix, so the Rust batch client does not yet exercise the corrected encoding; removing
that strip is tracked in #677.

### Read policies can compare a required enum field against a literal variant (#666)

`@@allow`/`@@deny` literal comparisons were limited to required `Boolean`, `Int` and `String` fields,
so a model discriminated by an enum column — the common shape where a table mixes public and
sensitive rows — could not express its own visibility rule declaratively. The two workarounds were a
parallel boolean discriminator that can drift out of sync, or moving the rule into hand-written Rust
and giving up the generic CRUD surface entirely.

A required enum field can now appear in a literal comparison on both model and view descriptors. The
variant name is validated against the declared enum and lowers to the existing `PolicyLiteral::String`
(enum columns are stored as `TEXT`). Optional- and list-arity enum fields are rejected with a specific
error rather than mis-compiling.

`in` against a set of variants is not implemented; `purpose == a || purpose == b` expresses the same
policy through the existing `Or` path. Field-level `@allow` remains a no-op — see #679.

### Typed TypeScript clients can read `ETag` and send `If-Match` (#610)

Generated TypeScript model methods gained `getWithResponse`, returning both the decoded value and the
`Response` so callers can read `ETag`, plus an optional `ifMatch` on `update` and `delete`. Previously
the runtime's `request()` discarded the `Response`, making the ETag unreachable from generated code
even though the server had been emitting it since 0.7.x.

Additive: `ifMatch` is an optional field on the existing options object, not a positional parameter,
so existing call sites keep compiling. Both the default and `--swr` presets are covered, and the swr
variant routes through the same decimal revival as its `get`, so a `Decimal` field comes back as a
`Decimal` rather than a string.

REST transport only. RPC has no per-route `If-Match` concept, and the generated README documents the
round trip only for REST schemas that actually declare a `@version` model.

### Release changelog seeding covers every declared changelog (#650)

`prepare-release.yml` seeded and verified only the root `CHANGELOG.md`, so the Dart package changelog
was silently skipped — a release could ship with it unseeded and no gate would notice. Both files are
now declared in one list, `.ci/changelog-files.sh`, consumed by the seed script, the check script and
the workflow, so the three cannot drift apart.

The workflow also now stages both files. Previously the seed wrote the second changelog and the commit
step never added it, which would have left the fix inert in production while every local test passed.
An empty or renamed declared set is rejected with a named error in all three consumers instead of
passing vacuously.

### `invoke_with_db`'s generated doc example can no longer be force-compiled (#611)

The illustrative example in every procedure's generated `invoke_with_db` doc comment was fenced
```` ```ignore ````. That is correct for plain `cargo test`, which skips it — but `cargo test --
--ignored` force-runs doctests in the ignored bucket, and this example was never meant to compile:
it references a `procedures::` module, free `db`/`registry` variables and an `await` outside an async
context. Any consumer whose CI runs `-- --ignored` in the crate hosting `include_server_schema!` got
one hard failure per procedure, unfixable from their side because the failing code is generated. One
downstream project worked around it with `[lib] doctest = false`.

The example is now fenced ```` ```text ````. Rustdoc only schedules a fenced block as a doctest when
it believes the block is Rust, so a `text` block is never a candidate in any mode and no flag can
force it. The example still renders verbatim.

Note the symptom was edition-dependent, which is worth knowing if you tried to reproduce it and
could not. On edition 2024 with merged doctests — the default on recent toolchains — `--ignored`
doctests are reported as passing *without being compiled at all*, so the failure never surfaced.
Edition 2021 consumers, including the original reporter, did hit it. The fix does not depend on
which mode is active.


## 0.8.5 (2026-08-21)

### Protobuf/gRPC support removed — breaking (ADR 0017)

`transport grpc` — the third transport a `.cstack` schema could declare, generating
`.proto` messages with a field-number lockfile, a tonic service, and Rust/Dart/
TypeScript(gRPC-Web) clients — is gone. The maintainer decided on 2026-08-13 to drop the
surface entirely rather than continue investing in it: it competed directly with REST and
RPC, the two transports every other part of the framework (policy, idempotency, rate
limiting, audit, the RPC batch envelope) is designed and tested against, while requiring
its own router construction, its own client codegen per language, and its own
presence-based route-suppression logic that turned out not to even solve its own
motivating case (see `docs/design/route-suppression.md` spike's §1, and its 2026-08-18
re-scoping to REST/RPC/clients only — the file itself was not deleted, only narrowed).

Removed: the `cratestack-grpc` and `cratestack-proto` crates; the `grpc` Cargo feature
from `cratestack-pg`, `cratestack-client-rust`, and `cratestack-macros`; three codegen
directories inside `cratestack-macros` (`include/server/grpc/`, `include/grpc_pb/`,
`include/client/grpc/`); the grpc-specific client generator modules in
`cratestack-client-dart` and `cratestack-client-typescript`, and the grpc-specific
runtime module (`src/grpc/`, the hand-written `tonic` client SDK) in
`cratestack-client-rust`; the `transport grpc` schema keyword; the `@pb(N)`
field-number attribute; the `generate-proto` CLI subcommand; `examples/grpc-widgets/`;
and `docs/design/protobuf.md` (the feature's design document, 566 lines) and
`docs/design/grpc-codegen-deduplication.md` (an unimplemented proposal for the removed
surface, now moot).

Also removed, all breaking for consumers of the published crates: the
`cratestack_axum::bridge_grpc_response` re-export (`cratestack-axum/src/rpc/mod.rs`);
the `TransportStyle::Grpc` enum variant in `cratestack-core` (breaking for any
exhaustive `match` over `TransportStyle`); and the `cratestack_client_rust::grpc` module
(feature-gated on `grpc`, off by default).

A schema that declares `transport grpc` or uses `@pb(N)` no longer parses. There is no
compatibility shim and no deprecation window, matching this framework's pre-1.0 lockstep
versioning — the same reasoning `docs/design/route-suppression.md` §5 applies to a
smaller-scoped removal. REST and RPC are unaffected: nothing in this change touches
either transport's grammar, dispatch, or generated clients. See
`docs/adr/0017-remove-grpc-protobuf.md` for the full decision record, including what it
supersedes and the alternatives considered (feature-flagging it off by default; a
deprecation cycle; narrowing scope instead of removing it — all rejected on the same
"fixed cost, not variable cost" ground).

### Typestate builders on every generated struct-shaped type — Rust and Dart, TypeScript excluded

Every struct-shaped type `include_*_schema!` emits now gets a companion builder alongside it: a
model struct, `Create{Model}Input`, `Update{Model}Input`, `{Model}Where`, `{Model}OrderByClause`,
`{Model}FindManyInput`, `view` structs, `type` structs, and per-procedure `Args` all do. Rust reaches
it as `{Type}::builder()`, on both the server and generated client; Dart reaches it as
`{Type}Builder()`. TypeScript is deliberately not covered by this change.

In Rust the builder is a typestate: each required field claims one type parameter, starting at a
`cratestack_core::builder::Unset` marker and flipping to `Set` the moment its setter is called, and
`build()` is only defined on the builder instantiation where every required field's parameter is
`Set`. Forgetting a field is therefore a compile error ("no method named `build` found for struct
`...Builder<Set, Unset>`"), not a `Result` the caller has to remember to check. `Option<T>` and
`Vec<T>` fields — the two shapes whose `Default` already means "the caller said nothing" — get no
type parameter at all, so an all-optional type like `{Model}Where` ends up with a plain, non-generic
builder. The value holder behind the setters is an anonymous tuple rather than a named
`{Type}BuilderFields` struct, specifically so it claims no identifier of its own in the generated
module — one fewer name a schema could ever collide with. On an update input, a setter's parameter
type is the field's type with one `Option` layer peeled off, so the outer `Option` becomes "did the
caller touch this field": `.title("x")` sets the column, `.email(None)` clears a nullable one, and a
field nobody called the setter for stays absent from the wire, exactly like today's struct-literal
update inputs.

Dart's builder is additive only — every existing named constructor and `fromWire` is unchanged, and
nothing is deprecated. Dart has no typestate to borrow, so the required-field check moves to runtime:
`build()` throws a `StateError` naming both the class and the specific field that was never set,
rather than constructing a struct with a bogus default.

**Breaking:** a schema that declares a `type`, `model`, `view`, or `enum` named `{X}Builder`, where
`X` is any name a `model`, `type`, or `view` declaration in that schema causes to be generated — for
a `model M` that means `M`, `Create{M}Input`, `Update{M}Input`, `{M}Where`, `{M}OrderByClause`, and
`{M}FindManyInput`, not just `M` itself — is now rejected at parse time and must rename the
colliding declaration. Likewise, a field set (a model's fields, a `type`'s fields, a `view`'s fields,
or a procedure's args) that declares both a `build` field and a `set_build`/`setBuild` field is
rejected: the Rust builder renames a field literally named `build` to a `set_build` setter so the
terminal `build()` method stays callable, and the same collision exists in the Dart setter names.
Both were previously silent — the schema parsed, and the generated code either failed to compile with
an error pointing at macro-expansion output instead of the schema, or (the Dart setter case) quietly
emitted two identically-named setters.

Dart consumers of a model named `Widget` should note that the newly-emitted `class WidgetBuilder` is
ambiguous against Flutter's own `WidgetBuilder` typedef if both are imported unprefixed — resolve
with `import '...' as prefix`. This is one more instance of an existing hazard rather than a new one:
`model Widget` already emitted `class Widget`, which collides with Flutter's own `Widget` the same
way, and callers of generated Dart clients already need to manage that.

### Builder list fields: `[]` default and an append setter, matched across Rust and Dart (#661)

Two follow-on fixes to the typestate/fluent builders added above, both scoped to list-arity
(`Field[]`) fields.

**Breaking (Dart):** an unset list field no longer makes `build()` throw. Dart previously marked
list fields "required" the same as scalars, so e.g. `PostStatusFilterBuilder().build()` threw
`StateError: PostStatusFilterBuilder.statuses is required but was not set` if `.statuses(...)` was
never called. It now builds with `statuses: []`, matching Rust, where an unset list field has always
defaulted to `Vec::new()` (`crates/cratestack-macros/src/builder/fields.rs::is_required` already
returned `false` for `TypeArity::List`). The generated constructor itself is unchanged — this only
affects the builder's own required-field bookkeeping.

**Added:** a `.add_{field}(item)` (Rust) / `.add{Field}(item)` (Dart) append setter next to every
list field's existing bulk setter. The bulk setter is unchanged and still replaces the whole list;
append pushes one element onto whatever is already there (allocating an empty list on first use).
The name is derived mechanically from the field name with no singularization (`children` stays
`add_children`/`addChildren`) — a schema field that would collide with the setter this generates
(e.g. a list field `tags` alongside a field literally named `add_tags`/`addTags`) is now rejected at
parse time with a message naming both fields, the same treatment as the existing `build`/`set_build`
collision check.

Covers every builder that carries a list field: model structs, `Create{Model}Input`,
`Update{Model}Input` (where `.add_{field}` on an untouched patch field implies "touched", same as
the bulk setter), `type` blocks, and per-procedure `Args` (Rust and Dart both — an earlier draft of
this change only wired the append setter into Dart's procedure-args builder).

One deliberate exclusion: a *relation*-valued list on a model class gets no append setter. Rust
builds model builders from `scalar_model_fields`, which drops relation fields outright, so a Dart
`addPosts` there would have no counterpart — reintroducing the very divergence this change removes.
The exclusion is scoped to the model class specifically, not to "any field naming a model": a `type`
block's fields go through `scoped_builder_fields`, which does not filter relations, so a model-typed
list inside a `type` keeps its append setter on both sides. The relation field's *bulk* setter is
untouched — Dart's model class genuinely carries relation fields (it is what included relations
decode into) and its builder mirrors its own constructor.

Note the practical reach of all of the above: a scalar list field is rejected outright on a
database-backed model (there is no SQL bind representation for one yet), so list builders apply to
`type` blocks, procedure arguments, and models in schemas consumed only via
`include_client_schema!`.

Also fixed in passing, surfaced by the same scalar-list-field builder work: a generated `{Model}Where`
struct no longer offers `.contains()`/`.starts_with()` on a `String[]`/`Cuid[]` field — those two
`FieldRef` methods were never implemented for `Vec<String>`, so a schema with a filterable scalar
list field failed to compile. `.equals()`/ordering ops are unaffected.

## 0.8.4 (2026-08-18)

### `/rpc/batch` authenticates the envelope once, not once per frame

A correctly-signed batch request to `POST /rpc/batch` could be rejected even though the identical
signing implementation was accepted on the unary `POST /rpc/{op_id}` route. `rpc_batch_dispatch`
re-entered the same per-op dispatch functions unary calls use, and each of those independently
calls `AuthProvider::authenticate` — but against a *fabricated* per-op identity (`/rpc/<op_id>`
plus that one frame's re-encoded input), never the real `POST /rpc/batch` request with its raw,
untouched body that a batch client actually sends and signs. For any `AuthProvider` whose verdict
is bound to the real request bytes (a body-hash-bound request-signing scheme, say), the fabricated
identity can never match. Independently, re-running `authenticate()` once per frame is also
incompatible with a provider that treats a successful authentication as consuming a single-use
nonce, since one client-issued nonce would be claimed once per frame instead of once per request.

`rpc_batch_dispatch` now authenticates the real envelope — method, `RPC_BATCH_PATH`, and the raw
body it received — exactly once, and threads the resulting `CratestackContext` through every
frame's dispatch via a new `CachedAuthProvider` instead of letting each frame re-derive (and
re-verify) its own. The per-op dispatch functions themselves are unchanged; `rpc_dispatch_inner`
is generic over its `Auth` parameter independently of the router's own, so only the concrete
provider batch dispatch hands them for the lifetime of one HTTP request changes. See
`docs/design/rpc-transport.md` §5 and `crates/cratestack-macros/src/include/server/rpc_module/
batch.rs`'s module doc for the full mechanism.

## 0.8.3 (2026-08-17)

Two fixes, both to things that reported success while doing nothing.

### The pub.dev publish could report success without publishing

`dart pub publish` exits 0 even when authentication fails. On 0.8.2 the release job printed
`Authentication failed!`, uploaded nothing, exited 0, and the run went green — every other channel
had already published irreversibly by then. A green release meant nothing about whether pub.dev
had received the package.

The publish step now asks pub.dev what it actually serves, polling for the documented ten-minute
indexing window, and fails the release if the version never appears. This is deliberately
cause-independent: an expired token, a rejected credential and a silent upload failure all land in
the same check.

The underlying credential failure is fixed too. `dart-lang/setup-dart` now sits immediately before
the publish rather than nine minutes earlier, because the OIDC token it mints is short-lived and
pub.dev was rejecting it as `Invalid JWT token: invalid timestamps` after the intervening Rust and
Android builds. Publishing invokes `$FLUTTER_ROOT/bin/dart` explicitly, since this package declares
`environment.flutter` and a standalone Dart SDK fails version solving outright.

### Riverpod clients emitted imports and a `part` directive for procedures they did not have

The generated `procedures.dart` import and its matching `part` directive were emitted
unconditionally, so a schema with no procedures produced a client referencing a file that was never
written. Both are now gated on actual use.

## 0.8.2 (2026-08-17)

Release-pipeline fixes only. No changes to the framework, the generators or any published API.

Three defects, each of which stopped a real release partway through:

- **`just release` staged only the root `Cargo.lock`**, leaving the five standalone workspace
  lockfiles — the four `examples/` verification workspaces and `crates/cratestack-studio-ui` — at
  the previous version. That turned `main` red on `facade disjointness`, whose job is
  `cargo metadata --locked` in exactly those directories. It now stages every lockfile via a glob,
  so a future standalone workspace is covered without another hand-listed path.
- **The pub.dev publish could hang indefinitely.** With no OIDC credential configured,
  `dart pub publish` falls back to interactive OAuth and waits on a browser redirect that never
  comes; no job in the release workflow set `timeout-minutes`, so it held a runner for GitHub's
  six-hour default. Bounded now, with a diagnostic naming the likely causes.
- **`dart-lang/setup-dart` was missing entirely** from the pub.dev job. It is the step that creates
  and configures the OIDC token — `permissions: id-token: write` only grants the ability to mint
  one. Without it, authentication could never succeed.

## 0.8.1 (2026-08-16)

The `cratestack_cbor` pub.dev package, plus generator and release fixes.

### New: `cratestack_cbor` on pub.dev

A native CBOR codec for Dart and Flutter, exposing one `CratestackCborCodec` API over two backends
chosen by conditional export: flutter_rust_bridge over a vendored prebuilt library natively, and
the existing `cratestack-cbor-wasm` artifact loaded via `dart:js_interop` on the web. It mirrors
`@cratestack/cbor`'s umbrella shape for JavaScript rather than introducing a third binding of the
same codec.

Supported platforms are **Linux desktop, web, and Android** (arm64-v8a, x86_64, armeabi-v7a), each
proven by a real build and a real run rather than a compile check — including an APK installed and
executed on an emulator. iOS, macOS and Windows are deliberately unsupported for now and throw a
clear `UnsupportedError`.

Measured speedup over pure-Dart `package:cbor` is **~3x**. `#[frb(sync)]` is mandatory to get it:
flutter_rust_bridge's default async dispatch measured *slower* than pure Dart.

### Breaking: TanStack Query emission is now opt-in

`generate-typescript` no longer emits `src/react-query.ts` unless `--tanstack` is passed, matching
how `--swr` and `--refine` already behave. TanStack was the only framework binding emitted
unconditionally, and because `src/index.ts` re-exported it, a Vue, Svelte or plain-Node consumer had
no supported way to avoid resolving `@tanstack/react-query` — the hooks are value imports, not types.

**Upgrading:** add `--tanstack` to keep the hooks. The break is loud (a module-resolution or type
error), never silent. Note that regeneration does not delete files it no longer produces, so remove
the stale `src/react-query.ts` by hand.

### Fixes

- Paged list routes computed `total_count` by re-running the filtered query with paging disabled and
  calling `.len()` on the decoded rows — transferring and decoding every matching row on every list
  request. It is now a real `COUNT(*)` aggregate built from the same `FindMany`, so the count's
  `WHERE` and policy scope are identical to the page query by construction.
- `install-cratestack-cli` retries transient network failures (`curl` 52/56) instead of failing
  unrelated PRs, while a genuine 404 still fails on the first attempt rather than burning the retry
  budget.
- Three Dart generator import defects, and `Bytes`/`Decimal` imports missing at some riverpod loci.

## 0.8.0 (2026-08-14)

Two breaking changes, both requiring a deliberate edit on upgrade. Read the migration section
first — neither is silent, but neither fixes itself.

### Breaking: `Cool*` types are now `Cratestack*`

The framework was originally called CoolStack, and the old name survived in the most-used part of
the public API — `CratestackError` and `CratestackContext` appear in essentially every
consumer-written `AuthProvider` impl, procedure signature, and policy call site.

All 11 types are renamed: `CoolError`, `CoolContext`, `CoolCodec`, `CoolErrorResponse`,
`CoolEventBus`, `CoolEventEnvelope`, `CoolAuthIdentity`, `CoolEventFuture`, `CoolEnvelope`,
`CoolPrincipal`, `CoolBody`. Public functions carrying the old name went with them —
`auth_error_to_cratestack_error`, `principal_to_cratestack_context`, `cratestack_error_to_status`,
`RpcErrorBody::from_cratestack`, and others.

**There are no `#[deprecated]` aliases.** This is a hard break in one release rather than a
deprecation cycle, which is defensible pre-1.0 with lockstep versioning and avoids shipping a
second vocabulary that has to be removed later anyway.

Note `CratestackContext` (the auth/principal result) is a different type from `RequestContext` (the
inbound request view), which is unchanged.

**Generated client SDKs are unaffected.** Only two `Cool*` occurrences existed across the Dart and
TypeScript client crates, so nothing published to npm or pub.dev changes shape.

### Breaking: decimal backends are additive, and the backend is now declared

`decimal-rust-decimal` and `decimal-bigdecimal` were mutually exclusive, enforced by a
`compile_error!`. That made them **non-additive**, violating Cargo's core contract: two independent
crates in one dependency graph, each making a legitimate backend choice, produced a build neither
author could fix.

```toml
# crate-a
cratestack = { package = "cratestack-pg", default-features = false, features = ["decimal-rust-decimal"] }
# crate-b
cratestack = { package = "cratestack-pg", default-features = false, features = ["decimal-bigdecimal"] }
```

Each compiled alone; together they could not compile at all. Both features may now be enabled
simultaneously, and each dependent gets the type it asked for.

The consequence you must act on: **the entry macros now take a required `decimal = RustDecimal |
BigDecimal` argument whenever a schema declares a `Decimal` field**, because the backend can no
longer be inferred from features alone.

```rust
include_server_schema!("schema.cstack", db = Postgres, decimal = RustDecimal);
```

Schemas with no `Decimal` field are unaffected. Reported by the ADORSYS-GIS/webank-services
maintainers with a two-manifest reproduction, which is what made it straightforward to confirm
fixed at both ends.

### Migrating from 0.7.x

1. Rename `Cool*` → `Cratestack*` throughout. A find-and-replace over the 11 type names and the
   renamed helper functions covers it; there is no behavioural change hiding in the rename.
2. If any schema declares a `Decimal` field, add `decimal = RustDecimal` (or `BigDecimal`) to its
   `include_server_schema!` / `include_embedded_schema!` / `include_client_schema!` call.

Nothing else in 0.8.0 requires action.

## 0.7.17 (2026-08-14)

A maintenance release cut from the 0.7.x line, before the breaking changes now on `main` for
0.8.0. Its main purpose is to verify that the VS Code extension's `.vsix` actually attaches to a
GitHub Release — that has never worked, and could only be proved by a real tag.

### The vsix is attached for the first time

`release-vscode.yml`'s five build legs had failed on every release. The extension packaged fine
(`Packaged: … 11 files, 2.01 MB`); the *upload* was rejected, because the step composed its path
from `find . -print`, which emits a leading `./`:

```
Invalid pattern 'packages/cratestack-vscode/./cratestack-vscode-linux-x64-0.7.16.vsix'.
Relative pathing '.' and '..' is not allowed.
```

Two earlier repairs each uncovered the next failure in the same chain rather than finishing it:
#584 fixed a `--` separator that broke packaging, and the first attempt at this fix used
`find -printf`, a GNU extension that would have passed on the three Linux/Windows legs and failed
on the two macOS ones. The step now uses a bash glob — no pipe, no `find`, no GNU extension.

### Stateful WireMock stubs enforce `If-Match`

`generate-wiremock`'s stubs for a `@version` model now mirror the real server's optimistic-locking
contract exactly: a missing `If-Match` is a 412, `If-Match: *` and an unquoted value are 400s, a
stale value is a 412, and a correct one succeeds with the version bumped and returned as a quoted
`ETag`. `DELETE` enforces it too. Models without `@version` are byte-for-byte unaffected.

### `examples/react-vite-refine`

A refine.dev admin app over a generated WireMock backend — schema to typed client to
`@cratestack/refine`'s dataProvider to a working UI, with no database and no hand-written server.
Live CRUD and optimistic locking are verified against a real container in CI. Note the stubs
implement no list filtering, sorting or pagination, so the example deliberately offers no such
controls rather than shipping ones that silently do nothing.

## 0.7.16 (2026-08-13)

### Breaking

#### `--preset swr` is replaced by an additive `--swr` flag (#589)

`cratestack generate-typescript --preset <default|swr>` produced *either* layout, so teams
wanting both ran the generator twice into two directories and depended on two packages. `--swr`
now **adds** the file-per-model + SWR-hooks layout under `src/swr/`, alongside the default one, in
the same package — reachable as `<package-name>/swr` (plus `/swr/models/*`, `/swr/procedures`,
`/swr/procedures.hooks`) through `exports` subpaths. One run, both layouts. `--preset` is gone
from `generate-typescript`; Dart's `--preset riverpod` is unchanged.

Note that the root and `/swr` `CratestackRuntime` are structurally identical but *nominally
distinct* classes, so a runtime built from the package root cannot be passed to a `/swr` hook.
Import it from `/swr` when using the hooks — the generated README says so too.

#### Composite `@@id([...])` is a clean error rather than a panic (#590)

`generate-typescript` and `generate-dart` aborted with `thread 'main' panicked … validated
schemas always have an id field` on any schema containing a composite primary key. That message
was also untrue: the parser accepts such schemas. `include_*_schema!` had rejected them properly
since #136; the CLI generators simply never went through that guard. Both now return the same
message the macros emit, naming the model and the tracking issue. The shared predicate lives in
`cratestack_core::composite_id` so the paths cannot drift apart again.

Colliding pluralized route segments (`model Bus` + `model Buse`, both `/buses`) are now rejected
at parse time (#596). The real server already panicked at startup on such schemas; every codegen
target now refuses them up front instead.

### Content negotiation honours the client's `Accept` preference (#593)

`select_response_content_type` walked the *server's* `response_types` list and returned the first
entry the client merely tolerated, discarding `Accept` ordering and `q=` weights. The generated
Rust client sends `Accept: application/cbor-seq, application/cbor` to prefer streaming and degrade
gracefully — and always got buffered cbor, then died in the frame decoder:

```
codec error: decode cbor-seq item: unexpected type array at position 0: expected map
```

Following `examples/README.md`'s streaming walkthrough verbatim crashed. Negotiation now
implements RFC 9110 §12.5.1 — preference order, `q=` weights, `q=0` exclusion, wildcard
specificity — and the streaming client checks `Content-Type` before framing bytes rather than
assuming. The example's own test previously ran against a mock that ignored negotiation entirely;
it now drives the real server.

### refine.dev support covers RPC schemas (#583, #586, #587)

`@cratestack/refine` gained `createCratestackRpcDataProvider` and `RpcResourceMap` alongside the
REST provider, and `generate-typescript --refine` emits the resource manifest for REST *and* RPC
schemas — same function name, transport-appropriate type, so consumer code is identical either
way. Only `transport grpc` is rejected. Optimistic locking works identically across transports:
both dispatch paths read `If-Match` from the same shared handler.

### WireMock stubs are stateful, and correct (#588, #591, #596)

`generate-wiremock` now emits stateful model CRUD for REST schemas — a created record appears in
the next list, an update is visible on the next get, a deleted record 404s — via
`wiremock-state-extension`, built by `crates/cratestack-mock-wiremock/docker/Dockerfile` (the
published jar is unusable against any `wiremock/wiremock` image; see the design doc). RPC stubs
and procedure stubs remain static, and list filtering/sorting/pagination are not implemented.

Three correctness defects found and fixed before this shipped: falsy values (`false`, `0`, `""`)
were silently dropped so a mock consumer could never toggle a boolean off or zero a counter;
concurrent updates to one record corrupted shared list state; and colliding route segments served
one model's data under another's shape. The image runs as a non-root user.

### Five test suites that had never executed now run (#597)

Suites that printed `ok` while doing nothing, all now wired into CI with `CRATESTACK_REQUIRE_DB=1`
turning a missing database into a failure rather than a skip:

- `cratestack-outbox`'s transactional guarantees — atomic persist, cursor-ordered drain, GC sweep.
  Previously "5 passed; finished in 0.00s" with no database touched; now 7.59s of real I/O.
- `cratestack-migrate`'s Postgres introspection (7 tests, behind a feature no job enabled).
- The CLI's `migrate baseline` tests (5 tests, gated on a variable CI never set).
- The `rate_limit` and `pgvector` feature-gated tests, which compiled to empty binaries.
- The generated RPC refine manifest, which CI never typechecked.

### Release pipeline

`just release` staged only two of the sixteen `packages/*/package.json` files, silently leaving
fourteen version bumps uncommitted and setting up an `EPUBLISHCONFLICT` on the next publish — the
same defect fixed in `prepare-release.yml` in #581 and never backported (#592). Trunk's unpinned
mid-build download of `wasm-bindgen` — which killed the v0.7.15 crates.io publish partway and
blocked three merges — is now pinned to the version derived from the studio-ui lockfile, so it
cannot drift back silently (#601). `@cratestack/refine` publishes from CI (#581), and the VS Code
extension's vsix build works again (#584).

`cratestack-studio` and `cratestack-cli` rejoin at this release; they were stranded at 0.7.12 by
the publish failures above.

### Documentation and examples

Several examples failed when their documented steps were followed verbatim — `pnpm install`
silently installing nothing in four browser examples, a command wrong in two ways, a bootstrap
step that would clobber tracked source (#595). In-repo prose contradicting shipped code was swept:
gRPC procedures described as unsupported after they shipped, `Value`'s wire format described as
tagged since #506 made it untagged, and three design docs claiming nothing had shipped (#594).
The npm bootstrap's build-and-verify steps are no longer optional (#585).

## 0.7.15 (2026-08-13)

### `@length` on a `Bytes` field now compiles instead of failing `cargo check` with E0308 (#572)

`cratestack check` accepted `@length(min: ..., max: ...)` on a `Bytes` field — the parser's
`check_length` deliberately permits `String` and `Bytes` alike — but the codegen for `@length`
called `::cratestack::validate_length` unconditionally, and that helper is hard-typed to `&str`
while `Bytes` fields generate as `Vec<u8>`; the emitted `validate()` then failed a schema author's
own `cargo check` with a type error inside macro-expanded code they never wrote. `crates/cratestack-macros/src/validators/emit.rs`
now dispatches `@length` on the field's scalar the same way `@range` already dispatches `Int`
versus `Decimal`, and `Bytes` gets a new sibling helper, `validate_length_bytes`, that counts raw
byte length rather than reusing `validate_length`'s `char`-count semantics — a `Vec<u8>` has no
character encoding to count against, so byte length is the only sensible reading, unlike `String`
where "length" is already ambiguous between chars and UTF-8 code units. `@range`, `@regex`,
`@email`, `@uri`, and `@iso4217` were audited against every scalar the parser permits them on and
found not to share this bug — `@length`/`Bytes` was the only parser-accepted, codegen-broken pair.

### JSON/CBOR `null` in a PATCH body now actually clears a nullable field, instead of silently doing nothing (#567)

**Behavior change for existing callers:** sending an explicit `null` for a nullable field in a
`PATCH` body previously left that column completely unchanged (a silent no-op indistinguishable
from omitting the key); it now sets the column to SQL `NULL`, matching what `Update{Model}Input`'s
own `Option<Option<T>>` shape always implied. `Update{Model}Input`'s generated `Deserialize` had no
`deserialize_with` for its nullable fields, so serde-derive's blanket `Option<T>: Deserialize`
collapsed both "key absent" and "key present with `null`" to the same outer `None`, and
`update_sql_value` reads a `None` field as untouched and skips the column — so "clear this field"
was unreachable over the wire on every transport (REST JSON, CBOR, and RPC all share the same
generated `Deserialize` impl, since `CoolCodec::decode` is generic over the target type). Fixing
only that inbound side would have been a worse regression on its own: every generated client
(Rust/Dart/TypeScript) builds a full `Update{Model}Input` with `..Default::default()` for untouched
fields and serializes the whole struct, so it was already sending `"field": null` for fields it
never touched — harmless only because the deserialize bug treated that the same as an absent key.
`crates/cratestack-macros/src/model/struct_only.rs::struct_field_definition` now emits
`#[serde(default, deserialize_with = "::cratestack::deserialize_double_option", skip_serializing_if
= "Option::is_none")]` on nullable update-input fields specifically (non-nullable fields, and every
`Create{Model}Input` field, are untouched by this change): the new `cratestack_core::patch::deserialize_double_option`
helper recurses into the inner `Option` on decode, and `skip_serializing_if` keeps an untouched
field off the wire entirely on encode, so the three states — field omitted, explicitly cleared,
explicitly set — stay distinguishable in both directions. #537's "an explicit `Some(None)` is a
validation no-op" design decision (`crates/cratestack-macros/src/validators/emit.rs`) still holds:
that state is now actually reachable over the wire for the first time, and the validator correctly
still skips it.
### `cratestack-studio`'s crate rustdoc, README, and starter `studio.toml` comments no longer contradict #553's shipped behavior (#507)

#553 routed `[target.db]` `@version` bumping onto every backend and `@@emit(...)` outbox writes onto
Postgres, but left the crate-level rustdoc, `README.md`, both starter `studio.toml` templates, and
`TargetDb::allow_unsafe_writes`'s own doc comment saying the pre-#553 thing: that `@version` is
"never bumped" and `@@emit(...)` "never writes" an outbox row, unconditionally, with routing
"remain[ing] an open, unimplemented option". All five now state the real, per-backend picture and
why it's permanent rather than a to-do: `include_embedded_schema!` treats `@@emit(...)` as a no-op
on the framework's own generated embedded backend (no design exists for a SQLite outbox anywhere in
the framework, embedded or Studio), so Studio refusing an `@@emit(...)` write on a non-Postgres
`[target.db]` target without `allow_unsafe_writes` mirrors a real framework capability boundary, not
an invented Studio-only guarantee — closing cratestack#507's SQLite half by leaving the refusal in
place and documenting it prominently (option "b"), not by giving SQLite an outbox nothing else in
the framework has. No code path changed; `crates/cratestack-studio/src/api/records/guards/tests.rs`
and `crates/cratestack-studio/tests/unsafe_db_writes.rs` already pinned the exact refusal shape
(refused for `@@emit(...)` alone or combined with `@version`, never for `@version` alone, never for
an unrelated model) and both still pass unchanged, alongside `tests/postgres_routed_writes.rs` and
`tests/postgres_unsafe_writes.rs` confirming #553's Postgres behavior is unaffected. Separately, a
post-merge review of #553 raised a P2 — Studio's own `build_update_sql` bumps `version` off only a PK
predicate, with no expected-version check, so two concurrent Studio writes can lose an update, distinct
from #553's proof that a *third party's* later CAS sees the bump — left open as a maintainer decision
(Studio is an admin surface; a raw overwrite there may be intended) rather than changed here.
### `@cratestack/refine` — a refine.dev `DataProvider` over the generated TypeScript REST client (#571)

New package `packages/cratestack-refine`, the safe end-user admin-UI surface `cratestack-studio`
deliberately isn't: Studio talks to `[target.db]` directly and bypasses `@@allow`, while a refine app
built on `@cratestack/refine` goes through the generated API and inherits policy, validation,
`@version` concurrency, and audit. Ships as a hand-written runtime package plus a small per-resource
manifest (`createCratestackDataProvider({ resource: { api, primaryKey, paged, versionField } })`),
not a code generator — the generated client carries no runtime metadata (primary-key name, `@@paged`,
`@version`) to discover, so a generator would only ever emit the same few-line object literal a
developer can write directly against their own schema. Implements `getList`/`getOne`/`getMany`/
`create`/`update`/`deleteOne`, plus `createMany`/`updateMany`/`deleteMany` as N sequential
single-record round trips (the generated REST client exposes no batch endpoint to back a real atomic
bulk operation). refine's filter operators map onto the generated list route's `field__operator=value`
query convention; an operator with no cratestack equivalent (`endswith`, `between`, `nin`, `containss`,
refine's `or`/`and` groups) throws rather than silently dropping the filter. `@version` models get
`If-Match` threaded automatically through both update and delete (cratestack#493/#519/#538), with a
stale write surfaced as a distinguishable `412` conflict rather than a generic failure — proven by a
test that drives a real generated client against a fake server enforcing the contract, not a mock that
always says yes. `liveProvider` and `authProvider` remain out of scope, tracked as follow-ups.
### `run_in_tx`/`db.transaction(...)` `AuditSink` gap: option (b) investigated and found not cleanly achievable, so option (c) is now documented prominently (#534)

#554 shipped option (a) for #534 (`run_in_tx` hands back a `RunInTxOutcome` carrying its
`AuditEvent`s; `dispatch_audit_sink` is `pub`), but left options (b) ("the runtime takes
ownership via a commit hook") and (c) ("accept and document permanently") as open maintainer
decisions. This closes that: #539's `db.transaction(...)` combinator, cited in #554's PR body
as a plausible host for (b), does not give the framework the reliable commit hook (b) needs —
its closure returns an arbitrary, caller-chosen `T`, so `transaction()` cannot discover which
`RunInTxOutcome`s the body produced, and `SqlxRuntime::pool()` staying public means a caller can
always bypass the combinator entirely via `db.pool().begin()` anyway, so any hook attached only
to `transaction()` would be incomplete by construction. **This PR takes option (c)**, documented
where a caller will actually meet it rather than in a `pub(crate)` doc comment: the
`SqlxRuntime::transaction`/generated `Cratestack::transaction` rustdoc, `cratestack_core::AuditSink`'s
trait doc, `run_in_isolated_tx`'s module doc, and the `cratestack-docs` audit-log guide (which
described the pre-#554 state and needed updating regardless). A new decisive test,
`chained_db_transaction_writes_do_not_auto_dispatch_to_sink` in `banking_chained_audit_tx.rs`,
locks in the documented claim against real Postgres — sabotaged (temporary auto-dispatch call
inserted) to confirm it actually fails before the fix, restored to confirm green — so a future
`db.transaction()` change that accidentally starts auto-dispatching gets caught rather than
silently making the docs false. Independently re-verified #554's claim that the identical
`run_in_tx` asymmetry on the `@@emit` event outbox needed no code change: `drain_event_outbox`
queries `cratestack_event_outbox WHERE delivered_at IS NULL` directly against the pool, with no
dependency on which transaction wrote a row, so it holds.
### `cratestack-client-flutter` gains a native CBOR<->JSON bridge for `flutter_rust_bridge` (#563)

`crates/cratestack-client-flutter`'s new `cbor` module wraps `cratestack-codec-cbor`'s `CborCodec`
for `flutter_rust_bridge`, laying the Rust-side groundwork for a `cratestack_cbor` pub.dev package
(publishing infrastructure is a separate follow-up). It composes with the existing
`FlutterCborSeqDecoder` rather than duplicating it: that type finds item boundaries in a streamed
`application/cbor-seq` body, `cbor::decode_json` decodes the bytes of each item once found. Because
flutter_rust_bridge has no dynamic "any JSON value" wire type, the boundary is JSON text — a Dart
caller runs `jsonEncode`/`jsonDecode` on its side. Round-trip tests cover the scalar matrix
generated clients use, including `Decimal` (round-trips byte-identical to `CborCodec`'s direct
encoding, matching Dart's existing `Decimal` -> `String` convention) and a documented, deliberate
non-match for `Uuid` (a JSON-shaped boundary can't carry `CborCodec`'s compact binary encoding for
it). A real benchmark against pure-Dart `package:cbor` (`benches/cbor_bridge/README.md`) measured
~3-4.4x, not the ~55x/~1000x originally estimated on a different stack — reported honestly rather
than carried over. flutter_rust_bridge glue generation follows the same permanent,
generate-don't-commit pattern `embedded_flutter_native` already established, now backed by a
`just frb-generate <dir>` recipe for local codegen.

## 0.7.14 (2026-08-12)

### The crates.io publish order no longer mistakes a dev-dependency for a cycle (#564)

`just release-publish` decides the order it uploads crates by topo-sorting the `cratestack-*`
workspace packages out of `cargo metadata`, and it counted every dependency edge regardless of
kind. That is wrong for dev-dependencies: `cargo publish` never needs one to already exist on the
registry in order to package a crate, so a dev-dependency edge places no constraint on publish
order and may legally point "backwards". #540 added `cratestack-api` as a dev-dependency of
`cratestack-macros` — for the `Authorized`-witness expansion tests — while `cratestack-api` already
depended on `cratestack-macros` normally, and the sort read that legitimate pair as a hard cycle
and aborted the whole publish. The sort now excludes `kind == "dev"` edges while still counting
build dependencies, which genuinely do constrain ordering. Two supporting changes: cycle detection
is unchanged and still aborts on a real cycle, and the recipe no longer discards `cargo metadata`'s
stderr, which previously reduced any underlying metadata failure to a single generic "failed to
compute publish order" line with the actual cause thrown away.

### `just bump` discovers standalone example workspaces instead of relying on a hand-maintained list (#565)

Several crates under `examples/` are their own `[workspace]` roots, deliberately excluded from the
root workspace so that a real `cargo tree` in each one proves facade disjointness for an external
consumer. Each therefore carries its own `Cargo.lock` that the root `cargo check` never touches, and
`just bump` has to refresh them explicitly or their locked path-dependency versions silently drift
behind the workspace version — breaking every `--locked` build and `cargo metadata` call in that
directory. The refresh step listed three such directories by hand, and carried a comment warning
that "every standalone workspace under examples/ must be listed here; adding one without a line
below is a latent CI break that only fires on the next bump". `examples/db-transaction-verification`
arrived with #539 and was never added, so the warning came true exactly as written. The step now
globs `examples/*/Cargo.toml` for a `[workspace]` table and refreshes whatever it finds, and asserts
it found at least one so that a future layout change breaks the bump loudly rather than silently
refreshing nothing — the same silent-skip failure the block exists to prevent.

### Every `AuditSink` dispatch site is now covered by a test bound to it (#473)

`@@audit` fan-out to an installed `AuditSink` is wired identically into eleven write paths — `create`,
`update`, `delete`, `upsert`, `upsert_do_nothing`, `update_many` and `delete_many`, plus the four
`batch_*` variants — but only two of them (`create` and `batch_create`) had a test asserting the sink
actually received the event. The other nine were structurally identical and entirely unverified, so a
copy/paste omission in any one of them would have passed CI silently. That matters more than an
ordinary coverage gap, because the whole value of an audit sink is that it fires unconditionally: one
that observes nine write kinds out of eleven is a weaker guarantee than one that observes none,
since the hole is invisible. `banking_audit.rs` now asserts sink receipt for all eleven, each test
checking an exact event count so that a double-dispatch regression fails too, and each was verified
by removing its own dispatch call and confirming that precisely the matching test — and no other —
turned red. No defect was found: all eleven already dispatched correctly. What changed is that this
is now demonstrated rather than assumed.

### The generated gRPC CRUD surface is now exercised by CI (#524)

`crates/cratestack-pg`'s gRPC integration tests are gated behind `#![cfg(feature = "grpc")]`, and
`grpc` is not one of the crate's default features. The CI test recipe never passed `--features grpc`,
so every one of those files — `transport_grpc.rs`, `grpc_auth_provider_extensions.rs` and
`trusted_proxy_client_ip_grpc.rs` — compiled to an empty zero-test binary and reported `ok` on every
run. Cargo builds every file under `tests/` regardless of an inner `#![cfg(...)]`, so nothing about
the output distinguished "these tests passed" from "these tests do not exist"; `--list` printed
`0 tests` where it now prints five. The recipe now enables the feature, which switches that entire
pre-existing gRPC suite on. Alongside it, a new test drives create/get/list/update/delete through the
real generated `into_router()` against a live Postgres, giving the five deduplicated CRUD arm
builders a regression test that fails when an arm is mis-wired — verified by swapping two arms'
dispatch functions and confirming the corresponding tests go red. That refactor previously rested on
a one-time manual `cargo expand` comparison with nothing checked in to catch a later mistake.

### The decimal-backend feature exclusivity is documented as a graph-wide invariant (#505)

`decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive, and because Cargo unifies
features across a dependency graph, that exclusivity is a property of the whole build rather than of
any one crate's manifest. Two independent libraries that each legitimately select a different backend
therefore cannot coexist in one binary, and neither author can resolve it alone. The `cratestack-core`
and `cratestack-pg` READMEs now state this explicitly rather than leaving it to be discovered by
hitting the `compile_error!`.

## 0.7.13 (2026-08-12)

### Fixed a validator attribute on a nullable field breaking `Update{Model}Input::validate()` (#537)

`@length`, `@range`, and every other validator attribute on a nullable field (`String?`, `Int?`, …)
made the generated `Update{Model}Input::validate()` fail to compile, because update inputs wrap
every field in an extra `Option<T>` ("field omitted") that is independent of the column's own
nullability ("set this column to NULL") — `cratestack-macros/src/validators/emit.rs` OR'd the two
conditions into a single boolean and only unwrapped one `Option` level instead of counting them
separately, leaving a `&Option<T>` where the validator helper expected `&T`. `Create{Model}Input`
was unaffected (a nullable field there is only ever a single `Option<T>`). The fix counts the two
levels independently and unwraps twice when both apply; an explicit "set to NULL" on update
(`Some(None)`) is treated as a no-op for validation purposes, matching how a nullable field is
already allowed to be null on create.
### Release-bump PRs now run CI, and `prepare-release` no longer strands already-written changelog prose (#531)

`v0.7.12` tagged and shipped with an unedited `CHANGELOG.md` seed — the placeholder text telling a
human to rewrite it into prose, including the sentence "Do not commit with this placeholder text,"
was itself committed, tagged, and published. Two independent defects combined to let that happen.
First, the "Prepare Release" workflow opened its bump PR (#528) using the default `GITHUB_TOKEN`;
GitHub's anti-recursion protection means an event raised by that token never triggers further
workflow runs, so the PR's head commit had zero check-runs — no changelog gate, no governance
check, no build or test, nothing. `prepare-release.yml` now opens that PR (and pushes its branch)
using the same `RELEASE_PAT` secret `cut-release-tag.yml` already relies on for its tag push,
falling back to `github.token` with a loud warning if the secret is unset, so the PR is raised as an
ordinary external event and the normal required checks run against it like any other PR. Second,
`changelog-seed.sh` inserted the new dated release heading *above* any existing `## Unreleased`
section instead of converting it — so prose written by the three PRs that had landed since
`v0.7.11` was stranded under a stale, buried `## Unreleased` heading while the release section
itself held only the placeholder seed. The script now converts an existing `## Unreleased` section
into the new dated heading in place, carrying its prose forward untouched, and falls back to the
seed only when there is genuinely nothing to carry (the section absent, or present but empty).
### `AuthProvider::authenticate` can now read the request's `http::Extensions` — breaking (#550)

`RequestContext` gained a new `pub extensions: &'a http::Extensions` field, populated on every
transport (REST, RPC unary/`/rpc/batch`/`/rpc/subscribe`, and gRPC) from the real inbound request, so
an `AuthProvider` implementation can observe whatever a preceding tower/axum layer inserted into
extensions before authentication ran — `ConnectInfo<SocketAddr>`, an mTLS peer identity, a tenant
already resolved upstream, a trace/session handle, and so on. Before this change the only way to pass
such data into `authenticate()` was to smuggle it back through a header, which is exactly the spoofable
channel the trusted-proxy work (#415/#416/#526) exists to constrain; extensions are populated
in-process by layers the deployer chose to install, so they are a legitimate trust source distinct from
headers/body, which remain wire-controlled and attacker-influenced. The plumbing reuses
`ClientIpContext` (already threaded through every generated dispatch fn for `ConnectInfo`/
`TrustedProxyConfig`) rather than adding a brand-new parameter, so all three transports pick up the new
field through the same seam gRPC's `into_router()` already used to read `ConnectInfo`/
`TrustedProxyConfig` off `http::Request::extensions()` directly.

**Breaking:** `RequestContext` is a public struct with public fields; any code that constructs one
directly (as opposed to only reading `&RequestContext<'_>` inside an `AuthProvider` impl, which needs
no changes) must add the new `extensions` field. The blanket `impl<F, E> AuthProvider for F where F:
Fn(&HeaderMap) -> Result<CoolContext, E>` is unaffected and needs no migration.

**Performance.** `ClientIpContext::from_extensions` now clones the request's full `http::Extensions`
map on every request, unconditionally, on every transport — measured (`cratestack-axum`'s
`tests_extensions_clone_cost.rs`) at roughly 30-150ns per request against a realistic served-router
extensions map, the same order of magnitude as (and never meaningfully above) the `HeaderMap` clone
every generated dispatch fn already pays unconditionally today; both are noise next to a real
request's network/DB round trip. This can't be avoided by borrowing instead of cloning: axum-core's
`FromRequestParts::from_request_parts` returns an owned value with no lifetime tied to its `&mut
Parts` argument, and by the time a generated dispatch fn runs, the original `Parts` no longer exists
as a distinct value to borrow from — see `ClientIpContext`'s doc comment for the full reasoning.
Consumers who insert a large non-`Arc`-backed value into `http::Extensions` now pay that clone's real
cost on every request, not just when it's read; wrap such values in `Arc` before inserting.
### `cratestack-studio` routes `[target.db]` writes through the generated server's own `@version`/`@@emit` primitives instead of refusing them (#507)

PR #516 made Studio refuse (`403 UNSAFE_DB_WRITE`) a `[target.db]` write to a model declaring
`@version` or `@@emit(...)` unless the target opted into `allow_unsafe_writes`, turning a silent
correctness bug into a choice. This closes the other half: wherever it's structurally possible,
Studio now applies those semantics for real instead of refusing. `@version` bumping is routed on
every backend (Postgres and SQLite alike bump the column server-side, exactly like the generated
server and `cratestack-rusqlite` already do), so a model that only declares `@version` is never
refused anymore. `@@emit(...)` is routed on Postgres targets by writing a `cratestack_event_outbox`
row through the same `cratestack_sqlx::enqueue_event_outbox`/`ensure_event_outbox_table` primitives
the generated server's descriptor path uses (now `pub`, and relaxed to accept `&str` model names
since Studio parses schemas at runtime rather than generating `'static` ones) — inside the same
transaction as the row mutation, so a crash between the two can't commit one without the other.
SQLite has no event-outbox equivalent at all, so a model declaring `@@emit(...)` on a SQLite
`[target.db]` target is still refused unless `allow_unsafe_writes` is set; that refusal, and the
audit-trail/log signal PR #516 added for it, are unchanged. `@@allow` enforcement on `[target.db]`
reads and writes remains deliberately out of scope, per the same maintainer framing PR #516 used.
### `run_in_tx` callers can now opt into `AuditSink` fan-out — breaking (#534)

#473/#517 made `AuditSink` a real installable seam for every `run()` write path, but explicitly
left `run_in_tx` (the caller-managed-transaction escape hatch) unable to fan out at all: dispatch
had no reliable "after commit" point inside the crate, `dispatch_audit_sink` was `pub(crate)`, and
no `run_in_tx` variant returned the `AuditEvent` a caller would have needed even if it had been
public. `crates/cratestack-pg/tests/banking_chained_audit_tx.rs` — two `run_in_tx` writes to
`@@audit` models chained in one caller-managed transaction — used to leave a real installed
`AuditSink` observing zero events for that transaction, silently, even though the in-database
`cratestack_audit` rows committed correctly.

Every `run_in_tx` variant (`create`, `update`, `delete`, `upsert` and `.do_nothing()`,
`update_many`, `delete_many` — seven call sites, plus their `Scoped*`/`.bind(ctx)` wrappers) now
returns a `RunInTxOutcome<T>` carrying the `AuditEvent`(s) it already built and persisted inside
`tx`, and `cratestack_sqlx::dispatch_audit_sink` is `pub` instead of `pub(crate)`. The generated
`Cratestack::dispatch_audit_sink(&self, events)` (and the `.bind(ctx)`-bound equivalent) is the
ergonomic surface: a caller who owns the transaction collects `audit_events` from each
`RunInTxOutcome` and calls it once, after their own `tx.commit()` succeeds — dispatch remains
caller-driven, not automatic, and still never runs from inside a transaction or for a rolled-back
one. The in-transaction `cratestack_audit` row write is unchanged (still the sole write, still the
source of truth), and `run()`'s existing single dispatch-after-its-own-commit is unaffected — a
sabotage-and-restore guard test (`banking_audit.rs::custom_audit_sink_receives_the_create_event`)
confirms `run()` still dispatches exactly once, not twice.

The identical `@@emit` event-outbox asymmetry needed no code change: `Cratestack::events().drain()`
(added by #390) already re-scans `cratestack_event_outbox` for undelivered rows rather than needing
a specific event handed back from `run_in_tx`, so it was already a working caller-driven opt-in —
just undocumented and untested for this shape until now. Both halves are covered by new tests in
`banking_chained_audit_tx.rs`, run against real Postgres.

**Breaking:** the seven `run_in_tx` methods (and their `Scoped*` wrappers) now return
`Result<RunInTxOutcome<T>, CoolError>` instead of `Result<T, CoolError>` — access the previous
return value via `.value` (e.g. `outcome.value.id`). Acceptable pre-1.0 under lockstep versioning;
see the PR body for the two documented alternatives (a runtime-owned commit hook, or leaving the
gap as permanently-documented behavior) the maintainer may still prefer instead.

### `IdempotencyLayer`/`RateLimitLayer` refuse requests they cannot fingerprint, instead of pooling them into a shared `"anonymous"` namespace — breaking (#416)

The default `IdempotencyLayer`/`RateLimitLayer` fingerprint hashes the `Authorization` header when
present and otherwise falls back to the verified TCP peer address via axum's
`ConnectInfo<SocketAddr>` (that fallback shipped in #459). But `ConnectInfo` is only populated when
the server is served through `.into_make_service_with_connect_info::<SocketAddr>()` — and nothing
in this repository does that by default; every shipped example uses plain `.into_make_service()`.
So in the real, documented, un-overridden default, every request without an `Authorization` header
silently collapsed onto a single shared `"anonymous"` idempotency namespace / rate-limit bucket:
two distinct unauthenticated callers reusing an `Idempotency-Key` could replay each other's
response, and any two such callers could exhaust each other's rate-limit budget. #526 fixed a
related hole (#415, `Forwarded`/`X-Forwarded-For` spoofing the fallback) but explicitly left this
one open per its own closing comment; #416 was nonetheless closed alongside it, which was a
mistake — the acceptance criteria ("default configuration cannot place distinct callers in a
shared namespace") were still unmet.

`default_principal_fingerprint`/`default_key_fn` now refuse the request (`412 Precondition
Failed`) instead of falling back to `"anonymous"` when neither an `Authorization` header nor a
`ConnectInfo<SocketAddr>` peer is available — there is no unforgeable value left to key on at that
point, so the fix does not manufacture one. `Forwarded`/`X-Forwarded-For` are still never
consulted (unchanged from #459/#526) — an attacker still cannot pick their own bucket. A `Once`
(process-lifetime, not per-request) `tracing::warn!` names the fix and the two ways to resolve it,
mirroring the identical pattern #526 introduced for the missing-`ConnectInfo` misconfiguration
warning.

**Breaking:** any deployment that (a) uses the default fingerprint/key function, (b) serves
without `into_make_service_with_connect_info`, and (c) receives requests without an `Authorization`
header now gets `412` on those requests instead of silent (and unsafe) success. `with_key_fn`/
`with_principal_fingerprint` overrides are unaffected — their closures remain infallible; opting
out of the default is the caller's explicit choice, including any deliberate shared bucket.

**Migration.** Either wire `.into_make_service_with_connect_info::<SocketAddr>()` (the socket peer
becomes the fallback identity, exactly as #459 intended), or supply
`IdempotencyLayer::with_principal_fingerprint(...)`/`RateLimitLayer::with_key_fn(...)` explicitly
for deployments that authenticate via cookies/mTLS rather than `Authorization` and cannot serve
through `into_make_service_with_connect_info`.

### `@status(<code>)` generated-client verification: Dart confirmed (#407)

Closes the last open acceptance criterion on cratestack#407. `@status(<code>)` itself shipped in
#511 (see the "Per-procedure `@status(<code>)`" entry below, now under the 0.7.11 section); its
AC5 required at least one generated client to be *proven*, not just inspected, to treat a
declared non-200 2xx as success. The Rust client was already proven end-to-end against a mock
returning a bare `202`. The Dart client had only been inspected — no explicit `validateStatus`
override was found in the REST-runtime templates, from which it was *inferred* (never run) that
Dio's own default `validateStatus` (`200 <= status < 300`) applies unmodified.

That inference now has real evidence: a new `just verify-dart` step generates a client from a
`@status(202)` fixture for both the `default` and `riverpod` presets (both talk HTTP directly via
`dio`, no Rust FFI bridge) and runs it against a real `dart:io HttpServer` answering with a bare
`202` — no `200` anywhere in the exchange — asserting the decoded reply. A `5xx` negative control
in the same test proves the client still surfaces real errors, so the positive assertion isn't
just "accepts everything." Both presets pass. RPC transport is out of scope, since `@status` is
already rejected at schema-compile time under `transport rpc`.

### `ProcedureRegistry` methods now require a witness only policy enforcement can produce — breaking (#512)

The most natural-looking way to call a procedure from non-HTTP code — `registry.my_procedure(&db,
&ctx, args).await` — silently skipped every `@allow`/`@deny` check. Policy enforcement lived only
in the generated `invoke_with_db`/`invoke` wrappers the axum/RPC/gRPC dispatch handlers call;
the `ProcedureRegistry` trait method a schema's implementor actually writes had no policy code at
all, and no example anywhere showed the (undocumented, closure-wrapping) shape that did. This was
the last remaining silent-bypass shape from epic #488 — severity isn't "policy can be bypassed"
(anyone with the database can do that already); it's that the bypass was the *default* outcome of
writing the most readable code, with nothing in the type system, no lint, and no example pointing
the other way.

Fixed at the type level, not by convention: every generated `ProcedureRegistry` method now takes a
trailing `Authorized` witness — a zero-sized marker type, private tuple field, defined in the same
generated module as `authorize_with_db`/`invoke_with_db`. Nothing outside that module — not this
same generated crate's `axum`/`rpc`/`grpc` dispatch code, not a `ProcedureRegistry` implementor's
own code, not any other procedure's module — can construct one; the only way to obtain one is to
call `authorize_with_db` (which runs `@allow`/`@deny` and any `@authorize` model checks) and get
back `Ok`. `registry.my_procedure(&db, &ctx, args)` is therefore now an ordinary Rust arity
mismatch, not a policy check that happens to run and pass — confirmed with a `trybuild`
compile-fail fixture (`crates/cratestack-macros/tests/ui_procedure_registry_witness.rs`) that
fails if the witness parameter is ever removed from codegen.

The HTTP/RPC/gRPC dispatch paths are unchanged: they already called `invoke_with_db`, which is now
also the sanctioned way to invoke a procedure from non-HTTP code (a cron job, background worker, or
admin tool) — see `examples/rpc-procedures/src/internal_worker.rs` for a worked example, including
using `auth().isSystem()` (#486)'s `SystemContext` as the caller identity.

**Migration.** Every existing `ProcedureRegistry` implementation gains a new, unused-by-default
last parameter on every method: add `_authorized: <procedure>::Authorized` (any name; the value has
no API surface — it exists only to be received, not read) to each method signature. Mechanical, no
behavior to reason about. If you were calling a `ProcedureRegistry` method directly rather than
through the generated router (the exact bypass this fixes), replace that call with
`<procedure>::invoke_with_db(&db, &args, &ctx, |authorized| async move { registry.<method>(&db,
&ctx, args, authorized).await }).await` — the same call the generated dispatch handlers make.
### `DELETE` on an `@version` model now enforces `If-Match`, matching `PATCH` — breaking (#519)

`DELETE` on a model declaring `@version` silently ignored optimistic concurrency: `PATCH`
required `If-Match` and returned `412` on a stale or missing value, but `DELETE` proceeded
regardless of whether the caller sent one, sent a stale one, or sent none at all. #493 assumed the
two verbs already behaved the same way, and PR #510 nearly shipped a client SDK documenting an
`If-Match`-on-`DELETE` round trip the server didn't actually support — the gap was caught in
review and filed as #519 for a maintainer decision instead. The decision: close the asymmetry
rather than document it. `DELETE` on an `@version` model now requires `If-Match` and returns `412`
on a stale or missing value, exactly like `PATCH`. The gate gates purely on the model declaring
`@version`, independent of whether it also declares `@@soft_delete` — a soft-deleted row is still
a real mutation (an `UPDATE ... SET deleted_at = NOW()` under the hood) and gets the same
protection a hard delete does, with no separate soft-delete carve-out.

Server-side: `cratestack-macros/src/axum/model/prep/etag.rs` emits a `delete_if_match_decl`/
`delete_if_match_apply` pair mirroring the existing `update_if_match_*` tokens, wired into
`build_delete_handler` in `handlers_crud.rs`. `cratestack-sqlx`'s `DeleteRecord`/
`ScopedDeleteRecord` gain an `if_match(expected: i64)` builder step mirroring
`UpdateRecordSet::if_match`; the version-mismatch-vs-policy-denial disambiguation this needs is
now shared between the update and delete paths via `query::support::probe_current_version`
(previously private to the update exec path only).

Client-side: `CratestackClient::delete_with_response` (`cratestack-client-rust`) and the generated
`<Model>Client::delete_with_response`/`delete` (`cratestack-macros`'s REST client codegen) had
their rustdoc corrected — both previously stated the server does *not* enforce `If-Match` on
`DELETE`; that claim is no longer true.

**Breaking:** any deployed client issuing a bare `DELETE` against a versioned model's REST route,
or calling the typed delegate's `.delete(id).run(...)` without `.if_match(expected)`, now gets
`412 Precondition Failed` instead of a successful delete.

**Migration.** Before upgrading, audit every caller that deletes an `@version` model:

1. **Generated REST/RPC clients:** read the current `ETag` first (`get_with_response`, or the
   `ETag` returned by a prior mutation) and pass it as `If-Match` on the delete call —
   `delete_with_response(id, &[("if-match", etag)])` instead of `delete_with_response(id, &[])`.
2. **Direct `cratestack-sqlx` delegate callers:** add `.if_match(expected_version)` to
   `cool.<model>().delete(id)` calls, the same way versioned `.update(id).set(input)` calls already
   need `.if_match(...)`.
3. **Hand-rolled HTTP clients:** send an `If-Match: "<version>"` header on `DELETE` requests
   against `@version` models, same as is already required on `PATCH`.

A model with no `@version` field is unaffected — the gate is a no-op when `version_column` is
`None`, exactly as it already is for `PATCH`.

### New crate: `cratestack-auth` — signed-request + identity-token auth (absorption 3 of 3)

SigV4-style canonical-request construction/signing/verification over Ed25519, SD-JWT id-token
issuance and verification, multi-issuer JWKS resolution, per-service signing identities, and
COSE-signed enrolment challenges — absorbed from a downstream project's `auth-kit`, by far the
largest of the three absorptions (~4,100 lines against `cratestack-service`'s and
`cratestack-outbox`'s few hundred each).

**Security verification (the reason this absorption existed at all).** The source crate's
`challenge_signing_key()` previously returned a hardcoded Ed25519 seed literal, permanently
compromised by having been committed to that repository's git history. That was already fixed
upstream before this absorption began: the function loads the seed from an env var
(`CHALLENGE_SIGNING_KEY_ENV`, renamed here to `CRATESTACK_AUTH_CHALLENGE_SIGNING_KEY`) and fails
closed — absent, empty, or whitespace-only is a hard `AuthError::MissingSigningKeyEnv`, never a
default or a silently-generated ephemeral key. This absorption re-verified that fix by grepping
the entire absorbed surface for byte-array literals that could be key material: every hit is
either a small-integer synthetic test fixture (`[7u8; 32]`, `[9u8; 32]`, `[11u8; 32]`, ...,
clearly distinct from the burned seed and never used as a production default) or
`ServiceSigningKey::ephemeral`'s `OsRng.fill_bytes` (test/local-dev only, explicitly documented as
such). No hardcoded key material was found anywhere in the absorbed code. The fail-closed
contract is preserved and given two new dedicated unit tests
(`challenge_signing_key_fails_closed_when_env_var_is_absent` /
`..._is_whitespace_only`) that exercise it directly against an injected lookup function, rather
than through a real env var.

**Why an injected lookup function, not `std::env::set_var` in tests:** this workspace `forbid`s
`unsafe_code`, and `std::env::set_var`/`remove_var` require an `unsafe` block as of the 2024
edition — the source crate's two `challenge_signing_key` tests wrapped exactly that in `unsafe`
blocks guarded by a process-wide `Mutex`. Ported here as `challenge_signing_key_from(lookup_env:
impl Fn(&str) -> Result<String, VarError>)`, the same injectable env-lookup seam
`cratestack-service`'s `ServiceConfig::from_env_with` established (#529) — no process environment
is touched, and the COSE build/parse round-trip test now exercises the COSE logic directly against
a freshly-generated test-only key (`build_cose_enroll_response_with_key`/
`parse_cose_enroll_response_with_key`) rather than through env-var loading, which is a strictly
better test of both concerns in isolation.

**Placement:** `cratestack-auth = 1` in `docs/adr/layers.toml` (ADR 0014) — verified via `cargo
tree -p cratestack-auth -e normal` (with and without `--no-default-features`): both report exactly
one `cratestack-*` line, `cratestack-core`. The types the source crate reached through a
`cratestack::{AuthProvider, CoolContext, RequestContext, CoolError}` facade re-export
(`cratestack = { package = "cratestack-pg" }` downstream) all turn out to live in
`cratestack-core` itself (`context.rs`), not in `cratestack-axum` — and the `cratestack::axum::
http::{HeaderMap, Method, Uri}` types it also reached through that facade are themselves just the
plain `http` crate, which `cratestack-axum` only re-exports (`pub use axum;` → `pub use http`
transitively). So the real graph never needed `cratestack-axum` at all; this crate depends on the
external `http` crate directly for those types, the same way `cratestack-core` itself already
does. Same "lowest layer the real graph supports" rule `cratestack-macros`/`cratestack-proto` use
in `layers.toml` for the identical reason (depends only on L0).

**Feature gating:** a default-on `axum` Cargo feature gates the three items that genuinely need
the `axum` crate (not just `http`): `require_signed_request` (the tower auth middleware),
`jwks_router` (the mountable `axum::Router` serving `/jwks.json`), and the `FromRequestParts`
extractor impls on `CurrentPrincipal`/`AuthenticatedPrincipal`. Everything else — signing and
verifying a canonical request, `SignedRequestAuthProvider`'s `cratestack_core::AuthProvider` impl,
SD-JWT issuance/verification, `ServiceSigningKey`, `MultiIssuerJwksVerifier` (a `reqwest` GET, not
an axum route), and COSE challenge build/parse — needs no axum route or middleware machinery and
stays available with the feature off, so a `cratestack-client`-only consumer that just wants to
*sign* outgoing requests can skip axum (and its own tower/hyper/matchit tree) entirely. Verified
both ways: `cargo check`/`cargo clippy`/`cargo test -p cratestack-auth` all pass with default
features (37 tests) and with `--no-default-features` (34 tests — the three `jwks_router`-dependent
tests are themselves feature-gated).

**Not split into two crates.** The predecessor absorptions raised "does the whole thing belong,
or does it split" as an open question per crate; here the axum-touching surface shrank to three
items once the `http`-vs-`axum` distinction above was drawn, which is small enough that gating
inside one crate (the `cratestack-service` `postgres`-feature shape) reads more like the rest of
this workspace's facade-disjointness pattern than standing up a second crate would.

**`ServiceConfig` overlap:** `cratestack-service` (#529) deliberately dropped `issuer_url`/
`jwks_url` from its own config, flagging them as this crate's concern. This absorption keeps that
split rather than extending `ServiceConfig`: `MultiIssuerJwksVerifier` is inherently
multi-issuer (a `{issuer → jwks_url}` map), which a single `issuer_url`/`jwks_url` field pair on
`ServiceConfig` could not represent, and `ServiceSigningKey::from_env`/`IdTokenVerifier::new`/
`SignedRequestVerifier::from_env` already have their own env-var-driven construction — adding a
third, `ServiceConfig`-shaped path would be a second, weaker way to build the same thing.

**Parameterised away:** every `VAAM_*` env var (`VAAM_AUTH_CHALLENGE_SIGNING_KEY` →
`CRATESTACK_AUTH_CHALLENGE_SIGNING_KEY`, and the four `VAAM_SIGNATURE_*`/`VAAM_ID_TOKEN_AUDIENCE`
equivalents → `CRATESTACK_AUTH_*`), the `urn:vaam:...` id-token grant type → `urn:cratestack:...`,
the `vaam:signature-nonce` Redis key prefix → `cratestack:signature-nonce`, the
`vaam-business-services` default audience → `cratestack-issued-tokens`, and every
`vaam-mobile`/`*.vaam.local`/`@vaam.store` literal in doc comments and test fixtures. Also dropped:
the downstream `error-kit` dependency (a CBOR/JSON/COSE content-negotiation envelope this crate's
`require_signed_request` middleware and principal extractors used only for a plain
`{code, message}` error body) in favor of a small in-crate JSON helper
(`response::error_response`) built on `cratestack_core::CoolErrorResponse`, the framework's own
REST error shape.

**Also fixed in passing:** `IdTokenVerifier::new`/`MultiIssuerJwksVerifier::new` both build a
`reqwest::Client` internally; under this workspace's `rustls-no-provider` reqwest feature that
panics at construction time unless a crypto provider is already installed process-wide. Both now
call the same `ensure_crypto_provider()` courtesy-fallback `cratestack-client-rust` already
established for the identical reason — a real gap the source crate didn't have to worry about
because its own application entrypoint happened to install a provider first.

**Test coverage added:** `middleware::require_signed_request` had no direct test in the source
crate; it now has three (missing `Authorization` header → 401, a validly signed request installs
the principal and reaches the handler, a tampered signature → 401), alongside the pre-existing
coverage this absorption carried over — canonical-request signing/verification (valid and
tampered/reused-nonce), SD-JWT selective-disclosure round-trips, JWKS-based JWT verification
(valid and tampered payload), and the cnf-bound proof-of-possession fallback (including the
revoked-device-resolver-is-authoritative regression).

No existing crate changed; this is purely additive.

### `migrate diff` now refuses a primary-key change on an existing table instead of silently emitting nothing, and two field-level `@id` on one model is rejected at parse time (#536)

Changing an existing table's `@@id([...])` — adding, removing, or reordering primary-key columns —
previously produced a completely empty diff: no operations, no error, no warning, letting the
schema and the database silently drift apart. `migrate diff`/`migrate baseline` now return a
structured `MigrateError::PrimaryKeyChanged` naming the table and the before/after key instead,
deliberately refusing rather than generating a half-designed migration: a correct primary-key
change needs constraint drop/recreate ordering, dependent foreign keys, and a data-safety story
for a populated table that the diff engine does not have. Separately, `@@id([a, b])` has always
been hard-rejected at macro expansion citing #136 (codegen still assumes a single scalar `@id`),
but the equivalent `a String @id` / `b String @id` spelling — two field-level `@id` attributes on
one model — validated cleanly and let `cratestack-migrate` silently emit a real multi-column
`PRIMARY KEY`, bypassing that same guard. `cratestack-parser` now rejects a second field-level
`@id` with the same #136 reasoning, so both spellings are refused consistently. Both changes are
purely additive refusals on constructs that either never reached codegen (`@@id([...])`, #136) or
validated cleanly by accident (two field-level `@id`); no existing valid schema is affected.

**Breaking (internal API):** `cratestack_migrate::diff`/`diff_projections` now return
`Result<Vec<Op>, MigrateError>` instead of an infallible `Vec<Op>`, so the refusal has somewhere
to go. Every in-tree caller (`cratestack-cli`'s `migrate diff`/`migrate baseline`, and every test
across `cratestack-migrate`/`cratestack-pg`) is updated; any out-of-tree direct caller of these two
functions needs the same `?`/`.expect(...)` treatment.

### Fixed a regression: renaming a primary-key column now migrates again instead of being refused as a primary-key change (#536)

A post-merge review of #536/#551 found that its new `PrimaryKeyChanged` refusal (above) compared
raw column *names* without resolving column-level `@rename(from = "...")` first, so renaming an
`@id`/`@@id([...])` column — a previously-working, fully-supported operation that lowers to a plain
`RENAME COLUMN` — was misdiagnosed as a primary-key change and refused outright. `RENAME COLUMN`
preserves whatever constraint already references the column on both backends, so the key's actual
structure (arity, order, logical identity) was never changing; only its name was. `diff::primary_key`
now resolves each previous-side key column through the same rename map `diff::columns` already
builds from `@rename(from = "...")` before comparing, extracted into a shared
`diff::columns::column_rename_map` helper so both call sites stay on one implementation. A genuine
reorder or a change to the key's column set is still refused exactly as before — resolving a rename
only changes a renamed column's *name* in the comparison, never its position.
### `cratestack-studio` no longer emits invalid SQL previewing a `Create`/`Update` on an `@version` model without a payload (#507)

A post-merge review of #553 found that `preview_sql`'s no-payload fallback (`sample_column_names`)
listed every scalar column, including `@version`, and then both the `Create` and `Update` branches
applied the version column a second time on top of that — seeding it to `0` again for `Create`, and
appending the `"version" = "version" + 1` bump again for `Update` — because that logic assumed
`@version` was never already present in the sample set (the way `collect_payload` already excludes it
from a real payload). The result was `INSERT INTO "t" ("id", "version", ..., "version") ...`, which
Postgres rejects with "column ... specified more than once" and SQLite rejects as a duplicate column
name, and two conflicting `SET` assignments to the same column on `Update`. Since Studio's preview
endpoint always calls `preview_sql` with `payload = None`, this broke SQL preview for every request
against every `@version` model on both the Postgres and SQLite backends. `sample_column_names` now
excludes `version_column` from the synthesized sample set, mirroring what the payload collectors
already did, so the version column is applied exactly once regardless of which branch produced it.

### CI now actually runs `cratestack-studio`'s four live-Postgres test files instead of silently skipping them (#507)

`crates/cratestack-studio/tests/{postgres_explain,postgres_routed_writes,postgres_row_keys,postgres_unsafe_writes}.rs`
are the decisive Postgres-backed coverage for Studio's data layer, but a coverage audit found the
`tests-studio` CI job set neither `CRATESTACK_TEST_DATABASE_URL` nor `CRATESTACK_USE_TESTCONTAINERS`,
so all four files skipped silently and reported `ok` on every run without ever touching a real
database — this is exactly how the duplicate-column bug PR #553 fixed shipped in the first place,
since its own regression test never actually executed in CI. Worse, three of the four files
(`postgres_explain`, `postgres_row_keys`, `postgres_unsafe_writes`) had never adopted the
testcontainers/`CRATESTACK_REQUIRE_DB` machinery `postgres_routed_writes` introduced for #507
itself, so turning those CI knobs on alone would have left them skipping forever; a new shared
`tests/support/pg.rs` (mirroring `cratestack-pg`'s and `cratestack-outbox`'s own test-support
modules) now backs all four files, and a dedicated `test-ci-studio-db` recipe runs them under
`CRATESTACK_USE_TESTCONTAINERS=1` as a second step in the `tests-studio` job (not a separate job,
so the Trunk UI build isn't paid for twice), with `CRATESTACK_REQUIRE_DB=1` turning a broken
Docker or a misconfigured run into a hard failure instead of a quiet skip.

## 0.7.12 (2026-08-11)


### New crate: `cratestack-outbox` — transactional outbox (absorption 2 of 3, phase 1b)

The transactional outbox pattern, absorbed from a downstream project's `events-kit`:
`OutboxClient::persist_in_tx` writes an event as an ordinary row inside the *caller's own*
Postgres transaction, so the event exists if and only if the business write that produced it
committed — closing the classic dual-write gap between "save the row" and "publish the event." A
separate snapshotter drains them via `OutboxClient::drain`, paging through events in `id`
(UUIDv7, insertion-order) ascending order via an opaque cursor; `axum_handler::{drain_handler,
gc_handler}` expose that drain, plus a retention sweep (`OutboxClient::gc_older_than`), as two
axum handlers with JSON/CBOR content negotiation.

Unlike the `cratestack-service` pilot, the source crate could not be ported as-is: it declared a
full `.cstack` schema and generated a typed `cratestack::Cratestack` handle via
`include_server_schema!(db = Postgres)`, purely to reach `.pool()` on it — every actual read and
write already ran raw `sqlx` against the table directly, because cratestack's typed
`Json<cratestack::Value>` column serialises a value in its externally-tagged wire form, which
does not match the plain-JSONB shape a downstream lake snapshotter expects. The generated
model's typed accessors and its `@@allow` policy check were never called from anywhere in the
source crate. Reaching for the macro would also have forced this crate to depend on the
`cratestack-pg` L5 facade — `include_server_schema!` is only reachable through it — for logic
that otherwise belongs at L2, and `docs/design/layering.md` §2's L5 rule ("a facade that grows a
function has stopped being a facade") already rules out folding this logic into `cratestack-pg`
directly. So the macro was dropped rather than ported: `cratestack-outbox` hand-writes its one
table directly against `cratestack-sqlx`, the same posture `cratestack-sqlx` itself already takes
for its own internal `cratestack_audit`/`cratestack_migrations` tables — a bare DDL constant
(`OUTBOX_EVENTS_DDL`) a caller copies into their own migration, plus hand-written queries against
it, going through `cratestack-sqlx`'s `run_in_isolated_tx_with_retries` and
`cool_error_from_sqlx` rather than the source crate's manual error-string mapping. The table is
renamed `cratestack_outbox_events` (from the source crate's bare `outbox_events`) to match that
same `cratestack_*` internal-table convention.

Layer assignment: `cratestack-outbox = 2` in `docs/adr/layers.toml` (ADR 0014) — its only normal
`cratestack-*` dependencies are `cratestack-core` (L1, `CoolError`/`TransactionIsolation`) and
`cratestack-sqlx` (L2), both `<= 2`, and nothing in the workspace depends on it. Unlike
`cratestack-service`, the `cratestack-sqlx` dependency here is unconditional rather than behind a
Cargo feature: this crate's only reason to exist is a Postgres transaction
(`OutboxClient::persist_in_tx` takes a `sqlx::Transaction<'_, sqlx::Postgres>` by construction),
so a feature gate would either always be on or produce an empty, non-functional crate when off —
the same posture `cratestack-sqlx`/`cratestack-redis` themselves already take for their own
backend dependency.

Also dropped while absorbing: the sibling `error-kit` dependency (a response-envelope crate with
a `meta`/COSE-body shape this crate's two handlers never used) in favor of a small in-crate
JSON/CBOR content-negotiation module; and the downstream `test-kit`/`VAAM_E2E_DATABASE_ADMIN_URL`
test harness, replaced with this workspace's own `CRATESTACK_TEST_DATABASE_URL` /
`CRATESTACK_USE_TESTCONTAINERS` convention (`tests/support/pg.rs`, copied from
`cratestack-pg`'s). No existing crate changed; this is purely additive.

### New crate: `cratestack-service` — service-bootstrap batteries (pilot absorption, phase 1b)

Every CrateStack-backed service ends up hand-writing the same handful of things that have
nothing to do with its schema: read the port from the environment, expose `/healthz` and
`/healthz/ready` for `kubectl`, install a `tracing_subscriber` before the first log line, and
serve the router. `cratestack-service` is a new, additive facade-adjacent crate that provides
all four, absorbed and generalized from two small internal helper crates (`telemetry-kit`,
`db-kit`) a downstream project had built for exactly this purpose because they are fused to
CrateStack's own trait system and can never be published standalone — they exist to fill a gap
the framework should arguably cover. This is the pilot for a larger absorption effort; two more
downstream crates (`events-kit`, `auth-kit`) are expected to follow the same shape.

`telemetry::init` wraps `tracing_subscriber` (env-driven filter, optional JSON output,
idempotent). `ServiceConfig::from_env` plus the `health` module provide an env-driven config
struct and a `/healthz`/`/healthz/ready` router whose readiness checks are opt-in — Redis and
object storage are only probed when their URL is actually configured, and a degraded dependency
now correctly returns HTTP 503 rather than 200 with a body field a kubelet probe never reads.
`run` installs request tracing and serves. Every environment variable this crate reads is
prefixed with a caller-supplied string (`ServiceConfig::from_env("AUTH", ...)` reads
`AUTH_SERVICE_HOST`, `AUTH_DATABASE_URL`, ...) — unlike the downstream code it was absorbed
from, this crate ships no fixed prefix, no default connection string, and no built-in
service-name-to-database-name table; those are application specifics, not framework surface.

The Postgres-specific pieces (`ServiceConfig::database_url`/`state`, the readiness Postgres
check, and the new `migrations` module — `migrations_from_dir`, loading an
`include_dir!`-embedded migration tree, plus a `run_migrations` convenience wrapper over
`cratestack-sqlx`'s existing `Migration`/`apply_pending`) sit behind a `postgres` Cargo feature,
on by default and named after `cratestack-pg`'s own feature of the same shape. Disabled
(`default-features = false`), this crate has no `cratestack-sqlx` — and therefore no `sqlx` — in
its dependency graph at all (verified via `cargo tree`), so a `cratestack-api` or
`cratestack-sqlite` consumer can use the config/health/telemetry/run surface without inheriting a
database binding their chosen facade deliberately excludes.

Layer assignment: `cratestack-service = 2` in `docs/adr/layers.toml` (ADR 0014) — its only real
`cratestack-*` dependencies are `cratestack-core` (L1) and, feature-gated, `cratestack-sqlx`
(L2), and nothing in the workspace depends on it (it's a leaf, consumed directly by application
binaries, the same posture every facade already has). No existing crate changed; this is purely
additive.

### `Forwarded`/`X-Forwarded-For` are now honored only from a configured trusted proxy — breaking (#415)

`enrich_context_from_headers` — public, re-exported as `cratestack::enrich_context_from_headers`
— trusted `Forwarded`/`X-Forwarded-For` unconditionally when recording the audit `client_ip`,
so a direct client could forge the value that lands in the audit trail. It now takes the
trusted-proxy configuration and the verified socket peer explicitly, and only honors headers
from a peer the consumer has allowlisted.

**Breaking:** `enrich_context_from_headers(ctx, headers)` is now
`enrich_context_from_headers(ctx, headers, trusted_proxy: Option<&TrustedProxyConfig>, peer:
Option<SocketAddr>)`. `parse_client_ip(headers)` is now `parse_client_ip(headers, max_hops:
usize, header: ForwardedHeader)`. `router()`'s own signature is unchanged (Option A′ — see
`docs/design/trusted-proxy-client-ip.md`).

**Migration.** Deployments behind a reverse proxy that rely on `Forwarded`/`X-Forwarded-For`
being recorded as audit `client_ip` must, after upgrading:

1. Serve via `.into_make_service_with_connect_info::<SocketAddr>()`.
2. Apply `.layer(axum::Extension(TrustedProxyConfig::trusting([<proxy IPs/CIDRs>]).max_hops(N)))`
   on **every** router the app serves — including the gRPC router (`into_router()`) for
   `transport grpc` schemas, which is a separate `axum::Router` instance not covered by
   protecting `router()` alone.
3. If the deployment's proxy emits RFC 7239 `Forwarded` rather than the legacy
   `X-Forwarded-For` (uncommon — most proxies emit XFF), additionally call
   `.forwarded_header(ForwardedHeader::Forwarded)` on the config. **Only one of the two headers
   is ever honored**, defaulting to `X-Forwarded-For`; see below.

Without both (1) and (2), `client_ip` is `None` (or the proxy's own address) rather than the
client's — the safe default, never a guess and never a trusted-by-default header. When that
combination is detected — a `TrustedProxyConfig` applied with no `ConnectInfo` peer ever
arriving — a `tracing::warn!` (logged once per process, not per request) now flags it, so the
misconfiguration is discoverable in logs rather than silently degrading `client_ip` to `None`
forever.

`max_hops` counts **right-to-left** — in from the end of the chain nearest the trusted proxy,
not the `max_hops`-th entry from the left. A left-to-right reading re-opens the exact spoofing
gap this change closes for any chain longer than one hop.

**Only `X-Forwarded-For` is honored by default; `Forwarded` requires an explicit opt-in.** RFC
7239 `Forwarded` and the legacy `X-Forwarded-For` are alternatives, not complements — a real
proxy (nginx, an AWS ALB, HAProxy's defaults) emits one or the other, never both meaningfully.
An earlier draft of this change inspected `Forwarded` first, unconditionally, whenever it was
present — since almost no real proxy ever sets that header, an attacker could add an entirely
unvalidated `Forwarded` header and have it silently override a legitimate, hop-counted
`X-Forwarded-For` chain, defeating the trust check outright. `TrustedProxyConfig` now takes a
`forwarded_header: ForwardedHeader` (`XForwardedFor` by default; `.forwarded_header(Forwarded)`
opts a deployment into the RFC 7239 header instead) and only that one header is ever inspected.

**The selected hop is validated as a real IP address before being recorded.** Neither
`parse_client_ip` nor `enrich_context_from_headers` previously checked the resolved value
against `IpAddr::from_str` — a malformed or spoofed string (not even a valid address, e.g.
`666.666.666.666`), or an unstripped port suffix (`10.0.0.5:5678`), could land in the audit
trail verbatim. The selected hop is now parsed as an `IpAddr` (handling a port-suffixed IPv4
address, bracketed IPv6 with or without a port, and RFC 7239's quoted-string `for="..."` syntax)
before being recorded; on failure this falls back to the verified socket peer if trusted, or
`None` otherwise — never an unparseable string.

**Duplicate header occurrences are merged, not first-wins.** RFC 7230 §3.2.2 makes repeated
list-type header lines semantically equivalent to one comma-joined value. `HeaderMap::get` only
returns the first occurrence; a proxy that appends its hop as a *second* `X-Forwarded-For`
header line (rather than extending the first) had that value silently dropped in favor of
whichever line an attacker sent first. Every occurrence of the selected header is now
concatenated, in wire order, before the chain is walked.

This PR enables the idempotency/rate-limit `ConnectInfo<SocketAddr>` fallback (already shipped
for #416) to actually engage in practice, for deployments that apply the migration above — it
does **not** close #416 itself. #416's acceptance criteria require the *default* configuration
(no operator action) to never place distinct callers in a shared namespace, and the default
still collapses unauthenticated callers onto `"anonymous"` unless an operator wires
`into_make_service_with_connect_info`. #416 stays open.

### Generated servers now enforce request/batch/response size bounds (#413) — breaking

The generated Axum surface had three independent missing limits on the same request path: `/rpc/batch`
decoded and dispatched an unbounded number of frames, no layer capped the size of an inbound request
body, and four `axum::body::to_bytes(response.into_body(), usize::MAX)` call sites in the RPC binding
buffered responses without limit. Individually survivable; together, a single oversized batch body could
multiply the per-frame `authenticate()` + policy + dispatch cost (identical to a unary call) by however
many minimal frames fit in an unbounded body, with each frame's response then buffered without limit too.

- **`/rpc/batch` frame cap.** Rejected before the per-frame dispatch loop runs (zero frames dispatch on an
  over-limit batch — not truncated to the first `BATCH_MAX_ITEMS`), matching the same `1000`-frame ceiling
  and `CoolError::Validation` error shape `cratestack-sqlx`'s and `cratestack-rusqlite`'s own batch-size
  guards already use.
- **Request body limit — makes an existing implicit limit explicit, not a new one.** `router()` and
  `rpc_router()` (both generated per schema) now take an explicit `body_limit_bytes: usize` parameter,
  applied once via `axum::extract::DefaultBodyLimit::max(..)`. The new
  `cratestack_core::DEFAULT_BODY_LIMIT_BYTES` constant is **2 MiB — deliberately matching axum's own
  built-in `Bytes` default**, which every generated handler already extracted against (`axum-core`'s
  `Bytes: FromRequest` refuses bodies over 2 MiB with no layer required at all). This makes the change
  **provably a no-op at runtime**: it names, documents, and makes overridable a limit that was already
  there and already enforced, rather than silently tightening what an existing deployment can send. See
  `docs/design/request-response-size-bounds.md` Decision 2 for the full reasoning (an earlier
  implementation pass set this to 1 MiB before that doc was consulted; corrected to 2 MiB once it was).
  **This is a genuine parameter, not a default a consumer re-layers on top of afterward** — re-layering
  `DefaultBodyLimit` on a `Router` that already has one baked in does not work in either direction
  (verified empirically; see the design doc and `crates/cratestack-core/src/limits.rs`'s module doc) — a
  deployment needing a larger limit passes a larger `body_limit_bytes` to `router(...)`/`rpc_router(...)`
  instead. `model_router()`/`procedure_router()` (the lower-level constructors `router()` merges) are
  unchanged and remain unbounded when called directly, matching their existing signatures.
- **Response-rebuffer bound.** The four `to_bytes(.., usize::MAX)` sites in
  `crates/cratestack-axum/src/rpc/{batch,error_encode,grpc_bridge,codec_helpers}.rs` now use
  `cratestack_core::MAX_RESPONSE_REBUFFER_BYTES` (8 MiB — 4× the request default, with headroom for a
  response that legitimately echoes a request payload plus server-added columns). Every site already
  matched on `to_bytes`'s `Result`, so exceeding the bound degrades to the existing synthesized
  `CoolError::Internal`/error-frame path, not a panic.

**Migration:** the *function signature* of `router(db, registry, codec, auth_provider)` and
`rpc_router(db, registry, codec, auth_provider)` is breaking — every call site needs a fifth argument to
compile against the new version, and `cratestack::DEFAULT_BODY_LIMIT_BYTES` is the value that reproduces
today's runtime behavior exactly (no request that succeeds today starts failing at the default). A
deployment that has already worked around axum's implicit 2 MiB `Bytes` cap for a specific large-payload
endpoint should pass a larger `body_limit_bytes` explicitly, now that the limit is named and real rather
than an axum implementation detail.

**Flagged, not changed:** `DEFAULT_BODY_LIMIT_BYTES` and `MAX_BODY_BYTES` (the idempotency middleware's
own request-buffering cap) are now numerically equal — both 2 MiB. Traced through the actual mechanism,
`axum::body::to_bytes(body, limit)` is literally `Limited::new(body, limit).collect()`, the same
`http_body_util::Limited` primitive `DefaultBodyLimit`'s `Bytes` extractor itself builds on — so there is
no body size where the two disagree on accept/reject; they're the identical check running twice for a
request that also carries an `Idempotency-Key`. The only observable difference for that narrow request
class is which layer rejects first and therefore which error shape a client sees (a custom 400 from
`IdempotencyService` vs. axum's raw 413) — not a coverage gap. Full trace in
`docs/design/request-response-size-bounds.md`'s coherence note. `MAX_BODY_BYTES` itself is unchanged.

### `cratestack-macros`: `resolver = "1"` removed — no more workspace-wide cargo warning (#497, #525)

`crates/cratestack-macros/Cargo.toml` carried a `resolver = "1"` that made **every** `cargo`
invocation anywhere in the workspace emit `warning: resolver for the non root package will be
ignored`, for every contributor and every CI job. Cargo genuinely ignores the field for the real
workspace build; its only effect was on the synthetic mini-workspace `trybuild` generates per
fixture, where resolver 1's unified feature pools papered over `cratestack-core` selecting no
decimal backend and tripping the "enable exactly one" `compile_error!`.

Removing the `NEITHER`-selected arm (#505, this release) removed the reason the field was
load-bearing, so it is simply deleted — one manifest, 56 deletions, no replacement mechanism
needed. `cargo test -p cratestack-macros --test ui` and `--test ui_semantic` still pass with
their full fixture counts, and `.ci/feature-matrix.sh` still fails when a `cratestack-core` edge
is deliberately re-pinned, so the guard that made this risky is verified intact rather than
assumed.

## 0.7.11 (2026-08-11)

### `cratestack-core`: selecting no decimal backend is no longer a hard compile error (#505, #521)

`cratestack-core` used to hard-fail with `compile_error!("enable exactly one decimal backend
feature — decimal-rust-decimal or decimal-bigdecimal")` whenever a consumer built it (directly
or transitively) with `default-features = false` and neither `decimal-rust-decimal` nor
`decimal-bigdecimal` selected. That bit a consumer that legitimately narrows its dependency graph
this way and never uses a `Decimal`-typed field at all — e.g. `cratestack-api` (`provider =
"none"`, no `model` blocks) — forcing it to name a decimal backend it never touches. The break
was invisible in a `cargo check --workspace` run (feature unification from other workspace
members hid it) until the affected crate was built alone — exactly what happened in the field
(ADORSYS-GIS/webank-services#279).

Selecting *both* backends at once is still a hard `compile_error!` — that half of the invariant
is unchanged and stays a graph-wide constraint (documented in `cratestack-core`'s crate-level
rustdoc and in `CLAUDE.md`): two independent dependents in the same build, each individually
well-formed and each deliberately choosing a different backend, can still force an unbuildable
combined graph. Making the two backends genuinely additive (or moving the choice off Cargo
features entirely) remains open, unaddressed by this change, and reserved for a future,
maintainer-scoped design decision.

**What actually changed:** `cratestack-core::Decimal` (and everything in this crate and its
downstream SQL layer that references it unconditionally — `cratestack-core::validate_range_decimal`,
`cratestack-sql::SqlValue::Decimal`, `cratestack-sql`'s `IntoSqlValue for Decimal`, and the
matching bind/decode arms in `cratestack-rusqlite` and `cratestack-sqlx`) is now `#[cfg]`-gated on
"a decimal backend is selected", the same pattern throughout. With neither backend selected,
these symbols simply don't exist on the public surface instead of hard-erroring the whole build —
a consumer that never references `Decimal`, directly or via a schema with no `Decimal`-typed
field, now builds cleanly across every facade (`cratestack-pg`, `cratestack-api`,
`cratestack-sqlite`, `cratestack-client`, plus `cratestack-axum`/`cratestack-studio`), even under
`--no-default-features`. A consumer that *does* try to use `Decimal` without picking a backend
now gets a plain rustc "cannot find type/variant `Decimal`" from wherever the reference lives,
instead of the old, single, clearer `compile_error!` naming the missing choice — a diagnostic
regression accepted in exchange for not hard-failing every backend-agnostic consumer.

No consumer-visible signature or behavior change for anyone who already selects a decimal backend
(the default, `decimal-rust-decimal`, or the opt-in `decimal-bigdecimal`) — this only affects
builds that previously hit the removed `compile_error!`.

### `AuditSink` gets a real installation path (#473, #517)

`cratestack_core::AuditSink` (plus `NoopAuditSink`/`MulticastAuditSink`) has existed since
before this release, but had nowhere to be installed: a consumer could construct a sink and had
no way to hand it to the runtime, and `AuditSink::record` was never invoked anywhere in the
workspace — `cratestack-sqlx/src/audit.rs`'s own module doc claimed fan-out "goes through
`AuditSink`" while that was, in fact, dead code.

`SqlxRuntime` now carries an installable `Arc<dyn AuditSink>` (default `NoopAuditSink`, so
existing `SqlxRuntime::new(pool)` callers see no behavior change), installed via
`SqlxRuntime::with_audit_sink` or, for schema consumers, the macro-generated
`CratestackBuilder::with_audit_sink` — the same shape `IdempotencyStore`/`RateLimitStore` use.
Every `@@audit` write path (`create`/`update`/`delete`/`upsert`, their `_many` and batch
variants) now fans the event out to the installed sink *after* its owning transaction commits,
never from inside it: the in-database `cratestack_audit` row remains the sole in-transaction
write and source of truth (unchanged, no double-write), and a sink is never invoked for a
mutation that ultimately rolled back. Sink errors are logged (`tracing::warn!`), not propagated
— by the time the sink runs, the mutation already committed, so failing the caller's request
over a downstream projection hiccup would be strictly worse than a best-effort delivery.
`run_in_tx` variants (caller-managed transaction) do not fan out, mirroring the existing event
outbox, which has never drained from `run_in_tx` either. **This is a real gap, not just a
deferral**: there is currently no way for a `run_in_tx` caller to opt into sink fan-out
themselves — the dispatch helper is crate-private and no `run_in_tx` variant returns the
`AuditEvent` it would need — so a caller chaining `run_in_tx` calls across a caller-managed
transaction (see `crates/cratestack-pg/tests/banking_chained_audit_tx.rs`) gets the
in-transaction `cratestack_audit` row on commit but a real installed `AuditSink` observes
nothing for that transaction, silently. Worth its own follow-up issue; see
`crates/cratestack-sqlx/src/audit/sink.rs`'s doc comment for the full reasoning. Dispatch is
also sequential, not concurrent, so the added latency of a slow sink is per-row on
`update_many`/`delete_many`/batch paths, not per-request.

### `cratestack-studio`: refuse silent `@version`/`@@emit` bypass on `[target.db]` writes — breaking (#516, cratestack#507)

A write through `cratestack studio` against a `[target.db]` target went straight to SQL: it never
bumped a model's `@version` column and never wrote a `cratestack_event_outbox` row for an
`@@emit`-annotated model, and neither omission was reported anywhere — the request returned `200`
with the updated row. Both consequences are silent and outlive the request: a stale `@version`
still satisfies a later `if_match`, so optimistic concurrency does not fail-safe, it silently does
not apply; and `@@emit` side effects (for example customer-facing delivery webhooks) never fire,
with no trace that one was skipped.

Studio now refuses `POST`/`PATCH`/`DELETE` on a `rw` `[target.db]` target against any model that
declares `@version` or `@@emit(...)`, returning `403 UNSAFE_DB_WRITE` and naming the specific
attribute(s) that triggered the refusal, unless the target sets `allow_unsafe_writes = true` in
`studio.toml`. The refusal runs in the HTTP handler (`require_safe_write`,
`crates/cratestack-studio/src/api/records/guards.rs`) before any `DataSource` call, so it applies
identically to Postgres- and SQLite-backed targets, and models with neither annotation are
unaffected either way. A write allowed only because a target opted in is also loud after the fact,
not just at the moment the config flag is set: it logs a `tracing::warn!` naming the target, model,
and skipped annotation(s), and `AuditEntry` gains an `unsafe_write: bool` field (default `false`,
`#[serde(default)]` so a pre-upgrade JSONL audit sidecar still replays cleanly) so `GET /api/audit`
and the sidecar can distinguish a bypass write from an ordinary one.

The `@@allow` half of the original report — an unauthenticated `[target.db]` read returning a
`@sensitive` field in cleartext — is deliberately left alone here; it is arguably intended for a
direct-DB admin tool and is tracked separately for a maintainer decision, not fixed unilaterally in
this change. Likewise, routing `[target.db]` writes through the same descriptor path the generated
server uses (so `@version`/`@@emit` would actually apply, rather than being refused) remains
unimplemented and is left for a future, larger change.

**Migration.** Any existing `rw` `[target.db]` deployment whose schema declares a model with
`@version` or `@@emit(...)` will start getting `403 UNSAFE_DB_WRITE` on `POST`/`PATCH`/`DELETE`
against that model through Studio. Add `allow_unsafe_writes = true` under that target's
`[target.db]` block in `studio.toml` to keep the previous (silent-bypass) behavior, or leave it
unset and route those writes through `[target.api]` instead, where `@version`, `@@emit`, and
`@@allow` all apply exactly as declared in the schema. Reads and `[target.api]`-only targets are
unaffected either way.

### Per-procedure `@status(<code>)` for REST success responses (#407, #511)

`generate_procedure_axum_handler` hardcoded `axum::http::StatusCode::OK` for every procedure's
`Ok(...)` response, with no schema-level way to declare a different 2xx status (e.g. `202
Accepted` for a submit-and-acknowledge procedure whose real verdict arrives later via webhook).
A schema author can now write:

```
procedure submitKycDocument(args: SubmitKycDocumentInput): KycPresignReply
  @status(202)
```

`cratestack-parser` validates the argument is a real `200..=299` status at schema-compile time
(anything outside that range, or on a `transport rpc` schema — see below — is rejected with a
clear diagnostic, not a runtime surprise); `cratestack-macros` threads the declared status into
`result_encoder` for the unary, `TypeArity::List`, and `@stream` branches alike, replacing the
previously-hardcoded literal in all three. Absent the attribute, codegen is byte-identical to
before (the pre-existing cratestack#283 pinned-token regression test is unchanged). Error
responses are untouched either way — `CoolError`'s own status mapping governs `Err(...)`
unconditionally, independent of `@status`.

`@status` is REST-only and is rejected at schema-compile time on `transport rpc` schemas: RPC
unary dispatch shares the exact same generated handler REST uses, so an unrejected `@status`
there would silently become wire-visible on the RPC response too. `transport grpc` is
unaffected either way — tonic's gRPC status model never reads the inner HTTP status this
attribute controls, so the combination is inert, not wrong, and stays allowed.

Known limitation, left for a follow-up rather than silently narrowed here: `@status(204)` is
accepted by the `200..=299` range check, but the REST encoder always serializes and attaches a
response body regardless of declared status, so a declared `204` currently produces a
`204 No Content` response that carries a body — a protocol violation per RFC 9110 §15.3.5.

### Typed Rust client can read response headers — `*_with_response` methods (#493, #510)

`decode_typed_response` (`cratestack-client-rust/src/client/decode.rs`) read `response.headers`
only to find `Content-Type`, then returned the decoded body alone — every typed call built on it
(`get`/`post`/`patch`/`delete`, and the generated `<Model>Client`'s `list`/`get`/`create`/`update`/
`delete`) discarded every response header. For any `@version` model, that made the typed client
structurally unable to do a concurrency-safe `PATCH`: CrateStack's optimistic-locking contract
requires `If-Match` on that verb, with the current version handed back as `ETag` on `GET` — so
the required round trip, `GET` → read `ETag` → `PATCH` with `If-Match`, had no typed path through
its middle step. The same gap hid `Idempotency-Replayed` (on a replayed create) and `Retry-After`
(on a `429`) from a typed caller. **Note:** `DELETE` is not part of that contract — the server
does not currently enforce `If-Match` on `DELETE` for any model, versioned or not (see below).

Added a `TypedResponse<Output> { value, status, headers }` (with a case-insensitive
`.header(name)` accessor, plus `.header_values(name)` for the rare header that legitimately
repeats, e.g. `Set-Cookie`) and a parallel `*_with_response` method next to every existing typed
method: `CratestackClient::{get,post,patch,delete}_with_response`, and on the generated REST
`<Model>Client`, `get_with_response`/`update_with_response`/`delete_with_response`. Purely
additive — `decode_typed_response` is now implemented in terms of a new
`decode_typed_response_with_metadata`, but keeps its exact original signature and behavior, so
every existing call site (including every already-generated client) keeps compiling and behaving
identically with no changes required.

`delete_with_response` ships alongside `get_with_response`/`patch_with_response` for surface
symmetry (status and headers on every write, not just versioned ones — useful for e.g. reading a
`Retry-After` on a `429`), but unlike `patch_with_response`, sending `If-Match` on a `DELETE` has
**no concurrency-safety effect today**: the server accepts and ignores it. Server-side `If-Match`
enforcement on `DELETE` is a real gap in CrateStack's optimistic-locking story — deliberately
*not* implemented here, since it is a separate feature decision outside this issue's scope, and
reported for its own follow-up issue instead.

Scoped to REST transport. RPC transport (`transport rpc`) has no `ETag`/`If-Match` handling
anywhere server-side — a schema-versioned model's concurrency control there, if any, would need
to travel through the request/response body, not an HTTP header — so there is nothing to wire on
the RPC client's `BatchableCall` surface for this issue. Projection reads (`get_view`/`list_view`/
`list_view_paged`) and `create_with_response` on the generated model client are also left
out-of-scope: the acceptance-driving round trip is `GET` → `ETag` → `PATCH` with `If-Match`, which
`get_with_response`/`update_with_response` cover in full; a create-side `Idempotency-Replayed`
reader is still reachable today via the (now also additive) `CratestackClient::post_with_response`
directly, just not yet wrapped by the generated `<Model>Client::create_with_response`.

Verified: `cargo test -p cratestack-client-rust` (unit coverage of
`decode_typed_response_with_metadata` against hand-built responses, including an `ETag`-shaped
header and a case-insensitive lookup; a real-HTTP-server integration test in
`tests/typed_response.rs` proving `get_with_response` → `ETag` → `patch_with_response` with
`If-Match` round-trips end-to-end, a 412-on-stale-`If-Match` case, and that the plain
`get`/`patch` methods are unchanged) and `cargo test -p cratestack-client` (a new
`tests/generated_client_versioning.rs`, using a schema borrowed verbatim from
`cratestack-pg/tests/fixtures/banking_versioning.cstack`, proving the *generated* `<Model>Client`
reaches the same round trip, not just the underlying runtime).

### `Value` serializes untagged on the wire, matching what it already persists — breaking (#506)

`cratestack_core::Value` derived `Serialize`/`Deserialize`, which emits serde's
externally-tagged enum representation. `Value::String("foo")` went on the wire as
`{"String":"foo"}` rather than `"foo"`, and an empty map as `{"Map":{}}` rather than `{}`.
cratestack#162 / #395 fixed that for a schema `Json` **column** by routing persistence through
`Value::to_plain_json`, but only for the column — every other path still carried the tag:
procedure arguments and results typed `Json`, auth claims, audit payloads, RPC error details.

The practical cost landed on consumers. A `Json?` procedure argument rejected `"foo"` and
required `{"String":"foo"}`, so every caller hand-wrote the tag at every call site, and every
generated Dart and TypeScript client inherited a shape no other JSON or CBOR producer emits.
The persisted shape and the wire shape disagreed for the same value.

`Serialize`/`Deserialize` are now hand-written and untagged (`cratestack-core/src/value/codec.rs`).
`serde_json::to_value(&value)` now produces exactly `value.to_plain_json()`, and
`deserialize_any` accepts whatever a self-describing format hands over. `to_plain_json` /
`from_plain_json` are kept: they are infallible and total (substituting `null` for a NaN float,
which the persistence layer relies on) and they make the on-disk contract explicit at the call
site rather than implicit in a serde impl.

Two format-specific details, both measured against the first-party backends rather than assumed:

- **`Null` serializes via `serialize_none`, never `serialize_unit`.** `minicbor-serde` encodes
  `()` as `0x80` — an empty *array*, not RFC 8949 null — while `None` correctly encodes as
  `0xf6`. `serialize_unit` would have put that non-conformant shape on the wire for any
  `Value::Null` nested in a list or sent as a bare argument. This matches the choice
  `ProjectedValue::Null` already makes for the same reason (#430).
- **`Bytes` branches on `is_human_readable()`.** Binary formats get a native byte string
  (CBOR `0x44 de ad be ef`) and round-trip losslessly. Human-readable formats get the same
  base64 string `to_plain_json` already writes, and inherit the same documented asymmetry —
  a JSON string always decodes back as `Value::String`, because nothing distinguishes base64
  from ordinary text.

**Migration.** Anything that persisted a `Value` through its serde impl rather than through
`to_plain_json` — a custom `AuditSink`, a Redis-backed store — will read old tagged rows as
`Value::Map` with a single variant-named key. Redis-backed state self-heals on TTL expiry.
Callers that hand-wrote the tag to satisfy the old wire format must stop: send `"foo"`, not
`{"String":"foo"}`. Regenerate Dart/TypeScript clients.

### `cratestack-codec-cbor`: corrected a false claim in the encoder comment

The comment asserted that `minicbor-serde` reports `is_human_readable() == true`. It reports
**false** — verified by encoding a probe type whose `Serialize` echoes the hint, which emits
`0xf4`. `cratestack-axum`'s `projection.rs` (#430) already documented the correct behavior, so
the two disagreed. The comment also still described the `Value::Null`-stripping workaround that
#430 removed. No behavior change; the code was right and the comment was wrong.

### Pluralizer gains the standard English `y -> ies` rule — breaking (#504, #509)

`cratestack_core::route_naming::pluralize` (and, via it, `cratestack-migrate::naming::table_name`)
had no `y -> ies` case: any model name ending in a consonant + `y` derived the wrong plural —
`category` -> `categorys`, `webhook_delivery` -> `webhook_deliverys` — instead of the
grammatically correct `categories` / `webhook_deliveries`. This wasn't just cosmetic: the derived
name is the actual SQL table the generated model client queries, so a consumer who hand-wrote a
migration using the correct English plural got `relation "webhook_deliverys" does not exist` the
moment the generated client touched that model — a real production defect downstream
(webank-services' `adminGetWebhooks`; see cratestack#504's linked ADR).

`pluralize` now applies the standard rule: consonant + `y` -> `ies` (`category` -> `categories`);
vowel + `y` (`day`) or anything else -> plain `+s`. `cratestack-migrate::naming::pluralize`, a
second hand-synced copy of the same function that had already drifted apart from this one, is
deleted; `cratestack-migrate::naming::table_name` now calls `cratestack_core::route_naming::pluralize`
directly, so there is exactly one implementation to keep correct going forward.

This changes both generated REST route segments and generated table names for **every**
model/view whose name ends in a consonant + `y`. It does *not* touch
`cratestack-client-typescript::naming` or `cratestack-client-dart::idents`, the two SDK
accessor/method-name generators (`db.categories()`, `useCategories()`) — they already implement
the correct consonant/vowel rule and were deliberately out of scope here.

**Migration.** This is a breaking change to generated table names and REST routes for any schema
with a model/view name ending in a consonant + `y` (`Category`, `Delivery`, `Entry`, `Query`,
...). `cratestack-migrate`'s diff engine matches tables **by name only** and never infers a
rename (`crates/cratestack-migrate/src/diff.rs`) — running `cratestack migrate diff` against a
schema with a deployed `categorys` table, without further action, emits `DropTable(categorys)` +
`CreateTable(categories)`, and applying that migration **destroys the table's data**. Before
running `migrate diff` after upgrading past this change, declare
`@@rename(from = "<old_table_name>")` on every affected model (e.g.
`@@rename(from = "categorys")` on `model Category`) so the diff engine emits
`ALTER TABLE ... RENAME TO ...` instead — verified end-to-end by
`crates/cratestack-migrate/src/emit/postgres/tests/renames.rs`'s
`pluralization_change_with_rename_marker_is_a_rename_not_drop_and_create` test (and its sibling
`..._without_rename_marker_drops_and_recreates`, which pins down the destructive default if this
step is skipped). Any generated Dart/TypeScript/Rust client built against the old route segment
for such a model will 404 against a server built with this fix, and vice versa, until both sides
are rebuilt together.

### CI, tooling, and internal fixes

`grpc/service.rs`'s five CRUD arm builders (`build_get_arm`/`build_delete_arm`/`build_create_arm`/
`build_update_arm`/`build_list_arm`) each independently reimplemented the same per-arm marker
struct, `UnaryService` impl, and `CoolError`-to-`tonic::Status` mapping, differing only in which
dispatch fn to call and how many arguments to thread through. Deduplicated into a shared
`build_unary_arm(ArmSpec)` helper; generated output is unchanged — verified byte-identical via
`cargo expand` before and after, including a paged-list and a create-disallowed fixture the
existing test suite didn't otherwise exercise (#524).

CI now actually runs `cratestack-pg/tests/decimal_bigdecimal_backend.rs`, the live-Postgres
`decimal-bigdecimal` round-trip test #495/#496 added but never wired into a job: `tests-db` only
built `cratestack-pg` under its default feature set, and `.ci/feature-matrix.sh` only ever ran
`cargo check`, never `cargo test`, so a runtime regression in that codec/bind path would have
passed every existing gate silently (#520).

`just regen-examples` regenerates the two committed generated example clients
(`examples/flutter-riverpod/client`, `examples/react-vite-swr/client`) locally, reusing the exact
generator invocations `ci.yml`'s drift-check steps run so the recipe and CI can't copy-paste
diverge. Both example CI jobs' downstream steps (`cargo test`, `flutter analyze`/`test`, `pnpm
install`/`tsc`) now run and report their own pass/fail even when the drift check itself fails,
instead of being silently skipped behind it (#508).

The release-bump commit's `git add` staged the root `Cargo.lock` by name only, silently dropping
the four other lockfiles `just bump` also refreshes on disk (`crates/cratestack-studio-ui/Cargo.lock`
and the three standalone `examples/*-verification*/Cargo.lock` files, each its own `[workspace]`
root) — the exact gap that broke v0.7.10's own release PR (`facade-disjointness` failed with
`--locked` because one of those lockfiles was stale). Now staged via a glob pattern, closing the
gap structurally rather than chasing individual filenames (#503).

## 0.7.10 (2026-08-09)

### Per-call-site `ON CONFLICT DO NOTHING` for idempotent inserts (#487, ADR 0038 blocker B3)

`.upsert(..).run(..)` only ever emitted `INSERT ... ON CONFLICT DO UPDATE`. A model with any `upsert_update_columns` had no way to express "insert, or read back without mutating" — the fallback to a no-op `pk = EXCLUDED.pk` self-assignment only kicked in when `descriptor.upsert_update_columns` was empty, a property of the *model*, not the call site. Concretely: a cash-in claim inserting a `PENDING` row and treating a unique violation as "already in flight" would have a retry's blank values silently overwrite an existing `COMPLETED` row's `transfer_ref`/`new_balance_xaf`/`completed_at` — ledger corruption on retry, not a cosmetic gap. Consumers were hand-rolling `DO NOTHING RETURNING id` + a fallback `SELECT` to avoid exactly this.

`UpsertRecord` (from `cool.model().upsert(input)`) gains `.do_nothing()`, switching to a distinct builder (`UpsertRecordDoNothing`, mirrored for the `.bind(ctx)`-scoped delegate as `ScopedUpsertRecordDoNothing`) whose `.run()`/`.run_in_tx()` return `UpsertOutcome<M>` — `Inserted(M)` or `Existing(M)` — instead of a bare `M`, since a real `DO NOTHING` returns nothing on conflict and the caller needs "I inserted this" distinguishable from "this already existed and I left it alone." This is additive: `.upsert(..).run(..)` without `.do_nothing()` keeps its `Result<M, CoolError>` signature and DO UPDATE semantics unchanged.

Race semantics, spelled out in `UpsertOutcome`'s doc comment: the runtime always resolves the conflict target under the same `SELECT ... FOR UPDATE` row lock the DO UPDATE path already uses. If the probe finds an existing row, that lock guarantees it's still there at commit, so `Existing` is returned directly with no second statement — DO NOTHING genuinely never touches the row (no trigger fan-out, no `xmax` bump, no WAL), unlike the DO UPDATE path's no-op self-assignment fallback, which is a real (if degenerate) write. If the probe finds nothing, the actual `INSERT ... ON CONFLICT DO NOTHING RETURNING` still runs (not a plain `INSERT`) because "no row" from a `SELECT` doesn't lock anything — a concurrent transaction can still win the race. On that loss, the runtime performs one more locked read to hand back the row the other transaction actually committed; if *that* row is deleted before the fallback read completes (a second, narrower race), it surfaces `CoolError::Conflict` rather than inventing a result, and the caller retries.

The existing empty-`upsert_update_columns` no-op-self-assignment fallback in the plain DO UPDATE path is kept, deliberately not merged into `.do_nothing()`: it exists to make `RETURNING` resolve for a *model* shape (zero eligible update columns), while `.do_nothing()` is an explicit *per-call* opt-in independent of that shape, and the two have different storage-layer effects (no-op `DO UPDATE` still fires triggers/bumps `xmax`; genuine `DO NOTHING` doesn't). Merging them would either force every empty-`upsert_update_columns` model onto DO-NOTHING semantics for existing callers or make `.do_nothing()` pay for trigger fan-out it explicitly asked to avoid.

Scoped to `cratestack-sqlx` (Postgres) only. `cratestack-rusqlite` has an equivalent `INSERT ... ON CONFLICT DO UPDATE` upsert path (`render_upsert_with_conflict`) and SQLite supports `DO NOTHING RETURNING` too, but that backend's upsert is a single statement with no pre-probe (no policy/audit/event machinery to preserve, but also no existing "inserted vs. existing" discriminator to build on) — giving it the same capability is a materially different, smaller design left as follow-up rather than folded in here.

New PG-backed regression coverage in `crates/cratestack-pg/tests/upsert_do_nothing.rs`: a ledger-corruption reproduction (insert with real values, retry via `.do_nothing()` with blank values, assert the row is byte-for-byte unmodified — confirmed failing on `main`/028cdc5 via `.upsert().run()`, the only pre-#487 API, before this change existed to fix it), an `Inserted`-vs-`Existing` distinguishability test with audit/event-outbox assertions (no `Updated` event or audit row on the `Existing` branch), a same-fixture regression test that the plain DO UPDATE path is unaffected, and an empty-`upsert_update_columns` model case.

### Generated Dart and TypeScript clients get a real `Decimal` type (#498) — breaking

`cratestack-client-dart` and `cratestack-client-typescript` used to carry every `Decimal`-typed field as an opaque wire-format string — harmless for the default `decimal-rust-decimal` backend (which never emits scientific notation), but silently wrong once #495/#496 made `decimal-bigdecimal` (arbitrary precision, beyond `rust_decimal`'s ~28-29 significant-digit cap) a real, selectable server backend: `bigdecimal`'s `Display` switches to scientific notation past a magnitude threshold (`"0.0000001"` on `rust_decimal`, `"1E-7"` on `bigdecimal`, for the identical value), so the *string form* a Dart/TS client saw depended on which backend built the server, and neither SDK could parse, compare, or do arithmetic on the value at all — the exact case #495/#496's own PR flagged as unfinished business.

The maintainer's recorded decision (of the three approaches priced in #498's own ticket — wire canonicalization, a real client-side decimal type, or refusing the combination) is the middle one: give the SDKs a real decimal type, not change what the wire carries.

- **Dart**: `Decimal`-typed fields (including `DecimalFilter`'s comparison operands) are now `package:decimal`'s `Decimal` class, not `String`. `wire_decode.rs`/`wire_encode.rs` decode via `Decimal.parse` (accepts both plain and scientific notation into the identical value) and encode via `.toString()` (always plain positional notation, matching `rust_decimal`'s own `Display`). Every generated `pubspec.yaml` (default, riverpod, and gRPC presets — gRPC reuses the same generated model classes) gains a `decimal: ^3.2.6` dependency. Because Dart's `Model.fromWire`/`.toWire()` factories are the single decode/encode chokepoint every transport (REST, RPC, and gRPC's own message registry — `grpc_runtime/decode.dart.j2`'s `decodeMessage` hands a plain `Map` to the exact same `fromWire`) already routes through, this is a complete fix: gRPC's proto3 wire type stays `string` (unchanged, see below) but the in-memory value on every transport is a real `Decimal`.
- **TypeScript**: `Decimal`-typed fields are now `decimal.js`'s `Decimal` class (re-exported from the generated `models.ts` as a `DecimalJs.clone({ toExpNeg: -1e9, toExpPos: 1e9 })` — an unbounded-exponent clone, so `.toString()`/`.toJSON()` always emit plain positional notation too, for the same reason as Dart's `.toString()` choice). `decimal.js` is a `dependencies` entry (not `peerDependencies`, unlike `@tanstack/react-query`) in every generated `package.json` — every consumer needs a working `Decimal` implementation and there's no app-owned-singleton constraint the way there is for React/react-query, so nothing is gained by pushing the choice onto the app. Adds ~32 KB minified / ~13 KB minified+gzip, zero transitive dependencies (measured: `npm view decimal.js dist.unpackedSize` reports 284 KB unpacked; a local `terser` minify of the runtime file alone is 32,328 bytes, 12,860 gzipped).

  Unlike Dart, this package had **no decode/encode chokepoint at all** before this change — every response was a blind `JSON.parse`/codec-decode cast with `as T`, no runtime transform of any kind (a `DateTime` field is still a bare `string`, by design). A real `Decimal` instance needs an actual runtime replace of the wire string, so `models.ts.j2` gains a `decimalShapes` registry (one `DecimalShape { keys, nested }` entry per model/`type` the schema declares — `crate::decimal::build_decimal_shapes`), a `reviveDecimalFields(value, shapeName)`/`revivePagedDecimalFields(value, shapeName)` pair keyed by that registry, and a `reviveDecimalScalar(value)` counterpart for a return value that is itself `Decimal` (not wrapped in an object). Every generated decode call site — the `default`-preset REST (`rest-client.ts.j2`) and RPC (`rpc-client.ts.j2`) clients' `list`/`get`/`create`/`update` methods **and their `ProceduresApi`**, the `swr` preset's per-model functions (`models-rest.ts.j2`/`models-rpc.ts.j2`) **and its `procedures.ts`** — calls one of these unconditionally, keyed by the decoded type's own registry entry name (a name with no entry, e.g. a plain scalar/enum return, is a documented fast-path no-op, so this is a uniform, always-present `.then(...)` wrapper rather than the generator branching per call site). A relation-embedded `Decimal` field (a `Post.author.balance` shape) revives too, not just a flat field on the root — `reviveShaped` routes a nested field to *its own* type's shape via `nested`, recursively. `Decimal.prototype.toJSON` (an alias for `.toString()`) makes the *encode* direction (`JSON.stringify`, which both a REST request body and this package's default `jsonRpcCodec` go through) work automatically, with no generated glue needed. `DecimalFilter`'s comparison operands are `ComparableFilter<Decimal>` now; they never need decode-side revival since a `Where`/`FindMany` argument only ever travels outbound.

  **A real bug was caught and fixed by a second reviewer before this landed, not just theorized:** the first version of this scheme (`crate::decimal::DecimalReachability`) kept a single flat `Set<string>` of every `Decimal` field name reachable from a response's root type — its own fields *and* every relation's/`type`'s fields, unioned together — and matched it against a decoded response's keys at *any* nesting depth. That is unsound the moment two *different* reachable types can each contribute a field name to the same flat set: an `Order.total: Decimal` + related `Account.total: String` schema, `include`-ing the relation, either threw `[DecimalError] Invalid argument]` decoding a real (non-numeric) account reference or silently corrupted a numeric-looking one (`"00123"` -> `Decimal("123")`, losing its leading zeros) — reproduced empirically (`tests/fixtures/decimal_name_collision.cstack`, `tests/decimal_collision_regression.rs`), not just reasoned about. Replaced with the path-aware `decimalShapes` registry above: every type keeps its own `Decimal` field names in its own shape, never merged with another type's, so `Account.total` is only ever checked against *Account's* shape (which correctly has no `total` key).

- **gRPC (both SDKs):** `grpc/wire.rs` still maps `Decimal` to proto3 `string` on the wire (unchanged, matching `cratestack-proto::emit::scalar::map_scalar`). Dart's gRPC preset reuses the identical `Model.fromWire`/`.toWire()` factories the REST/RPC presets use (confirmed by generating a `transport grpc` schema with a `Decimal` field and running `flutter analyze`/`flutter test` — clean, including a real relation-embedded-field and procedure-return-type round trip), so it's not just non-broken, it's *correct*: proto3 carries a string, the in-memory value is a real `Decimal` — and, since Dart's decode always goes through a real per-field-typed `fromWire`, never a flat name-keyed set, it was never exposed to the collision class above either. TypeScript's gRPC-Web preset gains a dedicated `"decimal"` `GrpcWireKind` (`wire.rs`, `grpc-web-runtime.ts.j2`'s `encodeScalar`/`decodeScalar`/`zeroValue`) — same proto3 `string` wire bytes, decoded into a real `Decimal` (imported from `./models.js`) rather than the raw JS `string` an earlier draft of this change left it as; a probe schema (`transport grpc`, a `Decimal` field) both `tsc`-typechecks and round-trips a scientific-notation value through the real generated `encodeMessage`/`decodeMessage`, proven by a real `npx vitest run`. gRPC's own decode (`decodeMessage`) is per-message-type-scoped by construction (field descriptors are looked up per message, never name-matched across types), so it was never exposed to the flat-key collision class either.

**Breaking, on the default `decimal-rust-decimal` backend, whether or not a schema uses `decimal-bigdecimal` at all:** existing app code doing `model.amountField` and expecting a `string`/`String` now gets a `Decimal`/`decimal.js` `Decimal` instance instead. Migration: replace `String`/string-typed usage of a `Decimal` field with the respective library's API — Dart: `Decimal.parse(input)` to construct, `.toString()` to format, comparison operators/`compareTo` work directly; TypeScript: `new Decimal(input)`, `.toString()`, `.plus()`/`.minus()`/`.cmp()` etc. instead of raw arithmetic/string comparison. `DecimalFilter`'s `eq`/`ne`/`lt`/`lte`/`gt`/`gte`/`in` fields need the same treatment when constructing a `Where`/`FindMany` argument by hand.

Verified: `cargo test -p cratestack-client-dart -p cratestack-client-typescript` (generator/snapshot suites, all fixtures reviewed line by line, not just re-recorded); two new real-toolchain round-trip tests — `cratestack-client-dart/tests/decimal_round_trip.rs` (`flutter pub get` + `flutter test` against a generated package) and `cratestack-client-typescript/tests/decimal_round_trip.rs` (`npm install` + `npx vitest run`) — proving a value beyond `rust_decimal`'s capacity, in both plain and scientific notation, round-trips through the real generated `fromWire`/`toWire` (Dart) and REST client (TypeScript) with its value intact; `just verify-dart`/`just verify-typescript` (generation + `flutter analyze`/`tsc` against the `ci_rest`/`ci_rpc`/riverpod fixtures, none of which declare a `Decimal` field, so this also proves the `DecimalFilter`-only boilerplate change doesn't regress every existing generated package).

### `auth().isSystem()` — a system principal for server-internal reads and writes (#486, ADR 0038 blocker B1)

Model policies can now name a trusted system principal instead of only end-user claims: `@@allow("update", auth().isSystem() || subjectId == auth().subjectId)`. This is the half of #486's proposal being shipped now — the spike in #485 (closed, do-not-merge) also prototyped `@@internal(...)` route suppression, but that covers REST only and is a false guarantee under `transport rpc` (a "suppressed" route would still be reachable over RPC); the spike's own recommendation, followed here, was to ship `isSystem()` first since it alone unblocks the read-side problem route suppression does nothing for. `@@internal` is **not** part of this change.

Today, giving server code (procedures, workers, reconciliation jobs) a way to write through the ORM means adding an `@@allow("update", ...)` policy, which also opens a public CRUD route for that action — and an owner-scoped `@@allow("detail", subjectId == auth().subjectId)` denies a legitimate internal read, since a service caller carries no subject claim. Both push consumers toward hand-written raw SQL instead of the generated surface.

`isSystem()` is a term a policy **names**, not a bypass flag: `db.model().unchecked().update(...)` was explicitly rejected because it would move authorization out of the schema into scattered call sites, with nothing distinguishing a legitimate escalation from an accidental one. `cratestack_core::SystemContext::for_service("...")` is the only way to obtain a `CoolContext` that satisfies `auth().isSystem()`; it has no `From`/`TryFrom<CoolContext>` and no constructor accepting an existing (e.g. request-derived) context, and the backing flag is a private, `#[serde(skip)]` field on `CoolContext` — so no `AuthProvider` implementation, and no deserialized/wire-carried context, can ever produce one. **Fail-closed:** a model whose policies never write `isSystem()` gains nothing from a system caller — the predicate only ever satisfies a clause a schema author wrote down, proven by `model_that_never_names_is_system_denies_system_callers` (`cratestack-sqlx`) and `system_caller_is_denied_on_a_model_that_never_names_is_system` (PG-backed, `cratestack-pg/tests/system_principal.rs`). **Auditable:** `SystemContext::for_service` records the service name as both the `id` (`system:<service>`) and `service` claims, which flow unchanged into the existing `cratestack_audit` actor — no new audit machinery needed, see `system_write_is_captured_in_the_audit_trail`.

Wired through all three places a model read policy is evaluated: the create-path in-process evaluator (`cratestack-sqlx::query::support::create::evaluate_input_predicate`), the `QueryBuilder` pushdown used by row-scoped write authorization (`query::support::policy_predicate::push_policy_predicate`), and the SQL-string renderer used by `find_unique`/list reads (`render::policy_predicate::render_policy_predicate`) — this last one is why PG-backed coverage exists alongside the unit tests: it's the read path route suppression could never have fixed. `auth().isSystem()` is recognised in the model policy-term parser (`cratestack-macros::policy::model::term`) ahead of the generic builtin-call parser, which would otherwise misparse `auth()`'s own parens as the function call.

Verified: `cargo test -p cratestack-core -p cratestack-policy -p cratestack-macros -p cratestack-sqlx`, plus `cratestack-pg/tests/system_principal.rs` against a real Postgres (`just test-pg-only system_principal`), covering all five acceptance criteria — system-permitted where named (read and write), fail-closed where not named, non-system callers unaffected, audit capture, and an HTTP-request forgery attempt (a plausible "naively forward a client claim" `AuthProvider` bug) that still cannot produce a system context.

### A real `decimal-bigdecimal` backend (#495) (#496) — breaking

`decimal-bigdecimal` was removed in #464/cfde4e0 for being a dead `compile_error!` — declared but never implemented. This implements it for real: `cratestack-core::Decimal` is now `cfg`-gated per backend (`rust_decimal::Decimal` under `decimal-rust-decimal`, `bigdecimal::BigDecimal` under `decimal-bigdecimal`), with two `compile_error!`s enforcing that exactly one is selected — neither (the pre-existing check) and now also both (new: the two are mutually exclusive, so `--all-features` trips it again, this time for a real reason).

The two backends are not drop-in equivalents: `rust_decimal::Decimal` is `Copy`; `bigdecimal::BigDecimal` heap-allocates and is not. A workspace-wide audit for double-moves/implicit-copy reliance (source and tests) found exactly one real call site — `cratestack-sqlx`'s `push_bind_value` dereferenced a `&Decimal` to bind it (`*value`), which only compiles for a `Copy` type — fixed with `.clone()`, which degrades to a cheap bitwise copy under `decimal-rust-decimal` and a real allocation under `decimal-bigdecimal`. No `derive(Copy)` on any `Decimal`-carrying type exists anywhere in the workspace. Both backends implement `Clone`/`Debug`/`Display`/`FromStr`/`PartialEq`/`PartialOrd`/`Ord`/`Eq`/`Hash`/`Default`, so no other trait bound needed to change.

Making the swap reachable, not just possible in `cratestack-core` alone, meant widening #421's "one shared `default-features = false` dependency edge" pattern to the full transitive closure between a facade and `cratestack-core`: `cratestack-sql`, `cratestack-policy`, `cratestack-parser`, `cratestack-proto`, `cratestack-macros`, `cratestack-axum`, `cratestack-codec-cbor`, and `cratestack-codec-json` all gained the same `default-features = false` (at the workspace-dependency site) plus explicit per-consumer `decimal-rust-decimal`/`decimal-bigdecimal` forwards that `cratestack-core`/`cratestack-sqlx`/`cratestack-rusqlite`/`cratestack-client-rust` already had — a single crate left pinning `decimal-rust-decimal` anywhere in that closure re-forces it for the whole graph, since Cargo features are additive and unify globally. `cratestack-sqlx`'s `decimal-bigdecimal` feature forwards to `sqlx-core`/`sqlx-postgres`'s own (implicit, un-gated-by-name) `bigdecimal` features, giving `cratestack_core::Decimal` real `sqlx::Type`/`Encode`/`Decode` impls against Postgres `NUMERIC` under either backend through the exact same integration points (`push_bind_value`, generated `row.try_get(...)`) — neither of which names a concrete backend type.

`cratestack-pg`'s `postgres` feature previously forwarded `cratestack-sqlx/decimal-rust-decimal` unconditionally (the #421 fix, reasonable when there was only one backend to want). That's now removed: forcing a specific backend there would make `--features postgres,decimal-bigdecimal` request both backends on `cratestack-sqlx` simultaneously, hitting the new mutual-exclusion `compile_error!` — exactly the outcome this issue exists to avoid. `postgres` alone (no explicit decimal feature) is consequently a deliberate compile failure now; `.ci/feature-matrix.sh` asserts this explicitly so a future "fix" that silently re-adds the force gets caught.

**Breaking:** any consumer of `cratestack-pg`, `cratestack-api`, `cratestack-sqlite`, or `cratestack-client` using `default-features = false` must now explicitly select `decimal-rust-decimal` or `decimal-bigdecimal` — a bare `--no-default-features` (or `default-features = false` with no re-added decimal feature) that used to silently resolve to `rust_decimal` is now a `compile_error!`. `examples/no-database-verification` hit exactly this (`cratestack-pg` with `default-features = false`, the configuration `crates/cratestack-pg/README.md` documents) and needed an explicit `features = ["decimal-rust-decimal"]` re-add — see that example's own `Cargo.toml` comment for why the *host*-side `cratestack-core` (compiled in for the `cratestack-macros` proc-macro) has no other path to a backend even when a target-side dependency happens to request one.

One structural limitation found and left as an intentional workaround, not a design choice: `cratestack-macros/tests/ui.rs`'s `trybuild`-based compile-fail suite generates a synthetic crate that copies `cratestack-macros`' entire `[dependencies]` table (including `cratestack-core`) onto its own dependency list — separate from the proc-macro's own resolution of the same package — and `trybuild`'s feature-copying logic only preserves `dep:xxx` weak-optional-dependency forwards, silently dropping ordinary `"pkg/feature"` strings. The copied `cratestack-core` edge therefore never received a decimal-backend forward and hit the "neither selected" `compile_error!` for a test unrelated to decimals at all. Fixed by adding `resolver = "1"` to `cratestack-macros`' own `[package]` — genuinely ignored by Cargo for build purposes on this crate as a real workspace member (the root `[workspace] resolver = "3"` governs there regardless; only `trybuild`'s standalone synthetic mini-workspace obeys it, reuniting the two copies' feature resolution the way they did before this backend existed), but **not silent**: it emits `warning: resolver for the non root package will be ignored` on every `cargo check`/`build`/`test` invocation anywhere in the workspace (verified: even `cargo check -p cratestack-core`, an unrelated crate, prints it), because Cargo evaluates the full workspace manifest tree regardless of which package is targeted. That tradeoff — permanent, harmless build noise vs. a broken `trybuild` suite, given `trybuild` has no configuration hook to avoid this and hardcoding the decimal feature on the edge instead would re-open the exact leak #495 closes — is accepted for now; see `cratestack-macros/Cargo.toml`'s own comment for the alternatives considered and rejected.

`cratestack-cli`'s four remaining tool-crate dependencies (`cratestack-studio`, `cratestack-mock-wiremock`, `cratestack-client-dart`, `cratestack-client-typescript`) were plain `.workspace = true` edges with no `default-features = false`, so their own `decimal-rust-decimal` default stayed force-enabled regardless of what `cratestack-cli` itself requested — `cargo check -p cratestack-cli --no-default-features --features decimal-bigdecimal` hard-failed with a `compile_error!` that pointed nowhere near the real cause. Fixed for #496 by widening the same `default-features = false` + explicit-forward treatment to those four edges too; `cratestack-cli` now fully displaces `rust_decimal` under `decimal-bigdecimal`, closing the gap the original #495 PR left as out-of-scope.

**Cross-backend wire compatibility constraint:** ordinary `Decimal` values encode identically on the wire (CBOR and JSON) under either backend, but `bigdecimal` emits scientific notation (e.g. `"1E-29"`) for values past `rust_decimal`'s ~28-29 significant-digit capacity, which a `rust_decimal` peer cannot decode. Since the shipped Dart and TypeScript client SDKs only ever target the default (`rust_decimal`) backend, a `decimal-bigdecimal` server cannot safely use its extra precision when talking to them — see `crates/cratestack-core/README.md` and the facades' feature docs for the full deployment constraint.

Verified: `cargo check -p cratestack-pg --no-default-features --features postgres,decimal-bigdecimal`, `cargo check -p cratestack-client --no-default-features --features decimal-bigdecimal`, `cargo check -p cratestack-cli --no-default-features --features decimal-bigdecimal`, and `cargo tree -p cratestack-client --no-default-features --features decimal-bigdecimal -e features | grep rust_decimal` (prints nothing) all pass, plus a new live-Postgres round-trip test (`cratestack-pg/tests/decimal_bigdecimal_backend.rs`, `required-features = ["postgres", "decimal-bigdecimal"]`, mirrors `pgvector_feature_forwarding.rs`'s pattern) confirming both an in-range and a beyond-`rust_decimal`-capacity `Decimal` field round-trip through `NUMERIC` under the new backend without precision loss.

### A fourth facade, `cratestack-client`, for pure HTTP-client SDK crates (#490)

`include_client_schema!` was previously only reachable through `cratestack-pg`, `cratestack-api`, or `cratestack-sqlite` — all three of which carry `cratestack-axum` (and therefore `axum`/`tower`/`hyper`/`tower-http`, a full server framework) unconditionally, even when a consumer only ever calls a cratestack server and never runs one. `cratestack-client` re-exports **only** `include_client_schema!` (not the other two entry macros — reaching for either now fails with a plain name-resolution error) plus the generated Rust client runtime and the handful of type re-exports client codegen actually references, derived empirically by tracing every `::cratestack::` path `include_client_schema!`'s expansion can emit. `cratestack-axum` is structurally absent from its dependency graph under default features — proved by a new standalone verification workspace, `examples/client-only-verification` (mirrors `examples/no-database-verification-api`'s cratestack#347 precedent: its own `[workspace]` root with a committed `Cargo.lock`, not a member of the root workspace, since Cargo unifies features across workspace members). This facade has no `grpc` Cargo feature: `cratestack-client-rust`'s own `grpc` feature pulls `tonic`, which pulls `axum` transitively, defeating the point; a gRPC-client consumer should depend on `cratestack-client-rust` directly with `features = ["grpc"]` instead.

Building the empirical re-export list surfaced a real, pre-existing gap: `RpcListInput`/`RpcPkInput`/`RpcUpdateInput`/`RpcListPredicate` (the RPC model-CRUD input envelopes `transport rpc` client codegen references as `::cratestack::rpc::*`) were defined in `cratestack-axum::rpc::inputs`, not `cratestack-core::rpc` alongside their sibling wire shapes (`RpcErrorBody`, `RpcRequest`, `RpcResponseFrame`) — an oversight relative to that module's own stated goal ("clients can depend on a single source of truth without pulling in axum"), invisible until a facade without `cratestack-axum` in its graph tried to compile a `transport rpc` schema with model CRUD. The four types move to `cratestack-core::rpc`, with `cratestack-axum::rpc` re-exporting them unchanged — same names, same shapes, same wire format, so `cratestack-pg`/`cratestack-api`/`cratestack-sqlite` see no behavior change.

The facade also declares `pgvector` and `rate_limit` Cargo features. `include_client_schema!` runs the same extension-declaration gate as the server and embedded macros, so without them a schema containing `extension pgvector { }` or `extension rate_limit { }` was a hard `compile_error!` through this facade with no feature to opt into — which, since a client SDK is generated from the same `.cstack` the server is built from, ruled out every server schema using embeddings or rate limiting. Unlike `cratestack-pg`'s same-named features these forward to `cratestack-macros` alone, with no runtime half: a `Vector(n)` field reaches the generated client as a plain `Vec<f32>` (the `pgvector` crate is involved only at the server's sqlx row-decode boundary) and `@no_rate_limit` only affects enforcement living in `cratestack-axum`. They are schema-compatibility switches, not feature implementations.
### `cratestack-axum` response content-type negotiation stops picking codecs the router can't actually encode (#489)

A router built with a single codec (e.g. `router(db, procedures, JsonCodec, auth)`) returned a spurious `406 Not Acceptable` — `no encoder configured for response Content-Type application/cbor` — whenever a client's `Accept` header named `application/cbor` alongside `application/json`, even though the router had a perfectly good JSON encoder. Root cause: `RouteTransportCapabilities::response_types` is a compile-time list describing what the *transport shape* (REST/RPC binding) can carry across every possible codec configuration, not what the concrete `HttpTransport` a given router was actually constructed with can encode — `select_response_content_type` picked the first entry of that static list the client's `Accept` named, with no way to know the router only had one codec wired up.

`HttpTransport` gains a `can_encode(&self, content_type: &str) -> bool` method (defaulted to `true` — preserves the pre-#489 behavior for any downstream impl that hasn't opted in, since a required method would be a breaking change to this public trait), implemented honestly by both in-repo impls: the blanket `impl<C: CoolCodec> HttpTransport for C` and `CodecSet<Primary, Secondary>` (including the `application/cbor-seq` special case, encodable whenever *either* slot is a CBOR codec, regardless of position). Response negotiation (`select_transport_response_content_type`, used by both `encode_transport_result_with_status_for` and the sequence/`@stream` encoders) and the `Accept` preflight (`validate_transport_request_headers_for`/`validate_transport_response_headers_for`) now both filter the advertised `response_types` through `can_encode` before matching against `Accept` — the preflight fix additionally means a mutation like a model `create` now fails fast on an unsatisfiable `Accept` *before* its DB write runs, not only afterward when the response encoder finally catches it. A `NotAcceptable` the negotiator does return now names what the router actually serves, not the static list. One behavior change reaches slightly beyond the literal repro: a request carrying **no `Accept` header at all** previously got `default_response_type` unchecked, which for a JSON-only router is the codegen-baked `application/cbor` — the same 406 by another route. It now falls back to the first encodable type instead. Routers whose default is genuinely encodable (every dual-codec router, and any single-codec router whose codec matches the default) negotiate exactly as before. RPC unary/batch dispatch and gRPC bridging funnel through the same `encode_transport_result_with_status_for`/`RPC_BINDING_CAPABILITIES` path, so they're fixed by the same change; `validate_subscribe_accept_header` (SSE `@@subscribe`) was audited and left alone since it always produces `text/event-stream` unconditionally — there's no static-list-vs-codec gap there to begin with.

### `ClientStateStore` moves out of `cratestack-client-rust` into `cratestack-core` (#475) (#482) — breaking

`cratestack-client-store-sqlite` and `cratestack-client-store-redis` are storage adapters, but both depended on `cratestack-client-rust` — an HTTP transport binding — for the sole reason that `ClientStateStore` (plus `PersistedClientState`, `RequestJournalEntry`, `InMemoryStateStore`, `JsonFileStateStore`) happened to be defined there: an L2 → L4 back-edge, the client-side twin of the `cratestack-sqlx`/`cratestack-redis` → `cratestack-axum` edge #465 fixed server-side and the violation `docs/design/layering.md` named as still open. The trait and its companion types move to `cratestack-core::store::client_state`, with `cratestack-client-rust::state` kept as a back-compat re-export so existing `use cratestack_client_rust::state::...` paths keep compiling. `cratestack-client-store-redis` no longer depends on `cratestack-client-rust` at all; `cratestack-client-store-sqlite` keeps it only as a `[dev-dependencies]` entry for its test fixtures — `cargo tree -p cratestack-client-store-sqlite -i cratestack-client-rust` now reports a dev-dependency path only, and the same command for `-store-redis` reports no path at all. **Breaking:** anyone implementing `ClientStateStore` directly against `cratestack_client_rust::ClientStateStore` (rather than the re-export) needs to retarget `cratestack_core::ClientStateStore`; the trait's shape is unchanged.

`CratestackClient::state()` and the internal `record_request` journal-write path convert the moved trait's `CoolError` back to `ClientError::State(..)` explicitly at both call sites, rather than through `ClientError`'s blanket `From<CoolError>` (which targets `ClientError::Codec`, for genuine wire-codec failures) — an initial version of this move routed state-store failures through that blanket conversion, which would have silently reclassified local state-store I/O failures (a locked/corrupt JSON file, a poisoned mutex) as fabricated HTTP-500 `RuntimeErrorCode::Codec` errors instead of `RuntimeErrorCode::State`, reaching as far as the Dart/Flutter FFI boundary. Regression tests (`client::core::tests::state_store_error_maps_to_client_error_state`, `client::headers::tests::record_request_state_store_error_maps_to_client_error_state`) exercise a rigged-to-fail state store and assert the resulting `ClientError` variant.

### CI, release tooling, and process fixes

Layer direction (ADR 0014) is now CI-enforced: `docs/adr/layers.toml` assigns every `cratestack-*` crate a layer, and `.ci/layer-direction-check.sh` reads the real `cargo metadata` dependency graph and fails on any `cratestack-*` → `cratestack-*` edge pointing at a higher layer, or on a crate under `crates/` missing from the manifest (#477).

The release pipeline now actually writes a changelog. Nothing did before — `prepare-release.yml` already walked the commit range to build its release-PR body, but discarded that output rather than persisting it, which is why this file had been caught up by hand-written backfill PRs instead. `.ci/changelog-seed.sh` seeds a `## X.Y.Z` section per release (grouped by conventional-commit type, marked with a TODO placeholder), `.ci/changelog-check.sh` fails CI on an unedited marker so a seed can't silently reach `main` as a raw commit list instead of prose, and `just changelog-seed VERSION` runs it locally (#479/#483). Everything from `v0.5.0` through `v0.7.8` — thirteen releases — was itself backfilled by hand in a dedicated pass, since it had gone undocumented (#478).

`@no_rate_limit` reached the generated `OpDescriptor` (`rate_limited_by_default: false`) and was covered by tests at every layer except the one that mattered: `cratestack-axum`'s `RateLimitLayer` never read the flag, so an annotated procedure was still throttled at runtime regardless. Fixed with an ops-filter wired into both REST and RPC dispatch (#474/#481).

`rust-version = "1.95.0"` is now declared in `[workspace.package]`, matching the existing `rust-toolchain.toml` pin, with a dedicated `msrv` CI job building the workspace on that exact toolchain and a three-way drift check (resolved `rustc --version`, the toolchain file, and `Cargo.toml`'s declared `rust-version`) added to the existing `check` job (#422/#480).

`AGENTS.md` now records that this repo's own `cratestack-sqlx`/`cratestack-pg`/`cratestack-cli` dependencies on `sqlx` are a deliberate exemption from the no-raw-SQL policy downstream consumers (webank-context ADR 0038) are adopting — this is the layer that wraps sqlx, not an instance of the drift that policy targets (#484).

`just bump`'s `cratestack-studio-ui` step changed directory with a bare `cd`, which leaked into every repo-root-relative path after it — including the standalone example-workspace lock-refresh steps `#422`/`#480` and later PRs added, which silently never ran as a result. Wrapped in a subshell like the steps around it (#494).

## 0.7.8 (2026-08-08)

### Rate-limit and idempotency layers stop trusting spoofable proxy headers (#416)

`cratestack-axum`'s idempotency and rate-limit layers previously fell back to a shared literal `"anonymous"` bucket whenever a request carried no `Authorization` header — weak, but at least not attacker-steerable. A first attempt at improving this replaced the fallback with a client-IP parsed from the `Forwarded`/`X-Forwarded-For` headers, which turned out to be worse: the crate has no trusted-proxy configuration to verify or strip those headers, so any caller reaching the service directly, or through a proxy that doesn't rewrite them, could mint a fresh rate-limit bucket per request or land in another caller's idempotency namespace just by setting an arbitrary header value.

The header-parsed fallback is replaced with axum's `ConnectInfo<SocketAddr>`, which reflects the actual accepted TCP socket and can't be spoofed by the client; when `ConnectInfo` isn't available the layers fall back to the original shared `"anonymous"` bucket rather than trusting an unverifiable header. This closes the header-spoofing hole, but not the underlying gap it was filed against (#416): no shipped example, including the flagship `server_basic.rs`, and no macro-generated wiring actually serves through `into_make_service_with_connect_info`, so in every default/documented deployment today, unauthenticated callers still collapse onto the shared bucket. #416 stays open; picking a config surface that guarantees `ConnectInfo` availability across every server-wiring path is left to the still-maintainer-blocked trusted-proxy design (#415).

### Storage traits move out of the HTTP crate; the layer model gets written down (#424, #472)

`IdempotencyStore`, `RateLimitStore`/`RateLimitConfig`/`RateLimitDecision`, and the idempotency-table DDL lived in `cratestack-axum`, which meant `cratestack-sqlx` and `cratestack-redis` depended on the HTTP transport crate solely to implement those traits — a back-edge against the intended `parser → core/policy/sql → macros → runtimes` direction. They move to `cratestack-core::store::{idempotency,ratelimit}` and `cratestack-sql::idempotency`, with re-exports kept in `cratestack-axum` for source compatibility; `cargo tree -i cratestack-axum` now returns no match from either `cratestack-sqlx` or `cratestack-redis` (#424).

A companion, docs-only change adds `docs/design/layering.md` and ADRs 0011–0016, naming six layers (L0 Schema IR through L5 Facades, plus the orthogonal compiler) and writing down the dependency-direction rule that `CLAUDE.md` previously expressed as a five-crate chain that no longer covers a thirty-crate workspace. Three ADRs are Accepted (the layer model itself, no IoC container, facade disjointness); three are Proposed, naming decisions still open for a maintainer call (#472).

### Feature graph: the default-features leak is closed, and the dead `decimal-bigdecimal` feature is gone (#421)

`cratestack-core` declared `default = ["decimal-rust-decimal"]`, but none of its ~27 internal dependency edges, nor the `cratestack-pg → cratestack-sqlx` / `cratestack-sqlite → cratestack-rusqlite` facade-to-runtime edges, set `default-features = false`, so that default was force-enabled workspace-wide regardless of what a consumer explicitly asked for. `default-features = false` is now set at the workspace-dependency site for `cratestack-core`, `cratestack-sqlx`, and `cratestack-rusqlite` (Cargo requires the override there, not per-member), with every plain `cratestack-core.workspace = true` edge re-enabling `decimal-rust-decimal` explicitly, since it's currently the only backend `cratestack-core` can compile with at all. Closing the leak also surfaced a real, previously-unreachable gap: `cratestack-sqlx`'s query-builder support code binds `cratestack_core::Decimal` unconditionally, so `cratestack-pg --no-default-features --features postgres` alone would have failed to compile without also forwarding `cratestack-sqlx/decimal-rust-decimal` — fixed in the same change.

Alongside this, the `decimal-bigdecimal` feature — reserved but never implemented, and an unconditional `compile_error!` if enabled — is removed rather than left as a no-op trap. A new `.ci/feature-matrix.sh`, wired into `just feature-matrix` and a CI job, checks every facade with its own decimal toggle (pg, sqlite, sql, sqlx, rusqlite, api, cli) under both its default and a narrowed `--no-default-features` selection, plus the wasm32-only backend paths. **Breaking** for anyone relying on the previous implicit default: a `--no-default-features` consumer of `cratestack-core`/`-sqlx`/`-rusqlite` must now request `decimal-rust-decimal` explicitly. This addresses part of #421 — removing rather than implementing an alternative backend means a consumer still can't select a non-default decimal backend, so the issue remains open.

### `cratestack-client-rust`: `reqwest::Error` no longer leaks through the public error type (#425) — breaking

`ClientError::Transport` previously wrapped `reqwest::Error` directly via `#[from]`, exposing a third-party error type in a public enum's match arm. `ClientError::Transport`/`RpcClientError::Transport` now wrap a new opaque `TransportError` instead, with `reqwest_error()`/`into_source()` accessors and `std::error::Error::source()` wired through so chain-walking still reaches the original `reqwest::Error`. `ClientError`, `RpcClientError`, and `OpKind` are now `#[non_exhaustive]`, so future variants don't break downstream exhaustive matches — `cratestack-client-flutter`'s conversion match needed a wildcard arm to keep compiling. `ExtensionKind` was deliberately left exhaustive: its own doc comment calls it "a closed list by design," and a first pass that added `#[non_exhaustive]` there was reverted after review, since it would have forced silent fallback arms into safety-critical internal matches (feature gating, DDL mapping).

### sqlx: unique-violation conflicts and the read-policy SQL contract

Single-row `create`/`update` operations that hit a unique-constraint violation returned a generic 500 instead of a 409 Conflict; fixed via a new `CoolError::ConflictTyped(DbErrorInfo)` variant that still carries the SQLSTATE and constraint name the existing `db_sqlstate()`/`db_constraint()` accessors depend on (#414). Separately, `render_read_policy_sql` now unconditionally wraps its output in a self-contained parenthesized group, matching the contract `push_action_policy_query` already committed to in 0.7.2 (#410) — described by its own commit as a latent-hazard fix rather than a live exploit, since both real call sites already wrapped defensively, but it closes the door on a future call site reintroducing an operator-precedence authorization bypass (#428).

### CI and test-infrastructure catch-up

A blocking `tests-redis` job now runs `cratestack-redis`'s test suite against a real Redis via testcontainers, mirroring the existing Postgres pattern with its own `CRATESTACK_REQUIRE_REDIS` guard (#418). CI also gained a `cargo check --target wasm32-unknown-unknown` step for `cratestack-sqlite`, a wasm32 build of the embedded-browser-vite example, and a `typescript-verify` job that generates and `tsc`-checks both REST and RPC TypeScript fixtures (#419).

`generated_routes_emit_tracing_events`, flaky enough to need a documented 3x CI retry, turned out to be a real bug: `init_tracing()` called `tracing::subscriber::set_default()`, which only installs a thread-local dispatcher on the one thread running the `std::sync::Once` closure, so every other worker thread spawned by `cargo test`'s multi-threaded harness fell back to `NoSubscriber` and silently dropped events. Switching to `set_global_default()` fixed it for real, and the CI retry loop is removed (#417). A trybuild fixture nominally testing malformed-policy diagnostics turned out to contain no `@@allow`/`@@deny` at all and is replaced with a genuinely malformed policy predicate (#420). Separately, the committed `examples/flutter-riverpod` client was regenerated after its templates changed in 0.7.5 but the fixture itself hadn't been, leaving `generate-dart --check` red on `main` since 2026-08-06 (#470).

### Docs: proposals for four decision-blocked issues (#469)

One design note each for #413, #415, #422, and #426 — confirmed defects that can't be implemented until a maintainer makes a call an agent has no standing to make. Docs only, no code changes.

## 0.7.7 (2026-08-08)

### `RequestAuthorizer::authorize` becomes async (#453) (#454) — breaking

`cratestack-client-rust`'s `RequestAuthorizer` trait had a synchronous `authorize` method, unusable for a real credential provider — an OAuth2 client-credentials token with a refresh-on-expiry cache, for instance, needs an HTTP call on a cache miss. The only workarounds were `block_on` (panics or deadlocks depending on the runtime) or pre-fetching and stashing a token, which reintroduces the expiry race the cache existed to avoid.

`authorize` is now `async fn`, via `#[async_trait]` rather than a bare AFIT, because both `CratestackClient::with_request_authorizer` and `CratestackGrpcClient::with_request_authorizer` store the authorizer behind `Arc<dyn RequestAuthorizer>` and native AFIT isn't object-safe — the same shape `cratestack_core::audit::AuditSink` already uses. **Breaking:** every implementor must change `authorize` to `async fn` and add `#[async_trait::async_trait]` to the impl block. This release updates every in-workspace implementor and the README's sample impl; external code implementing the trait needs the same change.

### TypeScript client: `Decimal` model fields now generate valid TypeScript (#456) (#455)

`ts_type()` in `cratestack-client-typescript` had no `Decimal` arm, so a model field typed `Decimal` fell through to the catch-all and was emitted verbatim as a TypeScript type name nothing declares — generation reported success, and the failure only surfaced later at `tsc` as `TS2304: Cannot find name 'Decimal'`, once per field. Fixed by mapping `Decimal` to `string`, matching the two sibling call sites already in the same crate. The new regression test asserts the emitted annotation itself and was verified against a consumer schema with three `Decimal` fields, which now passes `tsc --noEmit` with zero `TS2304` where it previously failed with six.

### Docs correction: half-landed-release recovery advice

The recovery guidance added in 0.7.6 (#450) claimed a half-landed release could be recovered by fixing the cause and re-running the failed jobs against the same tag. That's only true for a transient failure: every publish job checks out the release tag, so a fix merged to `main` afterward is absent from a re-run, and `workflow_dispatch` only rebuilds binaries — it never touches crates.io or npm. Recovering v0.7.5 hit exactly this, and was instead recovered by releasing v0.7.6; `docs/tooling/npm-publishing.md` now says so (#452).

## 0.7.6 (2026-08-07)

### Model responses no longer round-trip through `serde_json::Value` before the wire codec (#430, #449)

Every list/detail response row was projected through `serde_json::to_value` before the real wire codec (`JsonCodec`/`CborCodec`) touched it. `serde_json::Value` always reports itself as human-readable, so any field whose `Serialize` impl branches on that hint took the human-readable path unconditionally — for `Uuid`, that meant the generated Rust client's `Uuid::deserialize` ran its bytes-branch against a text string under the default CBOR wire format and failed on every model with a `Uuid` column. This was the reason `policy_db.rs::db_backed_policy_enforcement` had been `#[ignore]`d.

The fix introduces `cratestack_axum::ProjectedValue`, a format-preserving intermediate that keeps each scalar leaf behind a type-erased `erased_serde::Serialize` object instead of pre-serializing to JSON, deferring the human-readable decision to the actual target serializer chosen per request via content negotiation. Its `Null` variant calls `serialize_none()` directly, which also retires a documented workaround that stripped null map entries to dodge a separate `minicbor-serde` quirk — that old workaround had never been applied to nullable to-one relation `include`s, so this incidentally fixes a second, latent CBOR-null bug there too. Landing this also surfaced several of `db_backed_policy_enforcement`'s own latent bugs (id-reuse across seeded rows, unspecified tie-break ordering, a stale expectation, a wrong status code); those are fixed and the test now runs for real in CI.

### Required auth fields can no longer silently resolve to NULL (#431, #448)

A `@default(auth().field)` backed by a *required* field in the schema's `auth` block was, on a missing value in the actual auth context, silently written as NULL rather than rejected — a real policy bypass for tenant-scoping fields, since SQL's `NULL != X` evaluates to NULL, not true. `resolve_default_value()` now tracks the auth block's declared arity for the field via a new `auth_field_required` flag, and returns `CoolError::Validation` when a required auth field is absent, before policy evaluation runs. A follow-up commit fixed a regression the initial version introduced — the required-field check had jumped ahead of the existing anonymous-caller check, turning an unauthenticated request's expected 403 into a 422 — and adds no-DB unit coverage of all branches.

### Parser rejects `type`/`enum`/`model` names that collide once normalized (#429, #447)

Declarations of different kinds whose names collided only after `to_snake_case` normalization were previously accepted silently, even though `type`/`enum` land in the same generated `types` module and `model` in a `models` module, both re-exported at the parent scope — a real collision there generates conflicting Rust symbols. The fix reuses the `find_snake_case_collision` helper from 0.7.2 (#408) to reject the three kind-pairs that actually share generated symbols. A follow-up commit narrowed an over-eager first pass that also rejected `mixin` and `auth` against every other kind: neither a mixin's own name nor an auth block's name is ever emitted as a generated identifier, so reuse there is legitimate.

### CI: idempotent npm publishes, pinned `wasm-opt` for releases (#450)

The v0.7.5 release run half-landed: crates.io and two npm packages published at 0.7.5 while every other npm package was stranded at 0.7.4, and re-running the workflow couldn't recover it. Two causes: npm's retry to Sigstore's Rekor log can race its own already-landed write and get back a 409 that `sigstore-js` surfaces as fatal rather than benign; and `wasm-pack` only downloads its own pinned `binaryen` when no `wasm-opt` is already on `PATH`, leaving an unpinned network fetch on the release build's critical path. Neither publish job had re-run tolerance either — a bare `npm publish` fails outright on an already-published version. All five `npm publish` call sites now route through `.github/scripts/npm-publish.sh`, which retries the Sigstore 409 with backoff and treats "already published" as success; both the release and CI wasm jobs now pre-install a pinned `binaryen` onto `PATH` ahead of `wasm-pack`.

### Docs: three facades documented, vestigial studio-generator shim removed (#427, #446)

`CLAUDE.md`'s facade section is updated from describing two facades to all three (`cratestack-pg`, `cratestack-api`, `cratestack-sqlite`), and `crates/cratestack-studio-generator` — a one-line re-export of `cratestack-studio::eject` that no workspace member depended on — is deleted, along with its references from the root `Cargo.toml`, `README.md`, and CI.

## 0.7.5 (2026-08-06)

### Dart Riverpod preset: fix `flutter analyze` failures on no-model and paged-first-model schemas (#443, #444)

`generate-dart --preset riverpod`'s generated `test/<package>_test.dart` imported `flutter_riverpod`/`flutter_test` unconditionally, but the only code using them was gated on `override_proof`, which is `None` whenever the schema has no models (a `provider = "none"` procedures-only service) or its first model in schema order is paged. For that legitimate schema shape both imports went unused, and the generated package's own lint config enables `unused_import`, so `flutter analyze` failed unconditionally. The fix gates the `flutter_riverpod` import the same way the RPC template's `fast_immutable_collections` import already was, and replaces the top-level bare `assert(...)` query-parameter checks with a real executed `test(...)` case. Confirmed against a real no-model service: `flutter analyze` went from 2 `unused_import` warnings to 0. None of the existing riverpod-preset snapshot fixtures exercised this shape, which is how it went uncaught; the three affected snapshots were refreshed.

Workspace bumped to 0.7.5 (#445) — version-literal and lockfile updates only.

## 0.7.4 (2026-08-05)

### `cratestack-mock-wiremock`: WireMock stubs generated from schema procedures (#438, #439)

A new crate, `cratestack-mock-wiremock`, and a `cratestack generate-wiremock` CLI subcommand derive WireMock stub mappings directly from a `.cstack` schema's procedures, so integration/e2e tests can run against a mock backend whose wire contract cannot silently drift from the real one. v1 scope is deliberately narrow: happy-path stubs for `procedure`/`mutation procedure` under `transport rest`/`rpc`, matched on method and path only — model CRUD routes, `transport grpc`, error-case stubs, and auth emulation are deferred. The crate was validated end-to-end against a real 1900-line, 40-procedure schema, producing 40 correct mapping files and a clean `--check` rerun.

Two review findings landed before merge. The RPC-transport stub built its `urlPath` as `/rpc/<name>` instead of the actual `/rpc/procedure.<name>` the RPC dispatch generator emits — every RPC-transport stub would have silently never matched a real client's request. And the cycle guard for synthesizing stub payload values only checked for a direct repeat of the *same* type name, so a mutual cycle like `type A { b: B[] }` / `type B { a: A }` raised a false unbreakable-cycle error even though `{ "b": [] }` is a perfectly finite value. Both are fixed with regression tests.

### `cratestack-client-rust`: stop forcing `aws-lc-rs` onto every consumer (#440, #441)

0.7.3's reqwest dependency requested the `rustls` feature, which on reqwest 0.13 unconditionally selects `aws-lc-rs` as the TLS crypto provider. Because `cratestack-pg` depends on `cratestack-client-rust` unconditionally, this forced `aws-lc-rs` onto every workspace depending on `cratestack` at all — breaking a from-scratch musl/scratch build (`aws-lc-rs` needs a cross C toolchain; `ring` doesn't) and tripping any `cargo-deny` policy banning `aws-lc-rs`. The fix switches to reqwest's `rustls-no-provider` feature, which keeps the rustls-backed stack but drops the forced provider selection; because that feature panics at `Client::build()` time if no provider was installed, `CratestackClient::new` and Studio's `ApiSource::new` now install a `ring` fallback provider (idempotent, a no-op if a consumer already installed one). `cargo tree -i aws-lc-rs` now shows no match anywhere in the workspace.

Workspace bumped to 0.7.4 (#442) — no user-facing content beyond the version number.

## 0.7.3 (2026-08-05)

### `cratestack-client-rust`: unpin `reqwest` to 0.13, off the dead `rustls-tls` feature name (#435) (#436)

The workspace's `reqwest` entry requested `rustls-tls`, a 0.12-only feature name (0.13 renamed it to `rustls`/`rustls-no-provider`), so even though the bare version requirement looked 0.13-permissive, Cargo could only satisfy the edge with the newest 0.12.x release still carrying the old name. Any downstream workspace also depending on reqwest 0.13 directly ended up with two live, incompatible `reqwest` instances in one dependency graph — confirmed against a real downstream `Cargo.lock` — which silently defeated `CratestackClient::with_http_client`'s dependency-injection point, since a caller's 0.13-typed client didn't unify with this crate's 0.12-typed one.

The fix pins to `reqwest = "0.13"` with `rustls` (0.13's rename, which auto-installs the `aws-lc-rs` provider — later replaced in 0.7.4) plus the newly-required `query` feature, since 0.13 splits `RequestBuilder::query()` behind it and `cratestack-studio` calls it directly. Closes #435.

## 0.7.2 (2026-08-05)

### Extensions: a declarative surface for opt-in capabilities (epic #152 done)

`.cstack` schemas can now declare `extension rate_limit { }` / `extension pgvector { }` as a new top-level block, recorded on `Schema.declared_extensions` (#153). On its own this is declare-only, but it feeds a shared compile-time gate: all three entry macros check every declared extension against the compiling crate's own Cargo features and fail with a `compile_error!` naming the extension and the feature to enable, instead of silently doing nothing when declaration and feature disagree (#161). `include_embedded_schema!` also rejects `extension pgvector { }` unconditionally, since pgvector has no embedded equivalent.

`rate_limit` is the first extension built on that gate: a bare `@no_rate_limit` procedure attribute, valid only when the schema declares `extension rate_limit { }`, flips a procedure's `rate_limited_by_default` to `false` (#154) — deliberately narrower than the epic's own proposal, since `cratestack-axum`'s existing `RateLimitLayer`/`RateLimitConfig` stay unconditionally compiled, with no numeric config or store-selection changes.

### pgvector: vector columns, ANN indexes, and distance queries (#155, #156, #163)

`pgvector` goes from a declared name to a working scalar type across three phases. Phase 1 adds `Vector(n)` as a parametric scalar, emits `CREATE EXTENSION IF NOT EXISTS vector;` DDL, and wires `SqlValue::Vector`/`NullVector` through the sqlx encode/decode boundary behind a new `pgvector` Cargo feature; `include_embedded_schema!` rejects `Vector(n)` outright (#155). Phase 2 generalizes `@@index([...], using: ..., opclass: "...")` — a general-purpose model attribute, not pgvector-specific — so index DDL can request `ivfflat`/`hnsw` in place of the implicit btree, with existing `@unique`-derived indexes still rendering byte-identical DDL to before (#156). Finally, `FieldRef::distance_to(metric, query_vector)` gives `.asc()`/`.desc()` ordering and threshold filtering, with `VectorMetric::{L2,Cosine,InnerProduct}` mapping 1:1 to pgvector's operators (#163).

### Migration baselining: adopt an existing live database (epic #202 done)

`cratestack migrate` gains the ability to point at an already-running Postgres database and adopt it, closing the gap where `migrate diff` against a missing snapshot always diffed against an empty schema and emitted a full `CREATE TABLE` for tables that already existed. Phase A extracts the `Schema -> IR` projection step into a public `project()`/`Projections` seam, a pure refactor that Phase B plugs into (#203). Phase B adds `cratestack-migrate::introspect::postgres`, gated behind an opt-in `postgres-introspect` feature, which queries a live database's `information_schema`/`pg_catalog` state and produces the same `Projections` shape `project()` produces from a parsed schema — anything it can't map is reported as `UnmappedColumn` rather than guessed at (#204). Phase C wires both into `cratestack migrate baseline`: introspect, diff against the authored schema for a drift report (never a hard failure by default), write the introspected snapshot, and record a synthetic row in `cratestack_migrations` (#205).

**Breaking:** the migration snapshot format now stores `Projections` (the IR) instead of a `Schema`, bumping the on-disk snapshot format version from 1 to 2 — a baseline run has no `Schema` to write, and a drifted database's snapshot needs to reflect live reality rather than the aspirational schema.

### `@@subscribe`: SSE subscriptions for RPC transport (#183, #390)

A spike into whether the existing SSE streaming machinery could cover one-way `@@subscribe` model-event feeds — previously locked to a still-unimplemented WebSocket-only design — concluded yes: the cancellation objection that ruled out SSE for arbitrary streaming doesn't hold for a fire-and-forget, no-replay, one-subscription-per-connection feed (#183). `@@subscribe` — a bare model attribute requiring `@@emit(...)` and `transport rpc` — now emits `OpKind::Subscription`, dispatched at `GET /rpc/subscribe/{op_id}` through the existing outbox-drain pipeline. Backpressure is a bounded per-subscription channel that closes on overflow, surfaced as a terminal SSE error event (#390).

### gRPC: procedures and server-streaming (#208)

`procedure` declarations now reach the tonic gRPC service: unary procedures get a `UnaryService` method and list-arity procedures get a `ServerStreamingService` method, both dispatched through the same handler function — and therefore the same policy/audit pipeline — that REST and RPC already call.

### Correctness fixes: JSON columns, keyword-named fields, and route derivation

Both database backends persisted `Json`-typed columns through `cratestack_core::Value`'s own externally-tagged `Serialize`/`Deserialize` instead of plain JSON, so an empty map landed on disk as `{"Map": {}}` — breaking any read of jsonb the framework didn't write itself, and native `jsonb`/`->`/`->>` queries. Fixed on Postgres via a new `cratestack_sqlx::Json<T>` newtype (#162), then equivalently on the embedded rusqlite backend (#395).

A field named after a Rust keyword (`match`, `type`, `ref`, `move`, ...) emitted uncompilable code in every generated struct, decode impl, and client — fixed by funneling Rust-identifier emission through a shared `ident()` helper that emits raw-identifier form where one exists, and rejecting `self`/`Self`/`super`/`crate` at schema-parse time (#398).

The server's real Axum route derivation and the TypeScript/Dart client generators' route derivation were three independently-maintained algorithms that agreed on plain PascalCase names but diverged on any name containing a literal underscore, producing client routes the server never registered — unified onto a single `cratestack-core::route_naming` module (#345). Separately, the TypeScript `swr` preset's per-model file name could collide for two distinct, parser-valid model names, silently clobbering one file's output with the other's; generation now rejects that up front (#344).

### Parser and policy correctness

Field names were deduplicated on the raw `.cstack` name rather than `to_snake_case`, so two fields normalizing to the same SQL column compiled to valid Rust but emitted a table with a duplicate column and no error; and reserved-identifier rejection only ran at field call sites, so a colliding enum name failed later as an opaque parser error at the macro invocation. Both are now checked at every identifier site (#408).

`@allow(true)`/`@deny(true)` — a bare boolean literal as a procedure-level policy clause — failed to parse, falling through to field resolution and erroring as an unknown input field; a new `ProcedurePredicate::Literal(bool)` variant gives schema authors a direct way to mark a procedure public (#405, #406). Separately, `push_action_policy_query` wrapped its emitted SQL in parentheses only on its `@@deny`-present branch, leaving the other branch's boolean grouping dependent on the caller; both branches now wrap unconditionally. Fixing this also revived the `policy_db*` integration test suite, which had sat entirely `#[ignore]`d and run nowhere in CI (#410).

### Small fixes, CI, and release plumbing

`cratestack-cli` gains a working `--version`/`-V` flag (#201). `cargo deny check` is now a real gate — CI previously caught its non-zero exit, logged it as expected, and continued — with every existing license/advisory hit resolved on its merits (#409). `just bump` previously replaced every occurrence of the bare version literal across every `Cargo.toml`, which also rewrote unrelated third-party dependencies pinned to the same version number; the 0.7.1 → 0.7.2 bump itself broke this way, turning `serde_urlencoded = "0.7.1"` into a nonexistent `"0.7.2"`, and the replace is now scoped to actual `version =` keys (#432).

## 0.7.1 (2026-08-03)

A follow-up fix to 0.7.0's `FindMany<Model>`: `include_client_schema!` never generated the `PostFindManyInput`-style types the server composer did, so any schema using `FindMany<Model>` as a procedure argument failed Rust HTTP client generation with "cannot find type." Fixed by splitting the shared type generation out of the server-only query-builder wrapper, with a new regression test proving the wire format round-trips through the client-generated types, not just that the macro compiles (#381).

## 0.7.0 (2026-08-03)

### `FindMany<Model>`: built-in search-with-filters procedure argument (#371)

Procedures gain a built-in generic argument type for search-with-filters, following up on `PageInput` (0.6.7). A procedure can now declare `searchPosts(query: FindMany<Post>, page: PageInput): Page<Post>` — filtering/sorting and pagination stay two independent, orthogonal arguments. It's restricted to procedure-argument position, and `Model` must be a declared model rather than a `type` block, since filtering needs a real table's columns to validate field names against.

The shape went through a real redesign mid-implementation: the first cut reused the existing `list` route's flat string-DSL (`{ where: String?, orderBy: String? }`). That was replaced before release with structured, per-model typed filters across all three generators — Rust server (`PostWhere`/`PostSortField`/`PostFindManyInput`, built on a shared `FieldFilterInput<V>`), TypeScript, and Dart (default/riverpod) — since a caller-facing query language is worth getting typed once rather than passing through a string. `orderBy` is a `Vec<OrderByClause>` rather than a single object, since neither `serde_json::Map` nor JS object key order is guaranteed to preserve multi-key sort order.

Server-side codegen adds one `build_<model>_query_from_find_many` function per model, reusing the model's own already-generated list-route filter/sort machinery, so a `FindMany<Post>` argument validates against exactly the same allowed fields a REST `?where=` on `/posts` already does. The client-side `FindMany` type is deliberately non-generic across Rust/TypeScript/Dart, since the wire shape never depends on the model. Two real bugs surfaced only by running generated output through real tooling: `SearchPostsArgs` decoding via a now-nonexistent bare `FindMany.fromWire`, and a generated `models/post.dart` missing its `shared_types.dart` import.

Also, `cratestack-sqlite`'s README now documents the `codec-json` feature, which had gone undocumented since 0.6.8.

## 0.6.8 (2026-08-03)

Release pipeline and dependency-maintenance patch, no framework or generated-code changes. `release-cli.yml`'s five `publish-npm-*` jobs pinned `npm@^11` instead of always installing latest, after npm 12 changed `npm pack --dry-run --json`'s output shape and broke `@napi-rs/cli`'s pack detection (#369); `prepare-release.yml`'s Node version was bumped from 20 to 24 to match `ci.yml`, after an `undici`/Node version mismatch broke `swr_hooks_invalidation`'s vitest run on that job specifically (#377).

TypeScript, vitest, biome (1→2), turbo, and the vscode extension's dependencies move forward across the pnpm workspace, along with client codegen templates and example projects — each bump verified with a real build/typecheck/test run; two pins were deliberately held back after checking against the real toolchain (Dart `riverpod`'s analyzer ceiling, `embedded-browser-webpack`'s TypeScript pin against `ts-loader` 9.6.2). Also fixes #358: the `riverpod` preset's generated `build_runner` cap was `<2.15.0`, but the actual break is in 2.15.2; the cap is now `<2.15.2` (#364). A prior wasm32 import fix to `embedded-browser-vite`'s `mod wasm` block had never been copied to three sibling example crates carrying the identical block, so all three were silently failing to build for wasm32 (#373). Five further commits bring READMEs, example indexes, and CLI docs back in sync with shipped code, following an audit that found version pins as stale as 0.2.2 (#372–#376).

## 0.6.7 (2026-08-03)

### Embedded backend gets real pagination; new built-in `PageInput` (#363, #366)

`@@paged` shapes a generated `list` route's response envelope on REST/RPC/gRPC, but `include_embedded_schema!` generates no routes at all, so a `@@paged` model there previously just compiled to nothing, silently. The first fix attempt rejected `@@paged` on embedded schemas outright with a `compile_error!`, mirroring the existing `@@materialized`-on-embedded guard — but per the maintainer's pushback on that approach ("this is our software, what's blocking us?", #366), rejection papered over a gap that `cratestack-rusqlite` already had the pieces to close.

`FindMany` (on both models and views) now has `.paginate(PageInput) -> Page<M>` and `.paginate_in_tx`, backed by a new `render_count` and a real `COUNT(*)` run inside the same connection borrow as the paginated `SELECT`, so the count and the page it describes can't be split by a concurrent write. It's available unconditionally on every model, the same "no attribute wiring needed" treatment `@@audit`/`@@emit` already get.

Alongside this, a built-in `PageInput` procedure-argument type (`{ limit: Int?, offset: Int? }`) fills a gap on the request side that `Page<T>`/`PageInfo` already covered on the response side. `PageInput::resolve(max_limit)` applies the same `MAX_LIST_LIMIT` clamp rule generated `list` routes already use, and is wired through the Rust server and the Rust/TypeScript/Dart clients. gRPC's existing `@@paged`-independent behavior was confirmed already correct and left unchanged.

### Release and CI plumbing

Three small fixes: the `publish-npm-cbor-node` release job failed `tsc` on its first real OIDC publish attempt because `napi artifacts` only copies `.node` binaries, not `native.mjs`/`native.d.mts`, and the job never built `@cratestack/ts-types` as its own step (#362). The same PR also fixed `prepare-release.yml`'s `git add` list, the root cause of 0.6.6's bump PR landing with the lockfile bumped but the cbor family's `package.json`s left stale.

A new `install-cratestack-cli` composite GitHub Action downloads a prebuilt `cratestack-cli` binary for the runner's OS/arch, verifies its SHA-256, and adds it to `PATH` with no Rust toolchain required (#365). Getting it working against the real 0.6.6 release surfaced two platform-specific bugs in the same PR: a `grep -m1` piped from `curl` under `set -o pipefail` could SIGPIPE-abort the script, fixed by buffering and parsing with `jq`; and Windows' Git Bash `tar` can't read `.zip` archives, fixed by branching to PowerShell's `Expand-Archive` on Windows.

Finally, `examples/react-vite-daisyui`'s `tsconfig.json` was missing `allowImportingTsExtensions`, so `npm run typecheck` failed with TS5097 on its own `.ts`/`.tsx`-suffixed sibling imports (#367).

## 0.6.6 (2026-08-03)

Release-and-CI plumbing only, hardening the `@cratestack/cbor-node` npm publish pipeline: three fixes land back-to-back, working through the still-unproven release path one failure at a time. The Windows leg of the 0.6.5 release failed because the napi build step's multi-line `run:` used `\` line continuations, fine under bash but parsed by PowerShell as a unary `--` operator; `shell: bash` is now pinned on that step (#356). That surfaced a chain of pipeline gaps that had never been exercised end-to-end — `build-cbor-node`'s job gate excluded `workflow_dispatch`, `publish-npm-cbor-node` was missing the `napi create-npm-dirs`/`napi artifacts` scaffolding steps its own `prepublishOnly` hook depends on, and its artifact download switched from a flat layout to per-platform subdirectories to match how `napi artifacts` matches `.node` files to targets. Separately, `cratestack-cbor`/`-cbor-node`/`-cbor-web`'s `package.json` versions had been stuck at 0.5.2, which — because pnpm's `link-workspace-packages=true` only symlinks a workspace dependency when the pinned semver matches — silently resolved their `@cratestack/ts-types` dependency to the real published 0.5.2 package instead of local workspace source, so all three packages had quietly been building against three-versions-stale types (#357).

The same lockstep-version gap then broke the 0.6.6 bump itself: `prepare-release.yml`'s bump-PR `git add` list still only staged the original 11 api-family `package.json` files, so `just bump` wrote the cbor version bump to disk but git never committed it, breaking every JS CI job with `ERR_PNPM_OUTDATED_LOCKFILE`. Fixed by adding the cbor family to this workflow's own `git add` list (#360). No framework or generated-code behavior changed in this release.

## 0.6.5 (2026-08-03)

### `cratestack-api`: a third facade, and `db = None` genuinely drops `sqlx` (epic #326 done)

Epic #326's last story lands: `cratestack-pg` gains a default-on `postgres` Cargo feature gating `sqlx`/`cratestack-sqlx`, so a `db = None`-only consumer can `default-features = false` and have `sqlx` genuinely absent from `cargo tree`, not just unused. `rpc-procedures`, `rpc-batch`, `rpc-streaming`, and `rpc-batch-debounce` move off their old `connect_lazy(&url)` workaround onto real `datasource { provider = "none" }` + `db = None` schemas (#329).

A direct follow-up (#347, landed as #350) goes further: `crates/cratestack-api` is a new, fully separate third facade — following `cratestack-pg`'s and `cratestack-sqlite`'s exact structural pattern — that never depends on `cratestack-sqlx` under any feature. A new compile-time guard, `guard_server_postgres_backend`, turns `db = Postgres` under this sqlx-less facade into one clear `compile_error!` instead of a wall of unrelated resolution errors. `examples/no-database-verification-api` proves the absence with a real `cargo tree` check, and all four `db = None` examples migrate onto the new crate; `cratestack-pg` + `default-features = false` keeps working as the pre-existing alternative path.

### Native Rust gRPC client for `transport grpc` schemas (#209)

`include_client_schema!` now generates a typed, tonic-based Rust client for `transport grpc` schemas — one method per model CRUD verb, matching the surface the server runtime and the gRPC-Web TypeScript client already expose. The compile-time guard that unconditionally rejected client-side gRPC codegen splits into `guard_client_grpc_transport` (now feature-gated) and `guard_embedded_grpc_transport` (still an unconditional reject). `cratestack-client-rust` gains an optional `grpc` feature and a `CratestackGrpcClient<T>` runtime with its own `RequestAuthorizer`/schema-sha handling and a deliberate, documented reimplementation of `cratestack-grpc`'s canonicalization, kept byte-identical to the server side without pulling axum/tonic-web into client-only binaries.

### TypeScript client: query builders and Node ESM correctness

RPC transport's generated list/`use` hooks previously took only an untyped `Record<string, unknown>`. A new `CratestackRpcListQuery`/`toRpcListInput` pair — the RPC counterpart of REST's existing query-builder pair — gives it the same typed shape (#333, landed as #352).

More seriously: `CratestackFetchQuery` typed `where`/`filters`/`orFilters` as JSON-ish objects, and its fallback `JSON.stringify()`'d them into the URL — but the real server grammar (`FilterExpressionParser` in `cratestack-axum`) is a flat-text DSL, not JSON, so any caller populating these fields as documented got a hard 400 from a real server, with zero test coverage catching it. Fixed to mirror the Dart client's convention: `where`/`or` are now pre-built DSL strings, and `filters` is a flat `Record<string, string>`. **This is a breaking change to `CratestackFetchQuery`'s public shape**, fixed directly per this repo's hard-cutover convention (#351). A new test runs the generated client under real Node and feeds the captured request URL through the real `cratestack-axum` parser.

A third fix: every relative import/export in the TypeScript templates was extensionless, which resolves under a bundler but fails under plain Node's native ESM resolver with `ERR_MODULE_NOT_FOUND` — fixed at the template level and in the `swr` preset's dynamically-assembled cross-file imports (#315, landed as #343).

### Dart client: riverpod query forwarding and analyzer cleanliness

REST's generated list/get `@riverpod` providers now accept optional query objects and forward them to the underlying API calls, instead of always calling with zero arguments. This required hand-rolled `operator ==`/`hashCode` on those query classes — Riverpod's family providers dedupe by argument *value* equality, and a freshly-constructed-but-equal query previously never hit the cache. RPC's list provider forwards an untyped `IMap<String, Object?>` bag rather than a typed query builder, a documented decision to expose the existing untyped RPC contract now rather than design a full typed one (#331, landed as #349). Two pre-existing `flutter analyze` info-level findings are also fixed across every generated package, bringing default-severity `flutter analyze` to zero issues everywhere (#308, landed as #346).

### FIPS crypto: false success made impossible

`install_fips_crypto_provider()` returned `Ok(())` without installing any crypto provider, and the `aws-lc-rs` feature it gates enabled nothing — a false assurance in a compliance-facing API. Wiring a real provider needs the TLS backend to become a genuine per-crate choice first, which is out of scope here; until that lands, enabling the feature is now a hard `compile_error!` instead of a silent no-op (#334, landed as #341).

### Plumbing

The `@cratestack/cbor`/`cbor-node`/`cbor-web` family (shipped earlier but never released to npm) gets its release wiring: four new jobs in `release-cli.yml` build and publish the napi-rs native addon, the wasm-bindgen browser build, and the pure-TS umbrella in dependency order (#342). The gRPC e2e test added alongside the new Rust client was breaking every default `cargo test --workspace` run because nothing started its target server first; it now skips quietly on connection failure (#353). CHANGELOG.md also gained its 0.5.0–0.6.4 backfill in this range (#339).

## 0.6.4 (2026-08-02)

### Dart Riverpod preset: build_runner integration + example app (epic #297 done)

`generate-dart` gains an opt-in `--run-build-runner` flag that shells out to
`dart run build_runner build --delete-conflicting-outputs` after
generation, with a clear "no Dart SDK found on `PATH`" error (naming the
manual fallback command) rather than a panic or a silent no-op when the
tool isn't there. A real Flutter app, `examples/flutter-riverpod`, consumes
a `--preset riverpod` client with zero hand-written providers — the
epic's own success metric — overriding the adapter provider to point at a
real local server. This is the fourth and final story of epic #297,
closing it (#303).

### `datasource none`: procedures-only servers without a database (epic #326, in progress)

`.cstack` schemas can now declare `datasource { provider = "none" }`,
rejecting any `model` block in the same schema, and `include_server_schema!`
cross-checks this against its own `db` argument for the first time — until
now `db = Postgres` was the macro's only accepted value and the argument
was silently discarded rather than checked against anything. `db = None`
then generates a genuinely `PgPool`-free `Cratestack`/router — not an
unused parameter or an always-`None` `Option<PgPool>`, a structurally
different generated type, with `ModelRouterState` and the event module
omitted entirely rather than compiled in as dead code. A real integration
test round-trips a procedure call over HTTP with zero `sqlx` import
anywhere in its own setup (#327, #335). `sqlx` Cargo-feature-gating and
migrating the framework's own examples off their `connect_lazy` workaround
are tracked separately and still open (#329).

### Dart Riverpod preset: real equality for generated data classes

Every `riverpod`-preset generated Dart data class (models, `Create<M>Input`/
`Update<M>Input`, procedure argument wrapper types) now gets real
`operator ==`/`hashCode`/`copyWith` via `dart_mappable`. Without this, a
`@riverpod` "family" provider taking a generated class as its argument
never settled — Riverpod dedupes family providers by argument *value*
equality, and a freshly-constructed instance on every rebuild (an entirely
ordinary pattern) never matched a prior instance by identity, so the
provider restarted `AsyncLoading` forever. Reproduced live against a real
server before the fix. Relation-list fields also switched to
`fast_immutable_collections`' `IList<T>` in place of `List<T>` (#325, #336).

## 0.6.3 (2026-08-02)

Small follow-ups to the two client-preset epics landed in 0.6.1:

* One `@riverpod` provider per operation for the Dart `riverpod` preset —
  parameterized `Future` providers for reads, `AsyncNotifier` controllers
  for writes — built by watching the preset's existing per-model DI layer
  rather than reconstructing adapter access from scratch (#302).
* TypeScript `swr` preset: a `@@paged` model's generated file was missing
  its `Page`/`PageInfo` import, a real `tsc` failure (#318).
* TypeScript REST client: widened the `SCHEMA_SHA256` constant's type to
  `string` — with a real, non-empty schema hash baked in, TypeScript
  inferred a literal type and flagged the runtime's own `
  ""` check as
  having no possible overlap (#323).

## 0.6.1 (2026-08-02)

### TypeScript SWR preset

A second, opinionated TypeScript client preset, `--preset swr`: one file
per model with plain, framework-free async functions underneath and a
`useSWR`/`useSWRMutation` hook per operation on top, so the functions stay
usable from a script, a server action, or a test with zero React/SWR in
the import graph. Cache invalidation follows an explicit, documented rule
(create invalidates the list; update invalidates the list and the detail;
delete invalidates the list and drops the detail) proven by a real test
asserting exact refetch counts, not just that the code compiles. A real
end-to-end example app, `examples/react-vite-swr`, demonstrates it against
a live server with browser-observed cascading invalidation (#304, #305,
#320). Also fixes a real duplicate-key collision in the *default*
react-query preset's key object, surfaced while building the example
(#319).

### Dart Riverpod preset (started)

First story of a new, parallel Dart preset: `--preset riverpod` fans
generated output out to one file per model instead of a single monolithic
`models.dart`/`apis.dart` (#301). Also closes a real, pre-existing gap —
nothing in CI had ever run generated Dart through an actual Dart/Flutter
toolchain before this; the existing snapshot tests only assert on
generated *text*, which can't catch a missing import or an undefined
symbol (#300).

## 0.6.0 (2026-08-02)

### `@cratestack/api` split into a 9-package family

`@cratestack/api` is split into `ts-types`, `link-batch`, `link-logger`,
`runtime-fetch`, `runtime-axios`, `validator-zod`, `validator-yup`,
`adapter-tanstack-query`, and `adapter-rtk`, so a client that only needs
types isn't forced to ship batching/logging/HTTP-adapter code it never
calls. `@cratestack/api` itself becomes a backward-compatible re-export
shim over the new packages (#265). A follow-up fixes `link-batch`
silently dropping per-call headers/fetch overrides/codec choice when
partitioning a batch — flushes are now grouped by transport signature
instead of merged blindly (#273).

### RPC streaming: genuine incremental delivery + a first-party CBOR codec family

`@stream`-marked procedures now stream for real: the server encodes and
flushes each item onto the HTTP body as it's produced instead of
buffering the whole sequence before the first byte goes out, with a
CBOR-tagged sentinel (tag 48900) as the final item on a mid-stream
failure (#292, #294). The original design's mid-stream error mechanism (a
trailing content-type chunk) turned out to be physically unrealizable
over HTTP/browser `fetch` and was corrected before implementation, not
after (#289). The generated TypeScript RPC client gets a matching
`RpcStreamLink` chain and a hand-rolled CBOR-seq boundary scanner, tested
against real wire bytes captured from the generated server rather than
hand-built fixtures (#277, #299).

Alongside this, a new first-party CBOR codec family —
`@cratestack/cbor-node` (napi-rs), `@cratestack/cbor-web` (wasm-bindgen),
and `@cratestack/cbor` (an umbrella package with conditional `node`/
`browser`/default exports) — wraps the existing Rust `cratestack-codec-cbor`
crate for both native Node and browser targets (#286, #287, #288, #291,
#293).

### Migrations: foreign keys, `onDelete`/`onUpdate`, unique indexes

`@relation` fields now emit real `FOREIGN KEY` constraints (#260, #261),
with `onDelete`/`onUpdate` actions declarable in the schema (#268), and
model-level `@@unique([...])` now emits a real `CREATE UNIQUE INDEX`
(#266).

### Other fixes

* Two `sqlx` fixes preserve SQLSTATE/constraint-name classification on
  generated write queries and batch write queries, instead of collapsing
  every constraint violation into a generic error.
* Migration/DDL SQL is no longer split on literal `;` characters, which
  broke on any statement containing one inside a string or comment.
* Two macro-side fixes replace an exponential REST `orderBy` match-arm
  enumeration with runtime relation-hop resolution (#279), and drop a
  vestigial `Result` that was masking a real SQL-correctness gap (#280).

## 0.5.2 (2026-08-02)

Infra only: `npm publish` switched from long-lived tokens to OIDC trusted
publishing — no user-facing change (#221).

## 0.5.1 (2026-08-01)

Test-only: adds a relation-connectivity regression fixture closing a gap
left by the exponential-relation-codegen fix in 0.5.0 — no user-facing
change (#257).

## 0.5.0 (2026-08-01)

**Breaking:** relation codegen for models with many interrelated `@relation`
fields was exponential in the number of relations, making some real-world
schemas fail to compile at all. Fixed to be linear (#253). Also drops
stale version pins from example crates' path dependencies (#254).

## 0.4.18 (2026-07-31)

### Studio: Postgres row-keying fix, persistent audit log, EXPLAIN

`Row` is documented as keyed by `.cstack` field name, and the UI, cursor
pagination, relation-follow, and audit log all rely on that contract — but
the Postgres data source keyed rows by raw snake_case column name instead.
camelCase and snake_case coincide for single-word fields, which is why this
went unnoticed; on a realistic schema, every multi-word field silently broke
table rendering, pagination's "Next" button, relation follow, and the audit
log's recorded PK. Fixed by aliasing each projected column to its field name.

Also new: an opt-in persistent audit log (`[workspace] audit_file`, an
append-only JSONL sidecar replayed on boot, replacing in-memory-only
history), and query plans (`GET .../sql?explain=true` plus an "Explain"
toggle in the Studio UI). (#240)

### Studio: edit form no longer corrupts NULL columns on save

Opening a row with a NULL nullable column, clicking Edit, and clicking Save
without changing anything wrote the literal string `"—"` (the read-only
table's display placeholder for NULL) into that column instead of leaving it
NULL. The edit-form snapshot was reusing the display-formatting helper to
seed the editable form; it now maps NULL to the same "no value" sentinel
every editor widget already uses, matching what the save path already
expects. (#242)

## 0.4.17 (2026-07-30)

### Parser and migrate hardening around storage-type edge cases

A cluster of related fixes tightening what the parser accepts and what
`cratestack-migrate` emits, found while generating a round-trip test for
every builtin scalar/enum across Postgres, SQLite, and the LSP (#232, #237):

* Postgres now stores enums as `TEXT` + `CHECK` (not a native `CREATE TYPE
  ... AS ENUM`), and bareword enum defaults are quoted correctly in the
  emitted DDL (#233).
* `type` blocks can no longer be used as a model field's storage type —
  they're a payload shape for procedures, not a column type (#235).
* List-arity scalar/enum model fields are rejected on datasource-backed
  schemas, since there's no portable column type for "array of enum" across
  both backends (#229, via #236).
* Reconciled `#233`'s enum-list emitter test with `#229`/`#236`'s new
  list-arity parser rejection — the two landed close together and briefly
  disagreed on enum-list fields (#238).
* `Json` now derives `Default`, fixing a compile failure under
  `include_embedded_schema!` for models with a default-valued `Json` field
  (#234).

### Other fixes

* Rate-limit store errors are logged instead of failing the request
  silently (#215).
* A CI-only quality pipeline (informal replacement for a paid SonarQube
  instance) landed across several follow-up PRs — pinned-action scanners,
  PR review-comment output instead of Check annotations, and a documented
  gap-until-landed note for interim coverage (#216, #218, #220, #222, #225).

### Dart: native gRPC client generator

`generate-dart` gains a native gRPC client generator for schemas declaring
`transport grpc`, plus channel-shutdown and per-call option exposure on the
generated client, and gRPC-specific example/test templates (a pre-existing
RPC-transport example/test bug was caught and fixed during review) (#210,
via #211, #213, #214).

## 0.4.16 (2026-07-26)

No code changes. A clean recut of the release pipeline after v0.4.14 (which
shipped GitHub-Release-only by deliberate choice) and v0.4.15 (crates.io +
GitHub Release succeeded, but both npm publishes failed with `EOTP` — the
configured `NPM_TOKEN` wasn't an Automation-type token). v0.4.16 is the
first release to publish successfully to crates.io, npm (`@cratestack/cli`
and `@cratestack/api`), and GitHub Release binaries in one shot, with zero
manual publish steps.

## 0.4.15 (2026-07-26)

`cut-release-tag.yml`'s tag push now uses a dedicated `RELEASE_PAT` instead
of the default `GITHUB_TOKEN` (#197). GitHub's anti-recursion protection
silently no-ops any downstream workflow trigger from a push made with the
default token — the tag itself lands fine, but `release-cli.yml` never
fires off it. A PAT-authored push is treated as a normal external push and
correctly cascades into the rest of the pipeline.

## 0.4.14 (2026-07-26)

### Protobuf + gRPC support

`.cstack` schemas can now declare `transport grpc`, generating `.proto`
message/enum definitions (with a field-number lockfile so wire numbers
don't silently renumber across schema edits) and gRPC service surfaces.
Design doc (#166) and implementation (across #168–#172) landed same-day
(#167, #176). CRUD-only for this release — procedure/streaming support and
a Rust gRPC client were carved out as follow-up tickets.

### Schema-fingerprint drift header

Every response now carries an `x-cratestack-schema-sha` header — a
warn-only fingerprint of the server's schema, so a client running against
a stale generated SDK can detect drift without a hard version pin. Shipped
for the Rust server first (#179), then Dart and TypeScript REST/RPC clients
(#180).

### RPC client DX: composable link chain

The generated TypeScript RPC client gains a composable `RpcLink` chain
(request/response middleware — logging, batching, auth injection, etc.),
published alongside a new `@cratestack/api` npm package carrying the
batching link and other cross-cutting concerns out of the generated code
itself (#182, #186).

### CI-driven release pipeline

The first version of the fully automated release flow: a `prepare-release`
workflow bumps versions and opens a PR, merging it auto-tags via
`cut-release-tag.yml`, and the tag push triggers `release-cli.yml` to
publish crates.io + npm + GitHub Release binaries with no manual steps
(#188). Landed rough — this version alone needed eight follow-up fixes to
get a real dry run and then a real dispatch through the pipeline end to
end: missing GTK/WebKit deps in CI (#189), the release-check test stage
needing a bundled Studio UI first (#190), `cargo publish --dry-run`
needing `--allow-dirty` (#191) and `--no-verify` (#192) in dry mode, dry
mode needing to skip non-leaf crates entirely since a never-published
version can't resolve as a dependency (#193), and two npm `pnpm install`
call sites needing to skip the `cratestack-cli` binary download since
neither actually needs it (#194, #196). (The pipeline's tag-push
anti-recursion bug that blocked this version's own crates.io/npm publish
is the separate v0.4.15 fix above.)

### Other fixes

* `Cuid` scalar validation relaxed to accept `cuid2` ids, not just the
  original `cuid` format (#150, via #158).
* `cratestack-redis` gains a `tls-rustls` feature for `rediss://`
  connections (#151, via #159), and later in this same version switches
  to caching and reusing a single connection instead of opening one per
  call (#175, decision recorded in #177).
* Design doc proposing an `Extensions` concept, reframing the rate-limiting
  half of #139's declarative-surface decision (#160).
* Clippy `too_many_arguments`/`type_complexity` cleanup in `cratestack-sql`
  and `cratestack-sqlx` (#184, #185).

## 0.4.13 (2026-07-22)

A dense release — nine PRs, several the direct result of a full backlog
pass over long-open tickets:

* **`--check` drift-detection mode** for `generate-typescript` /
  `generate-dart`: exits non-zero if generated output would differ from
  what's on disk, for CI gates (#141).
* **Prebuilt `cratestack-cli` binaries** — GitHub Releases, `cargo-binstall`
  support, and an npm-installable wrapper, so installing the CLI no longer
  requires a Rust toolchain (#142).
* **`--full-selection` flag** for `generate-typescript`, emitting a fully-
  required model type alongside the normal partial-selection type (#140).
* **`cratestack diff`** — a new CLI subcommand that diffs two `.cstack`
  schemas and classifies each change by its effect on the generated wire
  contract (breaking / additive / internal-only), exiting non-zero on any
  breaking change so it can gate CI on schema PRs (#144).
* **Migrate baselining design spike** — a doc-only PR spiking Postgres
  live-schema introspection for baselining an existing database against a
  `.cstack` schema, not yet implemented (#135, via #143).
* **Composite primary keys** via `@@id([...])` — parser and
  `cratestack-migrate` DDL support landed; query builders, clients, and
  policy integration are follow-up work (#145).
* **Idempotency/rate-limiting declarative-surface decision** — a design
  doc settling that rate-limiting stays an imperative, hand-wired concern
  permanently, while idempotency is deferred pending an `OpExecutor` gate
  (#139, via #146).
* **`dbgenerated()` fix** — emits valid SQL instead of a broken default
  expression, and warns when the expression can't be verified against the
  target dialect (#148).
* **Type-block field-reference fix** — qualifies a `type` block's
  references to model types correctly instead of emitting an ambiguous
  reference (#137, via #147).

## 0.4.12 (2026-07-22)

The generated TypeScript RPC client runtime now satisfies its own
`exactOptionalPropertyTypes` compiler setting — a previous release enabled
the stricter TS option in the generated code but the runtime itself wasn't
compliant, so consumers with the same setting on saw type errors (#129).

## 0.4.11 (2026-07-22)

* Fixed `Page<T>`/`PageInfo`'s generated TypeScript shape not matching
  what the wire actually sends (#124).
* Capped the `list` route's page-size limit consistently across REST and
  RPC transports, and made the RPC codec pluggable rather than hardcoded
  (#126, closing #123 and #125).

## 0.4.10 (2026-07-22)

A round of audit-driven correctness fixes: a self-deadlock in the audit
path, a wrong soft-delete snapshot, a server-only field leaking into the
generated TypeScript client, and incorrect gating on TypeScript's generated
`create` calls (#120) — plus a fix for cross-binary test table-name
collisions inside `cratestack-pg`'s own test suite (#121).

## 0.4.9 (2026-06-17)

* Dart's CBOR decoder now normalizes decoded maps to `Map<String,
  Object?>` instead of a more loosely-typed map shape (#115).
* Fixed the `sqlite_offline_first` example failing to compile standalone,
  and guarded the embedded examples in CI (#106).

## 0.4.8 (2026-06-15)

Studio UI chrome revamp: reworked visual chrome and a multi-`.cstack`
target switcher, so one running Studio instance can browse several
schemas' targets from the same UI (#105). The repo also adopted an
AI-governance kit for issue/PR templates and contribution process around
this time (#104).

## 0.4.7 (2026-06-08)

For schemas using `transport rpc`, the op id is now the canonical request
identity — the value request signing and tracing key off, rather than an
incidental routing detail (#102).

## 0.4.6 (2026-06-07)

Fixed `BatchableCall` mis-encoding `None` optionals as a CBOR empty array
instead of a CBOR null in the Rust client (#100).

## 0.4.4 (2026-05-20)

* Published a documentation-only `cratestack` landing crate to crates.io
  — after the umbrella-facade split below removed the real `cratestack`
  crate, this keeps the name from going orphaned/squattable and points
  visitors at `cratestack-pg` / `cratestack-sqlite` (#97, doctests
  disabled on it in a same-day follow-up, #98).
* `CoolError` now preserves the full typed `DatabaseError` chain instead
  of flattening it, so callers can match on the underlying driver error
  (#99).

## 0.4.3 (2026-05-19)

Follow-up to the facade split below: fixed generator-fixture test paths
that still pointed at the removed `cratestack` umbrella instead of
`cratestack-pg` (#96).

## 0.4.2 (2026-05-19)

### Breaking: the `cratestack` umbrella facade was split

The single `cratestack` umbrella crate is gone. It has been carved into
two strictly disjoint sub-facades that consumers pick between via
Cargo's `package =` rename:

```toml
# Backend service (Postgres + Axum + generated Rust client runtime)
cratestack = { package = "cratestack-pg", version = "0.4" }

# Embedded / mobile / desktop / wasm (rusqlite + shared surface)
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

Schema macros (`include_server_schema!`, `include_embedded_schema!`,
`include_client_schema!`) continue to emit `::cratestack::*` paths
unchanged. Strict disjointness is enforced by what the consumer picks,
not by the macro.

**Why this matters in practice:**

* `cratestack-pg` does not pull in `cratestack-rusqlite`, so
  `libsqlite3-sys` is no longer in the dep graph. Backend services can
  now depend on the official `sqlx` umbrella crate (which optionally
  declares `sqlx-sqlite`) without tripping Cargo's `links = "sqlite3"`
  collision rule. Downstream `sqlx-shim` workarounds can be deleted.
* `cratestack-sqlite` keeps compiling on `wasm32-unknown-unknown`; it
  also exposes `cratestack-client-rust` on native targets so hybrid
  consumers (e.g. a Tauri or NAPI shell that ships an embedded DB
  *and* calls a remote backend) can still use `include_client_schema!`
  alongside `include_embedded_schema!`.

### Breaking: `Projection` trait moved + renamed

The `Projection` trait — implemented by every model's macro-emitted
`Selection` type to decode projected query responses — has moved from
`cratestack-client-rust` into `cratestack-core` and been renamed
**`ProjectionDecoder`**. The previous name collided with the SQL value
type `cratestack_sql::Projection<T>` (the actual `.select()` result
wrapper), which was the more central, user-facing meaning of the name.

* Old: `cratestack::client_rust::Projection`
* New: `cratestack::ProjectionDecoder`

`cratestack-client-rust` keeps re-exporting the trait under both
`ProjectionDecoder` and the deprecated `Projection` alias for one
release. Macro-emitted code now references the new name, so most
codebases will see no source-level impact.

### New: SQL views (ADR-0003)

A new `view` block in `.cstack` declares a read-only, SQL-defined
projection over one or more existing `model` blocks. Views generate
a typed Rust struct, a read-only delegate, and `CREATE VIEW` DDL
during migration generation, with the same `@@allow` policy
enforcement models get.

```cstack
view ActiveCustomer from Customer, Order {
  id          Int       @id  @from(Customer.id)
  email       String         @from(Customer.email)
  orderCount  Int

  @@server_sql("""
    SELECT c.id, c.email, COUNT(o.id)::int AS order_count
    FROM   customers c
    LEFT JOIN orders o ON o.customer_id = c.id
    GROUP  BY c.id, c.email
  """)
  @@embedded_sql("""
    SELECT c.id, c.email, COUNT(o.id) AS order_count
    FROM   customers c
    LEFT JOIN orders o ON o.customer_id = c.id
    GROUP  BY c.id, c.email
  """)

  @@allow("read", auth() != null)
}
```

```rust
let cool = cratestack_schema::Cratestack::builder(pool).build();
let rows = cool.views().active_customer().find_many().run(&ctx).await?;
```

#### Capabilities

* **Both backends.** `@@server_sql` runs against Postgres; `@@embedded_sql`
  runs against SQLite. The `@@sql` shorthand applies to both with a
  cargo warning that portability is the developer's problem.
* **Materialized views (server only).** `@@materialized` emits
  `CREATE MATERIALIZED VIEW` + `CREATE UNIQUE INDEX <name>_pkey ON
  <name> (<id>)` and produces a `refresh()` method on the delegate
  that runs `REFRESH MATERIALIZED VIEW CONCURRENTLY`. Embedded
  builds with a `@@materialized` view hard-error at macro expansion
  time — SQLite has no materialized views.
* **Type-level read-only.** `ViewDescriptor` does not implement
  `WriteSource`, so the bound on `CreateRecord` / `UpdateRecord` /
  `DeleteRecord` / `UpsertModelInput` simply fails to hold — there
  is no runtime check, the type system refuses.
* **`@@no_unique` gets its own delegate.** Views declared
  `@@no_unique` return a separate `ViewDelegateNoUnique<V>` type
  that omits `find_unique` (and `refresh()`) at the type level, so
  a call like `runtime.views().<v>().find_unique(())` is a compile
  error rather than a runtime `WHERE  = $1` footgun.
* **Migration ordering is automatic.** `cratestack-migrate` lands
  `DROP VIEW` ops before column / table drops the view referenced
  and `CREATE VIEW` ops after the matching column / table adds, so
  body changes that overlap with column changes still apply
  correctly. Body changes are modelled as `Drop + Create` (not
  `CREATE OR REPLACE VIEW`) to preserve that ordering invariant.
* **Policy enforcement is the same machinery models use.**
  `@@allow("read", expr)` lowers into the same `ReadPolicy` array
  consumed by `push_scoped_conditions`. Only the `"read"` action
  is accepted; any other action is a parse error.

Landed end-to-end across eight PRs:
[#84](https://github.com/cratestack/cratestack/pull/84) (parser + IR +
validator),
[#85](https://github.com/cratestack/cratestack/pull/85) (`ReadSource`
/ `WriteSource` traits + `ViewDescriptor`),
[#86](https://github.com/cratestack/cratestack/pull/86) (polymorphic
read helpers),
[#87](https://github.com/cratestack/cratestack/pull/87) (generic
read builders + `ViewDelegate`),
[#88](https://github.com/cratestack/cratestack/pull/88) (macro
emission + `runtime.views()` accessor),
[#89](https://github.com/cratestack/cratestack/pull/89) (migrate IR +
diff + per-backend DDL),
[#90](https://github.com/cratestack/cratestack/pull/90) (policy
lowering),
[#91](https://github.com/cratestack/cratestack/pull/91) (integration
tests vs real Postgres + SQLite). ADR-0003 is `Accepted` in the docs
repo (`cratestack-docs` [#21](https://github.com/cratestack/cratestack-docs/pull/21)).

### Cleanup

* `cratestack-macros` no longer emits selection / projection helpers
  behind a `cfg(not(target_arch = "wasm32"))` gate — `ProjectionDecoder`
  now lives in `cratestack-core` and works on every target.
* The umbrella's banking / policy / migrations / isolation /
  validation / generated-client integration tests are now under
  `crates/cratestack-pg/tests/`; the SQLite e2e test under
  `crates/cratestack-sqlite/tests/`. No test logic was changed.

### Other fixes

* Projected-query decoding now tolerates a missing optional field instead
  of erroring, matching how a partial `SELECT` projection is actually
  expected to behave (#93).
* `codec-json` is now an opt-out feature on `cratestack-client-rust`
  rather than always-on (#94).
* CI's rustdoc build now points at `cratestack-pg`, the facade split's
  replacement for the removed `cratestack` umbrella (#95), and the
  release workflow gained a test-retry + `SKIP_TESTS` escape hatch for
  known-flaky suites (#81).

## 0.3.7 (2026-05-18)

No code changes beyond the version bump itself.

## 0.3.6 (2026-05-18)

Release tooling: publish order is now computed from `cargo metadata`'s
real dependency graph instead of a hand-maintained list, so a new crate
gets the right publish position automatically instead of needing a
manual list edit every time (#80).

## 0.3.5 (2026-05-18)

Release tooling: `release-publish` is now idempotent and resumable — a
partial failure partway through publishing the workspace can be re-run
and picks up where it left off instead of re-attempting crates that
already published successfully (#79).

## 0.3.4 (2026-05-17)

Studio's `eject` command is redesigned from a UI-fork-only tool into a
full-project starter scaffold: `cratestack studio eject --out <dir>`
now writes a runnable binary crate (`Cargo.toml`, `src/main.rs`,
`studio.toml`, an example schema) with the Leptos UI already bundled
in; `--with-ui` additionally unpacks the UI's Trunk sources for
front-end customization. The UI itself moves to a sibling
`crates/cratestack-studio/ui/` crate, embedded into the release binary
as a tarball rather than generated from templates, and
`cratestack-studio-generator` folds into `cratestack-studio` (#78).

## 0.3.3 (2026-05-17)

### Studio rewrite — Phase 1d + 4 (typed editors + power tools)

The final phases of the Studio rewrite. Phase 1d retires the
one-text-box-per-field approach in the create + edit forms; Phase 4
ships SQL preview, drift detection, CSV/JSON export, schema search,
an audit log, and constraint-aware error mapping.

**Typed editors (Phase 1d).** The create form and the drawer's edit
mode now dispatch on each field's declared scalar:

- `<select>` for enums (variants pulled from the schema)
- `<textarea>` for `Json` (free-form, parsed on submit)
- `<input type="datetime-local">` for `DateTime` (auto-normalized to
  `YYYY-MM-DDTHH:MM:SSZ` before the request)
- `<input type="number" step="any">` for `Float` / `Decimal`
- `<input type="number" step="1">` for `Int`
- `<select>` (true/false) for `Boolean`
- plain text for `String`, `Cuid`, `Uuid`, `Bytes`

The `/api/targets/:key/models` response gains `is_enum` and
`enum_variants` per field so the UI doesn't need a second round-trip
to populate the dropdown.

**SQL preview (Phase 4).**

```
GET /api/targets/:key/models/:model/sql?op=list|get|create|update|delete&pk=…
```

Returns the SQL Studio would run plus an ordered parameter list:

```json
{
  "driver": "postgres",
  "sql": "WITH inserted AS ( INSERT INTO \"posts\" …",
  "params": [ { "index": 1, "binding": "title", "kind": "text" }, … ]
}
```

API-backed targets return **501 UNSUPPORTED** — Studio doesn't render
SQL it doesn't run.

**Drift indicator (Phase 4).**

```
GET /api/targets/:key/drift
```

Compares declared columns (from the `.cstack` schema) against the live
database. Each model carries one of: `ok`, `drift` (column mismatch),
`missing_table` (table absent), `unsupported` (API-only target), or
`skipped` (no @id or unsupported PK type). The UI renders an amber
`⚠ drift` badge in the sidebar next to any model that doesn't match,
and a red `✕ table` badge for missing tables.

**CSV/JSON export (Phase 4).**

```
GET /api/targets/:key/models/:model/export?format=csv|json&limit=N
```

Streams up to `EXPORT_CAP = 10_000` rows through cursor pagination
under the hood and returns one body. Sets `Content-Disposition:
attachment; filename="<target>-<table>.<ext>"` so browsers download
the file. CSV uses RFC-4180-style escaping (quote-wrap on commas,
quotes, or newlines; double up embedded quotes).

**Schema search (Phase 4).**

```
GET /api/targets/:key/search?q=<term>
```

Case-insensitive substring over models, fields, enums (and variants),
types, mixins, procedures. Hits return `kind`, optional `model`,
`name`, and a short `detail` so the dropdown can present them. The
search bar in the header debounces on input and shows the dropdown
inline.

**Audit log (Phase 4).** Every successful write (CREATE / UPDATE /
DELETE) is appended to an in-memory ring buffer (cap **500**, FIFO
when full) attached to the workspace. The `Audit` button in the
header opens an overlay listing the most recent entries:

```
GET /api/audit?limit=N
```

Returns newest-first. Entries carry `id`, `at` (RFC-3339), `target`,
`model`, `op`, and the row's `pk` (for CREATE, the post-insert value
the DB filled in).

**SQLSTATE → VALIDATION_ERROR mapping (Phase 4).** Constraint
failures from the driver are now mapped into the same per-field
`VALIDATION_ERROR` envelope the in-process validators produce, so the
UI can drop the message next to the input that broke:

| Source                       | Code           |
| ---------------------------- | -------------- |
| Postgres `23505` / SQLite `SQLITE_CONSTRAINT_UNIQUE` / `…_PRIMARYKEY` | `UNIQUE`       |
| Postgres `23503` / SQLite `SQLITE_CONSTRAINT_FOREIGNKEY`             | `FOREIGN_KEY`  |
| Postgres `23502` / SQLite `SQLITE_CONSTRAINT_NOTNULL`                | `REQUIRED`     |
| Postgres `22001` (string truncation)                                 | `LENGTH`       |
| Postgres `22P02` (invalid text representation)                       | `TYPE_MISMATCH`|
| Postgres `23514` / SQLite `SQLITE_CONSTRAINT_CHECK`                  | `REGEX`        |

Unrecognized driver errors still surface as `DATABASE_ERROR` (500).

**Validation codes.** Two new codes on top of Phase 3:

- `UNIQUE` — unique-constraint violation from the database.
- `FOREIGN_KEY` — foreign-key violation from the database.

**UI surfaces (Phase 4).**

- **Tools row.** Above the records table: an op selector + "Show SQL"
  button that fetches the preview and renders it as monospace SQL +
  bind list. Next to it: "Export JSON" / "Export CSV" links that
  point straight at the export endpoint so the browser handles the
  download.
- **Drift dots.** Each model in the sidebar carries a small status
  chip when its live shape doesn't match the schema.
- **Search.** The header's search input fans out to
  `/api/targets/:key/search` on every keystroke; results render in a
  dropdown below the input.
- **Audit overlay.** "Audit" button next to the target switcher
  toggles a 28rem-wide overlay listing recent writes by timestamp.

**Scope notes.**

- Audit log is in-memory only by design — Studio is a local admin
  tool. Restarting the binary clears the buffer.
- Drift inspection talks to `information_schema` (Postgres) and
  `PRAGMA table_info` (SQLite). API-backed targets are reported as
  `unsupported`.
- Export is bounded at 10_000 rows. Larger pulls should use the
  underlying database directly.

### Studio rewrite — Phase 1c + 3 (UI polish + write path)

Studio gains create / update / delete and the UI polish that goes
with it.

**Write API.** Three new endpoints:

```
POST   /api/targets/:key/models/:model/records          -> 201 + row
PATCH  /api/targets/:key/models/:model/records/:pk      -> 200 + row
DELETE /api/targets/:key/models/:model/records/:pk      -> 200 + row
```

All three reject requests against `mode = "ro"` targets with **403
FORBIDDEN**. Writes are wired on all three data sources: Postgres
uses `INSERT/UPDATE/DELETE … RETURNING *` wrapped in `row_to_json` for
type-blind projection; SQLite mirrors the shape with `RETURNING
json_object(...)`; the API source POSTs/PATCHes/DELETEs to the
upstream service's generated `/api/<plural-snake-model>` routes.

The Postgres write path binds typed values based on the field's
declared scalar — `String`/`Uuid`/`Cuid`/`Decimal`/`DateTime`/`Bytes`
as text, `Int` as `i64`, `Float` as `f64`, `Boolean` as `bool`, `Json`
through `sqlx::types::Json`. Anything else (enums) binds as text and
relies on the DB's enum cast.

**Validator pass-through.** A new `validators` module mirrors the
framework's macro-side validators (`@email`, `@length(min:, max:)`,
`@range(min:, max:)`, `@regex("...")`, `@uri`, `@iso4217`) against the
incoming JSON payload before Studio hits the database. Failures
surface as **422 VALIDATION_ERROR** with a structured per-field detail
list the UI can render inline:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "payload failed validation",
    "fields": [
      { "field": "title", "code": "LENGTH", "message": "field 'title' must be at least 3 characters long" },
      { "field": "authorEmail", "code": "EMAIL", "message": "field 'authorEmail' is not a valid email address" }
    ]
  }
}
```

Validation codes (all `SCREAMING_SNAKE_CASE`): `REQUIRED`,
`TYPE_MISMATCH`, `EMAIL`, `LENGTH`, `RANGE`, `REGEX`, `URI`, `ISO4217`.
The error envelope adds a `fields: []` array — omitted entirely on
non-validation errors so the existing error contract is unchanged.

**UI updates (Phase 1c + 3).**

- **Typed relation picker.** Drawer's relation follow swaps the free
  text input for a dropdown built from the model's `is_relation`
  fields. Labels show `<field> → <target> (<arity>)`.
- **RO / RW badge.** Each model header now displays a small badge
  reflecting the target's mode, so users see at a glance whether
  edits are allowed.
- **Create flow.** RW targets expose a `+ New` button above the
  records table that opens an inline form with one input per writable
  field. Validation errors surface per-field inline; on success the
  table reloads.
- **Edit flow.** RW targets expose an **Edit** button in the drawer
  that turns the field list into editable inputs. **Save** PATCHes
  the row; the response replaces the drawer's view. Per-field
  validation errors appear inline.
- **Delete flow.** RW targets expose a **Delete** button in the
  drawer guarded by a `window.confirm()` prompt. On success the
  drawer clears and the table reloads.
- **Pretty JSON viewer.** Object/array cell values in the drawer now
  render through `serde_json::to_string_pretty`.

**Error codes.** Two additions on top of Phase 1b's set:

- `FORBIDDEN` (403) — target is read-only.
- `VALIDATION_ERROR` (422) — payload-level validation failure with
  per-field detail.

The earlier (`BAD_REQUEST`) code is now reserved for malformed request
bodies (e.g. invalid JSON); validation errors get their own code so
the UI can route them into per-field error displays.

#### Scope notes

- Validators run before the DB. Constraint-level failures (UNIQUE,
  NOT NULL, CHECK, type mismatch beyond what we catch) still surface
  as `500 DATABASE_ERROR` with the underlying driver message; mapping
  SQLSTATE / SQLite extended codes to friendlier validation
  envelopes is Phase 4.
- The UI's create / edit form is a single text-input per field; typed
  pickers for enums and rich editors for JSON / DateTime / Decimal
  are Phase 1d.
- API targets accept writes and forward them to the upstream's REST
  routes verbatim. The upstream's own policy/auth enforces what's
  actually allowed.

### Studio rewrite — Phase 2 (`studio eject` + bundled UI)

Two things land in this phase. Both are about making Studio
distributable rather than dev-only.

**`cratestack studio eject --out <dir>`** writes a writable copy of
Studio's Leptos+Trunk UI into the target directory: `Cargo.toml`,
`Trunk.toml`, `index.html`, `src/{lib,api,app,types}.rs`, and a
purpose-built `README.md` that explains the standalone build flow.
Generated artifacts (`dist/`, `target/`, `Cargo.lock`) are skipped so
the eject output is a clean checkout. The UI tree is embedded into the
framework binary at compile time via `include_dir!`, so eject is a
single-step copy with no template substitution to drift.

```
cratestack studio eject --out ./fork
# wrote 9 files; cd ./fork && trunk serve
```

`--force` lets you overwrite an existing non-empty directory; without
it, eject refuses to clobber.

**`embed-ui` cargo feature** bundles the Trunk release build into the
Studio binary via `rust-embed`. Build flow:

```bash
cd crates/cratestack-studio/ui && trunk build --release
cargo build -p cratestack-cli --bin cratestack \
  --features cratestack-studio/embed-ui
```

With the feature on, `cratestack studio run` serves the SPA at `/`,
keeps the JSON API mounted at `/api/*`, and falls back to `index.html`
for unknown paths so the browser's client-side routing works. With
the feature off (the default), `/` still serves the Phase 1b stub
explainer so the binary stays minimal for dev.

Wiring: API routes are mounted before the UI routes, so any future
overlap resolves in favor of the JSON surface. The bundled-UI tests
explicitly assert that `/api/targets` still hits the JSON handler
when the SPA fallback is in play.

#### Crate / module changes

- `cratestack-studio` gains `mod eject` (with `eject()`, `EjectOptions`, `EjectError`, `EjectReport`) and an `embed-ui`-gated `mod ui_assets`.
- `cratestack-studio-generator` is now a thin re-export of `cratestack_studio::eject` so the CLI's existing import surface keeps working. New code should depend on `cratestack-studio` directly.
- `cratestack-cli`'s `studio eject` subcommand gains `--force` and now prints the eject report (file count + next-steps hint).
- New workspace deps: `include_dir = "0.7"`, `rust-embed = "8"` (used only when the `embed-ui` feature is on).

#### Scope notes

- The `embed-ui` feature requires a Trunk release build to have produced `crates/cratestack-studio/ui/dist/`. Building the feature without that tree fails fast at the embed step.
- Eject's output README points users at the framework's docs for upstream upgrades. There's no automated re-eject path — a forked UI is a fork.

### Studio rewrite — Phase 1b (read API completions + Leptos UI)

Phase 1b finishes the read story. SQLite targets are now a first-class
driver, the `@relation` traversal endpoint is wired, the API-backed
list/get path talks to deployed CrateStack services, and a Leptos+Trunk
web UI consumes all of it from the browser.

**SQLite via rusqlite.** A new `data::sqlite::SqliteSource` opens a
SQLite connection per target and projects rows through SQLite's
`json_object(...)` so the rest of the pipeline stays identical to the
Postgres path. Studio doesn't use `sqlx-sqlite` because the workspace's
`rusqlite 0.39 → libsqlite3-sys 0.37` pin conflicts with sqlx-sqlite's;
the rusqlite-based source has no such conflict. `[target.db]` URLs
accept `sqlite:`, `sqlite://`, `sqlite::memory:`, and bare file paths.

**Relation follow.** New endpoint
`GET /api/targets/:key/models/:m/records/:pk/rel/:field`. The
resolver reads `@relation(fields: [...], references: [...])` symmetrically
on both ends of a relation: the target is the field's declared type,
the source row's `fields[0]` supplies the bound value, and we filter
the target table on `references[0]`. List-arity fields return a paginated
page; Required-arity fields return a single optional row. Both sides
of the relation must declare `@relation` (which is what the CrateStack
parser already enforces).

**API list/get.** `data::api::ApiSource` now talks to a deployed
CrateStack service over the same REST routes the generated TypeScript
and Dart clients use: `GET <base>/api/<plural-snake-model>` for list,
`GET <base>/api/<plural-snake-model>/{id}` for find_unique. Studio
maps its cursor abstraction onto the upstream's offset/limit pagination
by encoding the next offset as the opaque cursor string. Auth headers
follow `[target.api.auth]` (`bearer { token = … }` or `header { name,
value }`). Relation follow against API targets returns `UNSUPPORTED` —
the generated REST surface doesn't expose arbitrary column filters.

**Dev CORS.** `[workspace] cors_dev = true` (the default) layers a
permissive CORS layer on the router so a Trunk dev server on
`localhost:8080` can talk to the Studio backend on `localhost:7878`.
Set `cors_dev = false` when binding to a wider interface.

**Leptos UI.** New `crates/cratestack-studio/ui/` crate — a Leptos
CSR app built by Trunk, intentionally excluded from the workspace so
`cargo check --workspace` doesn't pull in the `wasm32-unknown-unknown`
toolchain. Surface:

- Header with workspace name and target switcher (shows mode + db/api capability).
- Left sidebar listing the selected target's models.
- Records table with cursor-based pagination (previous/next).
- Record drawer with a per-field view, a relation-follow input, and a
  "Copy Rust query" button that writes the find_unique snippet to the
  system clipboard.

Run locally with `cratestack studio run` in one terminal and
`trunk serve` in `crates/cratestack-studio/ui/` in another; Trunk's
proxy forwards `/api/*` to the backend on port 7878.

**Error envelope additions.** Two new stable codes: `UNKNOWN_FIELD`
(unknown field name on relation follow, 404) and `NOT_A_RELATION`
(field exists but isn't a relation, 400). `INTERNAL_ERROR` is reserved
for blocking-task panics during the SQLite path.

#### Scope notes

- Relation follow is read-only and supports the two common shapes
  (outgoing 1-1 / many-1, inbound 1-many). Many-to-many through a
  junction table returns `UNSUPPORTED`.
- The UI's relation follow currently takes the field name as a free
  text input — a typed dropdown lands in Phase 1c once the UI threads
  the per-model relation field list down to the drawer.
- The Studio binary still ships without the UI compiled in. Phase 2's
  `studio eject` writes the UI's sources to a writable workspace; Phase
  2 / 3 also adds the `rust-embed` bundle for single-binary distribution.

### Studio rewrite — Phase 1a (read API)

The studio gains a real backend. `cratestack studio run` now parses
each target's `.cstack`, opens a sqlx Postgres pool (when the target
has a `[target.db]` block), and serves six read endpoints:

```
GET /api/targets
GET /api/targets/:key/schema
GET /api/targets/:key/models
GET /api/targets/:key/models/:model/records?cursor=…&limit=…
GET /api/targets/:key/models/:model/records/:pk
GET /api/targets/:key/models/:model/snippet?pk=…
```

`/snippet` returns a Rust `find_unique` call against the macro
delegate so you can paste it into a service crate. Primary-key
literals are typed: `String`/`Cuid`/`Uuid`/`Decimal` IDs render as
`"…".to_owned()`, `Int` IDs as `42_i64`.

Pagination is cursor-based on the model's `@id`. Cursors are bound as
text and cast in SQL (`$1::bigint` for Int PKs, no cast for text-shaped
PKs) so the Rust side stays blind to column types. Row projection uses
Postgres's `row_to_json(t.*)` over the model's scalar columns, which
keeps the dynamic decode path off the type-OID treadmill.

Studio now reads `env:NAME` and `file:PATH` references in
`studio.toml`. `target.db.url` and `target.api.auth.{token,value}` are
resolved at boot; unset env vars and missing files surface a load-time
error that names the bad config field.

API responses use a uniform error envelope —
`{"error": {"code": "…", "message": "…"}}` — with stable codes
(`UNKNOWN_TARGET`, `UNKNOWN_MODEL`, `NO_PRIMARY_KEY`,
`INVALID_PRIMARY_KEY`, `UNSUPPORTED`, `DATABASE_ERROR`,
`UPSTREAM_ERROR`).

#### Scope limits

- **Postgres only.** The workspace currently pins `rusqlite` (used by
  `cratestack-rusqlite` and `cratestack-client-store-sqlite`) against
  `libsqlite3-sys` 0.37, which conflicts with `sqlx-sqlite`'s pin.
  Phase 1b adds an alternate SQLite path that uses `rusqlite` directly
  so the two crates can coexist.
- **No relation follow yet.** `/api/targets/:key/models/:m/records/:pk/rel/:f`
  lands in Phase 1b alongside the UI.
- **API-only targets return 501 on list/get.** Schema and snippet
  endpoints work because they read the parsed schema, not the upstream;
  list/get against `[target.api]` targets is wired in Phase 1b.
- **Primary-key types.** Phase 1a accepts `String`, `Cuid`, `Uuid`,
  `Decimal`, and `Int`. Other PK types (`DateTime`, `Bytes`, etc.)
  return `UNSUPPORTED`.

### Studio rewrite — Phase 0 (breaking)

The Jinja-templated `cratestack generate-studio` scaffold is removed. In its
place is a new crate, `cratestack-studio`, and a new CLI surface,
`cratestack studio`, with three subcommands:

```sh
cratestack studio init                  # writes ./studio.toml
cratestack studio run                   # binds 127.0.0.1:7878 by default
cratestack studio eject --out ./out     # Phase 2 — currently returns NotImplemented
```

The studio now reads a workspace file (`studio.toml`) that lists one or
more `[[target]]` blocks. Each target points at a `.cstack` schema and
declares how the studio reaches its data: a `[target.db]` block for
direct sqlx connections, a `[target.api]` block for a deployed
cratestack service, or both. A target with neither channel is rejected
at load time.

Phase 0 only ships the skeleton: config loader, target validation, and
an Axum server that exposes `/` (stub page) and `/api/health` (workspace
+ target summary). Schema introspection, record browsing, mutations, and
the Leptos UI follow in Phases 1-4.

`cratestack-studio-generator` is now a transitional shim. Its 0.3.x
public API (`generate_package`, `StudioGeneratorConfig`,
`StudioGeneratorContext`, `StudioProfile`, `GeneratedStudioFile`,
`GeneratedStudioPackage`) is gone; the only remaining surface is a
placeholder `eject()` that will, in Phase 2, copy `cratestack-studio`'s
own sources into an output directory for users who want to fork the UI.

Migration for existing `generate-studio` users: run `cratestack studio
init` to seed a `studio.toml`, fill in your schemas and connection
strings, then `cratestack studio run`. There is no automated migration
of the 0.3.x multi-crate output — it was generated code and should be
regenerated from the new shape.

### RPC transport (v1): `transport rpc` as an alternative to REST

A `.cstack` schema now picks exactly one generation style via a
top-level `transport rest|rpc` directive (default `rest`, so existing
schemas parse unchanged) — one binding's worth of public surface, not
both. Under `transport rpc`, every CRUD verb per model and every
procedure gets an op id (`model.User.list`, `procedure.publishPost`),
dispatched over two endpoints instead of a route per model/verb:

```
POST /rpc/:op_id       # unary
POST /rpc/batch        # server may parallelize; no in-batch dependencies,
                        # no transactional mode — use a procedure or two
                        # round trips for composite ops
```

The op id lives in the URL rather than the request body — operationally
honest, since nginx/CDN/HTTP tracing all work per-route that way — and
client codegen branches on the schema's transport style, so a generated
SDK ships one client's worth of code, not both (#20–#24, examples in
#27). Error responses use gRPC-style codes in a stable `RpcErrorBody`
shape (#23). Streaming (`application/cbor-seq`) needed no code change
at all: content negotiation on the existing sequence encoder already
handled it (#24).

**Deferred:** the WebSocket binding and `@@subscribe`-driven
subscriptions from the original design are not part of this release —
today's audit/event-bus consumers are server-to-server and don't need
a WS channel, so this is picked up when a concrete consumer needs it,
not before (#25).

### ORM additions

Landed alongside the RPC work above, independent of transport style:

- **Transaction-aware writes**: `.for_update()` and `update_many` join
  the existing write surface, both participating correctly in an
  ambient transaction (#26).
- **Composite-key upsert** and **`find_unique` detail policy** support
  (#28).
- **Nullable-OR filter** and a **`COALESCE` multi-column filter** for
  querying across nullable columns without hand-written SQL (#29).
- **`aggregate`**, **`delete_many`**, and `NULLS FIRST`/`NULLS LAST`
  ordering (#37).
- **JSONB filter operators** — `json_has_key` + `json_get_text` (#42).
- **`FindMany.include()`** — to-one relation side-loading in a single
  round trip (#44).
- **PostGIS spatial filters** — `covers_geography` + `dwithin_geography`
  (#48).
- **Column projection** — `find_*.select(...)` returning a typed
  `Projection<T>` instead of the full model (#51).
- **`ProjectedFindMany.run_in_tx`**, plus an `enum` `Default` fix (#55).

### Client streaming (cbor-seq)

The generated clients gain first-class consumers for the streaming
transport introduced above:

- **Rust**: `RpcClient::call_streaming` returns an `mpsc::Receiver`,
  fed by a `cbor-seq` streaming decoder (#30, #34). Also gains a typed
  batch API — same method, two consumption modes (#53).
- **Dart**: `CborSeqStreamTransformer` + a decoder-handle contract
  (#43), and an `rpc_call_streamed` FFI entrypoint for
  `/rpc/{op_id}` (#39).
- **Flutter**: `execute_streamed` FFI shim over the cbor-seq path
  (#33), and `FlutterCborSeqDecoder` for `dio`-driven streaming (#40).
- **Codegen**: client generators now branch on `Schema.transport` to
  emit RPC clients where the schema calls for them (#32, #50).

### Workspace-wide 200-LoC refactor

Every `.rs` file under `crates/*/src/` is now ≤200 LoC, landed across 16
PRs (#57–#76). No public API changes — all splits preserve the crate
surface via `pub use` re-exports. The major rewrites:

- `cratestack-sqlx` and `cratestack-rusqlite` delegate / render / batch /
  value modules split into focused submodules
- `cratestack-axum` idempotency, rpc, transport, ratelimit, headers,
  codec all broken into per-concern files
- `cratestack-macros` four giants split (include / model / axum /
  relation), medium files re-grouped
- `cratestack-client-{dart,rust,typescript,flutter}` `lib.rs` split into
  per-concern modules (largest: client-rust at 2369 → 18 submodules)
- `cratestack-parser` 880-line `parse.rs`, 1086-line `validate.rs`, and
  1336-line `tests.rs` split per topic
- `cratestack-lsp` `main.rs` (1273 LoC) split into 11 submodules
- `cratestack-client-dart` README and rpc-runtime jinja templates split
  via `{% include %}` fragments (loader sets
  `set_keep_trailing_newline(true)` for byte-identical output)
- Inline `#[cfg(test)] mod tests` blocks throughout the workspace
  extracted into `tests_<topic>.rs` siblings

### README fixups

Four crate READMEs (`cratestack-axum`, `cratestack-sqlx`,
`cratestack-client-rust`, `cratestack-parser`) still referenced the
pre-0.3.0 macro names (`include_schema!`,
`include_client_macro!`) — updated to the current
`include_server_schema!` / `include_client_schema!`. The `client-rust`
README's two duplicate sections (one per old macro) collapse into one.

### Other

Test-support scaffolding (`tests/support/pg.rs`) covering
compose/testcontainers/skip backend selection for PG-backed integration
tests (#19), and an internal `cratestack-axum` module split
(codec/transport/headers/query) with deduped RPC helpers (#31).

## 0.3.2 (2026-05-14)

### Batch primitives — tRPC-style per-item envelope

Five new ORM methods on every model delegate, on both the sqlx (server) and rusqlite (embedded) backends:

```rust
cool.account().batch_get(vec![1, 2, 999]).run(&ctx).await?
cool.account().batch_create(vec![input_a, input_b]).run(&ctx).await?
cool.account().batch_update(vec![(1, patch_a, Some(0)), (2, patch_b, None)]).run(&ctx).await?
cool.account().batch_delete(vec![1, 2]).run(&ctx).await?
cool.account().batch_upsert(vec![input_a, input_b]).run(&ctx).await?
```

Every batch call returns `Result<BatchResponse<M>, CoolError>`. The outer `Result` is reserved for whole-batch infrastructure failures (size cap exceeded, duplicate input keys, DB connection lost). Per-item failures (validation, policy denial, NotFound, stale `if_match`, PK conflict) ride inside the envelope as `BatchItemStatus::Error { error: BatchItemError { code, message } }`, with `index` preserved so callers can pair results back to their input position.

```json
{
  "results": [
    { "index": 0, "status": "ok", "value": { ... } },
    { "index": 1, "status": "error", "error": { "code": "POLICY_DENIED", "message": "..." } },
    { "index": 2, "status": "ok", "value": { ... } }
  ],
  "summary": { "total": 3, "ok": 2, "err": 1 }
}
```

### Transactional model

- **Two single-statement ops** (`batch_get`, `batch_delete`) issue one `SELECT … WHERE pk IN (…)` or `DELETE … WHERE pk IN (…) RETURNING …`. Policy predicates merge into the WHERE; rows that don't match (because they don't exist, were already tombstoned, or the read/delete policy hid them) surface as per-item `NOT_FOUND`.
- **Three savepointed ops** (`batch_create`, `batch_update`, `batch_upsert`) run all items in one outer transaction with a per-item `SAVEPOINT`. A per-item failure rolls back its savepoint only — successful items in the same batch still commit. The audit log records one row per successful item, with the outer commit timestamp; failed items leave no audit row, no event outbox entry, no row mutation.
- The cap is `1000` items per call (`cratestack_core::BATCH_MAX_ITEMS`); over-sized batches are rejected before any SQL runs.

### Loud-fail on duplicate input keys

The framework refuses batches with duplicate primary keys at the outer guard, returning `CoolError::Validation` (or `RusqliteError::DuplicateBatchKey` on the embedded side) with the indices of the first and duplicate occurrences. Silently collapsing duplicates would break the per-item `index` mapping the envelope promises and hide caller bugs; we want callers to dedupe at the boundary they own.

Detection runs on:

- the PK list for `batch_get` / `batch_delete`
- the per-item PK in `batch_update` items
- `UpsertModelInput::primary_key_value()` for `batch_upsert`

`batch_create` skips the check — `CreateModelInput` doesn't expose the PK generically, and duplicate client-supplied PKs already trip the database's unique constraint per-item (surfacing as `CoolError::Conflict` in that item's envelope, while the rest of the batch commits cleanly via savepoint isolation).

### Internal

- New types in `cratestack-core`: `BatchItemResult<T>`, `BatchItemStatus<T>`, `BatchItemError`, `BatchSummary`, `BatchResponse<T>`, `BatchRequest<I>`, `BATCH_MAX_ITEMS`, `find_duplicate_position`.
- New trait in `cratestack-sql`: `ModelPrimaryKey<PK>`, emitted by the macro on every generated model struct. Used by `batch_get` / `batch_delete` to pair returned rows back to their input position.
- New helper in `cratestack-sql`: `find_duplicate_sql_value` for upsert-side dedup, since `SqlValue::Float` / `SqlValue::Decimal` don't admit a sound `Hash` impl.
- New `RusqliteError` variants: `BatchTooLarge { actual, maximum }` and `DuplicateBatchKey { first, duplicate }`.

### Worked example

The `examples/embedded-cli` notes app gains three batch subcommands that walk through the envelope in real terminal output:

```text
$ notes import bulk-load.json
OK  [0] 11111111-…  first
OK  [1] 22222222-…  second
summary: 2 total, 2 ok, 0 err

$ notes bulk-done 11111111-… 99999999-…
OK  [0] 11111111-…  first
ERR [1] NOT_FOUND: no row matched
summary: 2 total, 1 ok, 1 err
```

- `notes import <file.json>` — `batch_upsert` over a JSON file; replays converge.
- `notes bulk-done <id> [id...]` — `batch_update` to mark complete.
- `notes bulk-delete <id> [id...]` — `batch_delete`.

### Deferred

- **Auto-generated `POST /<model>/batch-*` axum routes**: the wire envelope types (`BatchRequest<I>` / `BatchResponse<T>`) are stable in `cratestack-core` so apps can hand-roll a thin handler against the ORM today. Macro-driven route emission per model lands in a follow-up.
- **Per-item `if_match` on the embedded `batch_update`**: the rusqlite layer doesn't enforce `@version` for single rows either; consistency over surprise.

## 0.3.1 (2026-05-14)

### New crate: `cratestack-migrate` — schema diff + migration generator

Implements ADR-0004, the *authoring* side of the migration story: a new
`cratestack-migrate` crate diffs a parsed `.cstack` against a committed
snapshot and emits per-backend SQL migrations. The runner (already in
`cratestack-sqlx`) is unchanged — it consumes the generated SQL
identically to hand-written migrations.

```
cratestack migrate diff --schema schema.cstack --out-dir migrations --backend both --name <slug>
```

Per-backend output lives under
`migrations/<postgres|sqlite>/<timestamp>_<slug>/` as `up.sql` /
`down.sql`, alongside a committed `schema.snapshot.json`. The diff
engine produces a backend-agnostic op list ordered by DDL dependencies
(enums → renames → drops → creates → adds → check constraints → enum
drops), covering table/column add-drop, indexes (from `@unique`),
column type/nullability/default changes, renames (`@@rename` /
`@rename`), enums, and check constraints (`@db_enforce` promotion of
`@range` / `@length` / `@iso4217`).

**Destructiveness gating.** Every op is classified Safe / Lossy /
Blocking; `--allow-destructive` is required to write any migration
containing a lossy op, and `down.sql` for a lossy migration is an
explicit error stub (`RAISE EXCEPTION` / `RAISE(FAIL, ...)`) rather
than a real rollback — matching the runner's irreversible-by-default
posture (#16).

**Deferred (intentional):** `migrate verify` and `migrate drift` need
ephemeral DB spawning and live introspection, each with its own CI
footprint; view-block IR ops need the `view` block itself (ADR-0003)
built out first; `DropEnumVariant` needs a Postgres swap-dance plus a
backfill plan for referencing rows.

### Examples, docs, and CI

- Pure-Rust example set covering all three 0.3.0 macros side by side
  (#10), and a root README rewrite for the macro split (#11).
- In-browser embedded SQLite example plus a wasm32 facade refactor
  (#12); `embedded-expo` × `embedded-flutter` × `tauri-native` (#14);
  `embedded-daemon` + `embedded-webhook` showing async I/O layered
  around the sync `ModelDelegate` (#15).
- CI's rustdoc job now restricts to the framework crates so it doesn't
  pull in GTK transitively via the Tauri examples (#13).

### Upsert primitive

New `.upsert(input)` on every model whose `@id` is client-supplied (i.e. has no `@default(...)`). Backed by `INSERT … ON CONFLICT (<pk>) DO UPDATE …`. Available on both the sqlx (server) and rusqlite (embedded) backends.

```rust
// Server (sqlx) — both create and update policies enforced, event/audit
// driven off a SELECT … FOR UPDATE probe inside the same transaction.
cool.tag().upsert(CreateTagInput { id, label }).run(&ctx).await?;

// Embedded (rusqlite) — single statement, no audit/event machinery.
delegate.upsert(CreateTagInput { id, label }).run()?;
```

Models with server-generated PKs (`@id @default(cuid())`, etc.) get **no** `UpsertModelInput` impl — calling `.upsert(...)` on them is a compile error rather than a runtime "not supported." Unique-key (non-PK) conflict targets are deferred.

Semantics:

- **Both create and update policies must allow the call** — evaluated at call time, before the runtime knows which branch will fire. Pre-flighting a read to pick a policy slot would leak row existence to the caller.
- **`@version` columns are bumped server-side** on the update branch (`<table>.<col> + 1`). `if_match` is not supported on upsert — use `.update(...).if_match(...)` if you need it.
- **Soft-deleted rows act as "no row"**: the INSERT branch will then trip the PK uniqueness constraint, which is the right outcome (refuse to silently revive a tombstone).
- **Event / audit fan-out** picks `Created` vs `Updated` based on whether the `SELECT FOR UPDATE` probe saw a row — not Postgres `xmax`, so the rusqlite mirror stays trivial.
- **Auth-derived defaults (`@default(auth().*)`) are excluded from the update branch** — they're identity bindings, and clobbering them on update would turn upsert into "take ownership of any row I name." The full list of columns the update branch is allowed to overwrite is exposed on `ModelDescriptor::upsert_update_columns`.

### Internal

- `ModelDescriptor::new(...)` gained one trailing argument (`upsert_update_columns`). Schemas built through `include_*_schema!` are unaffected; hand-rolled descriptors need the extra `&[]`.

## 0.3.0 (2026-05-13)

### Headline: three macros, one schema, no dead weight

The single `include_schema!` macro is gone. In its place are three role-specific macros that emit only what each deployment needs. No more mobile apps transitively pulling `sqlx` they don't use; no more server builds carrying `rusqlite` for nothing.

```rust
// Server (Postgres via sqlx) — full ORM, axum routes, procedures, events
include_server_schema!("schema.cstack", db = Postgres);

// Embedded (rusqlite) — works native and on `wasm32-unknown-unknown` via OPFS
include_embedded_schema!("schema.cstack");

// HTTP client — model/input stubs, procedure clients, zero DB
include_client_schema!("schema.cstack");
```

The split is **strict**: `include_server_schema!` does not emit anything rusqlite-related, and `include_embedded_schema!` does not emit anything sqlx-related. Each deployment shape pays only for its own surface.

### Breaking changes

- **Removed `include_schema!`.** Migrate server callers to `include_server_schema!("…", db = Postgres)`. Migrate sqlite/embedded callers to `include_embedded_schema!("…")`.
- **Renamed `include_client_macro!` → `include_client_schema!`** for naming consistency with the new macros.
- **`include_server_schema!` requires a `db = …` argument.** Today only `db = Postgres` is accepted; the parser is wired so adding `MySql` / `Sqlite`-via-sqlx in a future release is non-breaking at call sites that already pass `db = Postgres`.
- **`include_embedded_schema!` emits `::cratestack_rusqlite::*` paths**, not `::cratestack::*`. Embedded consumers should list `cratestack-rusqlite` and `cratestack-macros` directly in their `Cargo.toml`; the heavyweight `cratestack` facade is no longer required for an embedded build.
- **Deleted the `cratestack-sqlite-wasm` crate.** Originally written as a separate wasm32 backend; superseded by `rusqlite 0.39`, which targets wasm32 transparently via `sqlite-wasm-rs`. Use `cratestack-rusqlite` with the `wasm32-unknown-unknown` target and the new `cratestack_rusqlite::opfs::install_opfs_vfs()` helper (must run inside a Dedicated Worker).
- **Bumped `rusqlite` to `0.39`** (from the previously-resolved `0.32`). Internal `u64` columns now require the `fallible_uint` feature (enabled by default in our workspace pin).
- **Internal: `cratestack-sqlx` migrated off the `sqlx` umbrella crate** to depend on `sqlx-core` + `sqlx-postgres` directly. The umbrella's `sqlx-sqlite` leaked into the resolve graph and conflicted with `rusqlite 0.39`'s `libsqlite3-sys 0.37`. Public surface stays as `cratestack::sqlx::*` via a compatibility shim in `cratestack-sqlx` — no consumer changes required for code that referenced the facade path.
- **Internal: `cratestack-lsp` migrated from unmaintained `tower-lsp 0.20` to `tower-lsp-server 0.23`.** The fork ports the same crate to native `async fn` in traits (Rust 1.75+), drops `#[async_trait]` attributes, renames `lsp_types` → `ls_types`, and switches `Url` → `Uri` (from `fluent-uri`). User-facing LSP behavior unchanged.

### Migration cheat sheet

| Before | After |
|---|---|
| `include_schema!("schema.cstack");` (server context) | `include_server_schema!("schema.cstack", db = Postgres);` |
| `include_schema!("schema.cstack");` (sqlite/mobile context) | `include_embedded_schema!("schema.cstack");` |
| `include_client_macro!("schema.cstack");` | `include_client_schema!("schema.cstack");` |
| `use cratestack::include_schema;` | `use cratestack::{include_server_schema, include_embedded_schema, include_client_schema};` (pick what you need) |

### New features

- **In-browser SQLite ORM.** `cratestack-rusqlite` now compiles to `wasm32-unknown-unknown`. The new `cratestack_rusqlite::opfs::install_opfs_vfs(&OpfsOptions::default()).await?` installs the OPFS SAH-pool VFS so `RusqliteRuntime::open(filename)` persists across page reloads. Must run inside a Dedicated Worker.
- **Single SQLite backend everywhere.** The same `cratestack-rusqlite` crate now serves mobile (libsqlite3), desktop (libsqlite3), and browser (OPFS via `sqlite-wasm-rs`). One code path, one API.

### Known follow-ups

- `@@audit` and `@@emit` directives are currently no-ops in `include_embedded_schema!`. The local-journal / local-event-bus implementations need their own design pass (sync engine, conflict resolution); they will land in a follow-up release.
- `cratestack-sqlx` could lose its `cratestack::sqlx::*` compatibility shim once we've validated nobody depends on it externally. Tracked as a 0.4.0 cleanup.
- Multi-DB support (MySQL, SQLite-via-sqlx) for `include_server_schema!` — the `db = …` arg parser is ready; the codegen needs the abstraction.

## 0.2.3 (2026-05-12)

`cratestack-redis` gains **`RedisRateLimitStore`**, enforcing a single
global token-bucket per key across replicas via one atomic
read-refill-decrement-write Lua script; bucket state lives at
`<prefix>:rl:<sha256(key)>` with a self-refreshing `EXPIRE` so idle
keys evict themselves. Skips its live-Redis integration tests cleanly
when no Redis is configured, matching the sqlx-store test pattern
(#7).

## 0.2.2 (2026-05-12)

Docs-only: every crate README rewritten against its actual API
surface rather than aspirational/stale examples (#6).

## 0.2.1 (2026-05-12)

### New crate: `cratestack-rusqlite` — the embedded SQLite backend

The embedded backend's real implementation: `ddl`, `delegate`,
`render`, `row`, `runtime`, and `value` modules, plus an `ffi` layer
for non-Rust embedders (#4).

### New crate: `cratestack-redis` — `RedisIdempotencyStore`

A server-side Redis-backed idempotency store, sibling to
`cratestack-sqlx`'s `SqlxIdempotencyStore`, for multi-replica
deployments that need shared idempotency state across instances rather
than per-process memory. Atomicity comes from three Lua scripts
(`reserve_or_fetch`, `complete`, `release`) run via `EVALSHA` with
`NOSCRIPT` fallback; reservation lifetimes are driven by `PEXPIREAT`,
and token rotation on reclaim plus token/status guards inside
`complete`/`release` stop a stale handler from poisoning a newer
reservation. State lives in one Redis hash per `(principal, key)` at
`<prefix>:idem:<sha256(principal || 0x00 || key)>` (#5).

## 0.2.0 (2026-05-12)

The first version actually published to crates.io. (`v0.1.0` was never
published under that number — see the note at the bottom of this file.)

### Banking-readiness: a three-phase hardening pass

The framework's first push from e-commerce-production-grade toward
banking-grade, landed as one large merge (#2, #3):

- **Phase 1 — correctness & money.** `Decimal` scalar
  (feature-flagged `decimal-rust-decimal` / `decimal-bigdecimal`, the
  latter still a `compile_error!` stub), error redaction (4xx messages
  public, 5xx detail-only), optimistic locking (`@version` +
  If-Match/ETag), schema validation attributes (`@length` / `@range` /
  `@regex` / `@email` / `@uri` / `@iso4217`), idempotency
  (`IdempotencyLayer` + `SqlxIdempotencyStore`, opt-in via
  `Router::layer(...)`, not auto-wired into macro-generated routers).
- **Phase 2 — compliance & integrity.** Audit log (`@@audit`,
  before/after snapshots), field-level policy (`@readonly` /
  `@server_only`), transaction isolation (`@isolation("...")`),
  PII/data classification (`@pii` / `@sensitive`), correlation IDs
  (traceparent propagation).
- **Phase 3 — hardening & ecosystem.** HMAC signed envelope
  (`COSE_Sign1`/ES256 trait-ready, not yet implemented), rate limiting
  (`RateLimitLayer`), anti-replay nonce store, API versioning
  (`@api_version`), soft-delete (`@@soft_delete`, GC left as a
  follow-up), FIPS crypto feature flag (a real FIPS certification
  still needs a vendor-validated libcrypto), and a migration engine
  (`cratestack_migrations` table + `apply_pending` — schema-diff-driven
  generation was explicitly out of scope at this point; that landed
  later as `cratestack-migrate`, see `v0.3.1`).

**Known-outstanding at this point:** `IdempotencyLayer` still isn't
auto-wired into macro-generated routers by default; `RedisNonceStore`
doesn't exist yet (`RedisIdempotencyStore` and `RedisRateLimitStore`
land in `v0.2.1`/`v0.2.3`, right after this); `COSE_Sign1` has no real
ES256/EdDSA signing behind it yet, trait surface only.

### CLI, mixins, and the TypeScript client

- **`cratestack` CLI** for schema tooling, the framework's first
  command-line surface.
- **Mixin support** — `@use` composes shared field groups into
  `.cstack` models.
- **Generated TypeScript client** gains TanStack Query hooks, and a
  Rust **client-only macro** (the predecessor to what later became
  `include_client_schema!` in the `v0.3.0` three-macro split) for
  generated Rust clients, plus request-authorization support.
- **`cratestack-client-store-redis`** — a Redis-backed client-side
  state store.
- Backend-to-backend client guidance defaults to the CBOR codec and
  clarifies OAuth2 endpoint handling.

### Public release housekeeping

The repo's public GitHub Pages docs deployment (custom domain +
rustdoc root redirect) is fixed, and the codebase is scrubbed of
internal-only references from before the project's public rename —
this is the release the rest of this changelog's history starts
counting from.

---

`v0.1.0` doesn't have a section above because it was never published —
no crates.io release, no tag. It was the version number in `Cargo.toml`
during the project's pre-public "extraction" work (renaming from an
internal codename, stripping internal-only references, standing up the
CLI/docs/public-release plumbing) before the very first real release,
which shipped as `v0.2.0` above instead.
