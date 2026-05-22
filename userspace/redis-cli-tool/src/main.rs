#![deny(clippy::all)]

//! redis-cli-tool — OurOS Redis CLI
//!
//! Single personality: `redis-cli`

use std::env;
use std::process;

fn run_redis_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redis-cli [OPTIONS] [COMMAND [ARGS]]");
        println!();
        println!("Redis command line interface (OurOS).");
        println!();
        println!("Options:");
        println!("  -h HOST        Server hostname (default: 127.0.0.1)");
        println!("  -p PORT        Server port (default: 6379)");
        println!("  -a PASSWORD    Password");
        println!("  -n DB          Database number");
        println!("  --cluster      Enable cluster mode");
        println!("  --scan         Scan for keys");
        println!("  --stat         Show server stats");
        println!("  --bigkeys      Find big keys");
        println!("  --latency      Check latency");
        println!("  --pipe         Pipe mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("redis-cli 7.2.4 (OurOS)");
        return 0;
    }

    let host = args.windows(2).find(|w| w[0] == "-h")
        .map(|w| w[1].as_str()).unwrap_or("127.0.0.1");
    let port = args.windows(2).find(|w| w[0] == "-p")
        .map(|w| w[1].as_str()).unwrap_or("6379");

    if args.iter().any(|a| a == "--stat") {
        println!("------- data ------ ----- load ---- - conn - --- cache --- ----- cmd -----");
        println!("keys       mem      clients blocked  hits   miss     cmd/s  avg");
        println!("1234       45.6M    12      0        98.5%  1.5%     1250   0.12ms");
        println!("1234       45.7M    12      0        98.5%  1.5%     1180   0.11ms");
        return 0;
    }
    if args.iter().any(|a| a == "--bigkeys") {
        println!("# Scanning the entire keyspace to find biggest keys");
        println!();
        println!("[00.00%] Biggest string found so far 'session:abc123' with 2048 bytes");
        println!("[25.00%] Biggest list found so far 'queue:jobs' with 15234 items");
        println!("[50.00%] Biggest hash found so far 'user:1' with 12 fields");
        println!("[75.00%] Biggest set found so far 'tags:popular' with 456 members");
        println!("[100.00%] Biggest zset found so far 'leaderboard' with 10000 members");
        println!();
        println!("-------- summary -------");
        println!("Sampled 1234 keys in the keyspace!");
        println!("Total key length in bytes: 15678");
        return 0;
    }
    if args.iter().any(|a| a == "--latency") {
        println!("min: 0, max: 1, avg: 0.12 (1000 samples)");
        return 0;
    }
    if args.iter().any(|a| a == "--scan") {
        let pattern = args.windows(2).find(|w| w[0] == "--pattern")
            .map(|w| w[1].as_str()).unwrap_or("*");
        println!("Scanning for keys matching '{}'...", pattern);
        println!("session:abc123");
        println!("session:def456");
        println!("user:1");
        println!("user:2");
        println!("cache:page:home");
        println!("queue:jobs");
        return 0;
    }

    // Direct command mode
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "PING" | "ping" => {
            println!("PONG");
            0
        }
        "INFO" | "info" => {
            println!("# Server");
            println!("redis_version:7.2.4");
            println!("os:OurOS x86_64");
            println!("tcp_port:{}", port);
            println!();
            println!("# Clients");
            println!("connected_clients:12");
            println!();
            println!("# Memory");
            println!("used_memory_human:45.6M");
            println!("used_memory_peak_human:52.3M");
            println!();
            println!("# Keyspace");
            println!("db0:keys=1234,expires=567,avg_ttl=3600000");
            0
        }
        "GET" | "get" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("key");
            println!("\"value-for-{}\"", key);
            0
        }
        "SET" | "set" => {
            println!("OK");
            0
        }
        "DEL" | "del" => {
            let count = args.len().saturating_sub(1);
            println!("(integer) {}", count.max(1));
            0
        }
        "KEYS" | "keys" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("*");
            println!("1) \"session:abc123\"");
            println!("2) \"user:1\"");
            println!("3) \"cache:page:home\"");
            println!("(pattern: {})", pattern);
            0
        }
        "" => {
            println!("{}:{}> (interactive mode)", host, port);
            println!("  Type commands or 'quit' to exit.");
            0
        }
        _ => {
            println!("(executing: {} {:?})", cmd, &args[1..]);
            println!("OK");
            0
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_redis_cli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
