//! A `@computed` field on a **model** whose own declared type is a `type`
//! block, resolved and composed into the model read response over RPC.
//!
//! `docs/design/computed-fields.md`'s "Schema surface" has always said a
//! computed field's own type may be "a scalar, enum, or non-computed-bearing
//! `type`", and `validate::computed::validate_computed` enforces exactly that
//! rule over models and types alike. But `validate::type_names::
//! reject_type_decl_as_model_field_type` (#230/#235) rejected *every* model
//! field declared with a `type` block, including one carrying `@computed` —
//! so on a model the documented shape was unreachable. That rule is about
//! **storage**: a `type` has no `CREATE TYPE` behind it, so it cannot back a
//! column. A `@computed` field is never a column (`cratestack-migrate`'s
//! `convert.rs` skips it before `field_to_column` ever runs), so the rule
//! never applied to it.
//!
//! This test is the empirical proof that once the parser stops rejecting the
//! shape, the rest of the pipeline already carries it: the generated resolver
//! returns `Option<CompTvMedia>`, and `ProjectedValue::leaf` (which takes any
//! `T: Serialize`) serializes that struct as a nested object on the wire —
//! no scalar-only assumption anywhere on the server path.
//!
//! PG-gated: skips silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS`, same pattern as every other PG
//! integration test in this crate (see `tests/support/pg.rs`).

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RpcGetInput, RpcListInput};
use cratestack::sqlx::query;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/computed_fields_type_valued.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS comp_tv_products")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE comp_tv_products (
            id BIGINT PRIMARY KEY,
            title TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create comp_tv_products");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query("INSERT INTO comp_tv_products (id, title) VALUES (1, 'kettle'), (2, 'lamp')")
        .execute(pool)
        .await
        .expect("seed products");
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
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

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

/// Returns a whole `CompTvMedia` — an enum plus two scalars — rather than
/// one scalar. `width` (when supplied via `computedParams`) is folded into
/// the URL so the params path is exercised on a composite return value too.
#[derive(Clone)]
struct TestResolver;

impl cratestack_schema::ComputedFieldResolver for TestResolver {
    fn resolve_comp_tv_product_default_media(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::CompTvProduct,
        params: Option<&cratestack_schema::CompTvMediaParams>,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<
        Output = Result<Option<cratestack_schema::CompTvMedia>, CratestackError>,
    > + Send {
        let title = source.title.clone();
        let width = params.and_then(|params| params.width);
        async move {
            Ok(Some(cratestack_schema::CompTvMedia {
                assetId: format!("asset-{title}"),
                kind: cratestack_schema::CompTvMediaKind::image,
                url: match width {
                    Some(width) => format!("https://cdn.example/{title}.jpg?w={width}"),
                    None => format!("https://cdn.example/{title}.jpg"),
                },
            }))
        }
    }
}

fn test_router(pool: &cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    cratestack_schema::axum::rpc_router(
        db,
        NoProcedures,
        TestResolver,
        JsonCodec,
        PassThroughAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn rpc_unary(
    router: cratestack::axum::Router,
    op_id: &str,
    body: Vec<u8>,
) -> (StatusCode, cratestack::serde_json::Value) {
    let response = router
        .oneshot(
            Request::post(format!("/rpc/{op_id}"))
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&bytes).expect("response should decode as JSON");
    (status, value)
}

/// The whole point: `defaultMedia` arrives as a nested **object** with the
/// `type`'s own three fields, not as a scalar and not flattened into
/// sibling keys on the model.
#[tokio::test]
async fn rpc_get_composes_a_type_valued_computed_field_as_a_nested_object() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.CompTvProduct.get", body).await;
    assert_eq!(status, StatusCode::OK);

    let media = value
        .get("defaultMedia")
        .expect("defaultMedia should be present on the response");
    assert!(
        media.is_object(),
        "a `type`-valued computed field must serialize as a nested object, got: {media}"
    );
    assert_eq!(
        media.get("assetId"),
        Some(&cratestack::serde_json::Value::from("asset-kettle"))
    );
    assert_eq!(
        media.get("kind"),
        Some(&cratestack::serde_json::Value::from("image")),
        "an enum nested inside the resolver's return value keeps its own wire encoding"
    );
    assert_eq!(
        media.get("url"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/kettle.jpg"
        ))
    );
}

/// `computedParams` reaches a `type`-valued resolver on exactly the same
/// terms it reaches a scalar one.
#[tokio::test]
async fn rpc_get_passes_computed_params_to_a_type_valued_resolver() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        computed_params: Some(r#"{"defaultMedia":{"width":800}}"#.to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.CompTvProduct.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value.get("defaultMedia").and_then(|media| media.get("url")),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/kettle.jpg?w=800"
        ))
    );
}

/// Composition is per row on `list`, same as for a scalar computed field.
#[tokio::test]
async fn rpc_list_composes_a_type_valued_computed_field_per_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcListInput {
        sort: Some("id".to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("list input should encode");

    let (status, value) = rpc_unary(router, "model.CompTvProduct.list", body).await;
    assert_eq!(status, StatusCode::OK);

    let items = value.as_array().expect("list response should be an array");
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0]
            .get("defaultMedia")
            .and_then(|media| media.get("url")),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/kettle.jpg"
        ))
    );
    assert_eq!(
        items[1]
            .get("defaultMedia")
            .and_then(|media| media.get("url")),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/lamp.jpg"
        )),
        "each row resolves its own value rather than sharing one"
    );
}

/// `?fields=` selection still governs whether the resolver runs at all —
/// a `type`-valued field is projected (or not) like any other.
#[tokio::test]
async fn rpc_get_field_selection_can_exclude_a_type_valued_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        fields: Some(vec!["id".to_owned(), "title".to_owned()]),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.CompTvProduct.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        value.get("defaultMedia").is_none(),
        "an unselected computed field must not be resolved or projected: {value}"
    );
}
