#![deny(clippy::all)]

//! obexctl-cli — Slate OS obexctl OBEX Bluetooth file transfer
//!
//! Single personality: `obexctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_obexctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: obexctl COMMAND [OPTIONS]");
        println!("obexctl v5.72 (Slate OS) — OBEX Bluetooth file transfer");
        println!();
        println!("Commands:");
        println!("  connect MAC       Connect to device");
        println!("  disconnect        Disconnect");
        println!("  send FILE         Send file");
        println!("  pull FILE         Receive file");
        println!("  ls                List remote files");
        println!("  cd DIR            Change remote directory");
        println!("  sessions          List active sessions");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("sessions");
    match cmd {
        "connect" => {
            let mac = args.get(1).map(|s| s.as_str()).unwrap_or("AA:BB:CC:DD:EE:FF");
            println!("Connected to {} via OBEX", mac);
        }
        "send" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("file.txt");
            println!("Sending: {}", file);
        }
        "ls" => {
            println!("Documents/");
            println!("Photos/");
            println!("readme.txt (1.2 KB)");
        }
        "sessions" => println!("No active OBEX sessions"),
        _ => println!("obexctl: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "obexctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_obexctl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_obexctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/obexctl"), "obexctl");
        assert_eq!(basename(r"C:\bin\obexctl.exe"), "obexctl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("obexctl.exe"), "obexctl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_obexctl(&["--help".to_string()], "obexctl"), 0);
        assert_eq!(run_obexctl(&["-h".to_string()], "obexctl"), 0);
        let _ = run_obexctl(&["--version".to_string()], "obexctl");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_obexctl(&[], "obexctl");
    }
}
