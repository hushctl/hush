//! hush-hook — tiny shim invoked by Claude Code's hook system.
//!
//! Reads the hook's JSON stdin, stamps it with HUSH_WORKTREE_ID and the event
//! name (passed as argv[1]), writes one line of JSON to HUSH_HOOK_SOCKET, exits.
//!
//! Env-var gated: if HUSH_WORKTREE_ID or HUSH_HOOK_SOCKET is missing, the shim is
//! a no-op. This makes it safe even if a worktree's settings.local.json leaks
//! outside daemon-managed sessions — non-daemon claude invocations just don't
//! have the env vars and the shim does nothing.

use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let event = match args.get(1) {
        Some(s) => s.clone(),
        None => return ExitCode::SUCCESS, // no event name → no-op
    };

    let worktree_id = match env::var("HUSH_WORKTREE_ID") {
        Ok(s) => s,
        Err(_) => return ExitCode::SUCCESS, // not running under daemon
    };
    let socket_path = match env::var("HUSH_HOOK_SOCKET") {
        Ok(s) => s,
        Err(_) => return ExitCode::SUCCESS,
    };

    // Read stdin (Claude Code hook payload). It's bounded by Claude Code, so
    // a blocking read to EOF is fine.
    let mut payload = String::new();
    let _ = std::io::stdin().read_to_string(&mut payload);

    // Build the line we send to the daemon. Payload is the raw hook JSON
    // (or empty string if Claude didn't send anything). Wrap as a string
    // field so the daemon can re-parse if needed without breaking on bad
    // JSON.
    let line = format!(
        "{{\"event\":\"{}\",\"worktree_id\":\"{}\",\"payload\":{}}}\n",
        json_escape(&event),
        json_escape(&worktree_id),
        if payload.trim().is_empty() {
            "null".to_string()
        } else {
            payload.trim().to_string()
        }
    );

    let mut stream = match UnixStream::connect(&socket_path) {
        Ok(s) => s,
        Err(_) => return ExitCode::SUCCESS, // daemon not listening; don't break Claude Code
    };

    let _ = stream.write_all(line.as_bytes());
    let _ = stream.flush();
    ExitCode::SUCCESS
}

/// Minimal JSON string escaper for the event name and worktree ID. These
/// fields are tightly controlled (alphanumerics + underscore) so this only
/// needs to handle the safe subset.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
