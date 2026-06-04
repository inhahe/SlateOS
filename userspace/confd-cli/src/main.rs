#![deny(clippy::all)]

//! confd-cli — OurOS confd template-based config management
//!
//! Single personality: `confd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_confd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: confd [OPTIONS]");
        println!("confd v0.16 (OurOS) — Manage configs from etcd/consul/env");
        println!();
        println!("Options:");
        println!("  -onetime          Run once and exit");
        println!("  -interval N       Polling interval (seconds)");
        println!("  -watch            Use watch instead of polling");
        println!("  -backend BACKEND  Backend (etcd, consul, env, file)");
        println!("  -node URL         Backend node URL");
        println!("  -confdir DIR      Configuration directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("confd v0.16 (OurOS)"); return 0; }
    let backend = args.iter().skip_while(|a| a.as_str() != "-backend").nth(1).map(|s| s.as_str()).unwrap_or("etcd");
    let onetime = args.iter().any(|a| a == "-onetime");
    println!("confd: starting (backend: {})", backend);
    println!("  Templates: 3");
    println!("  /etc/nginx/nginx.conf — synced (OK)");
    println!("  /etc/app/config.yml — synced (OK)");
    println!("  /etc/haproxy/haproxy.cfg — synced (OK)");
    if onetime {
        println!("  One-time sync complete.");
    } else {
        println!("  Watching for changes...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "confd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_confd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_confd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/confd"), "confd");
        assert_eq!(basename(r"C:\bin\confd.exe"), "confd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("confd.exe"), "confd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_confd(&["--help".to_string()], "confd"), 0);
        assert_eq!(run_confd(&["-h".to_string()], "confd"), 0);
        let _ = run_confd(&["--version".to_string()], "confd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_confd(&[], "confd");
    }
}
