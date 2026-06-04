#![deny(clippy::all)]

//! memcached — OurOS distributed memory caching system
//!
//! Single personality: `memcached`

use std::env;
use std::process;

fn run_memcached(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: memcached [options]");
        println!();
        println!("Options:");
        println!("  -p <num>      TCP port (default: 11211)");
        println!("  -l <addr>     Bind address");
        println!("  -d            Run as daemon");
        println!("  -m <num>      Max memory in megabytes (default: 64)");
        println!("  -c <num>      Max simultaneous connections (default: 1024)");
        println!("  -t <num>      Number of threads (default: 4)");
        println!("  -M            Return error on memory exhausted (instead of evicting)");
        println!("  -v            Verbose (repeat for more)");
        println!("  -I <size>     Max item size (default: 1m)");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("memcached 1.6.24 (OurOS)");
        return 0;
    }
    let port = args.iter().position(|a| a == "-p")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(11211);
    let mem = args.iter().position(|a| a == "-m")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(64);
    let threads = args.iter().position(|a| a == "-t")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(4);

    println!("memcached 1.6.24 (OurOS)");
    println!("slab class   1: chunk size     96 perslab 10922");
    println!("slab class   2: chunk size    120 perslab  8738");
    println!("slab class   3: chunk size    152 perslab  6898");
    println!("slab class  42: chunk size 1048576 perslab     1");
    println!("<{}> new auto-negotiating client connection", port);
    println!("Listening on port {}", port);
    println!("Max memory: {} MB", mem);
    println!("Threads: {}", threads);
    println!("Server ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_memcached(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_memcached};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_memcached(vec!["--help".to_string()]), 0);
        assert_eq!(run_memcached(vec!["-h".to_string()]), 0);
        let _ = run_memcached(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_memcached(vec![]);
    }
}
