//! RPC-transport counterpart to `enum_query_filter.rs` (cratestack#928):
//! `model.Order.list`'s `RpcListInput.filters` must reach the same
//! enum-aware `<model>_filter_expr` REST now gets — RPC dispatch
//! synthesizes a URL query string from `filters` and re-enters the exact
//! same parsing path (`cratestack-axum/src/rpc/synthesize.rs`), so this
//! is the empirical proof that reuse actually holds for this fix, not
//! just a read of the generator source (mirrors `rpc_pagination.rs`'s
//! own reasoning for why it re-checks REST behavior over RPC too).

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RpcListInput, RpcListPredicate};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/enum_query_filter_rpc.cstack", db = Postgres);

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
    cratestack::sqlx::query("DROP TABLE IF EXISTS orders")
        .execute(pool)
        .await
        .expect("drop orders");
    cratestack::sqlx::query("CREATE TABLE orders (id BIGINT PRIMARY KEY, state TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create orders");
    cratestack::sqlx::query(
        "INSERT INTO orders (id, state) VALUES \
         (1, 'reserved'), (2, 'funded'), (3, 'funded'), (4, 'cancelled')",
    )
    .execute(pool)
    .await
    .expect("seed orders");
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

async fn list_with_filter(
    router: &cratestack::axum::Router,
    key: &str,
    value: &str,
) -> (StatusCode, cratestack::serde_json::Value) {
    let codec = CborCodec;
    let frame = codec
        .encode(&RpcListInput {
            filters: vec![RpcListPredicate {
                key: key.to_owned(),
                value: value.to_owned(),
            }],
            sort: Some("id".to_owned()),
            ..Default::default()
        })
        .expect("encode list input");

    let response = router
        .clone()
        .oneshot(
            Request::post("/rpc/model.Order.list")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value = codec.decode(&bytes).expect("body should decode");
    (status, value)
}

#[tokio::test]
async fn rpc_list_eq_filters_on_an_enum_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, body) = list_with_filter(&router, "state", "funded").await;
    assert_eq!(status, StatusCode::OK, "eq filter must not 400: {body}");
    let ids = body
        .as_array()
        .expect("list response should be an array")
        .iter()
        .map(|row| row["id"].as_i64().expect("id should be an int"))
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![2, 3]);
}

#[tokio::test]
async fn rpc_list_unknown_variant_names_field_and_accepted_values() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, body) = list_with_filter(&router, "state", "bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = body["message"].as_str().unwrap_or_default();
    assert!(message.contains("state"), "got: {message}");
    assert!(
        message.contains("reserved") && message.contains("funded") && message.contains("cancelled"),
        "got: {message}"
    );
}
