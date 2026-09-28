//! Appendix B excluded-construct reject suite (stable error codes).

use plc_compiler::{parse_st, ErrorCode};

fn assert_code(src: &str, code: ErrorCode) {
    let err = parse_st(src).expect_err("expected compile/parse error");
    assert_eq!(err.code, code, "source:\n{src}\nmessage: {}", err.message);
}

#[test]
fn reject_var_in_out() {
    assert_code(
        r"
        FUNCTION_BLOCK F
          VAR_IN_OUT x : BOOL; END_VAR
        END_FUNCTION_BLOCK
        ",
        ErrorCode::EExcludedVarInOut,
    );
}

#[test]
fn reject_string() {
    assert_code(
        r"
        PROGRAM P
          VAR x : STRING; END_VAR
        END_PROGRAM
        ",
        ErrorCode::EExcludedString,
    );
}

#[test]
fn reject_repeat() {
    assert_code(
        r"
        PROGRAM P
          REPEAT
            ;
          UNTIL TRUE
          END_REPEAT
        END_PROGRAM
        ",
        ErrorCode::EExcludedLoopCtrl,
    );
}

#[test]
fn reject_exit() {
    assert_code(
        r"
        PROGRAM P
          EXIT;
        END_PROGRAM
        ",
        ErrorCode::EExcludedLoopCtrl,
    );
}

#[test]
fn reject_ref_to() {
    assert_code(
        r"
        PROGRAM P
          VAR p : REF_TO BOOL; END_VAR
        END_PROGRAM
        ",
        ErrorCode::EExcludedPointer,
    );
}

#[test]
fn reject_nested_fb_type() {
    assert_code(
        r"
        FUNCTION_BLOCK Outer
          FUNCTION_BLOCK Inner
          END_FUNCTION_BLOCK
        END_FUNCTION_BLOCK
        ",
        ErrorCode::EExcludedNestedFbType,
    );
}

#[test]
fn reject_configuration() {
    assert_code(
        r"
        CONFIGURATION C
        END_CONFIGURATION
        ",
        ErrorCode::EExcludedConfig,
    );
}

#[test]
fn reject_unbounded_while() {
    use plc_compiler::{compile_project, CompileOptions};
    use std::path::PathBuf;
    // Inline via a tiny temp project is heavy; parse+bind through compile of ST string path:
    // Use parse then ensure WHILE without attr fails at codegen by compiling a temp file.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-reject-while");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/main.st"),
        r"
        PROGRAM Main
          VAR i : BOOL; END_VAR
          WHILE i DO
            i := FALSE;
          END_WHILE;
        END_PROGRAM
        ",
    )
    .unwrap();
    std::fs::write(
        dir.join("project.toml"),
        r#"
id = "t"
version = "0.1.0"
sources = ["src/main.st"]
[[task]]
name = "main"
program = "Main"
"#,
    )
    .unwrap();
    let err = compile_project(&dir.join("project.toml"), &CompileOptions::default()).unwrap_err();
    assert_eq!(err.code, ErrorCode::EUnboundedLoop);
}
