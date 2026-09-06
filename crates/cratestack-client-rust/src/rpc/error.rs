use cratestack_core::{CratestackError, rpc::RpcErrorBody};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::codec::HttpClientCodec;
use crate::error::{ClientError, TransportError};
use crate::runtime::wire::RuntimeResponseWire;

/// Error variant produced by the RPC client when a remote call fails with
/// an `RpcErrorBody` payload. Distinct from the REST `ClientError::Remote`
/// (which carries the `CratestackErrorResponse` shape) so library users can
/// switch on the gRPC-style `code` string directly.
#[derive(Debug, Clone)]
pub struct RpcRemoteError {
    pub status: StatusCode,
    pub body: RpcErrorBody,
}

impl std::fmt::Display for RpcRemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RPC call failed with code {} (status {}): {}",
            self.body.code,
            self.status.as_u16(),
            self.body.message
        )
    }
}

impl std::error::Error for RpcRemoteError {}

/// Top-level error returned by the RPC client. Mirrors `ClientError`
/// (the REST error type) but reports server-side failures as
/// `RpcRemoteError { code, message, details }` rather than the
/// REST-shaped `CratestackErrorResponse`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RpcClientError {
    #[error("transport error: {0}")]
    Transport(#[source] TransportError),
    /// RPC parity for [`ClientError::Middleware`] (cratestack#926) —
    /// a failure raised by a `reqwest_middleware` middleware rather
    /// than by the transport under it. `middleware` feature only.
    #[cfg(feature = "middleware")]
    #[error("middleware error: {0}")]
    Middleware(#[source] crate::middleware::MiddlewareError),
    #[error("codec error: {0}")]
    Codec(#[from] CratestackError),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("{0}")]
    Remote(RpcRemoteError),
}

/// Stable alias for the receiver shape that [`RpcClient::call_streaming`]
/// returns. Exists so macro-generated code (`include_client_schema!` for
/// `transport rpc` schemas) has a single name to bind without
/// re-spelling the tokio/error-type plumbing on every method, and so
/// downstream users have a typedef they can store in struct fields,
/// function returns, etc. without leaking the implementation detail.
pub type RpcStream<O> = tokio::sync::mpsc::Receiver<Result<O, RpcClientError>>;

/// Map a gRPC-style `RpcErrorBody.code` back to a sensible HTTP status.
/// Inverse of `cratestack_core::rpc::rpc_code`. Used for batch error
/// frames — the wire frame doesn't carry an HTTP status (the outer
/// `/rpc/batch` response is always 200), so we synthesize one from the
/// code for consistency with the unary `RpcRemoteError` shape.
pub(crate) fn http_status_for_rpc_code(code: &str) -> StatusCode {
    match code {
        "invalid_argument" => StatusCode::BAD_REQUEST,
        "unauthenticated" => StatusCode::UNAUTHORIZED,
        "permission_denied" => StatusCode::FORBIDDEN,
        "not_found" => StatusCode::NOT_FOUND,
        "conflict" => StatusCode::CONFLICT,
        "failed_precondition" => StatusCode::PRECONDITION_FAILED,
        // cratestack#846: emitted by `RateLimitLayer` on a throttled
        // request. Without this arm a batched throttle would surface to
        // the caller as a synthetic 500, hiding the one status a client's
        // backoff logic actually keys on.
        "resource_exhausted" => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn client_error_to_rpc(error: ClientError) -> RpcClientError {
    match error {
        ClientError::Transport(error) => RpcClientError::Transport(error),
        #[cfg(feature = "middleware")]
        ClientError::Middleware(error) => RpcClientError::Middleware(error),
        ClientError::Codec(error) => RpcClientError::Codec(error),
        ClientError::InvalidResponse(message) => RpcClientError::InvalidResponse(message),
        ClientError::BadInput(message) => RpcClientError::BadInput(message),
        ClientError::State(message) => RpcClientError::InvalidResponse(message),
        ClientError::Remote {
            status,
            error,
            message,
        } => {
            // Legacy translation path — should not fire for /rpc/... URLs
            // (the server-side dispatcher emits RpcErrorBody-shaped error
            // bodies), but keep a sensible fallback rather than dropping
            // the message on the floor.
            let body = error
                .map(cratestack_core::rpc::RpcErrorBody::from_cratestack_response)
                .unwrap_or_else(|| RpcErrorBody {
                    code: "internal".to_owned(),
                    message,
                    details: None,
                });
            RpcClientError::Remote(RpcRemoteError { status, body })
        }
    }
}

pub(crate) fn decode_rpc_unary_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Output, RpcClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            RpcClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_response::<Output>(content_type, &response.body)
            .map_err(RpcClientError::Codec)
    } else {
        let body = codec
            .decode_response::<RpcErrorBody>(content_type, &response.body)
            .unwrap_or_else(|_| RpcErrorBody {
                code: "internal".to_owned(),
                message: format!(
                    "unexpected RPC error body for status {}",
                    response.status_code
                ),
                details: None,
            });
        Err(RpcClientError::Remote(RpcRemoteError {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            body,
        }))
    }
}
