use cratestack_core::{CratestackError, CratestackErrorResponse};
use reqwest::StatusCode;

pub type HeaderPair<'a> = (&'a str, &'a str);
pub type QueryPair<'a> = (&'a str, &'a str);

/// Opaque wrapper around `reqwest::Error` that doesn't expose the type
/// in public match arms, preserving semver hygiene.
#[derive(Debug)]
pub struct TransportError {
    inner: Box<reqwest::Error>,
}

impl TransportError {
    /// Access the underlying `reqwest::Error`.
    ///
    /// Named `reqwest_error` rather than `source` so it doesn't collide
    /// with (and get silently shadowed by) `std::error::Error::source`,
    /// which returns a different type (`Option<&(dyn Error + 'static)>`)
    /// — see the trait impl below for the chain-walking accessor.
    pub fn reqwest_error(&self) -> &reqwest::Error {
        &self.inner
    }

    /// Consume this error and extract the underlying `reqwest::Error`.
    pub fn into_source(self) -> reqwest::Error {
        *self.inner
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.inner.as_ref())
    }
}

impl From<reqwest::Error> for TransportError {
    fn from(e: reqwest::Error) -> Self {
        TransportError { inner: Box::new(e) }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    #[error("transport error: {0}")]
    Transport(#[source] TransportError),
    /// A middleware in the chain failed the request before (or instead
    /// of) the transport did — cratestack#926. Only reachable under the
    /// `middleware` feature; a transport-level failure raised *through*
    /// a middleware chain still arrives as `Transport`, so this variant
    /// never widens the meaning of an existing one.
    #[cfg(feature = "middleware")]
    #[error("middleware error: {0}")]
    Middleware(#[source] crate::middleware::MiddlewareError),
    #[error("codec error: {0}")]
    Codec(#[from] CratestackError),
    #[error("state error: {0}")]
    State(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("remote call failed with status {status}: {message}")]
    Remote {
        status: StatusCode,
        error: Option<CratestackErrorResponse>,
        message: String,
    },
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Transport(TransportError::from(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connecting to a port nothing listens on is a reliable, offline way
    /// to synthesize a genuine `reqwest::Error` (connection-refused on
    /// loopback is near-instant and doesn't require real network access),
    /// so tests can exercise the actual `TransportError`/`ClientError::
    /// Transport` code path rather than only the untouched variants.
    async fn synthesize_reqwest_error() -> reqwest::Error {
        // `reqwest`'s `rustls-no-provider` feature requires a crypto
        // provider installed before the first `Client` is built (#440) —
        // mirrors `client/core.rs`'s `ensure_crypto_provider`.
        let _ = rustls::crypto::ring::default_provider().install_default();

        reqwest::Client::new()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .expect_err("connecting to a closed loopback port should fail")
    }

    #[tokio::test]
    async fn transport_error_display_and_accessor_forward_to_reqwest() {
        let reqwest_err = synthesize_reqwest_error().await;
        let expected_message = reqwest_err.to_string();

        let transport = TransportError::from(reqwest_err);

        assert_eq!(transport.to_string(), expected_message);
        assert_eq!(transport.reqwest_error().to_string(), expected_message);
    }

    #[tokio::test]
    async fn transport_error_into_source_round_trips_the_reqwest_error() {
        let reqwest_err = synthesize_reqwest_error().await;
        let expected_message = reqwest_err.to_string();

        let recovered = TransportError::from(reqwest_err).into_source();

        assert_eq!(recovered.to_string(), expected_message);
    }

    #[tokio::test]
    async fn client_error_transport_chains_via_std_error_source() {
        let reqwest_err = synthesize_reqwest_error().await;
        let expected_message = reqwest_err.to_string();

        let client_err = ClientError::from(reqwest_err);

        // `From<reqwest::Error>` must land in the `Transport` variant.
        assert!(matches!(client_err, ClientError::Transport(_)));
        // The outer `Display` must include the inner reqwest message.
        assert!(client_err.to_string().contains(&expected_message));

        // Any caller walking the error chain via the std trait (anyhow,
        // tracing-error, generic e.source() logging) must still reach the
        // underlying reqwest::Error — this is the behavior that regressed
        // when `#[from] reqwest::Error` was replaced by the opaque
        // `TransportError` wrapper without wiring `#[source]`/`impl Error`.
        let source = std::error::Error::source(&client_err)
            .expect("ClientError::Transport should chain to the reqwest::Error via source()");
        assert_eq!(source.to_string(), expected_message);
    }

    #[test]
    fn client_error_variants() {
        let transport_err = ClientError::State("state error".to_string());
        assert!(matches!(transport_err, ClientError::State(_)));

        let invalid_resp = ClientError::InvalidResponse("invalid".to_string());
        assert!(matches!(invalid_resp, ClientError::InvalidResponse(_)));

        let bad_input = ClientError::BadInput("bad".to_string());
        assert!(matches!(bad_input, ClientError::BadInput(_)));
    }

    #[test]
    fn client_error_displays_correctly() {
        let err = ClientError::State("test error".to_string());
        assert_eq!(err.to_string(), "state error: test error");

        let err = ClientError::InvalidResponse("invalid".to_string());
        assert_eq!(err.to_string(), "invalid response: invalid");
    }
}
