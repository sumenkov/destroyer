// tests/errors.rs
#[path = "../src/dev.rs"]
mod dev;

use std::process::{Command, Output};

//
// -------- CLI argument error cases (process::exit) --------
// These are run as subprocesses so that exit(1) doesn't kill the test runner.
//

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_destroyer"))
        .args(args)
        .output()
        .expect("binary build")
}

#[test]
fn cli_non_numeric_passes_fails() {
    let out = run(&["/dev/null", "notanumber"]);
    assert!(
        !out.status.success(),
        "expected failure, got {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Число проходов") || stderr.contains("error"),
        "stderr: {stderr}"
    );
}

#[test]
fn cli_zero_passes_fails() {
    let out = run(&["/dev/null", "0"]);
    assert!(
        !out.status.success(),
        "expected failure, got {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(">=") || stderr.contains("0"),
        "stderr: {stderr}"
    );
}

#[test]
fn cli_invalid_mode_fails() {
    let out = run(&["/dev/null", "1", "--mode", "wat"]);
    assert!(
        !out.status.success(),
        "expected failure, got {:?}",
        out.status
    );
}

#[test]
fn cli_negative_buf_fails() {
    let out = run(&["/dev/null", "1", "--buf", "-1"]);
    assert!(
        !out.status.success(),
        "expected failure, got {:?}",
        out.status
    );
}

//
// -------- dev helpers error cases --------
//

// #[test]
// fn alloc_aligned_with_zero_alignment_returns_err() {
//     let align = 0usize;
//     let len = 1024usize;
//     let res = dev::alloc_aligned(len, align);
//     assert!(res.is_err(), "align должен быть степенью двойки");
// }
