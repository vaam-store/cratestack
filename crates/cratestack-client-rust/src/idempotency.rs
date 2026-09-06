//! Per-request idempotency, carried to HTTP middleware as a typed
//! request extension (cratestack#926).

use reqwest::Method;

use crate::client::CratestackClient;

/// Whether one outbound request may be replayed byte-for-byte without
/// an idempotency key — the single fact a retry middleware needs and
/// cannot work out for itself.
///
/// Attached to every request this crate sends as a
/// `reqwest_middleware` request extension, so a middleware reads it
/// with `extensions.get::<RequestIdempotency>()`. Only observable under
/// the `middleware` feature: reqwest 0.13 keeps
/// `reqwest::Request::extensions_mut()` `pub(crate)`, so a bare
/// `reqwest::Client` has no extension map a caller can write to (see
/// `client::http::HttpRequestBuilder::with_extension`).
///
/// **Absent means "assume not idempotent."** A middleware that finds no
/// `RequestIdempotency` in the extensions is talking to something other
/// than this client, or to a build where the extension could not be
/// attached; failing closed there is the same rule the rest of this
/// crate follows.
///
/// # How the value is chosen
///
/// By default it is derived from the HTTP method
/// ([`RequestIdempotency::for_method`], RFC 9110 §9.2.2), which is
/// exactly right for REST CRUD: `GET`/`DELETE` are idempotent,
/// `POST`/`PATCH` are not. It is *not* right for two cases the method
/// alone cannot express, and for those the caller overrides it with
/// [`CratestackClient::with_idempotency`]:
///
/// * **RPC.** Every RPC call is `POST /rpc/{op_id}`, so the method
///   carries no information at all. The op's real answer is the
///   `idempotent_by_default` field on the `OpDescriptor` the schema
///   macro emits for that op (`cratestack_core::OpDescriptor`) — `true`
///   for reads and `@query` procedures, `false` for mutations.
/// * **REST procedures.** `POST /$procs/<name>` is a `POST` whatever
///   the procedure does, so a `@query` procedure looks non-idempotent
///   to the method-derived default.
///
/// Note that the REST-transport descriptor
/// (`cratestack_core::RouteTransportDescriptor`) does **not** carry an
/// `idempotent_by_default` field today — only the RPC-transport
/// `OpDescriptor` does. Wiring either descriptor through the *generated*
/// client so callers don't have to pass it by hand is follow-up work;
/// this type and [`CratestackClient::with_idempotency`] are the runtime
/// half that makes it expressible at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestIdempotency {
    idempotent: bool,
}

impl RequestIdempotency {
    /// The request may be retried as-is.
    pub const IDEMPOTENT: Self = Self { idempotent: true };

    /// The request must not be retried without an idempotency key.
    pub const NOT_IDEMPOTENT: Self = Self { idempotent: false };

    /// Wraps a raw flag — e.g. an `OpDescriptor`'s
    /// `idempotent_by_default`.
    pub const fn new(idempotent: bool) -> Self {
        Self { idempotent }
    }

    pub const fn is_idempotent(self) -> bool {
        self.idempotent
    }

    /// The RFC 9110 §9.2.2 answer for a method: `GET`, `HEAD`, `PUT`,
    /// `DELETE`, `OPTIONS` and `TRACE` are idempotent; everything else
    /// — `POST` and `PATCH` included — is not.
    ///
    /// `PATCH` is deliberately in the second group: RFC 5789 §2 makes
    /// it explicitly non-idempotent, and cratestack's generated `PATCH`
    /// route is a partial update, which a retry after an unseen success
    /// can re-apply against a changed row.
    pub fn for_method(method: &Method) -> Self {
        Self::new(matches!(
            method.as_str(),
            "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
        ))
    }
}

impl<C> CratestackClient<C> {
    /// Pin every request this client sends to `idempotency`, overriding
    /// the method-derived default.
    ///
    /// `CratestackClient` is cheap to clone (the `reqwest` client, the
    /// state store and the authorizer are all reference-counted), so
    /// the intended shape is a per-call clone rather than a second
    /// long-lived client:
    ///
    /// ```
    /// use cratestack_client_rust::{
    ///     CborCodec, ClientConfig, CratestackClient, RequestIdempotency,
    /// };
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = CratestackClient::cbor(ClientConfig::new(
    ///     "https://api.example.com".parse()?,
    /// ));
    ///
    /// // A `@query` procedure is a POST on the wire but is safe to
    /// // replay — say so, and a retry middleware will retry it.
    /// let readonly = client.clone().with_idempotency(RequestIdempotency::IDEMPOTENT);
    /// # let _ = readonly;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// This is the REST half of the per-call override; the RPC half is
    /// [`RpcClient::with_idempotency`](crate::RpcClient::with_idempotency),
    /// which forwards to this method.
    pub fn with_idempotency(mut self, idempotency: RequestIdempotency) -> Self {
        self.idempotency = Some(idempotency);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_and_idempotent_methods_are_replayable() {
        for method in [
            Method::GET,
            Method::HEAD,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
            Method::TRACE,
        ] {
            assert!(
                RequestIdempotency::for_method(&method).is_idempotent(),
                "{method} is idempotent per RFC 9110 §9.2.2"
            );
        }
    }

    /// `PATCH` is the one that reads as idempotent to intuition and
    /// isn't — RFC 5789 §2 says so explicitly, and a retried partial
    /// update is exactly the silent-corruption case this flag exists to
    /// prevent.
    #[test]
    fn post_and_patch_are_not_replayable() {
        for method in [Method::POST, Method::PATCH] {
            assert!(
                !RequestIdempotency::for_method(&method).is_idempotent(),
                "{method} must not be retried without an idempotency key"
            );
        }
    }

    #[test]
    fn constants_and_constructor_agree() {
        assert_eq!(
            RequestIdempotency::new(true),
            RequestIdempotency::IDEMPOTENT
        );
        assert_eq!(
            RequestIdempotency::new(false),
            RequestIdempotency::NOT_IDEMPOTENT
        );
        assert!(RequestIdempotency::IDEMPOTENT.is_idempotent());
        assert!(!RequestIdempotency::NOT_IDEMPOTENT.is_idempotent());
    }
}
