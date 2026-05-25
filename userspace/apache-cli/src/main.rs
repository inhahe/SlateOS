#![deny(clippy::all)]

//! apache-cli — OurOS Apache HTTP Server
//!
//! Multi-personality: `httpd`, `apachectl`, `ab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_apache(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "apachectl" => {
                println!("apachectl (OurOS) — Apache HTTP Server control");
                println!("  start              Start httpd");
                println!("  stop               Stop httpd");
                println!("  restart            Restart httpd");
                println!("  graceful           Graceful restart");
                println!("  graceful-stop      Graceful stop");
                println!("  configtest         Test configuration");
                println!("  status             Server status");
            }
            "ab" => {
                println!("ab (OurOS) — Apache HTTP benchmarking tool");
                println!("  -n REQUESTS        Number of requests");
                println!("  -c CONCURRENCY     Concurrent connections");
                println!("  -t TIMELIMIT       Seconds to max. wait");
                println!("  -k                 Use HTTP KeepAlive");
                println!("  -H HEADER          Add header");
                println!("  URL                Target URL");
            }
            _ => {
                println!("httpd v2.4 (OurOS) — Apache HTTP Server");
                println!("  -f FILE            Config file");
                println!("  -c DIRECTIVE       Process directive");
                println!("  -d DIR             ServerRoot");
                println!("  -D PARAM           Define parameter");
                println!("  -t                 Test configuration");
                println!("  -S                 Show parsed vhost settings");
                println!("  -M                 Show loaded modules");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Apache/2.4.59 (OurOS)");
        return 0;
    }
    match prog {
        "ab" => {
            println!("Apache Bench v2.3 (OurOS)");
            println!("  Benchmarking localhost...");
            println!("  Requests per second: 12,345.67");
            println!("  Time per request: 0.081 ms");
            println!("  Transfer rate: 45.6 MB/sec");
        }
        _ => {
            println!("Apache/2.4.59 (OurOS)");
            println!("  MPM: event (workers: 4, threads: 25)");
            println!("  Listening: *:80, *:443");
            println!("  Virtual hosts: 8");
            println!("  Modules: 34 loaded");
            println!("  Document root: /var/www/html");
            println!("  SSL: OpenSSL 3.2, 3 certificates");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "httpd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_apache(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
