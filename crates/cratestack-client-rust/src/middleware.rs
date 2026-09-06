//! `reqwest-middleware` integration (cratestack#926) — the `middleware`
//! feature.
//!
//! Everything here is additive: the default build compiles none of it
//! and resolves no extra dependency. See
//! [`CratestackClient::with_middleware_client`] for the constructor and
//! the crate README's "HTTP middleware" section for a
//! `reqwest-retry` + `reqwest-tracing` stack.

use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;

use crate::client::CratestackClient;
use crate::client::http::HttpClient;
use crate::codec::HttpClientCodec;
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::state::InMemoryStateStore;

/// A failure raised by a middleware in the chain, rather than by the
/// HTTP transport underneath it.
///
/// Opaque on purpose, exactly like `TransportError`: the concrete
/// payload is `reqwest_middleware`'s `anyhow::Error`, and naming that
/// type in a public signature would make an `anyhow` major a breaking
/// change for this crate. Walk into it with
/// [`std::error::Error::source`], or read the message via `Display`.
///
/// A `reqwest_middleware::Error::Reqwest` does **not** land here — it
/// maps to [`ClientError::Transport`], the same variant a plain-transport
/// failure has always produced, so switching a client onto a middleware
/// stack does not silently reclassify network errors.
#[derive(Debug)]
pub struct MiddlewareError {
    inner: Box<dyn std::error::Error + Send + Sync>,
}

impl std::fmt::Display for MiddlewareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for MiddlewareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.inner.as_ref())
    }
}

impl From<reqwest_middleware::Error> for ClientError {
    fn from(error: reqwest_middleware::Error) -> Self {
        match error {
            reqwest_middleware::Error::Reqwest(error) => Self::from(error),
            reqwest_middleware::Error::Middleware(error) => Self::Middleware(MiddlewareError {
                // `anyhow::Error: Into<Box<dyn Error + Send + Sync>>`,
                // which is what keeps `anyhow` out of this crate's own
                // dependency list.
                inner: error.into(),
            }),
        }
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Build a client whose every request runs through `http`'s
    /// middleware chain — retries, tracing spans, caching, or anything
    /// else implementing `reqwest_middleware::Middleware`.
    ///
    /// The middleware counterpart of
    /// [`with_http_client`](Self::with_http_client), and identical to it
    /// in every other respect: same in-memory state store, no request
    /// authorizer, no schema SHA, all attachable afterwards with the
    /// usual `with_*` builders.
    ///
    /// Each request carries a
    /// [`RequestIdempotency`](crate::RequestIdempotency) extension a
    /// retry middleware can read to decide whether replaying is safe —
    /// derived from the HTTP method unless
    /// [`with_idempotency`](Self::with_idempotency) overrode it.
    ///
    /// # Crypto provider
    ///
    /// Unlike [`new`](Self::new), this constructor cannot install the
    /// `rustls` crypto provider for you: you have already built the
    /// `reqwest::Client` inside `http` by the time it is called, and
    /// under reqwest's `rustls-no-provider` feature (#440) that is the
    /// call that panics when no provider is installed. Call
    /// [`ensure_crypto_provider`](crate::ensure_crypto_provider) — or
    /// install your own — *before* building the inner client. The same
    /// applies to [`with_http_client`](Self::with_http_client).
    ///
    /// ```
    /// use cratestack_client_rust::{
    ///     CborCodec, ClientConfig, CratestackClient, ensure_crypto_provider,
    /// };
    /// use reqwest_middleware::ClientBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Before any `reqwest::Client` is constructed.
    /// ensure_crypto_provider();
    ///
    /// let http = ClientBuilder::new(reqwest::Client::new())
    ///     // .with(RetryTransientMiddleware::new_with_policy(policy))
    ///     // .with(TracingMiddleware::default())
    ///     .build();
    ///
    /// let client = CratestackClient::with_middleware_client(
    ///     ClientConfig::new("https://api.example.com".parse()?),
    ///     CborCodec,
    ///     http,
    /// );
    /// # let _ = client;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_middleware_client(
        config: ClientConfig,
        codec: C,
        http: ClientWithMiddleware,
    ) -> Self {
        Self {
            http: HttpClient::Middleware(http),
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
            schema_sha: None,
            idempotency: None,
        }
    }
}
