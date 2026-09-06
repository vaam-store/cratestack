mod build_runner;
mod cli_handlers;
mod cli_support;
mod cli_types;
mod drift;
mod migrate;
mod schema_diff;

use anyhow::Result;
use clap::Parser;

use crate::cli_handlers::run;
use crate::cli_types::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser;

    use crate::Cli;
    use crate::cli_support::{json_check_failure, json_check_success};
    use crate::cli_types::{Command, DartPresetArg, StudioCmd};

    #[test]
    fn json_success_payload_has_empty_diagnostics() {
        let payload = json_check_success(Path::new("schema.cstack"));
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["schema"], "schema.cstack");
        assert_eq!(payload["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn json_failure_payload_exposes_structured_diagnostic_fields() {
        // `parse_schema_named` (not the anonymous `parse_schema`) so the
        // resulting error's file identity (cratestack#916) matches the
        // `schema.cstack` path `json_check_failure` is given below — the
        // same relationship `handle_check` relies on for a real schema file.
        let error = cratestack_parser::parse_schema_named(
            "schema.cstack",
            "model User {\n  email String\n}\n",
        )
        .expect_err("schema should fail validation");
        let payload = json_check_failure(Path::new("schema.cstack"), &error);
        let diagnostic = &payload["diagnostics"][0];

        assert_eq!(payload["ok"], false);
        assert_eq!(diagnostic["file"], "schema.cstack");
        assert_eq!(diagnostic["line"], 1);
        assert!(diagnostic["start"].as_u64().is_some());
        assert!(diagnostic["end"].as_u64().is_some());
        assert!(
            diagnostic["message"]
                .as_str()
                .expect("message should be a string")
                .contains("missing an @id field")
        );
    }

    #[test]
    fn generate_dart_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-dart",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
        ]);

        match cli.command {
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
            } => {
                assert_eq!(schema, PathBuf::from("schema.cstack"));
                assert_eq!(out, PathBuf::from("out"));
                assert_eq!(library_name, "cratestack_client");
                assert_eq!(base_path, "/api");
                assert_eq!(template_dir, None);
                assert!(!check);
                assert_eq!(preset, DartPresetArg::Default);
                assert!(
                    !run_build_runner,
                    "--run-build-runner must default to off (issue #303: opt-in, not default)"
                );
                assert!(
                    !no_native_cbor,
                    "native cbor must default to ON (cratestack#563 follow-up: \
                     cratestack_cbor 0.8.7 covers every platform but Linux arm64); \
                     --no-native-cbor must be passed explicitly to turn it off"
                );
            }
            _ => panic!("expected generate-dart command"),
        }
    }

    #[test]
    fn generate_dart_clap_accepts_no_native_cbor_flag() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-dart",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--no-native-cbor",
        ]);

        match cli.command {
            Command::GenerateDart { no_native_cbor, .. } => {
                assert!(no_native_cbor);
            }
            _ => panic!("expected generate-dart command"),
        }
    }

    #[test]
    fn generate_dart_clap_accepts_riverpod_preset() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-dart",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--preset",
            "riverpod",
        ]);

        match cli.command {
            Command::GenerateDart { preset, .. } => {
                assert_eq!(preset, DartPresetArg::Riverpod);
            }
            _ => panic!("expected generate-dart command"),
        }
    }

    #[test]
    fn generate_dart_clap_accepts_run_build_runner_flag() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-dart",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--preset",
            "riverpod",
            "--run-build-runner",
        ]);

        match cli.command {
            Command::GenerateDart {
                preset,
                run_build_runner,
                ..
            } => {
                assert_eq!(preset, DartPresetArg::Riverpod);
                assert!(run_build_runner);
            }
            _ => panic!("expected generate-dart command"),
        }
    }

    #[test]
    fn generate_typescript_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
        ]);

        match cli.command {
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
            } => {
                assert_eq!(schema, PathBuf::from("schema.cstack"));
                assert_eq!(out, PathBuf::from("out"));
                assert_eq!(package_name, "cratestack-client");
                assert_eq!(base_path, "/api");
                assert_eq!(template_dir, None);
                assert!(!check);
                assert!(!full_selection);
                assert!(
                    !swr,
                    "--swr must default to off (issue #591: opt-in, additive)"
                );
                assert!(
                    !refine,
                    "--refine must default to off (issue #571: opt-in, additive)"
                );
                assert!(
                    !tanstack,
                    "--tanstack must default to off (issue #617: opt-in, additive)"
                );
                assert!(
                    !no_native_cbor,
                    "native cbor must default to ON for RPC-transport schemas (issue #746: \
                     @cratestack/cbor is now the default RPC codec); --no-native-cbor must be \
                     passed explicitly to turn it off"
                );
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn generate_typescript_clap_accepts_no_native_cbor_flag() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--no-native-cbor",
        ]);

        match cli.command {
            Command::GenerateTypeScript { no_native_cbor, .. } => {
                assert!(no_native_cbor);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn generate_typescript_clap_accepts_swr_flag() {
        // Issue #591: `--swr` additionally emits the file-per-model +
        // hooks layout under `src/swr/`, alongside (not instead of) the
        // default layout.
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--swr",
        ]);

        match cli.command {
            Command::GenerateTypeScript { swr, .. } => {
                assert!(swr);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn generate_typescript_clap_accepts_tanstack_flag() {
        // Issue #617: `--tanstack` additionally emits `src/react-query.ts`
        // (TanStack Query hooks), the `./react-query.js` re-export, and the
        // `@tanstack/react-query` peer/dev dependency — all three were
        // unconditional before this flag existed.
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--tanstack",
        ]);

        match cli.command {
            Command::GenerateTypeScript { tanstack, .. } => {
                assert!(tanstack);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn generate_typescript_clap_accepts_full_selection_flag() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--full-selection",
        ]);

        match cli.command {
            Command::GenerateTypeScript { full_selection, .. } => {
                assert!(full_selection);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn generate_wiremock_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-wiremock",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
        ]);

        match cli.command {
            Command::GenerateWiremock {
                schema,
                out,
                base_path,
                check,
            } => {
                assert_eq!(schema, PathBuf::from("schema.cstack"));
                assert_eq!(out, PathBuf::from("out"));
                assert_eq!(base_path, "/api");
                assert!(!check);
            }
            _ => panic!("expected generate-wiremock command"),
        }
    }

    #[test]
    fn studio_run_clap_defaults() {
        let cli = Cli::parse_from(["cratestack", "studio", "run"]);
        match cli.command {
            Command::Studio {
                cmd: StudioCmd::Run { config, bind },
            } => {
                assert_eq!(config, PathBuf::from("studio.toml"));
                assert!(bind.is_none());
            }
            _ => panic!("expected studio run command"),
        }
    }

    #[test]
    fn studio_init_clap_defaults() {
        let cli = Cli::parse_from(["cratestack", "studio", "init"]);
        match cli.command {
            Command::Studio {
                cmd: StudioCmd::Init { out, force },
            } => {
                assert_eq!(out, PathBuf::from("."));
                assert!(!force);
            }
            _ => panic!("expected studio init command"),
        }
    }

    #[test]
    fn diff_clap_defaults() {
        let cli = Cli::parse_from(["cratestack", "diff", "old.cstack", "new.cstack"]);
        match cli.command {
            Command::Diff { old, new, json } => {
                assert_eq!(old, PathBuf::from("old.cstack"));
                assert_eq!(new, PathBuf::from("new.cstack"));
                assert!(!json);
            }
            _ => panic!("expected diff command"),
        }
    }

    #[test]
    fn version_long_flag_is_accepted() {
        let result = Cli::try_parse_from(["cratestack", "--version"]);
        let error = result.expect_err("--version should short-circuit parsing");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn version_short_flag_is_accepted() {
        let result = Cli::try_parse_from(["cratestack", "-V"]);
        let error = result.expect_err("-V should short-circuit parsing");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn studio_eject_requires_out() {
        let result = Cli::try_parse_from(["cratestack", "studio", "eject"]);
        assert!(
            result.is_err(),
            "studio eject must require --out, got {:?}",
            result.map(|cli| cli.command)
        );
    }
}
