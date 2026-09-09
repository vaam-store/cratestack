//! End-to-end REST regression coverage for cratestack#953. Equality
//! operators on an optional scalar used to be omitted from the generated
//! Axum filter switch, so the equality requests below returned HTTP 400
//! before they reached Postgres. The `isNull` case guards existing behavior.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/optional_query_filter.cstack", db = Postgres);

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
    cratestack::sqlx::query("DROP TABLE IF EXISTS optional_deliveries")
        .execute(pool)
        .await
        .expect("drop optional_deliveries");
    cratestack::sqlx::query(
        "CREATE TABLE optional_deliveries (id BIGINT PRIMARY KEY, verification_id TEXT NULL)",
    )
    .execute(pool)
    .await
    .expect("create optional_deliveries");
    cratestack::sqlx::query(
        "INSERT INTO optional_deliveries (id, verification_id) VALUES \
         (1, NULL), (2, 'verification-1'), (3, 'verification-2'), (4, 'verification-1')",
    )
    .execute(pool)
    .await
    .expect("seed optional_deliveries");
}

fn router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        (),
        CborCodec,
        AnyoneAuth,
    )
}

async fn list_ids(router: &cratestack::axum::Router, query: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/optional_deliveries?{query}"))
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

async fn seeded_router() -> Option<(pg::TestPg, cratestack::axum::Router)> {
    let test_pg = pg::connect_or_skip().await?;
    reset_and_seed(&test_pg.pool).await;
    let router = router(test_pg.pool.clone());
    Some((test_pg, router))
}

#[tokio::test]
async fn optional_string_equality_operators_filter_the_rest_list_route() {
    let _guard = pg::serial_guard().await;
    let Some((_test_pg, router)) = seeded_router().await else {
        return;
    };

    let cases = [
        ("verificationId=verification-1&sort=id", "2,4"),
        ("verificationId__eq=verification-1&sort=id", "2,4"),
        ("verificationId__ne=verification-1&sort=id", "3"),
        (
            "verificationId__in=verification-1,verification-2&sort=id",
            "2,3,4",
        ),
    ];
    for (query, expected_ids) in cases {
        let (status, ids) = list_ids(&router, query).await;
        assert_eq!(status, StatusCode::OK, "filter must not 400: {ids}");
        assert_eq!(ids, expected_ids, "unexpected rows for {query}");
    }
}

#[tokio::test]
async fn optional_string_is_null_filter_remains_available() {
    let _guard = pg::serial_guard().await;
    let Some((_test_pg, router)) = seeded_router().await else {
        return;
    };

    let (status, ids) = list_ids(&router, "verificationId__isNull=true&sort=id").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "isNull filter must not regress: {ids}"
    );
    assert_eq!(ids, "1");
}
