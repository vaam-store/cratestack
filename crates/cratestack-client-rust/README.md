# cratestack-client-rust

Rust HTTP client runtime for CrateStack services.

## Overview

`cratestack-client-rust` provides the typed client runtime that `include_client_schema!` builds its generated `client::Client` surface on top of. It owns the HTTP transport, codec negotiation, request authorization hook, and optional offline state journaling.

The CBOR and JSON codecs are re-exported as `CborCodec` and `JsonCodec`.

## Installation

```toml
[dependencies]
cratestack-client-rust = "0.7"
tokio = { version = "1", features = ["rt-multi-thread"] }
url = "2"
```

## Usage

```rust
use cratestack::include_client_schema;
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

include_client_schema!("../schemas/api.cstack");

let base_url = url::Url::parse("https://api.example.com")?;
let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
let client = cratestack_schema::client::Client::new(runtime);
```

This is the REST transport. For a schema declaring `transport rpc`, the generated
`cratestack_schema::rpc::Client` is built on top of `RpcClient` instead — see
[RPC Transport](#rpc-transport) below.

## RPC Transport

For schemas declaring `transport rpc`, `include_client_schema!` generates an RPC client
built on `RpcClient` rather than `CratestackClient`. `RpcClient` shares its transport,
codec, and state store with the REST client (both can be used side-by-side against the
same server) but dispatches unary calls to `POST /rpc/{op_id}` and supports request
batching via `BatchBuilder`/`BatchHandle`/`BatchableCall`, plus streamed responses via
`RpcStream`. See `docs/design/rpc-transport.md` in the repo for the wire-format spec.

## Codecs

```rust
use cratestack_client_rust::{CborCodec, JsonCodec};

let cbor_client = CratestackClient::new(config.clone(), CborCodec);
let json_client = CratestackClient::new(config, JsonCodec);
```

## Request Authorization

`with_request_authorizer` attaches an implementation of `RequestAuthorizer` that returns extra headers per call. The trait gets a canonical-request string the implementer can sign. `authorize` is `async` (issue #453), so credential providers that need to make a network call — refreshing a cached OAuth2 token, for instance — can do so directly instead of pre-fetching or blocking on the runtime:

```rust
use std::sync::Arc;
use cratestack_client_rust::{AuthorizationRequest, ClientError, RequestAuthorizer};

struct HmacAuthorizer { key: Vec<u8> }

#[async_trait::async_trait]
impl RequestAuthorizer for HmacAuthorizer {
    async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        let sig = sign(&self.key, &request.canonical_request_string());
        Ok(vec![(
            "authorization".to_owned(),
            format!("Signature {}", hex::encode(sig)),
        )])
    }
}

let client = runtime.with_request_authorizer(Arc::new(HmacAuthorizer { key }));
```

## HTTP Middleware

`middleware` feature (off by default; adds nothing to a default build's dependency
graph). It lets you hand the client a `reqwest_middleware::ClientWithMiddleware`
instead of a bare `reqwest::Client`, so retries, tracing spans, caching, or any other
`reqwest_middleware::Middleware` run under every request the generated client makes.

```toml
[dependencies]
cratestack-client-rust = { version = "0.11", features = ["middleware"] }
# The 0.13-era middleware crates — earlier lines require reqwest ^0.12 and
# would fork the dependency graph in two.
reqwest-middleware = "0.5"
reqwest-retry = "0.9"
reqwest-tracing = "0.7"
```

```rust
use std::time::Duration;

use cratestack_client_rust::{
    CborCodec, ClientConfig, CratestackClient, ensure_crypto_provider,
};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::RetryTransientMiddleware;
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_tracing::TracingMiddleware;

// Install a rustls crypto provider BEFORE building the inner
// `reqwest::Client` — see "Crypto provider" below.
ensure_crypto_provider();

let retry_policy = ExponentialBackoff::builder()
    .retry_bounds(Duration::from_millis(200), Duration::from_secs(8))
    .build_with_max_retries(4);

let http = ClientBuilder::new(reqwest::Client::new())
    .with(TracingMiddleware::default())
    .with(RetryTransientMiddleware::new_with_policy(retry_policy))
    .build();

let client = CratestackClient::with_middleware_client(
    ClientConfig::new(base_url),
    CborCodec,
    http,
);
```

### Crypto provider

`new` installs a `ring` fallback provider for you (issue #440). `with_http_client` and
`with_middleware_client` cannot: you build the inner `reqwest::Client` yourself, and
under reqwest's `rustls-no-provider` feature *that* is the call which panics when no
provider is installed. Call `ensure_crypto_provider()` (or install your own) first.

### Per-request idempotency

Retrying blindly is how a mobile client double-charges someone. Every request the
client sends carries a `RequestIdempotency` extension a middleware reads to decide
whether replaying is safe:

```rust
use cratestack_client_rust::RequestIdempotency;
use http::Extensions;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};

struct RetryIfReplayable;

#[async_trait::async_trait]
impl Middleware for RetryIfReplayable {
    async fn handle(
        &self,
        request: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        // Absent extension → fail closed: do not retry.
        let replayable = extensions
            .get::<RequestIdempotency>()
            .copied()
            .unwrap_or(RequestIdempotency::NOT_IDEMPOTENT)
            .is_idempotent();
        // ... retry only when `replayable` ...
        next.run(request, extensions).await
    }
}
```

The value defaults to the RFC 9110 answer for the HTTP method — `GET`/`DELETE`
idempotent, `POST`/`PATCH` not — which is exactly right for the generated REST CRUD
routes and wrong for the two cases a method cannot express:

- **RPC.** Every RPC call is `POST /rpc/{op_id}`, so the method says nothing. The real
  answer is `idempotent_by_default` on the `OpDescriptor` the schema macro emits.
- **REST `@query` procedures.** `POST /$procs/<name>` is a `POST` whatever the
  procedure does.

Override per call with a cheap clone — `CratestackClient::with_idempotency` for REST,
`RpcClient::with_idempotency` for RPC:

```rust
let response = rpc
    .clone()
    .with_idempotency(RequestIdempotency::new(OP.idempotent_by_default))
    .call::<_, Output>(OP.op_id, &input)
    .await?;
```

The extension is only observable under the `middleware` feature: reqwest 0.13 keeps
`Request::extensions_mut()` `pub(crate)`, so a bare `reqwest::Client` has no extension
map a caller can write to or read from.

## State Persistence

Journal requests for replay or offline recovery. The bundled implementations are `InMemoryStateStore` and `JsonFileStateStore`; the trait is `ClientStateStore`.

```rust
use std::sync::Arc;
use cratestack_client_rust::{ClientStateStore, JsonFileStateStore};

let store: Arc<dyn ClientStateStore> = Arc::new(JsonFileStateStore::new("./client_state.json"));
let runtime = runtime.with_state_store(store);
```

`with_optional_state_store(None)` is a no-op convenience for configuration-driven setup.

For a Redis-backed store, see `cratestack-client-store-redis`. For a SQLite-backed store, see `cratestack-client-store-sqlite`.

## See Also

- [Client Runtime](https://cratestack.dev/architecture/client-runtime)
- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `docs/design/rpc-transport.md` — RPC wire-format spec
- `cratestack-codec-cbor` — CBOR codec
- `cratestack-codec-json` — JSON codec
- `cratestack-client-store-redis` — Redis-backed `ClientStateStore`
- `cratestack-client-store-sqlite` — SQLite-backed `ClientStateStore`

## License

MIT
