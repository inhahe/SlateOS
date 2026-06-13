#![deny(clippy::all)]

//! rustdesk-cli — Slate OS RustDesk remote desktop
//!
//! Single personality: `rustdesk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rustdesk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rustdesk [OPTIONS]");
        println!("rustdesk v1.2 (Slate OS) — Open-source remote desktop");
        println!();
        println!("Options:");
        println!("  --id              Show your ID");
        println!("  --connect ID      Connect to remote ID");
        println!("  --server          Run as relay server");
        println!("  --version         Show version");
        println!();
        println!("Self-hosted alternative to TeamViewer/AnyDesk.");
        println!("End-to-end encryption, no account required.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rustdesk v1.2 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--id") {
        println!("Your ID: 847293651");
        return 0;
    }
    println!("rustdesk: remote desktop started");
    println!("  Your ID: 847293651");
    println!("  Status: ready");
    println!("  Encryption: end-to-end");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rustdesk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rustdesk(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rustdesk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rustdesk"), "rustdesk");
        assert_eq!(basename(r"C:\bin\rustdesk.exe"), "rustdesk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rustdesk.exe"), "rustdesk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rustdesk(&["--help".to_string()], "rustdesk"), 0);
        assert_eq!(run_rustdesk(&["-h".to_string()], "rustdesk"), 0);
        let _ = run_rustdesk(&["--version".to_string()], "rustdesk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rustdesk(&[], "rustdesk");
    }
}
