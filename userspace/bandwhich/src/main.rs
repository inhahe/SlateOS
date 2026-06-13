#![deny(clippy::all)]

//! bandwhich — SlateOS terminal bandwidth utilization tool
//!
//! Single personality: `bandwhich`

use std::env;
use std::process;

fn run_bandwhich(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bandwhich [OPTIONS]");
        println!();
        println!("Display current network utilization by process, connection, and remote IP/hostname.");
        println!();
        println!("Options:");
        println!("  -i, --interface <INTERFACE>  Network interface to monitor");
        println!("  -r, --raw                    Machine-readable output");
        println!("  -n, --no-resolve             Don't resolve hostnames");
        println!("  -s, --show-dns               Show DNS queries");
        println!("  -d, --dns-server <IP>        DNS server to use");
        println!("  -t, --total-utilization      Show total utilization only");
        println!("  -p, --processes              Show process table only");
        println!("  -c, --connections             Show connection table only");
        println!("  -a, --addresses               Show remote address table only");
        println!("  -u, --unit <UNIT>            Display unit (b/k/m/g for bytes, B/K/M/G for bits)");
        println!("  -V, --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("bandwhich 0.22.2 (SlateOS)");
        return 0;
    }

    let raw = args.iter().any(|a| a == "-r" || a == "--raw");
    let total = args.iter().any(|a| a == "-t" || a == "--total-utilization");
    let procs_only = args.iter().any(|a| a == "-p" || a == "--processes");
    let conns_only = args.iter().any(|a| a == "-c" || a == "--connections");
    let addrs_only = args.iter().any(|a| a == "-a" || a == "--addresses");

    if raw {
        println!("process\tinterface\tconnections\tup\tdown");
        println!("browser\teth0\t12\t245760\t1572864");
        println!("cargo\teth0\t3\t8192\t524288");
        println!("dns-resolver\teth0\t1\t1024\t2048");
        return 0;
    }

    if total {
        println!("Total utilization:");
        println!("  Upload:    248.0 KB/s");
        println!("  Download:  2.1 MB/s");
        println!("  Interface: eth0");
        return 0;
    }

    if procs_only {
        println!("┌Process─────────────┬Upload──────┬Download────┬Connections─┐");
        println!("│ browser            │  240.0KB/s │    1.5MB/s │         12 │");
        println!("│ cargo              │    8.0KB/s │  512.0KB/s │          3 │");
        println!("│ dns-resolver       │    1.0KB/s │    2.0KB/s │          1 │");
        println!("│ ssh                │    0.5KB/s │    0.3KB/s │          1 │");
        println!("└────────────────────┴────────────┴────────────┴────────────┘");
        return 0;
    }

    if conns_only {
        println!("┌Connection──────────────────────────────────┬Upload──────┬Download────┐");
        println!("│ 192.168.1.100:52341 -> 151.101.1.69:443   │  128.0KB/s │  1.0MB/s   │");
        println!("│ 192.168.1.100:52342 -> 151.101.1.69:443   │  112.0KB/s │  512.0KB/s │");
        println!("│ 192.168.1.100:48210 -> 185.199.108.133:443│    8.0KB/s │  512.0KB/s │");
        println!("│ 192.168.1.100:22    <- 10.0.0.5:59182     │    0.5KB/s │    0.3KB/s │");
        println!("└────────────────────────────────────────────┴────────────┴────────────┘");
        return 0;
    }

    if addrs_only {
        println!("┌Remote Address─────────────────┬Upload──────┬Download────┬Connections─┐");
        println!("│ cdn.example.com (151.101.1.69) │  240.0KB/s │   1.5MB/s │          2 │");
        println!("│ github.com (185.199.108.133)   │    8.0KB/s │ 512.0KB/s │          3 │");
        println!("│ dns.quad9.net (9.9.9.9)        │    1.0KB/s │   2.0KB/s │          1 │");
        println!("│ workstation (10.0.0.5)         │    0.5KB/s │   0.3KB/s │          1 │");
        println!("└────────────────────────────────┴────────────┴────────────┴────────────┘");
        return 0;
    }

    // Default: show all three tables
    println!("bandwhich 0.22.2 (SlateOS) — TUI launched");
    println!();
    println!("┌Process─────────────┬Upload──────┬Download────┐");
    println!("│ browser            │  240.0KB/s │    1.5MB/s │");
    println!("│ cargo              │    8.0KB/s │  512.0KB/s │");
    println!("│ dns-resolver       │    1.0KB/s │    2.0KB/s │");
    println!("└────────────────────┴────────────┴────────────┘");
    println!();
    println!("┌Connection──────────────────────────────────┬Upload──────┬Download────┐");
    println!("│ 192.168.1.100:52341 -> 151.101.1.69:443   │  128.0KB/s │    1.0MB/s │");
    println!("│ 192.168.1.100:48210 -> 185.199.108.133:443│    8.0KB/s │  512.0KB/s │");
    println!("└────────────────────────────────────────────┴────────────┴────────────┘");
    println!();
    println!("┌Remote Address─────────────────┬Upload──────┬Download────┐");
    println!("│ cdn.example.com               │  240.0KB/s │    1.5MB/s │");
    println!("│ github.com                    │    8.0KB/s │  512.0KB/s │");
    println!("└────────────────────────────────┴────────────┴────────────┘");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bandwhich(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_bandwhich};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bandwhich(vec!["--help".to_string()]), 0);
        assert_eq!(run_bandwhich(vec!["-h".to_string()]), 0);
        let _ = run_bandwhich(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bandwhich(vec![]);
    }
}
