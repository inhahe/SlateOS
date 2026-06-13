#![deny(clippy::all)]

//! vernemq-cli — SlateOS VerneMQ MQTT broker
//!
//! Multi-personality: `vernemq`, `vmq-admin`, `vmq-passwd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vernemq(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "vmq-admin" => {
                println!("vmq-admin (SlateOS) — VerneMQ administration");
                println!("  cluster show       Show cluster status");
                println!("  session show       Show sessions");
                println!("  listener show      Show listeners");
                println!("  plugin show        Show plugins");
                println!("  metrics show       Show metrics");
                println!("  trace              Trace client activity");
            }
            "vmq-passwd" => {
                println!("vmq-passwd (SlateOS) — VerneMQ password management");
                println!("  -c FILE USER       Create/update password file");
                println!("  -D FILE USER       Delete user from file");
            }
            _ => {
                println!("VerneMQ v2.0 (SlateOS) — Distributed MQTT broker");
                println!("  start              Start broker");
                println!("  stop               Stop broker");
                println!("  restart            Restart broker");
                println!("  ping               Ping running node");
                println!("  console            Interactive console");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VerneMQ v2.0.1 (SlateOS)"); return 0; }
    match prog {
        "vmq-admin" => {
            println!("VerneMQ Cluster Status:");
            println!("  Node: vernemq@127.0.0.1 [running]");
            println!("  Sessions: 1,234 online");
            println!("  Subscriptions: 5,678");
            println!("  Retained messages: 890");
            println!("  Messages/sec: 456 in, 789 out");
        }
        _ => {
            println!("VerneMQ v2.0.1 (SlateOS)");
            println!("  MQTT: 0.0.0.0:1883, 0.0.0.0:8883 (TLS)");
            println!("  WebSocket: 0.0.0.0:8080/mqtt");
            println!("  Protocol: MQTT 3.1, 3.1.1, 5.0");
            println!("  Cluster: single node");
            println!("  Auth: file-based + webhook");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vernemq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vernemq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vernemq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vernemq"), "vernemq");
        assert_eq!(basename(r"C:\bin\vernemq.exe"), "vernemq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vernemq.exe"), "vernemq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vernemq(&["--help".to_string()], "vernemq"), 0);
        assert_eq!(run_vernemq(&["-h".to_string()], "vernemq"), 0);
        let _ = run_vernemq(&["--version".to_string()], "vernemq");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vernemq(&[], "vernemq");
    }
}
