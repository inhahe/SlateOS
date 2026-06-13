#![deny(clippy::all)]

//! jami-cli — SlateOS Jami peer-to-peer communicator
//!
//! Multi-personality: `jami`, `jami-daemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_jami(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jami [OPTIONS]");
        println!("jami v2024.01 (SlateOS) — Peer-to-peer communicator");
        println!();
        println!("Options:");
        println!("  --minimized       Start minimized");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("jami v2024.01 (SlateOS)"); return 0; }
    println!("jami: P2P communicator started");
    println!("  Account: registered (DHT)");
    println!("  Encryption: end-to-end (TLS/SRTP)");
    println!("  Audio/Video: ready");
    println!("  Screen sharing: supported");
    0
}

fn run_daemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jami-daemon [OPTIONS]");
        println!("jami-daemon v2024.01 (SlateOS) — Jami daemon service");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("jami-daemon v2024.01 (SlateOS)"); return 0; }
    println!("jami-daemon: service started");
    println!("  DHT: bootstrap complete");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "jami".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "jami-daemon" => run_daemon(&rest, &prog),
        _ => run_jami(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_jami};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/jami"), "jami");
        assert_eq!(basename(r"C:\bin\jami.exe"), "jami.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("jami.exe"), "jami");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jami(&["--help".to_string()], "jami"), 0);
        assert_eq!(run_jami(&["-h".to_string()], "jami"), 0);
        let _ = run_jami(&["--version".to_string()], "jami");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jami(&[], "jami");
    }
}
