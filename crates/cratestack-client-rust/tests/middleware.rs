//! `middleware` feature (cratestack#926): a
//! `reqwest_middleware::ClientWithMiddleware` really does run under
//! `CratestackClient`, and the per-request `RequestIdempotency`
//! extension really is readable from inside a middleware.
//!
//! `#![cfg(feature = "middleware")]` rather than a `required-features`
//! `[[test]]` entry: this file compiles to an empty 0-test binary in a
//! default build, so a plain `cargo test` neither fails nor pretends to
//! have covered anything. `just test-ci-host` runs the feature-on pass.
//!
//! The retry middleware here is hand-rolled rather than `reqwest-retry`
//! on purpose. What is under test is *this crate's* contract — that the
//! extension is present and carries the right value — and asserting it
//! against a policy we wrote makes the assertion direct instead of
//! routed through a third crate's own retry heuristics. The README
//! shows the real `reqwest-retry` + `reqwest-tracing` wiring.

#![cfg(feature = "middleware")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::{IntoResponse, Response as AxumResponse};
use axum::routing::get;
use cratestack_client_rust::{
    CborCodec, ClientConfig, ClientError, CratestackClient, RequestIdempotency, RpcClient,
    RpcClientError, ensure_crypto_provider,
};
use cratestack_core::{CratestackCodec, CratestackErrorResponse};
use http::Extensions;
use reqwest::{Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use url::Url;

/// Counts every request that reaches it, then forwards unchanged.
struct CountingMiddleware {
    seen: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Middleware for CountingMiddleware {
    async fn handle(
        &self,
        request: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        self.seen.fetch_add(1, Ordering::SeqCst);
        next.run(request, extensions).await
    }
}

/// Retries a 503 up to `max_retries` times — but only when the request
/// is marked idempotent. A request with no `RequestIdempotency`
/// extension at all fails closed (no retry), same rule the rest of the
/// crate follows.
struct IdempotencyAwareRetryMiddleware {
    max_retries: usize,
    attempts: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Middleware for IdempotencyAwareRetryMiddleware {
    async fn handle(
        &self,
        request: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        // The one read this whole feature exists to make possible.
        // Absent extension → fail closed, i.e. do not replay.
        let replayable = extensions
            .get::<RequestIdempotency>()
            .copied()
            .unwrap_or(RequestIdempotency::NOT_IDEMPOTENT)
            .is_idempotent();

        let mut last = None;
        for _ in 0..=self.max_retries {
            let attempt = request
                .try_clone()
                .expect("a buffered body is always cloneable");
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let response = next.clone().run(attempt, extensions).await?;
            if !replayable || response.status() != StatusCode::SERVICE_UNAVAILABLE {
                return Ok(response);
            }
            last = Some(response);
        }
        Ok(last.expect("the loop body runs at least once"))
    }
}

/// Fails a middleware outright, to pin the `reqwest_middleware::Error`
/// → [`ClientError`] mapping.
struct AlwaysFailsMiddleware;

#[async_trait::async_trait]
impl Middleware for AlwaysFailsMiddleware {
    async fn handle(
        &self,
        _request: Request,
        _extensions: &mut Extensions,
        _next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        Err(reqwest_middleware::Error::middleware(
            std::io::Error::other("token refresh failed"),
        ))
    }
}

#[derive(Clone, Default)]
struct ServerState {
    /// How many 503s each route has left to emit before succeeding.
    failures_remaining: Arc<AtomicUsize>,
}

/// Emits `503` until `failures_remaining` is exhausted, then a real
/// CBOR-encoded `"ok"`. The content type matters: the client decodes
/// through its codec, so an untyped `application/octet-stream` body
/// fails before the retry assertion is ever reached.
async fn flaky(State(state): State<ServerState>) -> AxumResponse {
    let (status, body) = if state.failures_remaining.load(Ordering::SeqCst) > 0 {
        state.failures_remaining.fetch_sub(1, Ordering::SeqCst);
        (StatusCode::SERVICE_UNAVAILABLE, encode(&unavailable()))
    } else {
        (StatusCode::OK, encode(&"ok"))
    };
    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(CborCodec::CONTENT_TYPE),
    );
    response
}

/// A real server-shaped error body, so a 503 decodes into
/// `ClientError::Remote` rather than the "missing Content-Type"
/// fallback — the assertion under test is about the *status*, and it
/// should not be able to pass for the wrong reason.
fn unavailable() -> CratestackErrorResponse {
    CratestackErrorResponse {
        code: "unavailable".to_owned(),
        message: "try again".to_owned(),
        details: None,
    }
}

fn encode<T: serde::Serialize>(value: &T) -> Vec<u8> {
    CborCodec.encode(value).expect("encoding cannot fail here")
}

async fn spawn_server(failures: usize) -> (Url, ServerState, tokio::task::JoinHandle<()>) {
    let state = ServerState {
        failures_remaining: Arc::new(AtomicUsize::new(failures)),
    };
    let app = Router::new()
        .route("/thing", get(flaky).post(flaky))
        // Same handler behind the RPC dispatch path, so the parity test
        // below exercises `RpcClient` against identical server behaviour.
        .route("/rpc/model.Thing.list", axum::routing::post(flaky))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    let base_url = Url::parse(&format!("http://{address}/")).expect("base URL should parse");
    (base_url, state, handle)
}

fn middleware_client<M: Middleware>(middleware: M) -> ClientWithMiddleware {
    // #440: the inner `reqwest::Client` is built here, in the caller,
    // so `CratestackClient::new`'s own fallback never runs. This is the
    // precondition `with_middleware_client`'s doc comment names.
    ensure_crypto_provider();
    ClientBuilder::new(reqwest::Client::new())
        .with(middleware)
        .build()
}

#[tokio::test]
async fn middleware_runs_for_every_request_the_client_sends() {
    let (base_url, _state, _server) = spawn_server(0).await;
    let seen = Arc::new(AtomicUsize::new(0));
    let client = CratestackClient::with_middleware_client(
        ClientConfig::new(base_url),
        CborCodec,
        middleware_client(CountingMiddleware { seen: seen.clone() }),
    );

    let _: String = client
        .get("/thing", &[], &[])
        .await
        .expect("GET should succeed");
    let _: String = client
        .post("/thing", &"payload", &[])
        .await
        .expect("POST should succeed");

    assert_eq!(
        seen.load(Ordering::SeqCst),
        2,
        "both the REST GET and the REST POST must traverse the middleware chain"
    );
}

#[tokio::test]
async fn a_get_is_marked_idempotent_and_is_retried() {
    // Two 503s, then a 200 — only reachable if the middleware retried.
    let (base_url, _state, _server) = spawn_server(2).await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let client = CratestackClient::with_middleware_client(
        ClientConfig::new(base_url),
        CborCodec,
        middleware_client(IdempotencyAwareRetryMiddleware {
            max_retries: 3,
            attempts: attempts.clone(),
        }),
    );

    let body: String = client
        .get("/thing", &[], &[])
        .await
        .expect("a GET carries RequestIdempotency::IDEMPOTENT, so the retry policy replays it");

    assert_eq!(body, "ok");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        3,
        "two failures plus the successful replay"
    );
}

#[tokio::test]
async fn a_post_is_not_marked_idempotent_and_is_not_retried() {
    let (base_url, _state, _server) = spawn_server(2).await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let client = CratestackClient::with_middleware_client(
        ClientConfig::new(base_url),
        CborCodec,
        middleware_client(IdempotencyAwareRetryMiddleware {
            max_retries: 3,
            attempts: attempts.clone(),
        }),
    );

    let error = client
        .post::<_, String>("/thing", &"payload", &[])
        .await
        .expect_err("a POST must not be replayed on a 503");

    match error {
        ClientError::Remote { status, .. } => {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        }
        other => panic!("expected the first 503 to surface as ClientError::Remote, got {other:?}"),
    }
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "the non-idempotent POST is sent exactly once"
    );
}

/// The override is the whole point of `with_idempotency`: a `@query`
/// procedure is a `POST` on the wire and the method-derived default
/// therefore gets it wrong.
#[tokio::test]
async fn with_idempotency_overrides_the_method_derived_default_for_a_post() {
    let (base_url, _state, _server) = spawn_server(2).await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let client = CratestackClient::with_middleware_client(
        ClientConfig::new(base_url),
        CborCodec,
        middleware_client(IdempotencyAwareRetryMiddleware {
            max_retries: 3,
            attempts: attempts.clone(),
        }),
    )
    .with_idempotency(RequestIdempotency::IDEMPOTENT);

    let body: String = client
        .post("/thing", &"payload", &[])
        .await
        .expect("an explicitly idempotent POST is replayable");

    assert_eq!(body, "ok");
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn a_middleware_failure_maps_to_client_error_middleware() {
    let (base_url, _state, _server) = spawn_server(0).await;
    let client = CratestackClient::with_middleware_client(
        ClientConfig::new(base_url),
        CborCodec,
        middleware_client(AlwaysFailsMiddleware),
    );

    let error = client
        .get::<String>("/thing", &[], &[])
        .await
        .expect_err("the middleware fails the request");

    match &error {
        ClientError::Middleware(inner) => {
            assert!(
                inner.to_string().contains("token refresh failed"),
                "the middleware's own message must survive: {inner}"
            );
        }
        other => panic!("expected ClientError::Middleware, got {other:?}"),
    }

    // A middleware failure must NOT be misreported as a transport
    // failure, and must still chain for anyhow/tracing-style callers.
    assert!(!matches!(error, ClientError::Transport(_)));
    assert!(
        std::error::Error::source(&error).is_some(),
        "ClientError::Middleware should chain to the middleware's error"
    );
}

/// Transport parity (repo rule: REST and RPC ship together). Every RPC
/// call is `POST /rpc/{op_id}`, so the method-derived default marks all
/// of them non-idempotent — reads included. `RpcClient::with_idempotency`
/// is the escape hatch, and this pins both halves of its behaviour.
#[tokio::test]
async fn rpc_with_idempotency_controls_whether_a_post_is_replayed() {
    for (idempotency, failures, expected_attempts, should_succeed) in [
        (Some(RequestIdempotency::IDEMPOTENT), 2usize, 3usize, true),
        (None, 2, 1, false),
    ] {
        let (base_url, _state, _server) = spawn_server(failures).await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let rest = CratestackClient::with_middleware_client(
            ClientConfig::new(base_url),
            CborCodec,
            middleware_client(IdempotencyAwareRetryMiddleware {
                max_retries: 3,
                attempts: attempts.clone(),
            }),
        );
        let mut rpc = RpcClient::new(rest);
        if let Some(idempotency) = idempotency {
            rpc = rpc.with_idempotency(idempotency);
        }

        let result = rpc.call::<_, String>("model.Thing.list", &"input").await;

        assert_eq!(
            result.is_ok(),
            should_succeed,
            "idempotency={idempotency:?} should_succeed={should_succeed}, got {result:?}"
        );
        if !should_succeed {
            assert!(
                matches!(result, Err(RpcClientError::Remote(_))),
                "a non-replayed 503 surfaces as RpcClientError::Remote"
            );
        }
        assert_eq!(attempts.load(Ordering::SeqCst), expected_attempts);
    }
}
