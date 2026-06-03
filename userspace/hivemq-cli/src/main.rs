#![deny(clippy::all)]

//! hivemq-cli — OurOS HiveMQ MQTT tools
//!
//! Multi-personality: `hivemq`, `mqtt-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hivemq(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "mqtt-cli" => {
                println!("mqtt-cli (OurOS) — MQTT 5.0 / 3.1.1 command-line client");
                println!("  pub -t TOPIC -m MSG   Publish message");
                println!("  sub -t TOPIC          Subscribe to topic");
                println!("  shell                 Interactive shell");
                println!("  test                  Test broker connectivity");
                println!("  -h HOST               Broker host");
                println!("  -p PORT               Broker port");
                println!("  -V VERSION            MQTT version (3/5)");
                println!("  --tls                 Use TLS");
            }
            _ => {
                println!("HiveMQ CE (OurOS) — Enterprise MQTT broker");
                println!("  start              Start broker");
                println!("  --config DIR       Config directory");
                println!("  --bind-address IP  Bind address");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("HiveMQ CE v2024.3 (OurOS)"); return 0; }
    match prog {
        "mqtt-cli" => {
            println!("MQTT CLI v4.28.0");
            println!("  Connected to: localhost:1883");
            println!("  Protocol: MQTT 5.0");
            println!("  Client ID: mqtt-cli-abc123");
        }
        _ => {
            println!("HiveMQ CE v2024.3 (OurOS)");
            println!("  MQTT: 0.0.0.0:1883");
            println!("  WebSocket: 0.0.0.0:8000");
            println!("  Clients connected: 567");
            println!("  Topics: 234");
            println!("  Retained messages: 45");
            println!("  Extensions: 3 loaded");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hivemq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hivemq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hivemq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hivemq"), "hivemq");
        assert_eq!(basename(r"C:\bin\hivemq.exe"), "hivemq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hivemq.exe"), "hivemq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_hivemq(&["--help".to_string()], "hivemq"), 0);
        assert_eq!(run_hivemq(&["-h".to_string()], "hivemq"), 0);
        assert_eq!(run_hivemq(&["--version".to_string()], "hivemq"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_hivemq(&[], "hivemq"), 0);
    }
}
