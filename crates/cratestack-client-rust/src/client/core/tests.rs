//! Unit tests for `client::core`, split out of `core.rs` to keep that
//! file under the repo's ~200-LoC ceiling. `FailingStateStore` is
//! `pub(crate)` because `client::headers`'s tests reuse it.

use cratestack_core::CratestackError;

use super::*;

/// A `ClientStateStore` whose every operation fails, so tests can
/// observe how a local state-store failure gets classified without
/// touching the filesystem or any other real backend.
#[derive(Debug, Default)]
pub(crate) struct FailingStateStore;

impl ClientStateStore for FailingStateStore {
    fn load(&self) -> Result<PersistedClientState, CratestackError> {
        Err(CratestackError::Internal(
            "simulated state store failure".to_owned(),
        ))
    }

    fn save(&self, _state: &PersistedClientState) -> Result<(), CratestackError> {
        Err(CratestackError::Internal(
            "simulated state store failure".to_owned(),
        ))
    }
}

/// Regression test for #475's review findings: a `CratestackError` raised by
/// the state store must surface as `ClientError::State`, not get
/// silently reclassified as `ClientError::Codec` via the blanket
/// `From<CratestackError>` impl (which is meant for genuine wire-codec
/// failures, not local storage failures). Fails against the code that
/// used `.map_err(ClientError::from)` here.
#[test]
fn state_store_error_maps_to_client_error_state() {
    let client = CratestackClient::cbor(ClientConfig::new(
        "http://example.invalid".parse().expect("valid url"),
    ))
    .with_state_store(Arc::new(FailingStateStore));

    let error = client.state().expect_err("state store is rigged to fail");

    match error {
        ClientError::State(message) => {
            assert!(message.contains("simulated state store failure"));
        }
        other => panic!("expected ClientError::State for a state-store failure, got {other:?}"),
    }
}
