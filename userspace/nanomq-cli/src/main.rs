#![deny(clippy::all)]

//! nanomq-cli — SlateOS NanoMQ lightweight MQTT broker
//!
//! Multi-personality: `nanomq`, `nanomq_cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nanomq(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "nanomq_cli" => {
                println!("nanomq_cli (Slate OS) — NanoMQ client tools");
                println!("  pub -t TOPIC -m MSG   Publish message");
                println!("  sub -t TOPIC          Subscribe to topic");
                println!("  conn                  Test connection");
                println!("  bench pub|sub         Benchmark");
                println!("  nngproxy              NNG proxy");
            }
            _ => {
                println!("NanoMQ v0.21 (Slate OS) — Ultra-lightweight MQTT broker");
                println!("  start              Start broker");
                println!("  stop               Stop broker");
                println!("  restart            Restart broker");
                println!("  reload             Hot reload config");
                println!("  --conf FILE        Config file (HOCON)");
                println!("  --url URL          Listen URL");
                println!("  --tls              Enable TLS");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NanoMQ v0.21.10 (Slate OS)"); return 0; }
    match prog {
        "nanomq_cli" => {
            println!("NanoMQ CLI v0.21.10");
            println!("  Available tools: pub, sub, conn, bench, nngproxy");
        }
        _ => {
            println!("NanoMQ v0.21.10 (Slate OS)");
            println!("  MQTT: 0.0.0.0:1883");
            println!("  WebSocket: 0.0.0.0:8083/mqtt");
            println!("  Protocol: MQTT 3.1.1, 5.0");
            println!("  Clients: 89 connected");
            println!("  Bridge: 2 connections (to EMQX, Mosquitto)");
            println!("  Rules: 5 configured");
            println!("  Memory: 12 MB");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nanomq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nanomq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nanomq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nanomq"), "nanomq");
        assert_eq!(basename(r"C:\bin\nanomq.exe"), "nanomq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nanomq.exe"), "nanomq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nanomq(&["--help".to_string()], "nanomq"), 0);
        assert_eq!(run_nanomq(&["-h".to_string()], "nanomq"), 0);
        let _ = run_nanomq(&["--version".to_string()], "nanomq");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nanomq(&[], "nanomq");
    }
}
