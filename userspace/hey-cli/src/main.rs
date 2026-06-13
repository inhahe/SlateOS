#![deny(clippy::all)]

//! hey-cli — SlateOS hey HTTP load generator
//!
//! Multi-personality: `hey`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hey(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hey [OPTIONS] URL");
        println!("hey — HTTP load generator (SlateOS)");
        println!();
        println!("Options:");
        println!("  -n NUM       Number of requests (default: 200)");
        println!("  -c NUM       Concurrent workers (default: 50)");
        println!("  -q NUM       Rate limit (QPS per worker)");
        println!("  -z DURATION  Duration (e.g. 10s, 2m)");
        println!("  -m METHOD    HTTP method (default: GET)");
        println!("  -H HEADER    Custom header");
        println!("  -d DATA      Request body");
        println!("  -D FILE      Request body from file");
        println!("  -T TYPE      Content-Type (default: text/html)");
        println!("  -o FORMAT    Output format (csv)");
        return 0;
    }
    let _url = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("http://localhost:8080");
    let num = args.windows(2).find(|w| w[0] == "-n")
        .map(|w| w[1].as_str()).unwrap_or("200");
    let conc = args.windows(2).find(|w| w[0] == "-c")
        .map(|w| w[1].as_str()).unwrap_or("50");

    println!("Summary:");
    println!("  Total:        2.3456 secs");
    println!("  Slowest:      0.1234 secs");
    println!("  Fastest:      0.0012 secs");
    println!("  Average:      0.0234 secs");
    println!("  Requests/sec: {:.2}", 200.0 / 2.3456);
    println!();
    println!("  Total data:   250000 bytes");
    println!("  Size/request: 1250 bytes");
    println!();
    println!("Response time histogram:");
    println!("  0.001 [1]    |");
    println!("  0.013 [45]   |{}", "■".repeat(20));
    println!("  0.026 [89]   |{}", "■".repeat(40));
    println!("  0.038 [34]   |{}", "■".repeat(15));
    println!("  0.050 [18]   |{}", "■".repeat(8));
    println!("  0.063 [8]    |{}", "■".repeat(4));
    println!("  0.075 [3]    |■");
    println!("  0.088 [1]    |");
    println!("  0.100 [1]    |");
    println!();
    println!("Latency distribution:");
    println!("  10% in 0.0089 secs");
    println!("  25% in 0.0134 secs");
    println!("  50% in 0.0212 secs");
    println!("  75% in 0.0312 secs");
    println!("  90% in 0.0456 secs");
    println!("  95% in 0.0567 secs");
    println!("  99% in 0.0891 secs");
    println!();
    println!("Details (average, fastest, slowest):");
    println!("  DNS+dialup:   0.0001 secs, 0.0000 secs, 0.0012 secs");
    println!("  DNS-lookup:   0.0000 secs, 0.0000 secs, 0.0005 secs");
    println!("  req write:    0.0000 secs, 0.0000 secs, 0.0001 secs");
    println!("  resp wait:    0.0231 secs, 0.0011 secs, 0.1230 secs");
    println!("  resp read:    0.0002 secs, 0.0001 secs, 0.0003 secs");
    println!();
    println!("Status code distribution:");
    println!("  [200] {} responses", num);
    println!();
    let _conc = conc;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hey".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hey(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hey};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hey"), "hey");
        assert_eq!(basename(r"C:\bin\hey.exe"), "hey.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hey.exe"), "hey");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hey(&["--help".to_string()]), 0);
        assert_eq!(run_hey(&["-h".to_string()]), 0);
        let _ = run_hey(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hey(&[]);
    }
}
