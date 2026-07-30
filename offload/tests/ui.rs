#[test]
fn macro_diagnostics_and_valid_shapes() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui/pass.rs");
    tests.compile_fail("tests/ui/fail_*.rs");
}

#[cfg(feature = "an-mode")]
#[test]
fn crate_wide_an_mode_rejects_float_boundaries() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui-an-mode/*.rs");
}
