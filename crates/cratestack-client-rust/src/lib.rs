mod auth;
mod client;
mod codec;
mod config;
mod error;
mod idempotency;
#[cfg(feature = "middleware")]
mod middleware;
mod rpc;
mod runtime;
mod state;
mod streaming;
mod streaming_callback;

#[cfg(test)]
mod tests;

pub use cratestack_codec_cbor::CborCodec;
#[cfg(feature = "codec-json")]
pub use cratestack_codec_json::JsonCodec;
pub use cratestack_core::rpc::{
    RPC_BATCH_PATH, RPC_UNARY_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame, rpc_code,
};

pub use auth::{AuthorizationRequest, RequestAuthorizer};
pub use client::{CratestackClient, TypedResponse, ensure_crypto_provider};
pub use codec::HttpClientCodec;
pub use config::ClientConfig;
pub use cratestack_core::ProjectionDecoder;
/// Back-compat alias — the trait moved to `cratestack-core` and was
/// renamed `ProjectionDecoder` to free up the `Projection` name for
/// the SQL value type in `cratestack-sql`.
#[deprecated(
    since = "0.4.0",
    note = "use `cratestack::ProjectionDecoder` (moved to cratestack-core) instead"
)]
pub use cratestack_core::ProjectionDecoder as Projection;
pub use error::{ClientError, HeaderPair, QueryPair};
pub use idempotency::RequestIdempotency;
#[cfg(feature = "middleware")]
pub use middleware::MiddlewareError;
pub use rpc::batch::{BatchBuilder, BatchResults};
pub use rpc::batch_call::{BatchHandle, BatchableCall};
pub use rpc::client::RpcClient;
pub use rpc::error::{RpcClientError, RpcRemoteError, RpcStream};
pub use runtime::handle::RuntimeHandle;
pub use runtime::wire::{
    RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig, RuntimeErrorCode,
    RuntimeErrorWire, RuntimeHeader, RuntimeRequestWire, RuntimeResponseWire,
    RuntimeStateStoreConfig, RuntimeTransportConfig,
};
pub use state::{
    ClientStateStore, InMemoryStateStore, JsonFileStateStore, PersistedClientState,
    RequestJournalEntry,
};
pub use streaming::CborSeqChunkDecoder;
pub use streaming_callback::RuntimeChunkWire;
