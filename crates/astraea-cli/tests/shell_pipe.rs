//! astraeadb-issues.md #34: `shell` must not panic when stdin is a pipe.
//!
//! `main` is `#[tokio::main]`, and the shell builds its own current-thread
//! runtime for each request. Calling it directly from `main` meant
//! `Runtime::block_on` ran inside an existing runtime, which tokio forbids, so
//! the very first request panicked. That request is the connectivity Ping,
//! which happens before any input is read: the shell was therefore broken
//! interactively as well as piped, despite being reported as pipe-only.
//!
//! This test deliberately points at a closed port. The Ping then fails
//! gracefully and the shell warns and carries on, so the test needs no server
//! while still exercising the exact line that used to panic.

use std::io::Write;
use std::process::{Command, Stdio};

/// A port nothing should be listening on. If something is, the test still
/// passes for the right reason: it only asserts the absence of a panic.
const DEAD_PORT: &str = "127.0.0.1:59999";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_astraeadb"))
}

fn run_with_stdin(input: &str) -> (String, Option<i32>) {
    let mut child = cli()
        .args(["shell", "-a", DEAD_PORT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the cli");
    child
        .stdin
        .as_mut()
        .expect("stdin was not piped")
        .write_all(input.as_bytes())
        .expect("failed to write to the shell");
    let out = child.wait_with_output().expect("the shell never exited");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (combined, out.status.code())
}

#[test]
fn shell_does_not_panic_on_piped_stdin() {
    let (output, code) = run_with_stdin("MATCH (n) RETURN count(n) AS node_count\n");

    assert!(
        !output.contains("panicked"),
        "the shell panicked on piped stdin:\n{output}"
    );
    assert!(
        !output.contains("Cannot start a runtime from within a runtime"),
        "the nested-runtime bug is back:\n{output}"
    );
    assert_ne!(code, None, "the shell was killed by a signal");
}

#[test]
fn shell_does_not_panic_on_empty_pipe() {
    // Reaches the Ping and then EOF, which is the shortest path through the
    // code that used to panic.
    let (output, _) = run_with_stdin("");
    assert!(
        !output.contains("panicked"),
        "the shell panicked on an empty pipe:\n{output}"
    );
}

#[test]
fn shell_reports_an_unreachable_server_instead_of_crashing() {
    let (output, _) = run_with_stdin(".quit\n");
    assert!(
        output.contains("could not reach server"),
        "expected a warning about the unreachable server, got:\n{output}"
    );
}
