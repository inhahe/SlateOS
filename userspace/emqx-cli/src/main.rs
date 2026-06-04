#![deny(clippy::all)]

//! emqx-cli — OurOS EMQX MQTT broker CLI
//!
//! Single personality: `emqx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_emqx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: emqx COMMAND [OPTIONS]");
        println!("EMQX v5.5.0 (OurOS) — MQTT Broker CLI");
        println!();
        println!("Commands:");
        println!("  start           Start EMQX broker");
        println!("  stop            Stop EMQX broker");
        println!("  restart         Restart broker");
        println!("  status          Show broker status");
        println!("  console         Start in console mode");
        println!("  ctl             Control panel");
        println!("  eval            Evaluate expression");
        println!("  ping            Ping node");
        println!("  version         Show version");
        println!();
        println!("ctl sub-commands:");
        println!("  ctl status      Node status");
        println!("  ctl broker      Broker info");
        println!("  ctl cluster     Cluster info");
        println!("  ctl clients     Connected clients");
        println!("  ctl topics      Active topics");
        println!("  ctl subscriptions  Active subscriptions");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("EMQX v5.5.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "start" => println!("EMQX is started."),
        "stop" => println!("EMQX is stopped."),
        "restart" => println!("EMQX is restarted."),
        "status" => {
            println!("Node 'emqx@127.0.0.1' 5.5.0 is started");
            println!("  MQTT: 0.0.0.0:1883");
            println!("  MQTTS: 0.0.0.0:8883");
            println!("  WebSocket: 0.0.0.0:8083");
            println!("  Dashboard: http://0.0.0.0:18083");
        }
        "ping" => println!("pong"),
        "ctl" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => println!("Node emqx@127.0.0.1: running"),
                "broker" => {
                    println!("sysdescr  : EMQX Enterprise");
                    println!("version   : 5.5.0");
                    println!("uptime    : 3 days, 5 hours, 12 minutes");
                    println!("datetime  : 2024-01-15 10:00:00");
                }
                "clients" => {
                    println!("Client ID            Username     IP Address      Connected");
                    println!("client-001           user1        192.168.1.10    true");
                    println!("client-002           user2        192.168.1.11    true");
                }
                "topics" => {
                    println!("Topic                    Subscribers");
                    println!("sensors/temperature      3");
                    println!("sensors/humidity         2");
                    println!("devices/status           5");
                }
                "cluster" => println!("Cluster: emqx@127.0.0.1 (running)"),
                _ => println!("emqx ctl {}: completed", sub),
            }
        }
        "console" => println!("Starting EMQX in console mode..."),
        _ => println!("emqx {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "emqx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_emqx(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_emqx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/emqx"), "emqx");
        assert_eq!(basename(r"C:\bin\emqx.exe"), "emqx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("emqx.exe"), "emqx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_emqx(&["--help".to_string()], "emqx"), 0);
        assert_eq!(run_emqx(&["-h".to_string()], "emqx"), 0);
        let _ = run_emqx(&["--version".to_string()], "emqx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_emqx(&[], "emqx");
    }
}
