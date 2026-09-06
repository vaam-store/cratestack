//! cratestack#906: `--rtk` must refuse a schema whose procedure endpoint
//! key collides with a derived model endpoint key, mirroring
//! `tests/tanstack_hook_collisions.rs`'s coverage shape for the analogous
//! `--tanstack` check.
//!
//! RTK Query's endpoint map is a single object literal, so a collision
//! here is a same-object duplicate key (`ts(1117)`) — see
//! `crate::rtk::collisions`'s module doc.

use cratestack_client_typescript::{
    TypeScriptGeneratorConfig, TypeScriptGeneratorError, generate_package,
};

const QUERY_FIXTURE: &str = "tests/fixtures/rtk_query_endpoint_collision.cstack";
const QUERY_FIXTURE_RPC: &str = "tests/fixtures/rtk_query_endpoint_collision_rpc.cstack";
const MUTATION_FIXTURE: &str = "tests/fixtures/rtk_mutation_endpoint_collision.cstack";

fn parse(fixture_path: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema_file(fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"))
}

fn rtk_config() -> TypeScriptGeneratorConfig {
    TypeScriptGeneratorConfig {
        package_name: "rtk-fixture-client".to_owned(),
        rtk: true,
        ..TypeScriptGeneratorConfig::default()
    }
}

/// Asserts the typed error, not just that generation failed — a schema
/// can fail for unrelated reasons and an `is_err()` assertion would
/// happily pass on any of them.
#[track_caller]
fn expect_collision(
    fixture_path: &str,
    expected_procedure: &str,
    expected_identifier: &str,
    expected_operation: &str,
) {
    let schema = parse(fixture_path);
    let error = generate_package(&schema, &rtk_config())
        .expect_err("--rtk must reject a procedure colliding with a model's generated endpoint");

    match error {
        TypeScriptGeneratorError::RtkEndpointNameCollision {
            procedure,
            identifier,
            model,
            operation,
        } => {
            assert_eq!(procedure, expected_procedure);
            assert_eq!(identifier, expected_identifier);
            assert_eq!(model, "Post");
            assert_eq!(operation, expected_operation);
        }
        other => panic!("expected RtkEndpointNameCollision, got {other:?}"),
    }
}

#[test]
fn rtk_rejects_a_query_procedure_colliding_with_a_model_endpoint() {
    expect_collision(QUERY_FIXTURE, "list_post", "listPost", "list");
}

/// The transport-parity half. A REST-only fix would pass every other test
/// here while leaving the identical hazard live in `rtk-rpc.ts.j2`.
#[test]
fn rtk_rejects_the_same_collision_on_rpc_transport() {
    let schema = parse(QUERY_FIXTURE_RPC);
    assert_eq!(
        schema.transport,
        cratestack_core::TransportStyle::Rpc,
        "fixture must actually be RPC transport, or this test silently re-runs the REST case"
    );
    expect_collision(QUERY_FIXTURE_RPC, "list_post", "listPost", "list");
}

#[test]
fn rtk_rejects_a_mutation_procedure_colliding_with_a_model_endpoint() {
    expect_collision(MUTATION_FIXTURE, "create_post", "createPost", "create");
}

/// The check must fire **only** under `--rtk` (mirroring decision spike
/// #317, applied to `--tanstack` originally). Without this, rejecting
/// unconditionally would satisfy every assertion above while breaking
/// schemas that never touch the flag.
#[test]
fn default_layout_accepts_what_rtk_rejects() {
    for fixture_path in [QUERY_FIXTURE, QUERY_FIXTURE_RPC, MUTATION_FIXTURE] {
        let schema = parse(fixture_path);
        generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                package_name: "default-fixture-client".to_owned(),
                ..TypeScriptGeneratorConfig::default()
            },
        )
        .unwrap_or_else(|error| {
            panic!(
                "default layout must not be constrained by --rtk's naming scheme, but \
                 {fixture_path} failed: {error}"
            )
        });
    }
}
