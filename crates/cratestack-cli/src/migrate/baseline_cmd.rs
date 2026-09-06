//! `cratestack migrate baseline` handler (issue #205).
//!
//! Adopts an already-existing Postgres database as the starting point
//! for `migrate diff`: introspects the live schema, diffs it against
//! the authored `.cstack` schema for a drift report (design doc
//! §5.4), writes the snapshot from the introspected shape, and seeds
//! a synthetic row into `cratestack_migrations` so the runtime
//! applier agrees with the authoring side about what's already there
//! (design doc §5.3, option (b) — snapshot + synthetic runner row;
//! see the PR description for the reasoning).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use cratestack_core::Schema;
use cratestack_migrate::introspect::postgres::introspect;
use cratestack_migrate::{
    Snapshot, diff_projections, project, projections_checksum, write_snapshot,
};
use cratestack_sqlx::{Migration, apply_pending};
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};

use crate::cli_types::BaselineBackendArg;

use super::backend::Backend;
use super::drift_report;

pub(crate) fn handle_baseline(
    schema: PathBuf,
    database_url: String,
    out_dir: PathBuf,
    backend: BaselineBackendArg,
    strict: bool,
) -> Result<()> {
    // Exhaustive destructure rather than an `assert`/ignored binding:
    // `BaselineBackendArg` has exactly one variant today (baseline is
    // Postgres-only for v1, design doc §6 open question 2), so this
    // fails to compile — not silently ignores the value — the day a
    // second variant is added without updating this handler.
    let BaselineBackendArg::Postgres = backend;

    // Checked before parsing the schema or touching the network:
    // refusing an already-baselined backend should be immediate, with
    // no writes and no DB round-trip — see the hard requirement in
    // issue #205.
    let snapshot_path = out_dir
        .join(Backend::Postgres.slug())
        .join("schema.snapshot.json");
    if snapshot_path.exists() {
        bail!(
            "migrate baseline: a snapshot already exists at {} — refusing to overwrite an \
             already-managed backend. Remove it first if you really intend to re-baseline.",
            snapshot_path.display()
        );
    }

    let next_schema = cratestack_parser::parse_schema_file(&schema)
        .map_err(|error| anyhow::anyhow!("{}", crate::cli_support::render_schema_error(&error)))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start tokio runtime")?;

    runtime.block_on(run_baseline(
        &next_schema,
        &database_url,
        &snapshot_path,
        strict,
    ))
}

async fn run_baseline(
    next_schema: &Schema,
    database_url: &str,
    snapshot_path: &Path,
    strict: bool,
) -> Result<()> {
    let pool: PgPool = PoolOptions::<Postgres>::new()
        .max_connections(2)
        .connect(database_url)
        .await
        .with_context(|| format!("failed to connect to '{database_url}'"))?;

    let report = introspect(&pool)
        .await
        .context("failed to introspect the live database")?;

    let authored = project(next_schema);
    let ops = diff_projections(&report.projections, &authored)
        .context("diffing the introspected database against the authored schema")?;

    println!("{}", drift_report::render(&ops, &report.unmapped_columns));

    if strict && !ops.is_empty() {
        bail!(
            "migrate baseline: --strict refuses to baseline with {} pending drift change(s); \
             resolve the drift above (or drop --strict) and try again. No snapshot was written \
             and no baseline row was recorded.",
            ops.len()
        );
    }

    let snapshot = Snapshot::from_projections(report.projections.clone());
    write_snapshot(&snapshot, snapshot_path)
        .with_context(|| format!("writing snapshot to {}", snapshot_path.display()))?;

    let table_count = report.projections.tables.len();
    let checksum = projections_checksum(&report.projections)
        .context("failed to checksum the introspected shape")?;
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let baseline = Migration {
        id: format!("{timestamp}_baseline"),
        description: format!("baseline: adopted {table_count} existing table(s)"),
        up_pre: None,
        up: baseline_marker_sql(table_count, &checksum),
        down: None,
    };
    apply_pending(&pool, std::slice::from_ref(&baseline))
        .await
        .context("failed to record the baseline row in cratestack_migrations")?;

    println!(
        "migrate baseline: wrote {} and recorded baseline `{}` in cratestack_migrations",
        snapshot_path.display(),
        baseline.id
    );
    Ok(())
}

/// The `up` script recorded for the synthetic baseline row. Pure SQL
/// comments — baseline never applies DDL, it only records provenance
/// — but the embedded checksum still flows into
/// [`Migration::checksum`], so the stored row's checksum changes
/// whenever the introspected shape does (design doc §5.3: "so a
/// second baseline run against a since-drifted DB is detectable").
fn baseline_marker_sql(table_count: usize, introspected_shape_checksum: &str) -> String {
    format!(
        "-- cratestack migrate baseline: adopted {table_count} existing table(s) as-is.\n\
         -- No DDL is applied by this migration — it only records that the\n\
         -- pre-existing schema state is this database's migration starting point.\n\
         -- introspected-shape checksum: {introspected_shape_checksum}\n"
    )
}
