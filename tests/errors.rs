// tests/errors.rs
#[path = "../src/dev.rs"]
mod dev;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

fn bin() -> Command {
    Command::cargo_bin("destroyer").expect("binary build")
}

//
// -------- CLI argument error cases (process::exit) --------
// These are run as subprocesses so that exit(1) doesn't kill the test runner.
//

#[test]
fn cli_non_numeric_passes_fails() {
    let mut cmd = bin();
    cmd.arg("/dev/null").arg("notanumber");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Число проходов").or(predicate::str::contains("error")));
}

#[test]
fn cli_zero_passes_fails() {
    let mut cmd = bin();
    cmd.arg("/dev/null").arg("0");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains(">=").or(predicate::str::contains("0")));
}

#[test]
fn cli_invalid_mode_fails() {
    let mut cmd = bin();
    cmd.args(["/dev/null", "1", "--mode", "wat"]);
    cmd.assert()
        .failure();
}

#[test]
fn cli_negative_buf_fails() {
    let mut cmd = bin();
    cmd.args(["/dev/null", "1", "--buf", "-1"]);
    cmd.assert()
        .failure();
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
