#[test]
fn trybuild_command() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/command_view_ok.rs");
    t.pass("tests/trybuild/command_concrete_ok.rs");
    t.pass("tests/trybuild/command_with_args_ok.rs");
    t.compile_fail("tests/trybuild/command_bad_context.rs");
    t.compile_fail("tests/trybuild/command_no_ctx_arg.rs");
    t.compile_fail("tests/trybuild/command_shared_ref_rejected.rs");
    t.compile_fail("tests/trybuild/command_owned_ctx_rejected.rs");
    t.compile_fail("tests/trybuild/command_bad_arg_type.rs");
}
