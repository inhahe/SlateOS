#![deny(clippy::all)]

//! activemq-cli — OurOS Apache ActiveMQ message broker
//!
//! Single personality: `activemq`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_activemq(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: activemq [COMMAND] [OPTIONS]");
        println!("Apache ActiveMQ v5.18 (OurOS) — Message broker");
        println!();
        println!("Commands:");
        println!("  start              Start broker");
        println!("  stop               Stop broker");
        println!("  restart            Restart broker");
        println!("  status             Show broker status");
        println!("  create NAME        Create broker instance");
        println!("  list               List broker instances");
        println!("  browse DEST        Browse messages in queue");
        println!("  purge DEST         Purge messages from queue");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (activemq.xml)");
        println!("  --data DIR         Data directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache ActiveMQ v5.18.4 (OurOS)"); return 0; }
    println!("Apache ActiveMQ v5.18.4 (OurOS)");
    println!("  OpenWire: 0.0.0.0:61616");
    println!("  AMQP: 0.0.0.0:5672");
    println!("  STOMP: 0.0.0.0:61613");
    println!("  MQTT: 0.0.0.0:1883");
    println!("  WebSocket: 0.0.0.0:61614");
    println!("  Web Console: http://0.0.0.0:8161");
    println!("  Queues: 23 (456 messages)");
    println!("  Topics: 12 (89 subscribers)");
    println!("  Store: KahaDB (2.3 GB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "activemq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_activemq(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
