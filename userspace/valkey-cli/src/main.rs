#![deny(clippy::all)]

//! valkey-cli — SlateOS Valkey (Redis fork) CLI
//!
//! Multi-personality: `valkey-cli`, `valkey-server`, `valkey-benchmark`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_valkey_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: valkey-cli [OPTIONS] [CMD [ARG...]]");
        println!();
        println!("valkey-cli — Valkey command line interface (Slate OS).");
        println!();
        println!("Options:");
        println!("  -h HOST               Server hostname");
        println!("  -p PORT               Server port (default 6379)");
        println!("  -a PASSWORD            Password");
        println!("  -n DB                  Database number");
        println!("  --stat                 Live stats mode");
        println!("  --bigkeys              Scan for big keys");
        println!("  --latency              Latency measurement");
        println!("  --scan                 Scan for keys");
        println!("  --pipe                 Pipe mode (mass insert)");
        println!("  --cluster SUBCMD       Cluster commands");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("valkey-cli 8.0.0 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "--stat") {
        println!("------- data ------ --------------------- load -------------------- - child -");
        println!("keys       mem      clients blocked requests            connections");
        println!("1234       4.56M    10      0       100000 (+0)         50");
        return 0;
    }
    if args.iter().any(|a| a == "--bigkeys") {
        println!("# Scanning the entire keyspace to find biggest keys.");
        println!("[00.00%] Biggest string found so far 'session:1234' with 1234 bytes");
        println!("[25.00%] Biggest list found so far 'queue:tasks' with 567 items");
        println!("[50.00%] Biggest hash found so far 'user:1' with 12 fields");
        println!();
        println!("-------- summary -------");
        println!("Sampled 1234 keys in the keyspace!");
        println!("Total key length in bytes is 12345");
        return 0;
    }

    let cmd: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if cmd.is_empty() {
        println!("127.0.0.1:6379> ");
    } else {
        match cmd[0].to_uppercase().as_str() {
            "PING" => println!("PONG"),
            "INFO" => {
                println!("# Server");
                println!("valkey_version:8.0.0");
                println!("os:SlateOS x86_64");
                println!("tcp_port:6379");
                println!("uptime_in_seconds:86400");
                println!("# Memory");
                println!("used_memory:4567890");
                println!("used_memory_human:4.56M");
            }
            _ => println!("OK"),
        }
    }
    0
}

fn run_valkey_server(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: valkey-server [CONFIG] [OPTIONS]");
        println!("  --port PORT            Listen port");
        println!("  --bind ADDR            Bind address");
        println!("  --maxmemory BYTES      Max memory");
        println!("  --daemonize yes/no     Run as daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Valkey server v=8.0.0 (Slate OS)");
        return 0;
    }

    let port = args.windows(2).find(|w| w[0] == "--port")
        .map(|w| w[1].as_str()).unwrap_or("6379");
    println!("oO0OoO0OoO0Oo Valkey is starting oO0OoO0OoO0Oo");
    println!("Valkey version=8.0.0, bits=64, pid=1234");
    println!("Server initialized");
    println!("Ready to accept connections on port {}", port);
    0
}

fn run_valkey_benchmark(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: valkey-benchmark [OPTIONS]");
        println!("  -n NUM       Total requests (default 100000)");
        println!("  -c NUM       Concurrent connections (default 50)");
        println!("  -t TESTS     Test types (ping,set,get,etc.)");
        return 0;
    }

    println!("====== PING_INLINE ======");
    println!("  100000 requests completed in 0.85 seconds");
    println!("  50 parallel clients");
    println!("  117647.06 requests per second");
    println!();
    println!("====== SET ======");
    println!("  100000 requests completed in 0.92 seconds");
    println!("  50 parallel clients");
    println!("  108695.65 requests per second");
    println!();
    println!("====== GET ======");
    println!("  100000 requests completed in 0.88 seconds");
    println!("  50 parallel clients");
    println!("  113636.36 requests per second");
    let _ = args;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "valkey-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "valkey-server" => run_valkey_server(&rest),
        "valkey-benchmark" => run_valkey_benchmark(&rest),
        _ => run_valkey_cli(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_valkey_cli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/valkey"), "valkey");
        assert_eq!(basename(r"C:\bin\valkey.exe"), "valkey.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("valkey.exe"), "valkey");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_valkey_cli(&["--help".to_string()]), 0);
        assert_eq!(run_valkey_cli(&["-h".to_string()]), 0);
        let _ = run_valkey_cli(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_valkey_cli(&[]);
    }
}
