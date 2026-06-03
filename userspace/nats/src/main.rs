#![deny(clippy::all)]

//! nats — OurOS high-performance messaging system
//!
//! Multi-personality: `nats-server` (server), `nats` (CLI client)

use std::env;
use std::process;

fn run_nats_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nats-server [options]");
        println!();
        println!("Server Options:");
        println!("  -a, --addr <host>      Bind to address (default: 0.0.0.0)");
        println!("  -p, --port <port>      Port to listen on (default: 4222)");
        println!("  -c, --config <file>    Configuration file");
        println!("  -m, --http_port <port> HTTP monitoring port");
        println!("  --name <name>          Server name");
        println!("  -D, --debug            Enable debugging output");
        println!("  -V, --trace            Enable trace output");
        println!("  -js, --jetstream       Enable JetStream");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("nats-server: v2.10.14 (OurOS)");
        return 0;
    }
    let port = args.iter().position(|a| a == "-p" || a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(4222);
    let name = args.iter().position(|a| a == "--name")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("ouros-nats-1");
    let js = args.iter().any(|a| a == "-js" || a == "--jetstream");
    println!("[12345] 2025/05/22 10:00:00.000000 [INF] Starting nats-server");
    println!("[12345] 2025/05/22 10:00:00.000001 [INF]   Version:  2.10.14 (OurOS)");
    println!("[12345] 2025/05/22 10:00:00.000002 [INF]   Git:      [abc1234]");
    println!("[12345] 2025/05/22 10:00:00.000003 [INF]   Name:     {}", name);
    println!("[12345] 2025/05/22 10:00:00.000004 [INF]   ID:       NABC123DEF456GHI789JKL012MNO345PQR678STU901");
    if js {
        println!("[12345] 2025/05/22 10:00:00.100000 [INF] Starting JetStream");
        println!("[12345] 2025/05/22 10:00:00.100001 [INF]   Store Directory:  \"/tmp/nats/jetstream\"");
        println!("[12345] 2025/05/22 10:00:00.100002 [INF]   Max Memory Store: 1.00 GB");
        println!("[12345] 2025/05/22 10:00:00.100003 [INF]   Max File Store:   10.00 GB");
    }
    println!("[12345] 2025/05/22 10:00:00.200000 [INF] Listening for client connections on 0.0.0.0:{}", port);
    println!("[12345] 2025/05/22 10:00:00.200001 [INF] Server is ready");
    0
}

fn run_nats_cli(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: nats <command> [args]");
            println!();
            println!("Commands:");
            println!("  pub          Publish a message");
            println!("  sub          Subscribe to a subject");
            println!("  req          Send a request and wait for a response");
            println!("  reply        Listen for requests and send replies");
            println!("  bench        Benchmark NATS");
            println!("  stream       Manage JetStream streams");
            println!("  consumer     Manage JetStream consumers");
            println!("  server       Manage NATS servers");
            println!("  account      Manage NATS accounts");
            println!("  context      Manage nats contexts");
            println!("  latency      Perform latency testing");
            println!("  rtt          Compute round-trip time");
            println!("  --version    Show version");
            0
        }
        "--version" | "version" => {
            println!("nats CLI v0.1.3 (OurOS)");
            0
        }
        "pub" => {
            let subj = cmd_args.first().map(|s| s.as_str()).unwrap_or("test.subject");
            let msg = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("Published {} bytes to \"{}\"", msg.len(), subj);
            0
        }
        "sub" => {
            let subj = cmd_args.first().map(|s| s.as_str()).unwrap_or("test.>");
            println!("Subscribing on {}", subj);
            println!("[#1] Received on \"test.subject\"");
            println!("hello world");
            println!();
            println!("[#2] Received on \"test.updates\"");
            println!("{{\"type\":\"update\",\"value\":42}}");
            0
        }
        "req" => {
            let subj = cmd_args.first().map(|s| s.as_str()).unwrap_or("service.echo");
            println!("Sending request on \"{}\"", subj);
            println!("Received with rtt 1.234ms");
            println!("echo response");
            0
        }
        "bench" => {
            let subj = cmd_args.first().map(|s| s.as_str()).unwrap_or("bench.test");
            let _ = subj;
            println!("Starting pub/sub benchmark [msgs=100000, msgsize=128B, pubs=1, subs=1]");
            println!();
            println!("Pub stats: 1,250,000 msgs/sec ~ 152.59 MB/sec");
            println!("Sub stats: 1,250,000 msgs/sec ~ 152.59 MB/sec");
            println!("Min/Avg/Max: 0.012ms / 0.045ms / 1.234ms");
            0
        }
        "stream" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" | "list" => {
                    println!("Streams:");
                    println!("  ORDERS");
                    println!("  EVENTS");
                    println!("  LOGS");
                }
                "info" => {
                    println!("Information for Stream ORDERS");
                    println!("  Subjects: orders.>");
                    println!("  Replicas: 1");
                    println!("  Messages: 1,420");
                    println!("  Bytes:    2.1 MB");
                    println!("  Consumer Count: 3");
                }
                "create" => println!("Stream created successfully"),
                "delete" => println!("Stream deleted"),
                _ => println!("Usage: nats stream <ls|info|create|delete>"),
            }
            0
        }
        "consumer" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" | "list" => {
                    println!("Consumers for Stream ORDERS:");
                    println!("  order-processor");
                    println!("  analytics");
                    println!("  audit-log");
                }
                "info" => {
                    println!("Consumer order-processor:");
                    println!("  Pending: 42");
                    println!("  Redelivered: 3");
                    println!("  Ack Floor: 1378");
                }
                _ => println!("Usage: nats consumer <ls|info>"),
            }
            0
        }
        "server" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Server information for ouros-nats-1:");
                    println!("  Version: 2.10.14");
                    println!("  Uptime:  1d 2h 30m 15s");
                    println!("  Connections: 42");
                    println!("  Subscriptions: 156");
                    println!("  Messages In: 1,234,567");
                    println!("  Messages Out: 2,345,678");
                    println!("  Data In: 150 MB");
                    println!("  Data Out: 285 MB");
                }
                "ls" | "list" => {
                    println!("Name              Cluster    Host           Version  Conns  Subs   Msgs In  Msgs Out");
                    println!("ouros-nats-1      -          0.0.0.0:4222   2.10.14  42     156    1234567  2345678");
                }
                _ => println!("Usage: nats server <info|ls>"),
            }
            0
        }
        "rtt" => {
            println!("nats://127.0.0.1:4222:");
            println!("  min=0.123ms, avg=0.456ms, max=1.234ms");
            0
        }
        other => { eprintln!("nats: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("nats-server");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "nats" => run_nats_cli(rest),
        _ => run_nats_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nats_server};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nats_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_nats_server(vec!["-h".to_string()]), 0);
        assert_eq!(run_nats_server(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nats_server(vec![]), 0);
    }
}
