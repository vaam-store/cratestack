use std::sync::Arc;

use cratestack_codec_cbor::CborCodec;

use crate::auth::RequestAuthorizer;
use crate::client::http::HttpClient;
use crate::codec::HttpClientCodec;
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::idempotency::RequestIdempotency;
use crate::state::{ClientStateStore, InMemoryStateStore, PersistedClientState};

/// Installs a `ring`-backed `rustls::crypto::CryptoProvider` if the process
/// doesn't already have one (#440).
///
/// `reqwest`'s `rustls-no-provider` feature — deliberately chosen over
/// `rustls` so this crate stops forcing `aws-lc-rs` on every consumer of
/// `cratestack-pg` (see the workspace `Cargo.toml`'s `reqwest` entry) — ships
/// no crypto provider at all: `reqwest::Client::new()`/`ClientBuilder::build()`
/// PANIC at construction time if `rustls::crypto::CryptoProvider::get_default()`
/// finds nothing installed. Unlike the old `rustls` feature's silent
/// `aws-lc-rs` install, that's a worse zero-config default than what this
/// crate had before, not merely a neutral one.
///
/// `install_default()` only ever takes effect the FIRST time it succeeds
/// process-wide — it's a courtesy fallback, not an override. A consumer that
/// installs its own provider (any backend, including `aws-lc-rs`) before
/// constructing its first `CratestackClient` keeps that choice; this only
/// fires when nobody has chosen anything yet, which is exactly the gap
/// `rustls-no-provider` otherwise turns into a panic. The `Err` it returns
/// on a race with another caller installing first (or a no-op call to this
/// same function from a second `CratestackClient::new`) is expected and
/// intentionally ignored.
///
/// Public since cratestack#926: [`CratestackClient::with_http_client`]
/// and `with_middleware_client` both take an already-built
/// `reqwest::Client`, so the panic happens in the *caller's* code,
/// before this crate gets a chance to install anything. Callers on
/// those paths need a way to say "install the fallback now", and this
/// is it — idempotent, safe to call from anywhere, and a no-op once any
/// provider is installed.
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Clone)]
pub struct CratestackClient<C = CborCodec> {
    pub(crate) http: HttpClient,
    pub(crate) config: ClientConfig,
    pub(crate) codec: C,
    pub(crate) state_store: Arc<dyn ClientStateStore>,
    pub(crate) request_authorizer: Option<Arc<dyn RequestAuthorizer>>,
    /// The generating schema's `SCHEMA_SHA256` (issue #178) — `None` for a
    /// hand-constructed client that isn't wrapped by generated `Client`
    /// code (e.g. a bare `CratestackClient` in a test). Set automatically
    /// by the schema-generated `Client::new` wrapper, never by the schema
    /// author directly. Sent as `x-cratestack-schema-sha` on every request
    /// when present; the server-side counterpart only ever warns on
    /// mismatch, never rejects.
    pub(crate) schema_sha: Option<&'static str>,
    /// Per-call idempotency override (cratestack#926) set by
    /// [`Self::with_idempotency`]. `None` — the default — means "derive
    /// it from the HTTP method", which is the right answer for every
    /// REST CRUD route and the wrong one for RPC (all `POST`) and for
    /// `@query` procedures. See [`RequestIdempotency`].
    pub(crate) idempotency: Option<RequestIdempotency>,
}

impl CratestackClient<CborCodec> {
    pub fn cbor(config: ClientConfig) -> Self {
        Self::new(config, CborCodec)
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub fn new(config: ClientConfig, codec: C) -> Self {
        ensure_crypto_provider();
        Self {
            http: HttpClient::Plain(reqwest::Client::new()),
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
            schema_sha: None,
            idempotency: None,
        }
    }

    /// Build a client on a caller-supplied `reqwest::Client` — a
    /// custom timeout, proxy, or mTLS identity.
    ///
    /// Install a `rustls` crypto provider (e.g. via
    /// [`ensure_crypto_provider`]) *before* constructing `http`:
    /// unlike [`Self::new`], this constructor is handed an already-built
    /// client, so the #440 panic fires in the caller, not here.
    pub fn with_http_client(config: ClientConfig, codec: C, http: reqwest::Client) -> Self {
        Self {
            http: HttpClient::Plain(http),
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
            schema_sha: None,
            idempotency: None,
        }
    }

    pub fn with_state_store(mut self, state_store: Arc<dyn ClientStateStore>) -> Self {
        self.state_store = state_store;
        self
    }

    pub fn with_optional_state_store(self, state_store: Option<Arc<dyn ClientStateStore>>) -> Self {
        match state_store {
            Some(state_store) => self.with_state_store(state_store),
            None => self,
        }
    }

    pub fn with_request_authorizer(
        mut self,
        request_authorizer: Arc<dyn RequestAuthorizer>,
    ) -> Self {
        self.request_authorizer = Some(request_authorizer);
        self
    }

    /// Stamps the generating schema's `SCHEMA_SHA256` onto this client, so
    /// every subsequent request carries `x-cratestack-schema-sha` (issue
    /// #178). Called by the schema-generated `Client::new` wrapper, not
    /// meant to be called directly by schema authors — public because the
    /// generated code lives in a downstream crate.
    pub fn with_schema_sha(mut self, schema_sha: &'static str) -> Self {
        self.schema_sha = Some(schema_sha);
        self
    }

    pub fn state(&self) -> Result<PersistedClientState, ClientError> {
        // Not `.map_err(ClientError::from)`: `ClientError`'s only
        // `From<CratestackError>` impl targets `ClientError::Codec` (for genuine
        // wire-codec failures), which would misclassify a purely local
        // state-store failure as a remote/codec error — see #475's review
        // findings and `error.rs`'s `state_store_error_maps_to_client_error_state`.
        self.state_store
            .load()
            .map_err(|error| ClientError::State(error.to_string()))
    }
}

#[cfg(test)]
pub(crate) mod tests;
