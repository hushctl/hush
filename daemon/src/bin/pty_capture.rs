//! Standalone byte-capture harness for debugging terminal emulator behavior.
//!
//! Spawns a child command in a fresh pty, forwards this terminal's stdin into
//! that pty, and logs every inbound byte (hex + timestamp) to a file. Output
//! from the child streams to stdout unchanged so the user can interact with
//! the program normally.
//!
//! Intended use: run `cargo run --bin pty-capture -- claude mcp add kinobi ...`
//! from a native terminal (iTerm / Terminal.app), trigger the flow that
//! misbehaves in Hush, and diff the captured input bytes against the bytes
//! Hush sends for the same keystrokes. Any divergence is the bug.
//!
//! Raw-mode handling is done by shelling out to `stty`, which keeps this
//! file free of extra dependencies (no termios/nix crates needed).

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::process::{Command as StdCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: pty-capture <command> [args...]");
        eprintln!("       CAPTURE_LOG=/tmp/native.log pty-capture claude");
        std::process::exit(2);
    }

    let log_path = std::env::var("CAPTURE_LOG")
        .unwrap_or_else(|_| "/tmp/pty_capture.log".to_string());
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("open log file");
    let log = Arc::new(Mutex::new(log_file));
    {
        let mut l = log.lock().unwrap();
        let _ = writeln!(l, "--- {} capture start, child={:?} ---", ts_ms(), args);
        let _ = l.flush();
    }
    eprintln!("[pty-capture] logging input bytes to {log_path}");
    eprintln!("[pty-capture] run your flow, then press Ctrl+D or exit the child to finish");

    // Save original termios so we can restore on exit.
    let orig_stty = StdCommand::new("stty")
        .arg("-g")
        .stdin(Stdio::inherit())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Raw-ish: pass through every keystroke byte to us unmodified.
    let _ = StdCommand::new("stty")
        .arg("raw")
        .arg("-echo")
        .stdin(Stdio::inherit())
        .status();

    let (rows, cols) = tty_size();
    let pty = NativePtySystem::default();
    let pair = pty
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(&args[0]);
    for a in &args[1..] {
        cmd.arg(a);
    }
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let mut child = pair.slave.spawn_command(cmd).expect("spawn child");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let mut writer = pair.master.take_writer().expect("take writer");

    // stdin → pty, logging each chunk.
    let log_in = log.clone();
    let stdin_thread = std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 256];
        loop {
            let n = match stdin.read(&mut buf) {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            let slice = &buf[..n];
            let hex: String = slice.iter().map(|b| format!("{:02x}", b)).collect();
            if let Ok(mut l) = log_in.lock() {
                let _ = writeln!(l, "{} IN  hex={} bytes={:?}", ts_ms(), hex, slice);
                let _ = l.flush();
            }
            if writer.write_all(slice).is_err() {
                break;
            }
        }
    });

    // pty → stdout, unlogged (stdout noise would make the log unreadable).
    let mut stdout = std::io::stdout();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let _ = stdout.write_all(&buf[..n]);
                let _ = stdout.flush();
            }
        }
    }

    let _ = child.wait();
    let _ = stdin_thread.join();

    if !orig_stty.is_empty() {
        let _ = StdCommand::new("stty")
            .arg(&orig_stty)
            .stdin(Stdio::inherit())
            .status();
    }

    {
        let mut l = log.lock().unwrap();
        let _ = writeln!(l, "--- {} capture end ---", ts_ms());
        let _ = l.flush();
    }
}

fn tty_size() -> (u16, u16) {
    let out = StdCommand::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .output();
    if let Ok(o) = out {
        if let Ok(s) = String::from_utf8(o.stdout) {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if parts.len() == 2 {
                let rows: u16 = parts[0].parse().unwrap_or(24);
                let cols: u16 = parts[1].parse().unwrap_or(80);
                return (rows, cols);
            }
        }
    }
    (24, 80)
}

fn ts_ms() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}
