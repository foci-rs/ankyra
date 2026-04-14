use std::process::Command;
use std::sync::OnceLock;

fn rustc_is_1_85() -> bool {
    static IS_1_85: OnceLock<bool> = OnceLock::new();

    *IS_1_85.get_or_init(|| {
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .expect("failed to run rustc --version");
        let version = String::from_utf8_lossy(&output.stdout);

        version.starts_with("rustc 1.85.")
    })
}

fn compile_fail_unless_1_85(t: &trybuild::TestCases, path: &str) {
    if rustc_is_1_85() {
        eprintln!("skipping {path}: rustc 1.85 renders this diagnostic differently");
        return;
    }

    t.compile_fail(path);
}

#[test]
fn trybuild_command() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/command_view_ok.rs");
    t.pass("tests/trybuild/command_concrete_ok.rs");
    t.pass("tests/trybuild/command_with_args_ok.rs");
    t.pass("tests/trybuild/command_in_submodule_ok.rs");
    t.pass("tests/trybuild/command_in_shutdown_ok.rs");
    t.pass("tests/trybuild/command_underscore_param_stripped_ok.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/command_bad_context.rs");
    t.compile_fail("tests/trybuild/command_no_ctx_arg.rs");
    t.compile_fail("tests/trybuild/command_shared_ref_rejected.rs");
    t.compile_fail("tests/trybuild/command_owned_ctx_rejected.rs");
    t.compile_fail("tests/trybuild/command_bad_arg_type.rs");
    t.compile_fail("tests/trybuild/command_unknown_attr_rejected.rs");
}

#[test]
fn trybuild_reply() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/reply_ok.rs");
    t.pass("tests/trybuild/reply_call_site_ok.rs");
    t.pass("tests/trybuild/reply_from_call_site_ok.rs");
    t.pass("tests/trybuild/reply_in_submodule_ok.rs");
    t.pass("tests/trybuild/reply_pascal_to_snake_ok.rs");
    t.pass("tests/trybuild/reply_snake_preserved_ok.rs");
    t.pass("tests/trybuild/reply_with_lifetime_ok.rs");
    t.compile_fail("tests/trybuild/reply_bad_field_type.rs");
    t.compile_fail("tests/trybuild/reply_call_site_outside_handler.rs");
    t.compile_fail("tests/trybuild/reply_from_no_sender.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/duplicate_reply_names_rejected.rs");
    t.compile_fail("tests/trybuild/reply_with_type_generic_err.rs");
}

#[test]
fn trybuild_output() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/output_ok.rs");
    t.pass("tests/trybuild/output_explicit_format_ok.rs");
    t.pass("tests/trybuild/output_call_site_ok.rs");
    t.pass("tests/trybuild/output_from_call_site_ok.rs");
    t.pass("tests/trybuild/output_in_submodule_ok.rs");
    t.pass("tests/trybuild/output_pascal_to_snake_ok.rs");
    t.pass("tests/trybuild/output_with_lifetime_ok.rs");
    t.compile_fail("tests/trybuild/output_bad_format_mismatch.rs");
    t.compile_fail("tests/trybuild/output_bad_field_type.rs");
    t.compile_fail("tests/trybuild/output_call_site_outside_handler.rs");
}

#[test]
fn trybuild_constant() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/const_int_ok.rs");
    t.pass("tests/trybuild/const_str_ok.rs");
    t.pass("tests/trybuild/constant_in_submodule_ok.rs");
    t.pass("tests/trybuild/constant_screaming_snake_preserved_ok.rs");
    t.compile_fail("tests/trybuild/const_bad_type.rs");
}

#[test]
fn trybuild_enumeration() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/enum_ok.rs");
    t.pass("tests/trybuild/enum_range_ok.rs");
    t.pass("tests/trybuild/enumeration_in_submodule_ok.rs");
    t.compile_fail("tests/trybuild/enum_bad_attr_rejected.rs");
}

#[test]
fn trybuild_provider() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/provider_ok.rs");
    t.pass("tests/trybuild/submodule_command_ok.rs");
    t.pass("tests/trybuild/nested_submodule_ok.rs");
    t.pass("tests/trybuild/submodule_reply_output_ok.rs");
    t.compile_fail("tests/trybuild/provider_dup_in_list.rs");
    t.compile_fail("tests/trybuild/provider_unknown_key_rejected.rs");
    t.compile_fail("tests/trybuild/provider_rejects_leading_colons.rs");
    t.compile_fail("tests/trybuild/provider_rejects_extern_crate_path.rs");
    t.compile_fail("tests/trybuild/provider_rejects_turbofish.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/provider_rejects_same_leaf_ident.rs");
}

#[test]
fn trybuild_config() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/shutdown_from_call_site_ok.rs");
    t.compile_fail("tests/trybuild/config_bare_ident_rejected.rs");
    t.compile_fail("tests/trybuild/static_string_unlisted_literal.rs");
    t.compile_fail("tests/trybuild/static_string_without_config_crate.rs");
    t.compile_fail("tests/trybuild/shutdown_unlisted_literal.rs");
    t.compile_fail("tests/trybuild/shutdown_from_no_sender.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/cross_kind_collision.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/reply_case_collision_err.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/send_reply_bound_missing.rs");
    compile_fail_unless_1_85(&t, "tests/trybuild/provider_stale_path.rs");
    compile_fail_unless_1_85(
        &t,
        "tests/trybuild/fail_config_cross_provider_name_collision.rs",
    );
    t.compile_fail("tests/trybuild/config_app_not_str_rejected.rs");
}
