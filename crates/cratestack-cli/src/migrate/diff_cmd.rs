//! `cratestack migrate diff` handler.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use cratestack_migrate::{
    EmittedMigration, Snapshot, diff_projections, emit::postgres, emit::sqlite, project,
    read_or_empty, write_snapshot,
};

use crate::cli_types::MigrateBackendArg;

use super::backend::{Backend, expand};
use super::slug::sanitize_slug;

pub(crate) fn handle_diff(
    schema: PathBuf,
    out_dir: PathBuf,
    backend: MigrateBackendArg,
    name: String,
    allow_destructive: bool,
) -> Result<()> {
    let next_schema = cratestack_parser::parse_schema_file(&schema)
        .map_err(|error| anyhow::anyhow!("{}", crate::cli_support::render_schema_error(&error)))?;

    let slug = sanitize_slug(&name);
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let directory_name = format!("{timestamp}_{slug}");

    let backends = expand(backend);
    let mut nothing_to_do = true;
    let next_projections = project(&next_schema);

    for backend in backends {
        let backend_dir = out_dir.join(backend.slug());
        let snapshot_path = backend_dir.join("schema.snapshot.json");

        let prev_snapshot = read_or_empty(&snapshot_path)
            .with_context(|| format!("reading snapshot at {}", snapshot_path.display()))?;

        let ops = diff_projections(&prev_snapshot.projections, &next_projections)
            .with_context(|| format!("diffing [{}]", backend.slug()))?;
        if ops.is_empty() {
            println!("migrate diff [{}]: no changes", backend.slug());
            continue;
        }

        let migration = match backend {
            Backend::Postgres => postgres::emit(&ops),
            Backend::Sqlite => sqlite::emit(&ops),
        };

        if migration.has_lossy && !allow_destructive {
            bail!(
                "migrate diff [{}]: refusing to write destructive migration without \
                 --allow-destructive. The diff contains DROP operations that would \
                 destroy data on apply.",
                backend.slug()
            );
        }

        if !migration.unverified_dbgenerated.is_empty() {
            let columns = migration
                .unverified_dbgenerated
                .iter()
                .map(|(table, column)| format!("{table}.{column}"))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "migrate diff [{}]: warning: {} column(s) use `@default(dbgenerated())` \
                 with no way to verify a real Postgres-level default exists ({columns}). \
                 See the generated migration for details — INSERTs that omit these \
                 columns will fail with a NOT NULL violation unless a default is set \
                 some other way.",
                backend.slug(),
                migration.unverified_dbgenerated.len()
            );
        }

        let migration_dir = backend_dir.join(&directory_name);
        write_migration(&migration_dir, &migration)
            .with_context(|| format!("writing migration to {}", migration_dir.display()))?;

        if migration.up_pre.is_some() {
            // Loud, because the failure it prevents is invisible until
            // production: a blocking migration passes against the empty
            // database CI migrates and fails against the one with rows.
            eprintln!(
                "migrate diff [{}]: this migration blocks on existing data. Scaffolded \
                 {}/up.pre.sql — fill in the backfill before deploying anywhere with \
                 rows. It runs immediately before up.sql in the same transaction.",
                backend.slug(),
                migration_dir.display()
            );
        }

        let next_snapshot = Snapshot::from_projections(next_projections.clone());
        write_snapshot(&next_snapshot, &snapshot_path)
            .with_context(|| format!("updating snapshot at {}", snapshot_path.display()))?;

        nothing_to_do = false;
        println!(
            "migrate diff [{}]: wrote {} ({}{}{})",
            backend.slug(),
            migration_dir.display(),
            if migration.has_blocking {
                "blocking"
            } else if migration.has_lossy {
                "lossy"
            } else {
                "safe"
            },
            if migration.has_blocking && migration.has_lossy {
                "+"
            } else {
                ""
            },
            if migration.has_blocking && migration.has_lossy {
                "lossy"
            } else {
                ""
            },
        );
    }

    if nothing_to_do {
        println!("migrate diff: schema is in sync with all selected backends");
    }
    Ok(())
}

fn write_migration(directory: &Path, migration: &EmittedMigration) -> Result<()> {
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    let up_path = directory.join("up.sql");
    let down_path = directory.join("down.sql");
    fs::write(&up_path, &migration.up).with_context(|| format!("writing {}", up_path.display()))?;
    fs::write(&down_path, &migration.down)
        .with_context(|| format!("writing {}", down_path.display()))?;
    if let Some(up_pre) = &migration.up_pre {
        let up_pre_path = directory.join("up.pre.sql");
        // Create-new, never truncate. `migrate diff` writes a fresh
        // timestamped directory every run, so a collision here means
        // something already put a file at this path — and that file is
        // hand-authored backfill SQL by definition. Losing it silently
        // to a scaffold full of TODO comments is the one failure mode
        // this file must not have.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&up_pre_path)
        {
            Ok(mut file) => io::Write::write_all(&mut file, up_pre.as_bytes())
                .with_context(|| format!("writing {}", up_pre_path.display()))?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                eprintln!(
                    "migrate diff: {} already exists — keeping it, scaffold not written",
                    up_pre_path.display()
                );
            }
            Err(error) => {
                return Err(error).with_context(|| format!("creating {}", up_pre_path.display()));
            }
        }
    }
    Ok(())
}
