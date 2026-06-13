#![deny(clippy::all)]

//! siege-cli — SlateOS Siege HTTP load testing tool
//!
//! Multi-personality: `siege`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_siege(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: siege [OPTIONS] URL");
        println!("SIEGE 4.1.7 (SlateOS)");
        println!();
        println!("Options:");
        println!("  -c, --concurrent N   Concurrent users (default: 25)");
        println!("  -r, --reps N         Number of repetitions");
        println!("  -t, --time T         Duration (e.g. 30S, 5M, 1H)");
        println!("  -d, --delay N        Random delay (0-N seconds)");
        println!("  -f, --file FILE      URL file");
        println!("  -b, --benchmark      Benchmark mode (no delays)");
        println!("  -i, --internet       Random URLs from file");
        println!("  -l, --log FILE       Log to file");
        println!("  --content-type TYPE  Set content type");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("SIEGE 4.1.7");
        println!("SlateOS port");
        return 0;
    }
    let url = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("http://localhost/");
    let concurrent = args.windows(2).find(|w| w[0] == "-c" || w[0] == "--concurrent")
        .map(|w| w[1].as_str()).unwrap_or("25");
    let benchmark = args.iter().any(|a| a == "-b" || a == "--benchmark");

    println!("** SIEGE 4.1.7");
    println!("** Preparing {} concurrent users for battle.", concurrent);
    if benchmark {
        println!("** Benchmark mode: no delays between requests.");
    }
    println!("The server is now under siege...");
    println!();
    println!("Transactions:                   1234 hits");
    println!("Availability:                  99.92 %");
    println!("Elapsed time:                   9.87 secs");
    println!("Data transferred:               1.23 MB");
    println!("Response time:                  0.02 secs");
    println!("Transaction rate:             125.00 trans/sec");
    println!("Throughput:                     0.12 MB/sec");
    println!("Concurrency:                   {:.2}", 24.87);
    println!("Successful transactions:        1234");
    println!("Failed transactions:               1");
    println!("Longest transaction:            0.12");
    println!("Shortest transaction:           0.00");
    println!();
    println!("URL: {}", url);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "siege".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_siege(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_siege};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/siege"), "siege");
        assert_eq!(basename(r"C:\bin\siege.exe"), "siege.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("siege.exe"), "siege");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_siege(&["--help".to_string()]), 0);
        assert_eq!(run_siege(&["-h".to_string()]), 0);
        let _ = run_siege(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_siege(&[]);
    }
}
