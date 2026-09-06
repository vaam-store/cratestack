//! The HTTP transport [`CratestackClient`] sends through.
//!
//! Two shapes, one call surface. [`HttpClient::Plain`] wraps a bare
//! `reqwest::Client` — what every client built by
//! [`CratestackClient::new`] / [`CratestackClient::with_http_client`]
//! gets, and the only variant that exists in a default build.
//! [`HttpClient::Middleware`] wraps a
//! `reqwest_middleware::ClientWithMiddleware` and only exists under the
//! `middleware` feature (cratestack#926).
//!
//! An enum rather than a `dyn HttpTransport` trait: the two builders
//! are structurally identical (`headers` / `body` / `send`) but share
//! no trait, return different error types, and `reqwest`'s builder is
//! not object-safe to wrap without boxing every request. Two variants
//! and a `match` cost one branch per request and keep the crate free of
//! a public transport trait it would then have to keep semver-stable.
//!
//! [`CratestackClient`]: crate::CratestackClient
//! [`CratestackClient::new`]: crate::CratestackClient::new
//! [`CratestackClient::with_http_client`]: crate::CratestackClient::with_http_client

use reqwest::header::HeaderMap;
use reqwest::{Method, Url};

use crate::error::ClientError;

#[derive(Debug, Clone)]
pub(crate) enum HttpClient {
    Plain(reqwest::Client),
    #[cfg(feature = "middleware")]
    Middleware(reqwest_middleware::ClientWithMiddleware),
}

impl HttpClient {
    /// Start a request, mirroring `reqwest::Client::request`.
    pub(crate) fn request(&self, method: Method, url: Url) -> HttpRequestBuilder {
        match self {
            Self::Plain(client) => HttpRequestBuilder::Plain(client.request(method, url)),
            #[cfg(feature = "middleware")]
            Self::Middleware(client) => HttpRequestBuilder::Middleware(client.request(method, url)),
        }
    }
}

impl From<reqwest::Client> for HttpClient {
    fn from(client: reqwest::Client) -> Self {
        Self::Plain(client)
    }
}

#[cfg(feature = "middleware")]
impl From<reqwest_middleware::ClientWithMiddleware> for HttpClient {
    fn from(client: reqwest_middleware::ClientWithMiddleware) -> Self {
        Self::Middleware(client)
    }
}

/// The subset of `RequestBuilder` this crate actually uses, unified
/// across both transports. Deliberately not the full builder surface —
/// every method here has an exact counterpart on both underlying types.
#[must_use = "HttpRequestBuilder does nothing until you `send` it"]
pub(crate) enum HttpRequestBuilder {
    Plain(reqwest::RequestBuilder),
    #[cfg(feature = "middleware")]
    Middleware(reqwest_middleware::RequestBuilder),
}

impl HttpRequestBuilder {
    pub(crate) fn headers(self, headers: HeaderMap) -> Self {
        match self {
            Self::Plain(builder) => Self::Plain(builder.headers(headers)),
            #[cfg(feature = "middleware")]
            Self::Middleware(builder) => Self::Middleware(builder.headers(headers)),
        }
    }

    pub(crate) fn body(self, body: Vec<u8>) -> Self {
        match self {
            Self::Plain(builder) => Self::Plain(builder.body(body)),
            #[cfg(feature = "middleware")]
            Self::Middleware(builder) => Self::Middleware(builder.body(body)),
        }
    }

    /// Attach a typed request extension, readable from a middleware's
    /// `&mut http::Extensions`.
    ///
    /// **A no-op on [`Self::Plain`], by necessity, not by choice.**
    /// reqwest 0.13.4 declares `Request::extensions()` /
    /// `Request::extensions_mut()` `pub(crate)`
    /// (`reqwest/src/async_impl/request.rs:108,114`), so there is no
    /// supported way to put a value into a bare `reqwest::Request`'s
    /// extension map from outside the crate — and with no middleware
    /// chain there would be nothing to read it back out either.
    /// Extensions are therefore a `middleware`-feature concept end to
    /// end; the plain arm swallows the value so callers don't have to
    /// branch on the feature at every call site.
    pub(crate) fn with_extension<T>(self, extension: T) -> Self
    where
        T: Clone + Send + Sync + 'static,
    {
        match self {
            Self::Plain(builder) => {
                let _ = extension;
                Self::Plain(builder)
            }
            #[cfg(feature = "middleware")]
            Self::Middleware(builder) => Self::Middleware(builder.with_extension(extension)),
        }
    }

    /// Send the request, normalising both transports' error types onto
    /// [`ClientError`].
    ///
    /// `reqwest_middleware::Error::Reqwest` lands in the same
    /// `ClientError::Transport` variant a plain-transport failure
    /// always did, so existing `match` arms keep working unchanged when
    /// a caller switches a client over to a middleware stack; only a
    /// failure raised *by* a middleware becomes the new
    /// `ClientError::Middleware`. See `crate::middleware`.
    pub(crate) async fn send(self) -> Result<reqwest::Response, ClientError> {
        match self {
            Self::Plain(builder) => builder.send().await.map_err(ClientError::from),
            #[cfg(feature = "middleware")]
            Self::Middleware(builder) => builder.send().await.map_err(ClientError::from),
        }
    }
}
