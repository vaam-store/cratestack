<div align="center">

# CrateStack

**Write one `.cstack` schema. Get a typed Rust server, an offline-first embedded database, and Rust / Dart / TypeScript clients — all generated at compile time.**

[![CI](https://github.com/cratestack/cratestack/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/cratestack/cratestack/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cratestack-pg.svg?label=crates.io)](https://crates.io/crates/cratestack-pg)
[![docs.rs](https://img.shields.io/docsrs/cratestack-pg?label=docs.rs)](https://docs.rs/cratestack-pg)
[![npm](https://img.shields.io/npm/v/%40cratestack%2Fcli?label=npm)](https://www.npmjs.com/package/@cratestack/cli)
[![MSRV](https://img.shields.io/badge/MSRV-1.98.0-blue)](rust-toolchain.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[Documentation](https://cratestack.dev) · [Quickstart](https://cratestack.dev/getting-started/quickstart) · [Examples](examples) · [Roadmap](ROADMAP.md) · [Contributing](CONTRIBUTING.md) · [Get help](SUPPORT.md)

</div>

---

CrateStack is a schema-first framework for Rust. You describe your models,
procedures, and access policies once in a `.cstack` file; a procedural macro
reads it **at compile time** and emits the typed Rust surface for whatever the
consuming crate is — an HTTP service, an on-device SQLite store, or a client SDK
for someone else's service.

There is no runtime reflection, no code-generation step to wire into your build,
and no generated Rust files to commit. Change the schema, and the compiler tells
you what broke.

```cstack
model Post {
  id        Int      @id
  title     String
  published Boolean  @default(false)
  authorId  Int

  author    User?    @relation(fields: [authorId], references: [id])

  @@allow("read",   published == true)
  @@allow("create", auth() != null)
  @@allow("update", auth().role == "admin")
}

procedure getFeed(args: FeedArgs): Post[]
```

From that one file you get — depending on which macro the crate calls — sqlx
`FromRow` impls and an Axum router with CRUD and procedure routes, policy
enforcement wired into every read and write, projection/filter/sort query
parsing, CBOR and JSON codecs, a `rusqlite`-backed on-device store that also
compiles to `wasm32-unknown-unknown`, and typed client SDKs in three languages.

## Why

- **One source of truth.** The server, the offline store, and every client are
  generated from the same schema, so they cannot drift. A field rename is a
  compile error in all of them, not a bug you find in production.
- **Compile-time, not runtime.** Codegen happens during `cargo build`. Nothing
  to run, nothing to check in, nothing to keep in sync.
- **Access policy lives with the data.** `@@allow` / `@@deny` sit on the model
  and are enforced by generated server code — not reimplemented in each handler.
- **Rust owns the state, on every target.** The same embedded schema compiles to
  iOS, Android, desktop, and the browser (OPFS-backed SQLite over wasm), so your
  UI layer stays a UI layer.
- **You only pay for your shape.** Four strictly disjoint facades: a mobile build
  never links sqlx, a client SDK never links Axum, a server never links rusqlite.

## Quickstart

Install the CLI — no Rust toolchain needed if you use npm:

```bash
npm install --global @cratestack/cli        # prebuilt binary
# or
cargo install cratestack-cli
```

Validate a schema:

```bash
cratestack check --schema schema.cstack
```

Add the facade that matches what you're building:

```toml
[dependencies]
cratestack = { package = "cratestack-pg", version = "0.11" }
```

And point the macro at your schema:

```rust
use cratestack::include_server_schema;

include_server_schema!("schema.cstack", db = Postgres);
```

That's the whole integration. `cratestack_schema` now exists, fully typed.

The [quickstart guide](https://cratestack.dev/getting-started/quickstart) walks
through a complete service. To just watch something run:

```bash
git clone https://github.com/cratestack/cratestack.git && cd cratestack
cargo run --example sqlite_quickstart -p cratestack-sqlite
```

## Pick your facade

All four expose their library as `cratestack`, so you select one with Cargo's
`package =` rename and your code reads the same either way.

| You're building | Dependency | Entry macro |
| --- | --- | --- |
| An HTTP service that owns a Postgres database | `cratestack = { package = "cratestack-pg", version = "0.11" }` | `include_server_schema!("schema.cstack", db = Postgres)` |
| A procedures-only service with no database at all | `cratestack = { package = "cratestack-api", version = "0.11" }` | `include_server_schema!("schema.cstack", db = None)` |
| A mobile / desktop / browser app with a local SQLite | `cratestack = { package = "cratestack-sqlite", version = "0.11" }` | `include_embedded_schema!("schema.cstack")` |
| A crate that only *calls* a CrateStack service | `cratestack = { package = "cratestack-client", version = "0.11" }` | `include_client_schema!("../schemas/billing.cstack")` |

**Pick exactly one per crate.** The macros are strictly disjoint by design and
the split is enforced by CI, not just documented:

- `cratestack-pg` does not pull `libsqlite3-sys`, so it coexists with the
  official `sqlx` umbrella without `links = "sqlite3"` collisions.
- `cratestack-api` has no `cratestack-sqlx` dependency under any feature gate.
- `cratestack-client` has no `cratestack-axum` — and therefore no
  `axum`/`tower`/`hyper`/`tower-http` — in its graph under default features.

The `facade-disjointness` CI job proves the last two with a real `cargo tree` over
[`examples/client-only-verification`](examples/client-only-verification) and
[`examples/no-database-verification`](examples/no-database-verification).

## What you get

### Server (`include_server_schema!`)

- sqlx-backed `FromRow<PgRow>` impls, model descriptors, and a `Cratestack`
  runtime over `sqlx::PgPool`
- generated Axum CRUD and procedure routes, with host-owned auth wiring through
  an `AuthProvider` you implement
- **policy enforcement** on models, views, and procedures from `@@allow` /
  `@@deny`
- list-route query parsing for `fields`, `include`, `includeFields[path]`,
  `sort`, `limit`, `offset`, scalar filters, grouped `where`, and relation
  filters — with route-level validation errors for unknown or disallowed
  selections
- **SQL views** — read-only projections over one or more models, with
  server-side `@@materialized` and `refresh()` via
  `REFRESH MATERIALIZED VIEW CONCURRENTLY` ([ADR-0003](https://cratestack.dev/internals/views-adr))
- events and `@@emit` subscriptions, plus a transactional outbox
  (`cratestack-outbox`) for persisting a domain event in the same transaction as
  the write it accompanies
- `tracing` instrumentation on generated routes; subscriber and exporter setup
  stays yours
- Redis-backed idempotency and rate-limit stores (`cratestack-redis`)

### Embedded (`include_embedded_schema!`)

The same schema, driving a local SQLite database — from one source to three
targets:

| Target | How |
| --- | --- |
| Native mobile (iOS, Android) | FFI / `flutter_rust_bridge` |
| Native desktop (Linux, macOS, Windows) | direct |
| Browser | `wasm32-unknown-unknown` + OPFS persistence via `sqlite-wasm-rs` |

It's a **sync** API — `rusqlite` with bundled SQLite, no tokio on the data path
— which means smaller binaries and much friendlier FFI and JS bridging.

Policies parse but are **not enforced** here, deliberately: clients are
untrusted, and authorization is the server's job.

### Clients

| Language | Generated by | Ships |
| --- | --- | --- |
| Rust | `include_client_schema!` | typed procedure clients + a reqwest-backed `Client` facade |
| Dart / Flutter | `cratestack generate-dart` | models, selection builders, API facades, `--preset riverpod` layer |
| TypeScript | `cratestack generate-typescript` | framework-neutral fetch client, TanStack Query hooks, `--swr` layer |

Every generated client speaks the same HTTP projection contract the routes parse,
so `fields`, `include`, `sort`, and `where` behave identically across languages.

### Transports

A schema declares **REST routes** (the default) or **`transport rpc`**, which
collapses the surface to `POST /rpc/{op_id}` and `POST /rpc/batch`, dispatched by
a generated match on dotted op IDs. The two are mutually exclusive per schema and
ship together — every request/response feature lands on both.

## Ecosystem

**Rust crates** — [`cratestack-pg`](https://crates.io/crates/cratestack-pg) ·
[`cratestack-api`](https://crates.io/crates/cratestack-api) ·
[`cratestack-sqlite`](https://crates.io/crates/cratestack-sqlite) ·
[`cratestack-client`](https://crates.io/crates/cratestack-client) ·
[`cratestack-cli`](https://crates.io/crates/cratestack-cli), plus
`cratestack-auth` (Ed25519 request signing, SD-JWT identity tokens, multi-issuer
JWKS), `cratestack-outbox`, `cratestack-service` (env config, health checks,
graceful shutdown), and `cratestack-migrate`.

`cratestack-exec` is the transport-neutral execution layer (L3) that owns
idempotency admission; you don't depend on it directly — it arrives transitively
through whichever facade your schema selected, and the HTTP entry point stays
`cratestack_axum::idempotency::IdempotencyLayer`, now a thin adapter over
`OpExecutor::admit`.

**npm packages** — [`@cratestack/cli`](https://www.npmjs.com/package/@cratestack/cli),
`@cratestack/ts-types`, `@cratestack/runtime-fetch`, `@cratestack/runtime-axios`,
`@cratestack/adapter-tanstack-query`, `@cratestack/adapter-rtk`,
`@cratestack/refine`, `@cratestack/validator-zod`, `@cratestack/validator-yup`,
`@cratestack/link-batch`, `@cratestack/link-logger`, and the CBOR codecs
`@cratestack/cbor`, `@cratestack/cbor-node`, `@cratestack/cbor-web`.

**Dart packages** — `cratestack_cbor` on pub.dev.

**Editor tooling** — the `CrateStack Schema` extension for `.cstack` files, on
the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=cratestack.cratestack-vscode-plugin)
and [Open VSX](https://open-vsx.org/extension/cratestack/cratestack-vscode-plugin).
It wraps `cratestack-lsp`, a standalone language server you can point any
LSP-capable editor at.

**Studio** — an admin and testing surface over your schemas. Describe the
workspace once in a `studio.toml` and the shipped binary serves the UI:

```bash
cratestack studio init      # writes ./studio.toml
cratestack studio run       # binds 127.0.0.1:7878
```

`cratestack studio eject --out <dir>` gives you a self-contained starter crate to
customize; add `--with-ui` for the Leptos front-end sources.

## Examples

Thirty-odd runnable, end-to-end projects live in [`examples/`](examples) — full
index in [`examples/README.md`](examples/README.md). A sample:

| | Run |
| --- | --- |
| Smallest embedded program (in-memory DB) | `cargo run --example sqlite_quickstart -p cratestack-sqlite` |
| Postgres server + Axum + procedures | `cargo run --example server_basic -p cratestack-pg` |
| Note-taking CLI on file-backed SQLite | `cargo run -p embedded-cli-example -- --db /tmp/notes.db add "First"` |
| Microservice: owns a DB *and* calls upstream | `cargo run -p microservice-pair-example` |
| BFF / orchestrator fanning out to two services | `cargo run -p client-multi-service-example` |
| Browser: wasm + OPFS + Vite | `examples/embedded-browser-vite` |
| React 19 + Vite + Tailwind + DaisyUI | `examples/react-vite-daisyui` |
| Next.js 16: wasm in the browser, napi on the server, typed HTTP client upstream | `examples/react-nextjs-daisyui` |
| Flutter + Riverpod, generated Dart client | `examples/flutter-riverpod` |
| Tauri 2 desktop shell | `examples/tauri-web` |

## Project status

**CrateStack is pre-1.0.** The public crates version together and minor releases
can break. Known limits worth knowing before you adopt it:

- `db = Postgres` is the only sqlx backend today. The parser is wired so adding
  others is non-breaking at existing call sites.
- Generated routers enforce a **single configured codec** rather than negotiating
  per request. `application/cbor-seq` is a documented target, not an
  implementation.
- The embedded backend does not enforce `@@allow` / `@@deny`. This is a design
  decision, not a gap — see above.
- Exact typed non-Rust client generation across arbitrary projection shapes is
  still stabilizing.
- Runtime custom-field resolution beyond the generated trait metadata isn't
  supported.

Ecosystem-wide breaking changes are documented in [`CHANGELOG.md`](CHANGELOG.md).
What's shipped, what's being built, and what's been ruled out is in
[`ROADMAP.md`](ROADMAP.md).

## Contributing

Contributions are welcome, including your first one.

- **[Your first contribution](docs/contributing/first-contribution.md)** — a
  start-to-finish walkthrough assuming no prior knowledge of this codebase.
- **[Filing an issue](docs/contributing/filing-an-issue.md)** — which form to
  pick and what makes a report we can act on.
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — the full development workflow.
- **[Code of Conduct](CODE_OF_CONDUCT.md)** · **[Security policy](SECURITY.md)**

Good places to start: [`good first issue`](https://github.com/cratestack/cratestack/labels/good%20first%20issue)
and [`help wanted`](https://github.com/cratestack/cratestack/labels/help%20wanted).

Building from source:

```bash
# --exclude embedded_flutter_native: that crate needs flutter_rust_bridge-generated
# glue that isn't checked in, so a bare --workspace build fails on a fresh checkout.
cargo build --workspace --exclude embedded_flutter_native
cargo test  --workspace --exclude embedded_flutter_native
just all-checks     # the canonical pre-PR gate: fmt, clippy, check, cargo-deny
```

Never pass `--all-features`: it enables both mutually-exclusive `decimal-*`
backends and trips a `compile_error!` in `cratestack-core`.

### AI governance

This project follows the
[ADORSYS-GIS AI Governance](https://adorsys-gis.github.io/ai-governance/)
discipline: **AI may accelerate the work, but humans own intent, verification,
and consequences.** Every pull request declares how AI was used, links a source
of truth, and shows verification evidence — enforced by CI. Using an assistant is
fine; submitting work you can't explain is not.

## License

MIT — see [LICENSE](LICENSE).
