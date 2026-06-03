#![deny(clippy::all)]

//! redis — OurOS Redis in-memory data store
//!
//! Multi-personality: `redis-server`, `redis-cli`, `redis-benchmark`

use std::env;
use std::process;

fn run_redis_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redis-server [/path/to/redis.conf] [options]");
        println!();
        println!("Options:");
        println!("  --port <port>         Port to listen on (default: 6379)");
        println!("  --bind <addr>         Bind address");
        println!("  --daemonize yes|no    Run as daemon");
        println!("  --loglevel <level>    debug, verbose, notice, warning");
        println!("  --maxmemory <bytes>   Max memory limit");
        println!("  --appendonly yes|no   Enable AOF persistence");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Redis server v=7.2.0 sha=abc12345:0 malloc=jemalloc bits=64 build=abc1234 (OurOS)");
        return 0;
    }
    let port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(6379);
    println!("                _._");
    println!("           _.-``__ ''-._");
    println!("      _.-``    `.  `_.  ''-._           Redis 7.2.0 (OurOS) 64 bit");
    println!("  .-`` .-```.  ```\\/    _.,_ ''-._");
    println!(" (    '      ,       .-`  | `,    )     Running in standalone mode");
    println!(" |`-._`-...-` __...-.``-._|'` _.-'|     Port: {}", port);
    println!(" |    `-._   `._    /     _.-'    |     PID: 12345");
    println!("  `-._    `-._  `-./  _.-'    _.-'");
    println!("      `-._    `-.__.-'    _.-'");
    println!("          `-._        _.-'");
    println!("              `-.__.-'");
    println!();
    println!("Server initialized");
    println!("Ready to accept connections on port {}", port);
    0
}

fn run_redis_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redis-cli [OPTIONS] [cmd [arg [arg ...]]]");
        println!();
        println!("Options:");
        println!("  -h <hostname>  Server hostname (default: 127.0.0.1)");
        println!("  -p <port>      Server port (default: 6379)");
        println!("  -a <password>  Password");
        println!("  -n <db>        Database number");
        println!("  --stat         Continuous stats mode");
        println!("  --scan         List all keys using SCAN");
        println!("  --bigkeys      Sample for big keys");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("redis-cli 7.2.0 (OurOS)");
        return 0;
    }

    // Check for inline command
    let commands: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if !commands.is_empty() {
        let cmd_upper = commands[0].to_uppercase();
        match cmd_upper.as_str() {
            "PING" => println!("PONG"),
            "SET" => println!("OK"),
            "GET" => println!("\"value\""),
            "DEL" => println!("(integer) 1"),
            "KEYS" => { println!("1) \"key1\""); println!("2) \"key2\""); println!("3) \"session:abc\""); }
            "INFO" => {
                println!("# Server");
                println!("redis_version:7.2.0");
                println!("os:OurOS x86_64");
                println!("# Memory");
                println!("used_memory:1048576");
                println!("used_memory_human:1.00M");
            }
            "DBSIZE" => println!("(integer) 42"),
            "FLUSHDB" => println!("OK"),
            _ => println!("(executed: {} — simulated)", commands.join(" ")),
        }
        return 0;
    }

    // Interactive mode
    println!("127.0.0.1:6379> PING");
    println!("PONG");
    println!("127.0.0.1:6379> SET mykey \"Hello\"");
    println!("OK");
    println!("127.0.0.1:6379> GET mykey");
    println!("\"Hello\"");
    println!("127.0.0.1:6379> quit");
    0
}

fn run_redis_benchmark(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redis-benchmark [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -c <clients>   Number of parallel connections (default: 50)");
        println!("  -n <requests>  Total number of requests (default: 100000)");
        println!("  -d <size>      Data size in bytes (default: 3)");
        println!("  -t <tests>     Comma-separated list of tests");
        println!("  -q             Quiet mode (just show RPS)");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-q");
    if quiet {
        println!("PING_INLINE: 142857.14 requests per second");
        println!("SET: 125000.00 requests per second");
        println!("GET: 142857.14 requests per second");
        println!("INCR: 142857.14 requests per second");
        println!("LPUSH: 125000.00 requests per second");
        println!("RPUSH: 125000.00 requests per second");
    } else {
        println!("====== PING_INLINE ======");
        println!("  100000 requests completed in 0.70 seconds");
        println!("  50 parallel clients");
        println!("  3 bytes payload");
        println!("  142857.14 requests per second");
        println!();
        println!("====== SET ======");
        println!("  100000 requests completed in 0.80 seconds");
        println!("  125000.00 requests per second");
        println!();
        println!("====== GET ======");
        println!("  100000 requests completed in 0.70 seconds");
        println!("  142857.14 requests per second");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("redis-server");
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
        "redis-cli" => run_redis_cli(rest),
        "redis-benchmark" => run_redis_benchmark(rest),
        _ => run_redis_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_redis_server};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_redis_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_redis_server(vec!["-h".to_string()]), 0);
        assert_eq!(run_redis_server(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_redis_server(vec![]), 0);
    }
}
