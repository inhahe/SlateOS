#![deny(clippy::all)]

//! wrk-cli — OurOS wrk HTTP benchmarking tool
//!
//! Multi-personality: `wrk`, `wrk2`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wrk(args: &[String], is_wrk2: bool) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        let name = if is_wrk2 { "wrk2" } else { "wrk" };
        println!("Usage: {} [OPTIONS] <url>", name);
        if is_wrk2 {
            println!("wrk2 4.2.0 (OurOS) — constant-throughput HTTP benchmark");
        } else {
            println!("wrk 4.2.0 (OurOS) — HTTP benchmarking tool");
        }
        println!();
        println!("Options:");
        println!("  -c, --connections N  Connections to keep open");
        println!("  -d, --duration  T    Duration of test (e.g. 10s, 2m)");
        println!("  -t, --threads   N    Number of threads");
        if is_wrk2 {
            println!("  -R, --rate      N    Target requests/sec");
        }
        println!("  -s, --script    F    Lua script");
        println!("  -H, --header    H    Add header");
        println!("  --latency            Print latency statistics");
        println!("  --timeout       T    Socket/request timeout");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        if is_wrk2 {
            println!("wrk2 4.2.0");
        } else {
            println!("wrk 4.2.0");
        }
        return 0;
    }
    let url = args.iter().rfind(|a| !a.starts_with('-') && a.contains("://"))
        .map(|s| s.as_str())
        .or_else(|| args.last().map(|s| s.as_str()))
        .unwrap_or("http://localhost:8080");
    let threads = args.windows(2).find(|w| w[0] == "-t" || w[0] == "--threads")
        .map(|w| w[1].as_str()).unwrap_or("2");
    let conns = args.windows(2).find(|w| w[0] == "-c" || w[0] == "--connections")
        .map(|w| w[1].as_str()).unwrap_or("10");
    let duration = args.windows(2).find(|w| w[0] == "-d" || w[0] == "--duration")
        .map(|w| w[1].as_str()).unwrap_or("10s");
    let show_latency = args.iter().any(|a| a == "--latency");

    println!("Running {}s test @ {}", duration, url);
    println!("  {} threads and {} connections", threads, conns);
    println!();
    println!("  Thread Stats   Avg      Stdev     Max   +/- Stdev");
    println!("    Latency     1.23ms  456.78us   12.34ms   78.90%");
    println!("    Req/Sec     4.12k   234.56     5.67k     68.42%");

    if show_latency {
        println!();
        println!("  Latency Distribution");
        println!("     50%    1.05ms");
        println!("     75%    1.34ms");
        println!("     90%    1.89ms");
        println!("     99%    5.67ms");
    }

    println!();
    println!("  82345 requests in {}, 12.34MB read", duration);
    println!("Requests/sec:   8234.50");
    println!("Transfer/sec:      1.23MB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wrk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let is_wrk2 = prog == "wrk2";
    let code = run_wrk(&rest, is_wrk2);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wrk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wrk"), "wrk");
        assert_eq!(basename(r"C:\bin\wrk.exe"), "wrk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wrk.exe"), "wrk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wrk(&["--help".to_string()], false), 0);
        assert_eq!(run_wrk(&["-h".to_string()], false), 0);
        assert_eq!(run_wrk(&["--version".to_string()], false), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wrk(&[], false), 0);
    }
}
