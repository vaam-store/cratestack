//! End-to-end REST coverage for cratestack#928: an enum-typed field
//! (`Order.state`) must be filterable on the generated list route —
//! `eq`, `ne` and `in` at minimum, with an unknown variant rejected by a
//! `BadRequest` that names the field and the accepted values. Before the
//! fix, EVERY one of these requests 400s with `unsupported query filter
//! 'state' for Order` — the whole field silently had no filter arm at
//! all (`crates/cratestack-macros/src/axum/filter_arms/tests.rs` proves
//! the same mechanism without a database; this test proves it against
//! the real generated REST route and a live Postgres row set).
//!
//! Ordering operators (`lt`/`gt`) must stay unsupported — declaration
//! order is not a meaningful ordering to expose — so this test also
//! asserts `state__lt=funded` still 400s exactly like it always has.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/enum_query_filter.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AnyoneAuth;

impl AuthProvider for AnyoneAuth {
    type Error = CratestackError;

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
    cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        (),
        CborCodec,
        AnyoneAuth,
    )
}

async fn list_state_ids(router: &cratestack::axum::Router, query: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/orders?{query}"))
                .header("accept", CborCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    if status != StatusCode::OK {
        let error: cratestack::serde_json::Value =
            CborCodec.decode(&body).expect("error body should decode");
        return (
            status,
            error["message"].as_str().unwrap_or_default().to_owned(),
        );
    }
    let rows: Vec<cratestack::serde_json::Value> = CborCodec
        .decode(&body)
        .expect("list response should decode");
    let ids = rows
        .iter()
        .map(|row| row["id"].as_i64().expect("id should be an int"))
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    (status, ids)
}

#[tokio::test]
async fn enum_field_eq_filters_the_list_route() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, ids) = list_state_ids(&router, "state=funded&sort=id").await;
    assert_eq!(status, StatusCode::OK, "eq filter must not 400: {ids}");
    assert_eq!(ids, "2,3");
}

#[tokio::test]
async fn enum_field_ne_filters_the_list_route() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, ids) = list_state_ids(&router, "state__ne=funded&sort=id").await;
    assert_eq!(status, StatusCode::OK, "ne filter must not 400: {ids}");
    assert_eq!(ids, "1,4");
}

#[tokio::test]
async fn enum_field_in_filters_the_list_route() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, ids) = list_state_ids(&router, "state__in=funded,reserved&sort=id").await;
    assert_eq!(status, StatusCode::OK, "in filter must not 400: {ids}");
    assert_eq!(ids, "1,2,3");
}

#[tokio::test]
async fn enum_field_unknown_variant_names_field_and_accepted_values() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, message) = list_state_ids(&router, "state=bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        message.contains("state"),
        "error must name the field, got: {message}"
    );
    assert!(
        message.contains("reserved") && message.contains("funded") && message.contains("cancelled"),
        "error must name the accepted values, got: {message}"
    );
}

/// Declaration order is not a meaningful ordering to expose (issue
/// #928's explicit ask) — `lt`/`gt` must stay unsupported for an enum
/// field exactly like they were (never generated) before this fix.
#[tokio::test]
async fn enum_field_ordering_operators_stay_unsupported() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());

    let (status, message) = list_state_ids(&router, "state__lt=funded").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        message.contains("unsupported query filter"),
        "got: {message}"
    );
}
