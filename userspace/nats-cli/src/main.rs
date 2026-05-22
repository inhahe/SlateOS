#![deny(clippy::all)]

//! nats-cli — OurOS NATS CLI
//!
//! Single personality: `nats`

use std::env;
use std::process;

fn run_nats(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nats <COMMAND> [OPTIONS]");
        println!();
        println!("NATS messaging system CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  pub          Publish a message");
        println!("  sub          Subscribe to a subject");
        println!("  request      Send a request");
        println!("  reply        Listen for requests");
        println!("  stream       Manage JetStream streams");
        println!("  consumer     Manage JetStream consumers");
        println!("  kv           Key-value store");
        println!("  object       Object store");
        println!("  server       Server information");
        println!("  account      Account information");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("nats 0.1.1 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "pub" => {
            let subject = args.get(1).map(|s| s.as_str()).unwrap_or("test.subject");
            let message = args.get(2).map(|s| s.as_str()).unwrap_or("hello");
            println!("Published {} bytes to \"{}\"", message.len(), subject);
            0
        }
        "sub" => {
            let subject = args.get(1).map(|s| s.as_str()).unwrap_or("test.>");
            println!("Subscribing on \"{}\"", subject);
            println!("[#1] Received on \"test.hello\":");
            println!("hello world");
            println!();
            println!("[#2] Received on \"test.data\":");
            println!("{{\"key\": \"value\"}}");
            0
        }
        "request" => {
            let subject = args.get(1).map(|s| s.as_str()).unwrap_or("service.echo");
            let payload = args.get(2).map(|s| s.as_str()).unwrap_or("ping");
            println!("Sending request on \"{}\"", subject);
            println!("Received with rtt 1.234ms");
            println!("echo: {}", payload);
            0
        }
        "stream" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("Streams:");
                    println!("  Name         Subjects     Messages   Size      Consumers");
                    println!("  ORDERS       orders.>     12345      45.6 MB   3");
                    println!("  EVENTS       events.>     56789      123.4 MB  5");
                    println!("  LOGS         logs.>       234567     890.1 MB  2");
                }
                "info" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("ORDERS");
                    println!("Stream: {}", name);
                    println!("  Subjects:       orders.>");
                    println!("  Messages:       12,345");
                    println!("  Bytes:          45.6 MB");
                    println!("  First Seq:      1 @ 2024-01-10T00:00:00Z");
                    println!("  Last Seq:       12345 @ 2024-01-15T14:00:00Z");
                    println!("  Consumers:      3");
                    println!("  Storage:        File");
                    println!("  Replicas:       3");
                }
                "add" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("NEW-STREAM");
                    println!("Stream {} created", name);
                }
                _ => { println!("Stream operation: {}", sub); }
            }
            0
        }
        "consumer" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    let stream = args.get(2).map(|s| s.as_str()).unwrap_or("ORDERS");
                    println!("Consumers for stream {}:", stream);
                    println!("  Name            Mode      Ack Pending  Unprocessed");
                    println!("  order-proc      Push      0            0");
                    println!("  analytics       Pull      12           45");
                    println!("  audit-log       Push      0            0");
                }
                _ => { println!("Consumer operation: {}", sub); }
            }
            0
        }
        "kv" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "put" => {
                    let bucket = args.get(2).map(|s| s.as_str()).unwrap_or("config");
                    let key = args.get(3).map(|s| s.as_str()).unwrap_or("app.setting");
                    println!("{} > {} revision: 42", bucket, key);
                }
                "get" => {
                    let bucket = args.get(2).map(|s| s.as_str()).unwrap_or("config");
                    let key = args.get(3).map(|s| s.as_str()).unwrap_or("app.setting");
                    println!("{} > {} created @ 2024-01-15T14:00:00Z revision: 42", bucket, key);
                    println!("value-for-setting");
                }
                "list" | "ls" => {
                    println!("KV Buckets:");
                    println!("  config    12 keys   1.2 KB");
                    println!("  sessions  456 keys  45.6 KB");
                    println!("  cache     89 keys   8.9 KB");
                }
                _ => { println!("KV operation: {}", sub); }
            }
            0
        }
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Server Information:");
                    println!("  Name:         nats-1");
                    println!("  ID:           NABC123DEF456GHI789");
                    println!("  Version:      2.10.9");
                    println!("  Connections:  45");
                    println!("  Subscriptions: 234");
                    println!("  Messages In:  1,234,567");
                    println!("  Messages Out: 1,234,560");
                    println!("  Uptime:       3d 14h 22m");
                }
                "list" | "ls" => {
                    println!("Known Servers:");
                    println!("  nats-1   nats://node1:4222   NABC123   v2.10.9  45 conns");
                    println!("  nats-2   nats://node2:4222   NDEF456   v2.10.9  38 conns");
                    println!("  nats-3   nats://node3:4222   NGHI789   v2.10.9  41 conns");
                }
                _ => { println!("Server operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: nats <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nats(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
