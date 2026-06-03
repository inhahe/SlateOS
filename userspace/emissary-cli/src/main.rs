#![deny(clippy::all)]

//! emissary-cli — OurOS Emissary-ingress API gateway
//!
//! Single personality: `emissary`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_emissary(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: emissary [COMMAND] [OPTIONS]");
        println!("Emissary-ingress v3.9 (OurOS) — Kubernetes-native API gateway");
        println!();
        println!("Commands:");
        println!("  serve              Start Emissary");
        println!("  diagnostics        Run diagnostics");
        println!("  intercept          Traffic intercept");
        println!("  check              Health check");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  --config-dir DIR   Config directory");
        println!("  --debug            Debug mode");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Emissary-ingress v3.9.1 (OurOS)"); return 0; }
    println!("Emissary-ingress v3.9.1 (OurOS)");
    println!("  Envoy: v1.28 (sidecar)");
    println!("  Mappings: 34");
    println!("  Hosts: 8");
    println!("  TLS contexts: 5");
    println!("  Rate limit services: 2");
    println!("  Auth services: 1 (ext_authz)");
    println!("  Listeners: 80, 443");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "emissary".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_emissary(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_emissary};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/emissary"), "emissary");
        assert_eq!(basename(r"C:\bin\emissary.exe"), "emissary.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("emissary.exe"), "emissary");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_emissary(&["--help".to_string()], "emissary"), 0);
        assert_eq!(run_emissary(&["-h".to_string()], "emissary"), 0);
        assert_eq!(run_emissary(&["--version".to_string()], "emissary"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_emissary(&[], "emissary"), 0);
    }
}
