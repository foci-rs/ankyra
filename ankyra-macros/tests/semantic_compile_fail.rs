use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

#[derive(Debug)]
struct Case {
    fixture: &'static str,
    code: Option<&'static str>,
    fragments: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        fixture: "command_bad_context",
        code: Some("E0277"),
        fragments: &["State", "ClockCtxView"],
    },
    Case {
        fixture: "duplicate_reply_names_rejected",
        code: Some("E0428"),
        fragments: &["Dup"],
    },
    Case {
        fixture: "provider_rejects_same_leaf_ident",
        code: None,
        fragments: &["duplicate entry `foo`", "crate::a::foo", "crate::b::foo"],
    },
    Case {
        fixture: "static_string_without_config_crate",
        code: Some("E0433"),
        fragments: &["_ankyra_config", "klipper_static_string"],
    },
    Case {
        fixture: "cross_kind_collision",
        code: None,
        fragments: &["duplicate protocol name `ping`"],
    },
    Case {
        fixture: "output_format_collision_err",
        code: None,
        fragments: &["two #[klipper_output] structs share a format string"],
    },
    Case {
        fixture: "reply_case_collision_err",
        code: None,
        fragments: &["duplicate protocol name `foo_bar`"],
    },
    Case {
        fixture: "send_reply_bound_missing",
        code: Some("E0277"),
        fragments: &["SendReply<NotAggregated>", "_ankyra_config::Sender"],
    },
    Case {
        fixture: "provider_unknown_reply_rejected",
        code: None,
        fragments: &["__ankyra_descriptor_Missing"],
    },
    Case {
        fixture: "provider_reply_listed_as_output_rejected",
        code: Some("E0308"),
        fragments: &["OutputDescriptor", "ReplyDescriptor"],
    },
    Case {
        fixture: "provider_stale_path",
        code: Some("E0433"),
        fragments: &["wrong_module"],
    },
    Case {
        fixture: "fail_config_cross_provider_name_collision",
        code: None,
        fragments: &["duplicate protocol name `get_clock`"],
    },
];

#[test]
fn compiler_authored_failures_match_semantic_expectations() {
    let workspace = workspace_root();

    for case in CASES {
        let project = prepare_project(&workspace, case);
        let output = cargo_check(&workspace, &project);
        let diagnostics = diagnostics(&output);

        assert!(
            !output.status.success(),
            "{} compiled successfully, expected failure\n{diagnostics}",
            case.fixture
        );

        if let Some(code) = case.code {
            assert!(
                diagnostics.contains(code),
                "{} did not emit expected diagnostic code {code}\n{diagnostics}",
                case.fixture
            );
        }

        for fragment in case.fragments {
            assert!(
                diagnostics.contains(fragment),
                "{} did not emit expected diagnostic fragment {fragment:?}\n{diagnostics}",
                case.fixture
            );
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ankyra-macros must live under the workspace root")
        .to_path_buf()
}

fn prepare_project(workspace: &Path, case: &Case) -> PathBuf {
    let project = workspace
        .join("target")
        .join("semantic-compile-fail")
        .join(case.fixture);

    if project.exists() {
        fs::remove_dir_all(&project).expect("failed to remove stale semantic fixture project");
    }

    let src_dir = project.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create semantic fixture project");

    let manifest = format!(
        r#"[package]
name = "semantic-{name}"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
ankyra = {{ path = "{ankyra}" }}
ankyra-macros = {{ path = "{ankyra_macros}" }}
"#,
        name = case.fixture.replace('_', "-"),
        ankyra = workspace.join("ankyra").display(),
        ankyra_macros = workspace.join("ankyra-macros").display(),
    );

    fs::write(project.join("Cargo.toml"), manifest).expect("failed to write semantic manifest");

    let fixture = workspace
        .join("ankyra-macros")
        .join("tests")
        .join("trybuild")
        .join(format!("{}.rs", case.fixture));
    fs::copy(fixture, src_dir.join("main.rs")).expect("failed to copy semantic fixture source");

    project
}

fn cargo_check(workspace: &Path, project: &Path) -> Output {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    Command::new(cargo)
        .arg("check")
        .arg("--message-format=json")
        .arg("--quiet")
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .env(
            "CARGO_TARGET_DIR",
            workspace
                .join("target")
                .join("semantic-compile-fail-target"),
        )
        .output()
        .expect("failed to run cargo check for semantic fixture")
}

fn diagnostics(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            text.push_str(line);
            text.push('\n');
            continue;
        };

        if value["reason"] != "compiler-message" {
            continue;
        }

        let message = &value["message"];
        if let Some(code) = message["code"]["code"].as_str() {
            text.push_str(code);
            text.push('\n');
        }
        if let Some(summary) = message["message"].as_str() {
            text.push_str(summary);
            text.push('\n');
        }
        if let Some(rendered) = message["rendered"].as_str() {
            text.push_str(rendered);
            text.push('\n');
        }
    }

    text
}
