#[rustversion::attr(before(1.96), ignore)]
#[test]
fn trybuild_command() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/command_view_ok.rs");
    t.pass("tests/trybuild/command_concrete_ok.rs");
    t.pass("tests/trybuild/command_with_args_ok.rs");
    t.pass("tests/trybuild/command_in_submodule_ok.rs");
    t.pass("tests/trybuild/command_in_shutdown_ok.rs");
    t.pass("tests/trybuild/command_underscore_param_stripped_ok.rs");
    t.compile_fail("tests/trybuild/command_no_ctx_arg.rs");
    t.compile_fail("tests/trybuild/command_shared_ref_rejected.rs");
    t.compile_fail("tests/trybuild/command_owned_ctx_rejected.rs");
    t.compile_fail("tests/trybuild/command_bad_arg_type.rs");
    t.compile_fail("tests/trybuild/command_unknown_attr_rejected.rs");
}

#[rustversion::attr(before(1.96), ignore)]
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
    t.pass("tests/trybuild/reply_ty_annotation_borrows_temporary_ok.rs");
    t.compile_fail("tests/trybuild/reply_bad_field_type.rs");
    t.compile_fail("tests/trybuild/reply_call_site_outside_handler.rs");
    t.compile_fail("tests/trybuild/reply_from_no_sender.rs");
    t.compile_fail("tests/trybuild/reply_with_type_generic_err.rs");
    t.compile_fail("tests/trybuild/reply_call_site_ty_mismatch.rs");
    t.compile_fail("tests/trybuild/reply_from_ty_mismatch.rs");
    t.compile_fail("tests/trybuild/reply_from_ty_expr_mismatch.rs");
}

#[rustversion::attr(before(1.96), ignore)]
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
    t.compile_fail("tests/trybuild/output_call_site_ty_mismatch.rs");
    t.compile_fail("tests/trybuild/output_from_ty_mismatch.rs");
}

#[rustversion::attr(before(1.96), ignore)]
#[test]
fn trybuild_constant() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/const_int_ok.rs");
    t.pass("tests/trybuild/const_str_ok.rs");
    t.pass("tests/trybuild/constant_in_submodule_ok.rs");
    t.pass("tests/trybuild/constant_screaming_snake_preserved_ok.rs");
    t.compile_fail("tests/trybuild/const_bad_type.rs");
}

#[rustversion::attr(before(1.96), ignore)]
#[test]
fn trybuild_enumeration() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/enum_ok.rs");
    t.pass("tests/trybuild/enum_range_ok.rs");
    t.pass("tests/trybuild/enumeration_in_submodule_ok.rs");
    t.compile_fail("tests/trybuild/enum_bad_attr_rejected.rs");
}

#[rustversion::attr(before(1.96), ignore)]
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
}

#[rustversion::attr(before(1.96), ignore)]
#[test]
fn trybuild_config() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/shutdown_from_call_site_ok.rs");
    t.compile_fail("tests/trybuild/config_bare_ident_rejected.rs");
    t.compile_fail("tests/trybuild/static_string_unlisted_literal.rs");
    t.compile_fail("tests/trybuild/shutdown_unlisted_literal.rs");
    t.compile_fail("tests/trybuild/shutdown_from_no_sender.rs");
    t.compile_fail("tests/trybuild/config_app_not_str_rejected.rs");
}

#[rustversion::attr(before(1.96), ignore)]
#[test]
fn trybuild_frame_budget() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/reply_frame_budget_exact_ok.rs");
    t.compile_fail("tests/trybuild/reply_frame_budget_over_by_one.rs");
    t.compile_fail("tests/trybuild/reply_frame_oversize.rs");
    t.compile_fail("tests/trybuild/output_frame_oversize.rs");
}
