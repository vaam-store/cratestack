//! Per-target materialization: parse the `.cstack`, open the pool /
//! HTTP client, wrap the result in an `Arc<dyn DataSource>`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};

use crate::config::{ApiAuth, DbDriver, StudioConfig, TargetConfig, resolve_secret};
use crate::data::DataSource;
use crate::data::api::ApiSource;
use crate::data::postgres::PostgresSource;
use crate::data::sqlite::SqliteSource;

use super::{LoadedTarget, WorkspaceError, open_sqlite};

pub(super) async fn load_target(
    target: &TargetConfig,
    parent: &StudioConfig,
    base_dir: &Path,
) -> Result<LoadedTarget, WorkspaceError> {
    let schema_path = if target.schema.is_absolute() {
        target.schema.clone()
    } else {
        base_dir.join(&target.schema)
    };

    let schema_text =
        std::fs::read_to_string(&schema_path).map_err(|source| WorkspaceError::SchemaIo {
            key: target.key.clone(),
            path: schema_path.clone(),
            source,
        })?;
    // `parse_schema_named` (not the anonymous `parse_schema`) so the
    // resulting `SchemaError` carries this target's real schema path as its
    // file identity (cratestack#916) — `error.render()` below then reflects
    // `schema_path`, not the library's internal `"<schema>"` placeholder.
    let schema =
        cratestack_parser::parse_schema_named(&schema_path.display().to_string(), &schema_text)
            .map_err(|error| WorkspaceError::SchemaParse {
                key: target.key.clone(),
                path: schema_path.clone(),
                rendered: error.render(),
            })?;
    let schema = Arc::new(schema);

    let source: Arc<dyn DataSource> = if let Some(db) = &target.db {
        build_db_source(target, db, schema.clone()).await?
    } else if let Some(api) = &target.api {
        build_api_source(target, api, schema.clone())?
    } else {
        unreachable!("StudioConfig::validate rejects targets without db or api");
    };

    let display_name = target
        .display_name
        .clone()
        .unwrap_or_else(|| target.key.clone());

    Ok(LoadedTarget {
        key: target.key.clone(),
        display_name,
        mode: parent.target_mode(target),
        schema,
        schema_path,
        source,
        has_db: target.db.is_some(),
        has_api: target.api.is_some(),
        allow_unsafe_db_writes: target.db.as_ref().is_some_and(|db| db.allow_unsafe_writes),
    })
}

async fn build_db_source(
    target: &TargetConfig,
    db: &crate::config::TargetDb,
    schema: Arc<cratestack_core::Schema>,
) -> Result<Arc<dyn DataSource>, WorkspaceError> {
    match db.driver {
        DbDriver::Postgres => {
            let url = resolve_secret(&db.url, &format!("target[{}].db.url", target.key))?;
            let pool: PgPool = PoolOptions::<Postgres>::new()
                .max_connections(db.max_connections.unwrap_or(5))
                .acquire_timeout(Duration::from_secs(10))
                .connect(&url)
                .await
                .map_err(|source| WorkspaceError::Pool {
                    key: target.key.clone(),
                    driver: DbDriver::Postgres,
                    source,
                })?;
            Ok(Arc::new(PostgresSource::new(pool, schema)))
        }
        DbDriver::Sqlite => {
            let url = resolve_secret(&db.url, &format!("target[{}].db.url", target.key))?;
            let target_key = target.key.clone();
            let conn = tokio::task::spawn_blocking(move || open_sqlite(&url))
                .await
                .map_err(|e| WorkspaceError::SqliteJoin {
                    key: target_key.clone(),
                    message: e.to_string(),
                })?
                .map_err(|source| WorkspaceError::SqliteOpen {
                    key: target.key.clone(),
                    source,
                })?;
            Ok(Arc::new(SqliteSource::new(conn, schema)))
        }
        other => Err(WorkspaceError::UnsupportedDriver {
            key: target.key.clone(),
            driver: other,
        }),
    }
}

fn build_api_source(
    target: &TargetConfig,
    api: &crate::config::TargetApi,
    schema: Arc<cratestack_core::Schema>,
) -> Result<Arc<dyn DataSource>, WorkspaceError> {
    let token_field = format!("target[{}].api.auth.token", target.key);
    let resolved_auth = api
        .auth
        .as_ref()
        .map(|auth| match auth {
            ApiAuth::Bearer { token } => Ok::<_, WorkspaceError>(ApiAuth::Bearer {
                token: resolve_secret(token, &token_field)?,
            }),
            ApiAuth::Header { name, value } => Ok(ApiAuth::Header {
                name: name.clone(),
                value: resolve_secret(value, &format!("target[{}].api.auth.value", target.key))?,
            }),
        })
        .transpose()?;

    Ok(Arc::new(
        ApiSource::new(api.base_url.clone(), resolved_auth.as_ref(), schema).map_err(|source| {
            WorkspaceError::HttpClient {
                key: target.key.clone(),
                source,
            }
        })?,
    ))
}
