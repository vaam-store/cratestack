use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::cli_support::{
    hash_schema_source, into_generated_files, json_check_failure, json_check_success,
    parse_schema_or_render, render_schema_error, write_generated_files,
};
use crate::cli_types::{Cli, Command, DartPresetArg, MigrateAction, OutputFormat, StudioCmd};
use crate::drift::check_drift;

#[cfg(test)]
mod tests_generate;
mod tests_query;

pub(crate) fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Check { schema, format } => handle_check(schema, format)?,
        Command::GenerateDart {
            schema,
            out,
            library_name,
            base_path,
            template_dir,
            check,
            preset,
            run_build_runner,
            no_native_cbor,
        } => handle_generate_dart(
            schema,
            out,
            library_name,
            base_path,
            template_dir,
            check,
            preset,
            run_build_runner,
            // The CLI surface is inverted (`--no-native-cbor`, defaulting to
            // off i.e. native-on) but the generator API stays in terms of
            // `native_cbor` — only this boundary converts between the two.
            !no_native_cbor,
        )?,
        Command::GenerateTypeScript {
            schema,
            out,
            package_name,
            base_path,
            template_dir,
            check,
            full_selection,
            swr,
            refine,
            tanstack,
            no_native_cbor,
        } => handle_generate_typescript(
            schema,
            out,
            package_name,
            base_path,
            template_dir,
            check,
            full_selection,
            swr,
            refine,
            tanstack,
            // Same inversion boundary as `GenerateDart`'s `no_native_cbor`
            // above: the CLI surface is `--no-native-cbor` (default off,
            // i.e. native-on), but the generator API stays in terms of
            // `native_cbor`.
            !no_native_cbor,
        )?,
        Command::GenerateWiremock {
            schema,
            out,
            base_path,
            check,
        } => handle_generate_wiremock(schema, out, base_path, check)?,
        Command::Studio { cmd } => handle_studio(cmd)?,
        Command::PrintIr { schema } => handle_print_ir(schema)?,
        Command::Migrate { action } => match action {
            MigrateAction::Diff {
                schema,
                out_dir,
                backend,
                name,
                allow_destructive,
            } => crate::migrate::handle_diff(schema, out_dir, backend, name, allow_destructive)?,
            MigrateAction::Baseline {
                schema,
                database_url,
                out_dir,
                backend,
                strict,
            } => crate::migrate::handle_baseline(schema, database_url, out_dir, backend, strict)?,
        },
        Command::Diff { old, new, json } => handle_diff_schemas(old, new, json)?,
    }

    Ok(())
}

fn handle_check(schema: PathBuf, format: OutputFormat) -> Result<()> {
    match cratestack_parser::parse_schema_file(&schema) {
        Ok(_) => match format {
            OutputFormat::Human => {
                println!("schema OK: {}", schema.display());
            }
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_check_success(&schema))?
                );
            }
        },
        Err(error) => match format {
            OutputFormat::Human => {
                return Err(anyhow::anyhow!(render_schema_error(&error)));
            }
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_check_failure(&schema, &error))?
                );
                std::process::exit(1);
            }
        },
    }

    Ok(())
}

fn handle_generate_dart(
    schema: PathBuf,
    out: PathBuf,
    library_name: String,
    base_path: String,
    template_dir: Option<PathBuf>,
    check: bool,
    preset: DartPresetArg,
    run_build_runner: bool,
    native_cbor: bool,
) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    let schema_sha256 = hash_schema_source(&schema)?;
    let package = cratestack_client_dart::generate_package(
        &parsed,
        &cratestack_client_dart::DartGeneratorConfig {
            library_name,
            base_path,
            template_dir,
            preset: preset.into(),
            schema_sha256,
            native_cbor,
        },
    )?;
    let files = into_generated_files(package.files);

    if check {
        return check_drift(&out, &files, "Dart");
    }

    write_generated_files(&out, files)?;
    println!("generated Dart client package: {}", out.display());

    if run_build_runner {
        println!(
            "running `dart run build_runner build --delete-conflicting-outputs` in {}...",
            out.display()
        );
        crate::build_runner::run_build_runner(&out)?;
        println!("build_runner finished");
    }

    Ok(())
}

fn handle_generate_typescript(
    schema: PathBuf,
    out: PathBuf,
    package_name: String,
    base_path: String,
    template_dir: Option<PathBuf>,
    check: bool,
    full_selection: bool,
    swr: bool,
    refine: bool,
    tanstack: bool,
    native_cbor: bool,
) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    let schema_sha256 = hash_schema_source(&schema)?;
    let package = cratestack_client_typescript::generate_package(
        &parsed,
        &cratestack_client_typescript::TypeScriptGeneratorConfig {
            package_name,
            base_path,
            template_dir,
            swr,
            full_selection,
            refine,
            tanstack,
            schema_sha256,
            native_cbor,
        },
    )?;
    let files = into_generated_files(package.files);

    if check {
        return check_drift(&out, &files, "TypeScript");
    }

    write_generated_files(&out, files)?;
    println!("generated TypeScript client package: {}", out.display());
    Ok(())
}

fn handle_generate_wiremock(
    schema: PathBuf,
    out: PathBuf,
    base_path: String,
    check: bool,
) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    let package = cratestack_mock_wiremock::generate_package(
        &parsed,
        &cratestack_mock_wiremock::WireMockGeneratorConfig { base_path },
    )?;
    let files = into_generated_files(package.files);

    if check {
        return check_drift(&out, &files, "WireMock");
    }

    write_generated_files(&out, files)?;
    println!("generated WireMock stub mappings: {}", out.display());
    Ok(())
}

fn handle_diff_schemas(old: PathBuf, new: PathBuf, json: bool) -> Result<()> {
    let old_schema = parse_schema_or_render(&old)?;
    let new_schema = parse_schema_or_render(&new)?;
    let diff = crate::schema_diff::diff_schemas(&old_schema, &new_schema);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&crate::schema_diff::render_json(&diff))?
        );
    } else {
        print!(
            "{}",
            crate::schema_diff::render_human(
                &diff,
                &old.display().to_string(),
                &new.display().to_string()
            )
        );
    }

    if diff.has_breaking() {
        std::process::exit(1);
    }
    Ok(())
}

fn handle_print_ir(schema: PathBuf) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    println!("{parsed:#?}");
    Ok(())
}

fn handle_studio(cmd: StudioCmd) -> Result<()> {
    match cmd {
        StudioCmd::Init { out, force } => handle_studio_init(out, force),
        StudioCmd::Run { config, bind } => handle_studio_run(config, bind),
        StudioCmd::Eject {
            out,
            name,
            force,
            with_ui,
        } => handle_studio_eject(out, name, force, with_ui),
    }
}

fn handle_studio_init(out: PathBuf, force: bool) -> Result<()> {
    std::fs::create_dir_all(&out)
        .with_context(|| format!("failed to create output directory '{}'", out.display()))?;
    let target = out.join(cratestack_studio::DEFAULT_CONFIG_FILE);
    if target.exists() && !force {
        bail!(
            "'{}' already exists; pass --force to overwrite",
            target.display()
        );
    }
    std::fs::write(&target, cratestack_studio::STARTER_CONFIG)
        .with_context(|| format!("failed to write '{}'", target.display()))?;
    println!("wrote starter studio config: {}", target.display());
    Ok(())
}

fn handle_studio_run(config: PathBuf, bind: Option<String>) -> Result<()> {
    let bind_addr: SocketAddr = match bind {
        Some(value) => value
            .parse()
            .with_context(|| format!("invalid --bind '{value}'"))?,
        None => cratestack_studio::DEFAULT_BIND
            .parse()
            .expect("default bind is a valid socket addr"),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start tokio runtime")?;

    runtime.block_on(async {
        cratestack_studio::run(cratestack_studio::ServerOptions {
            config_path: config,
            bind: bind_addr,
        })
        .await
        .map_err(anyhow::Error::from)
    })
}

fn handle_studio_eject(
    out: PathBuf,
    name: Option<String>,
    force: bool,
    with_ui: bool,
) -> Result<()> {
    let report = cratestack_studio::eject(&cratestack_studio::EjectOptions {
        out: out.clone(),
        name,
        force,
        with_ui,
    })?;
    println!(
        "ejected starter project to '{}' ({} files written)",
        report.out.display(),
        report.written.len()
    );
    if report.with_ui {
        println!(
            "next steps: `cd {} && cargo run` to start the studio, \
             and `(cd ui && trunk serve)` to iterate on the UI",
            report.out.display(),
        );
    } else {
        println!(
            "next steps: `cd {} && cargo run` to start the studio",
            report.out.display(),
        );
    }
    Ok(())
}
