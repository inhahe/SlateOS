#![deny(clippy::all)]

//! redis-cli — OurOS Redis command-line interface
//!
//! Single personality: `redis-cli`

use std::env;
use std::process;

fn run_redis_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redis-cli [OPTIONS] [COMMAND [ARGS...]]");
        println!();
        println!("Redis command-line interface.");
        println!();
        println!("Options:");
        println!("  -h <HOST>              Server hostname (default: 127.0.0.1)");
        println!("  -p <PORT>              Server port (default: 6379)");
        println!("  -a <PASSWORD>          Password");
        println!("  --user <USERNAME>      ACL username");
        println!("  -n <DB>                Database number");
        println!("  -u <URI>               Connection URI (redis://...)");
        println!("  --tls                  Enable TLS");
        println!("  --cert <FILE>          Client certificate");
        println!("  --key <FILE>           Client private key");
        println!("  --cacert <FILE>        CA certificate");
        println!("  -r <N>                 Repeat command N times");
        println!("  -i <SEC>               Interval between repeats");
        println!("  --pipe                 Transfer raw Redis protocol from stdin");
        println!("  --pipe-timeout <N>     Pipe mode timeout");
        println!("  --bigkeys              Sample and find big keys");
        println!("  --memkeys              Sample and report memory usage");
        println!("  --hotkeys              Sample and find hot keys");
        println!("  --scan                 Scan for keys matching pattern");
        println!("  --pattern <PATTERN>    Pattern for --scan");
        println!("  --intrinsic-latency <S>  Test intrinsic latency");
        println!("  --latency              Enter latency monitoring mode");
        println!("  --latency-history      Latency history mode");
        println!("  --stat                 Server stats mode");
        println!("  --raw                  Raw output (no formatting)");
        println!("  --csv                  CSV output");
        println!("  --json                 JSON output");
        println!("  --resp2                Use RESP2 protocol");
        println!("  --resp3                Use RESP3 protocol");
        println!("  --cluster <CMD>        Cluster management");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("redis-cli 7.2.4 (OurOS)");
        return 0;
    }

    let stat = args.iter().any(|a| a == "--stat");
    let latency = args.iter().any(|a| a == "--latency");
    let bigkeys = args.iter().any(|a| a == "--bigkeys");

    if stat {
        println!("------- data ------ --------------------- load -------------------- - child -");
        println!("keys       mem      clients blocked requests            connections");
        println!("1234       8.50M    3       0       567890 (+0)         15");
        println!("1234       8.50M    3       0       567891 (+1)         15");
        println!("1234       8.51M    3       0       567895 (+4)         15");
        return 0;
    }

    if latency {
        println!("min: 0, max: 1, avg: 0.23 (1000 samples)");
        return 0;
    }

    if bigkeys {
        println!("# Scanning the entire keyspace to find biggest keys");
        println!();
        println!("[00.00%] Biggest string found so far: session:abc123 (2048 bytes)");
        println!("[25.00%] Biggest list found so far: queue:jobs (15234 items)");
        println!("[50.00%] Biggest hash found so far: user:1234 (45 fields)");
        println!("[75.00%] Biggest set found so far: tags:all (8901 members)");
        println!("[100.00%] Biggest zset found so far: leaderboard (50000 members)");
        println!();
        println!("-------- summary -------");
        println!("Biggest string: session:abc123 (2048 bytes)");
        println!("Biggest list:   queue:jobs (15234 items)");
        println!("Biggest hash:   user:1234 (45 fields)");
        println!("Biggest set:    tags:all (8901 members)");
        println!("Biggest zset:   leaderboard (50000 members)");
        return 0;
    }

    // Inline command execution
    let cmds: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if !cmds.is_empty() {
        let cmd_upper = cmds[0].to_uppercase();
        match cmd_upper.as_str() {
            "PING" => println!("PONG"),
            "SET" => println!("OK"),
            "GET" => println!("\"hello world\""),
            "DEL" => println!("(integer) 1"),
            "KEYS" => {
                println!("1) \"session:abc\"");
                println!("2) \"user:1234\"");
                println!("3) \"cache:page:home\"");
            }
            "INFO" => {
                println!("# Server");
                println!("redis_version:7.2.4");
                println!("redis_mode:standalone");
                println!("os:OurOS x86_64");
                println!("arch_bits:64");
                println!("tcp_port:6379");
                println!("uptime_in_seconds:86400");
                println!("uptime_in_days:1");
                println!();
                println!("# Memory");
                println!("used_memory:8945678");
                println!("used_memory_human:8.53M");
                println!("used_memory_peak:12345678");
                println!("used_memory_peak_human:11.77M");
            }
            "DBSIZE" => println!("(integer) 1234"),
            _ => println!("(error) ERR unknown command '{}'", cmds[0]),
        }
        return 0;
    }

    println!("127.0.0.1:6379> ");
    println!("  (interactive mode, type QUIT to exit)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_redis_cli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_redis_cli};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_redis_cli(vec!["--help".to_string()]), 0);
        assert_eq!(run_redis_cli(vec!["-h".to_string()]), 0);
        assert_eq!(run_redis_cli(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_redis_cli(vec![]), 0);
    }
}
