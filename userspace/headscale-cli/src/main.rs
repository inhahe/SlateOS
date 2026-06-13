#![deny(clippy::all)]

//! headscale-cli — Slate OS Headscale coordination server
//!
//! Single personality: `headscale`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_headscale(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: headscale [COMMAND] [OPTIONS]");
        println!("Headscale v0.23 (Slate OS) — Self-hosted Tailscale coordination server");
        println!();
        println!("Commands:");
        println!("  serve              Start server");
        println!("  users list|create|delete  Manage users");
        println!("  nodes list|register|delete|expire  Manage nodes");
        println!("  preauthkeys list|create|expire  Auth keys");
        println!("  routes list|enable|disable  Manage routes");
        println!("  apikeys list|create|expire  API keys");
        println!("  policy get|set     ACL policy");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (YAML)");
        println!("  --force            Force operation");
        println!("  --output json|yaml Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Headscale v0.23.0 (Slate OS)"); return 0; }
    println!("Headscale v0.23.0 (Slate OS)");
    println!("  Users: 5");
    println!("  Nodes: 23 (18 online)");
    println!("  Pre-auth keys: 3 active");
    println!("  Routes: 7 advertised, 5 enabled");
    println!("  DNS: MagicDNS enabled");
    println!("  DERP: 3 relay servers");
    println!("  gRPC: 0.0.0.0:50443");
    println!("  HTTP: 0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "headscale".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_headscale(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_headscale};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/headscale"), "headscale");
        assert_eq!(basename(r"C:\bin\headscale.exe"), "headscale.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("headscale.exe"), "headscale");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_headscale(&["--help".to_string()], "headscale"), 0);
        assert_eq!(run_headscale(&["-h".to_string()], "headscale"), 0);
        let _ = run_headscale(&["--version".to_string()], "headscale");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_headscale(&[], "headscale");
    }
}
