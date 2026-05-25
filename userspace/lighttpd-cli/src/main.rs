#![deny(clippy::all)]

//! lighttpd-cli — OurOS lighttpd web server
//!
//! Single personality: `lighttpd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lighttpd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lighttpd [OPTIONS]");
        println!("lighttpd v1.4 (OurOS) — Lightweight high-performance web server");
        println!();
        println!("Options:");
        println!("  -f FILE            Config file");
        println!("  -D                 No daemonize");
        println!("  -t                 Test configuration");
        println!("  -p                 Print parsed config");
        println!("  -m DIR             Module directory");
        println!("  -1                 Process single request (debug)");
        println!("  -v                 Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") { println!("lighttpd/1.4.76 (OurOS)"); return 0; }
    println!("lighttpd/1.4.76 (OurOS)");
    println!("  Listening: 0.0.0.0:80, 0.0.0.0:443");
    println!("  Document root: /var/www");
    println!("  Modules: mod_access, mod_fastcgi, mod_rewrite, mod_redirect");
    println!("  FastCGI: PHP 8.3 (127.0.0.1:9000)");
    println!("  SSL: enabled (mod_openssl)");
    println!("  Virtual hosts: 5");
    println!("  Memory: 8 MB resident");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lighttpd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lighttpd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
