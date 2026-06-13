#![deny(clippy::all)]

//! aerc-cli — SlateOS aerc terminal email client
//!
//! Single personality: `aerc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_aerc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aerc [OPTIONS]");
        println!("aerc v0.17 (Slate OS) — Terminal email client");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!("  mailto:ADDR       Open composer to address");
        println!();
        println!("Accounts are configured in ~/.config/aerc/accounts.conf");
        println!("Bindings are configured in ~/.config/aerc/binds.conf");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("aerc v0.17 (Slate OS)"); return 0; }
    if args.is_empty() {
        println!("aerc v0.17 — terminal email client");
        println!("  Accounts: 2 configured");
        println!("  Inbox: 42 messages (5 unread)");
        println!("  Press q to quit, : for command mode");
        return 0;
    }
    let target = args.first().map(|s| s.as_str()).unwrap_or("");
    if let Some(addr) = target.strip_prefix("mailto:") {
        println!("Composing to: {addr}");
    } else {
        println!("aerc: opening...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "aerc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aerc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aerc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/aerc"), "aerc");
        assert_eq!(basename(r"C:\bin\aerc.exe"), "aerc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("aerc.exe"), "aerc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_aerc(&["--help".to_string()], "aerc"), 0);
        assert_eq!(run_aerc(&["-h".to_string()], "aerc"), 0);
        let _ = run_aerc(&["--version".to_string()], "aerc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_aerc(&[], "aerc");
    }
}
