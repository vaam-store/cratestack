//! RPC-transport regression coverage for cratestack#953. RPC list filters
//! synthesize URL query pairs and re-enter the generated REST parser, so an
//! optional-field equality fix must be verified on this transport too.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RpcListInput, RpcListPredicate};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/optional_query_filter_rpc.cstack",
    db = Postgres
);

mod support;

use support::pg;

#[derive(Clone)]
struct AnyoneAuth;

impl AuthProvider for AnyoneAuth {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

async fn reset_and_seed(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("DROP TABLE IF EXISTS rpc_optional_deliveries")
        .execute(pool)
        .await
        .expect("drop rpc_optional_deliveries");
    cratestack::sqlx::query(
        "CREATE TABLE rpc_optional_deliveries (id BIGINT PRIMARY KEY, verification_id TEXT NULL)",
    )
    .execute(pool)
    .await
    .expect("create rpc_optional_deliveries");
    cratestack::sqlx::query(
        "INSERT INTO rpc_optional_deliveries (id, verification_id) VALUES \
         (1, NULL), (2, 'verification-1'), (3, 'verification-2')",
    )
    .execute(pool)
    .await
    .expect("seed rpc_optional_deliveries");
}

fn router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        NoProcedures,
        (),
        CborCodec,
        AnyoneAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

#[tokio::test]
async fn rpc_list_eq_filters_on_an_optional_string_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool);
    let codec = CborCodec;
    let frame = codec
        .encode(&RpcListInput {
            filters: vec![RpcListPredicate {
                key: "verificationId".to_owned(),
                value: "verification-1".to_owned(),
            }],
            sort: Some("id".to_owned()),
            ..Default::default()
        })
        .expect("encode list input");

    let response = router
        .oneshot(
            Request::post("/rpc/model.RpcOptionalDelivery.list")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let rows: Vec<cratestack::serde_json::Value> =
        codec.decode(&bytes).expect("list response should decode");
    let ids = rows
        .iter()
        .map(|row| row["id"].as_i64().expect("id should be an int"))
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![2]);
}
