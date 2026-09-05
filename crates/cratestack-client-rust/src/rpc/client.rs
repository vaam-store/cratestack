use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::{RPC_BATCH_PATH, RpcRequest, RpcResponseFrame};
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::config::ClientConfig;
use crate::idempotency::RequestIdempotency;
use crate::rpc::batch::BatchBuilder;
use crate::rpc::error::{RpcClientError, client_error_to_rpc, decode_rpc_unary_response};
use crate::streaming::pump_streamed_response_typed;

// `RPC_BATCH_PATH` from core is the axum-template form `"/rpc/batch"`,
// so we just reuse it. The unary path is templated (`/rpc/{op_id}`) so
// we format it per call instead of using the constant directly.
const RPC_BATCH_PATH_PLAIN: &str = RPC_BATCH_PATH;

/// Thin RPC client built on top of the REST client's transport + codec
/// plumbing.
///
/// Shares a `reqwest::Client` and a codec impl with `CratestackClient`,
/// but speaks the `/rpc/...` URL space instead of REST routes. Both
/// clients can be used side-by-side against the same server.
#[derive(Clone)]
pub struct RpcClient<C = CborCodec> {
    pub(crate) inner: CratestackClient<C>,
}

impl RpcClient<CborCodec> {
    pub fn cbor(config: ClientConfig) -> Self {
        Self::new(CratestackClient::cbor(config))
    }
}

impl<C> RpcClient<C>
where
    C: HttpClientCodec + Clone,
{
    /// Build an RPC client on top of an existing REST client. The two
    /// share their `reqwest::Client`, codec, and state store.
    pub fn new(inner: CratestackClient<C>) -> Self {
        Self { inner }
    }

    /// Underlying REST client. Exposed for callers that want REST + RPC
    /// side-by-side (e.g. a long migration window between the two).
    pub fn inner(&self) -> &CratestackClient<C> {
        &self.inner
    }

    /// RPC half of the per-call idempotency override — forwards to
    /// [`CratestackClient::with_idempotency`], which documents the whole
    /// mechanism.
    ///
    /// RPC needs this more than REST does: every RPC call is
    /// `POST /rpc/{op_id}`, so the method-derived default marks
    /// *everything* non-idempotent, reads included. The per-op truth is
    /// the `idempotent_by_default` field on the `OpDescriptor` the
    /// schema macro emits (`cratestack_core::OpDescriptor`):
    ///
    /// ```text
    /// let response = rpc
    ///     .clone()
    ///     .with_idempotency(RequestIdempotency::new(OP.idempotent_by_default))
    ///     .call::<_, Output>(OP.op_id, &input)
    ///     .await?;
    /// ```
    ///
    /// Streamed calls ([`Self::call_streaming`]) carry the extension
    /// too, but a partially-consumed stream cannot be replayed from the
    /// middleware layer — leave those at the non-idempotent default
    /// unless the middleware only retries before the first byte.
    pub fn with_idempotency(self, idempotency: RequestIdempotency) -> Self {
        Self {
            inner: self.inner.with_idempotency(idempotency),
        }
    }

    /// Start a new typed batch. Use with [`BatchableCall::queue`] from
    /// the macro-generated typed methods (or any hand-built
    /// [`BatchableCall`]) to compose a heterogeneous batch, then
    /// `batch.send().await` for a single `POST /rpc/batch` round-trip.
    pub fn batch_builder(&self) -> BatchBuilder<C> {
        BatchBuilder::new(self.clone())
    }

    /// POST /rpc/{op_id} — unary call.
    ///
    /// `op_id` is the dotted dispatch key the server emits — `model.X.list`
    /// / `model.X.get` / `model.X.create` / `model.X.update` /
    /// `model.X.delete` for CRUD verbs and `procedure.<name>` for procedures.
    pub async fn call<I, O>(&self, op_id: &str, input: &I) -> Result<O, RpcClientError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let body = self
            .inner
            .codec
            .encode(input)
            .map_err(RpcClientError::Codec)?;
        let path = format!("/rpc/{}", op_id);
        let response = self
            .inner
            .request_raw_with_query_and_accept(Method::POST, &path, Some(body), None, &[], None)
            .await
            .map_err(client_error_to_rpc)?;
        decode_rpc_unary_response(&self.inner.codec, &response)
    }

    /// POST /rpc/batch — sequence of `RpcRequest` frames in, sequence of
    /// `RpcResponseFrame` frames out. Per-frame errors do not poison the
    /// batch (each frame's `output` / `error` is reported independently).
    pub async fn batch(
        &self,
        requests: &[RpcRequest],
    ) -> Result<Vec<RpcResponseFrame>, RpcClientError> {
        let body = self
            .inner
            .codec
            .encode(&requests)
            .map_err(RpcClientError::Codec)?;
        let response = self
            .inner
            .request_raw_with_query_and_accept(
                Method::POST,
                RPC_BATCH_PATH_PLAIN,
                Some(body),
                None,
                &[],
                None,
            )
            .await
            .map_err(client_error_to_rpc)?;
        decode_rpc_unary_response::<C, Vec<RpcResponseFrame>>(&self.inner.codec, &response)
    }

    /// POST /rpc/{op_id} — sequence response, item-at-a-time.
    ///
    /// Returns a bounded `mpsc::Receiver` that yields each cbor-seq
    /// item as bytes arrive over the network — no full-body buffering
    /// before the first item reaches the caller. Transport / decode
    /// failures appear as terminal `Err` items on the channel; the
    /// receiver returning `None` indicates a clean stream close.
    ///
    /// Non-2xx responses are buffered and surfaced as a single
    /// `RpcClientError::Remote(RpcRemoteError { ... })` from this
    /// function (the channel is never opened) — same shape as the
    /// unary `call` path. The server must return `application/cbor-seq`
    /// for streaming; on a buffered `application/cbor` response this
    /// method will misframe the body.
    pub async fn call_streaming<I, O>(
        &self,
        op_id: &str,
        input: &I,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<O, RpcClientError>>, RpcClientError>
    where
        I: Serialize,
        O: DeserializeOwned + Send + 'static,
    {
        let body = self
            .inner
            .codec
            .encode(input)
            .map_err(RpcClientError::Codec)?;
        let path = format!("/rpc/{}", op_id);
        let response = self
            .inner
            .request_streamed_with_query_and_accept(
                Method::POST,
                &path,
                Some(body),
                None,
                &[],
                self.inner.codec.sequence_accept_header_value(),
            )
            .await
            .map_err(client_error_to_rpc)?;

        // Bounded channel — 16 in-flight items matches the REST
        // `post_list_streamed` shape and keeps consumer memory tight.
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(pump_streamed_response_typed::<C, O, RpcClientError, _>(
            self.inner.codec.clone(),
            response,
            tx,
            client_error_to_rpc,
        ));
        Ok(rx)
    }
}
