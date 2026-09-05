# Contributing

CrateStack is early public-release software. Keep changes small, tested, and aligned with the
schema-first framework boundary described in `README.md`.

> **New here?** Read **[Your first contribution](docs/contributing/first-contribution.md)** instead
> — it's a start-to-finish walkthrough (clone, build, change, test, PR) that assumes no prior
> knowledge of this codebase. This page is the reference for the details it summarises.

## Ways to contribute

All of these are real contributions. They're listed roughly by how much context they need:

| | Where to start |
| --- | --- |
| Report a bug | [Bug report form](https://github.com/cratestack/cratestack/issues/new?template=bug-report.yml) — the smallest reproducing `.cstack` schema is worth more than anything else you can add |
| Ask a question | [Question form](https://github.com/cratestack/cratestack/issues/new?template=question.yml) — if the docs didn't answer it, that's useful signal |
| Fix docs, a typo, or a stale command | Open a PR directly; see [Your first contribution](docs/contributing/first-contribution.md) |
| Suggest a feature | [Idea form](https://github.com/cratestack/cratestack/issues/new?template=feature-request.yml) — describe the problem, not just the solution |
| Fix a bug or build a feature | [`good first issue`](https://github.com/cratestack/cratestack/labels/good%20first%20issue) · [`help wanted`](https://github.com/cratestack/cratestack/labels/help%20wanted) |
| Add an example | `examples/` — see its [README](examples/README.md) for the two homes examples live in |

Not sure which issue form applies? [Filing an issue](docs/contributing/filing-an-issue.md) explains
the difference between the four short reporting forms (for you) and the three governance planning
forms (which maintainers fill in on your behalf).

Looking for something worth doing? [`ROADMAP.md`](ROADMAP.md) lists what's in flight,
what's unclaimed, and what's been deliberately ruled out.

Everyone participating here is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Conventions worth knowing before you write code

Two will otherwise cost you a review round-trip:

- **~200-line file ceiling.** Split larger files by concern rather than growing one — this is why
  `crates/cratestack-macros/` and `crates/cratestack-axum/` are nested so deeply. Enforced by CI
  (`file-length ceiling`).
- **REST and RPC ship together.** Anything touching the request/response surface (query parameters,
  projections, response shapes, client call surfaces) must land on **both** transports in the same
  PR — server dispatch, the `RpcListInput`/`RpcGetInput` frame slots, and every generated client
  (Rust/Dart/TS, plus the swr/riverpod layers). RPC dispatch synthesizes a URL query string and
  re-enters the REST parsing path (`crates/cratestack-axum/src/rpc/synthesize.rs`), so this is
  usually one frame field plus one `pairs.push`. Shipping REST-only has taken three follow-up PRs to
  correct before; that's why the rule exists.
- `unsafe_code = "forbid"` workspace-wide. The handful of FFI-boundary crates that override it each
  document why.
- Rust sources use `snake_case` filenames; everything else is `kebab-case`.

## Before opening a pull request

1. Run `cargo fmt`.
2. Run `cargo check --workspace --exclude embedded_flutter_native --all-targets`. (`just all-checks` wraps fmt + clippy + this check.) Do **not** add `--all-features`: it enables both mutually-exclusive `decimal-*` backends and trips a `compile_error!` in `cratestack-core`. Exclude `embedded_flutter_native` — its `flutter_rust_bridge`-generated glue isn't checked in, so a bare `--workspace` build fails with E0583.
3. Run `cargo test --workspace --exclude embedded_flutter_native`. PG-backed integration tests (`banking_*`, `policy_db_*`, `generated_client_rust`) skip cleanly when `CRATESTACK_TEST_DATABASE_URL` isn't set, so you only see partial coverage on this command.
4. Run `just test-pg` to exercise the PG-backed paths. The recipe brings the Postgres container in `compose.yml` up before tests and tears it down on exit — even if tests fail — so you never leave a container behind. Use `just test-pg-only` for the faster `cratestack`-crate-only inner loop.
   - **Alternative — testcontainers**: `just test-pg-tc` runs the same suite but with `CRATESTACK_USE_TESTCONTAINERS=1`, which makes each test binary spawn its own ephemeral PG via `testcontainers`. Cleanup is automatic via `Drop`; you'll see a per-binary spin-up cost of a few seconds. Use this when you want stronger isolation guarantees (CI does), accept that you can't `psql` into a mid-test container easily.
   - **On rootless Docker, set `DOCKER_HOST` first** — otherwise every DB-backed test skips and still prints `ok`. See below.
5. Run package-specific checks for editor or generated-client changes when applicable.

### Rootless Docker: `DOCKER_HOST` and the false green

If you run Docker rootless, `just test-pg-tc` (and any other `CRATESTACK_USE_TESTCONTAINERS=1` recipe)
will appear to pass while touching no database at all.

The `docker` CLI reads `docker context`, so it finds your rootless socket and every `docker` command
works. `testcontainers-rs`/`bollard` do **not** read `docker context` — they default to
`unix:///var/run/docker.sock`, which on a rootless host is either absent or `root:docker` and
unreadable by you. The container then fails to start, our `connect_or_skip()` helpers treat that as
"no database available", and the tests skip. A skipped test still reports `test result: ok`.

`docker info` succeeding is **not** evidence the Rust client can connect — it goes through the CLI.

Export the endpoint your context actually points at, derived rather than hardcoded:

```bash
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
```

The recipes deliberately don't set this for you: the value encodes your uid
(`unix:///run/user/1000/docker.sock`), and CI's runner uses the default socket, so baking one in would
fix one machine and break the other.

**Make a skip fail loudly instead of passing quietly.** `CRATESTACK_REQUIRE_DB=1` turns a failed
connection into a panic (`CRATESTACK_REQUIRE_REDIS=1` for the Redis suites). CI sets it on the DB
shard for exactly this reason, and it's worth setting locally whenever you're relying on a green run
as evidence:

```bash
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
export CRATESTACK_REQUIRE_DB=1
just test-pg-tc
```

**Telling a skip from a pass after the fact:** compare elapsed time, not the summary line. Both print
`ok`, and the skip notice goes to stderr, which cargo captures for passing tests. A real PG-backed
binary takes seconds; a skipped one reports `finished in 0.00s`.

Two more things worth knowing when running these locally:

- Prefer `--test-threads=1` for PG binaries. Each `connect_or_skip()` starts its own container, so a
  parallel run can hit a rootless-Docker port-bind race that looks like a test failure but is
  infrastructure flakiness — re-run the single binary in isolation to tell them apart.
- `just test-pg`/`test-pg-only` use `compose.yml`, which pins a specific Postgres image. If that image
  isn't cached, `just pg-up` waits on the pull with its output suppressed, which looks like a hang.
  `just test-pg-tc` avoids that path entirely.

Do not commit generated build output, local database state, or registry tokens.

## Changelog

Every PR with a user-visible change adds an entry under the `## Unreleased` heading at the top of
`CHANGELOG.md` (and `dart-packages/cratestack_cbor/CHANGELOG.md` if the change touches that package).
Don't create the heading yourself and don't file entries under the newest dated (`## X.Y.Z`) section —
that section belongs to an already-released version. A release bump promotes `## Unreleased` into a new
dated section and re-seeds a fresh, empty `## Unreleased` above it (`.ci/changelog-seed.sh`), so the
heading is always there for the next PR.

## AI Governance

This repository follows the [ADORSYS-GIS AI Governance](https://adorsys-gis.github.io/ai-governance/) discipline:
**AI may accelerate the work, but humans own intent, verification, and consequences.**

- **Open issues** with the structured forms — [Epic](.github/ISSUE_TEMPLATE/epic.yml),
  [User Story](.github/ISSUE_TEMPLATE/user-story.yml), or
  [Development Ticket](.github/ISSUE_TEMPLATE/dev-ticket.yml). Blank issues are disabled on purpose.
- **Open pull requests** using the [pull request template](.github/PULL_REQUEST_TEMPLATE.md). Fill in every section.
- Always complete the **AI Usage Declaration**, link a **source of truth** (a URL or `#123` reference), and attach **verification evidence** (commands, logs, links, or checked boxes).

A governance CI check (`.github/workflows/governance.yml`) enforces that every PR body declares AI usage,
references a source of truth, and shows verification evidence. Work is **Ready** only when its intent is clear,
its source of truth is linked, and any AI-generated content has been reviewed by a human; it is **Done** only
when acceptance criteria are met, tests pass, evidence is attached, and a named human owner accepts
responsibility — see the [AI Working Agreement](https://adorsys-gis.github.io/ai-governance/12-ai-working-agreement)
and the [Doctrine](https://adorsys-gis.github.io/ai-governance/13-doctrine).
