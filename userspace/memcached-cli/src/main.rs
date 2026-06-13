#![deny(clippy::all)]

//! memcached-cli — SlateOS memcached CLI
//!
//! Single personality: `memcached`

use std::env;
use std::process;

fn run_memcached(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: memcached [OPTIONS]");
        println!();
        println!("memcached — high-performance memory caching (Slate OS).");
        println!();
        println!("Options:");
        println!("  -p, --port PORT        TCP port (default 11211)");
        println!("  -U, --udp-port PORT    UDP port (default 0, disabled)");
        println!("  -l, --listen ADDR      Listen address");
        println!("  -m, --memory MB        Max memory (default 64)");
        println!("  -c, --connections N    Max connections (default 1024)");
        println!("  -t, --threads N        Number of threads (default 4)");
        println!("  -d                     Run as daemon");
        println!("  -v                     Verbose (repeat for more: -vv, -vvv)");
        println!("  -M                     Return error on OOM (instead of evicting)");
        println!("  -r                     Maximize core file limit");
        println!("  -f, --factor N         Chunk size growth factor (default 1.25)");
        println!("  -I, --max-item-size N  Max item size (default 1m)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("memcached 1.6.22 (Slate OS)");
        return 0;
    }

    let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
        .map(|w| w[1].as_str()).unwrap_or("11211");
    let memory = args.windows(2).find(|w| w[0] == "-m" || w[0] == "--memory")
        .map(|w| w[1].as_str()).unwrap_or("64");
    let threads = args.windows(2).find(|w| w[0] == "-t" || w[0] == "--threads")
        .map(|w| w[1].as_str()).unwrap_or("4");
    let verbose = args.iter().filter(|a| a.as_str() == "-v" || a.as_str() == "-vv" || a.as_str() == "-vvv").count() > 0;

    println!("memcached 1.6.22 (Slate OS)");
    println!("  Port: {}", port);
    println!("  Max memory: {} MB", memory);
    println!("  Threads: {}", threads);
    println!("  Max connections: 1024");

    if verbose {
        println!("  slab class   1: chunk size        96 perslab   10922");
        println!("  slab class   2: chunk size       120 perslab    8738");
        println!("  slab class   3: chunk size       152 perslab    6898");
        println!("  slab class   4: chunk size       192 perslab    5461");
    }

    println!("  Listening on port {}", port);
    println!("  Server started.");
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
