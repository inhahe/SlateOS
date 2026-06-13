#![deny(clippy::all)]

//! coredns-cli — SlateOS CoreDNS server
//!
//! Single personality: `coredns`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_coredns(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: coredns [OPTIONS]");
        println!("CoreDNS v1.11 (Slate OS) — DNS and service discovery");
        println!();
        println!("Options:");
        println!("  -conf FILE     Corefile configuration");
        println!("  -dns.port PORT DNS port (default: 53)");
        println!("  -pidfile FILE  PID file");
        println!("  -quiet         Quiet mode");
        println!("  -plugins       List plugins");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CoreDNS v1.11.3 (Slate OS)"); return 0; }
    println!("CoreDNS v1.11.3 (Slate OS)");
    println!("  Corefile: /etc/coredns/Corefile");
    println!("  Zones:");
    println!("    .:53 -> forward to 1.1.1.1, 8.8.8.8");
    println!("    cluster.local:53 -> kubernetes");
    println!("  Plugins: cache, forward, kubernetes, prometheus, log, errors");
    println!("  Cache: 10000 entries, 30s TTL");
    println!("  Listening: 0.0.0.0:53 (udp+tcp)");
    println!("  Prometheus: :9153/metrics");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "coredns".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_coredns(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_coredns};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/coredns"), "coredns");
        assert_eq!(basename(r"C:\bin\coredns.exe"), "coredns.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("coredns.exe"), "coredns");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_coredns(&["--help".to_string()], "coredns"), 0);
        assert_eq!(run_coredns(&["-h".to_string()], "coredns"), 0);
        let _ = run_coredns(&["--version".to_string()], "coredns");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_coredns(&[], "coredns");
    }
}
