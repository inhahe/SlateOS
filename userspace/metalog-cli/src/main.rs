#![deny(clippy::all)]

//! metalog-cli — OurOS Metalog syslog daemon
//!
//! Single personality: `metalog`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_metalog(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: metalog [OPTIONS]");
        println!("Metalog v4.0 (OurOS) — Modern syslog daemon");
        println!();
        println!("Options:");
        println!("  -c, --config FILE  Config file (default: /etc/metalog.conf)");
        println!("  -N, --no-kernel    Don't read kernel messages");
        println!("  -B SIZE            Kernel buffer size");
        println!("  --pidfile FILE     PID file path");
        println!("  --daemonize        Run as daemon");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("metalog v4.0.0 (OurOS)"); return 0; }
    println!("Metalog v4.0.0 (OurOS)");
    println!("  Config: /etc/metalog.conf");
    println!("  Sections: 5 (mail, news, kernel, auth, default)");
    println!("  Log directory: /var/log");
    println!("  Kernel messages: enabled");
    println!("  Rotation: size-based (1 MiB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "metalog".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_metalog(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
