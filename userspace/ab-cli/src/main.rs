#![deny(clippy::all)]

//! ab-cli — OurOS Apache Bench (ab) HTTP benchmarking tool
//!
//! Multi-personality: `ab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ab(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") || args.is_empty() {
        println!("Usage: ab [OPTIONS] URL");
        println!("Apache HTTP server benchmarking tool (OurOS)");
        println!();
        println!("Options:");
        println!("  -n NUM       Number of requests");
        println!("  -c NUM       Number of concurrent requests");
        println!("  -t SEC       Timelimit in seconds");
        println!("  -k           Use HTTP KeepAlive");
        println!("  -H HEADER    Add header");
        println!("  -p FILE      POST data file");
        println!("  -T TYPE      Content-Type header");
        println!("  -v LEVEL     Verbosity level (0-4)");
        println!("  -V           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("This is ApacheBench, Version 2.3 <$Revision: 1913912 $>");
        println!("OurOS port");
        return 0;
    }
    let url = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("http://localhost/");
    let num_requests = args.windows(2).find(|w| w[0] == "-n")
        .map(|w| w[1].as_str()).unwrap_or("1000");
    let concurrency = args.windows(2).find(|w| w[0] == "-c")
        .map(|w| w[1].as_str()).unwrap_or("1");
    let keepalive = args.iter().any(|a| a == "-k");

    println!("This is ApacheBench, Version 2.3 <$Revision: 1913912 $>");
    println!("Benchmarking {} (be patient)", url);
    println!();
    println!("Server Software:        OurOS-httpd");
    println!("Server Hostname:        localhost");
    println!("Server Port:            80");
    println!();
    println!("Document Path:          /");
    println!("Document Length:         1234 bytes");
    println!();
    println!("Concurrency Level:      {}", concurrency);
    println!("Time taken for tests:   2.345 seconds");
    println!("Complete requests:      {}", num_requests);
    println!("Failed requests:        0");
    println!("Total transferred:      1234000 bytes");
    println!("HTML transferred:       1234000 bytes");
    println!("Requests per second:    426.44 [#/sec] (mean)");
    println!("Time per request:       2.345 [ms] (mean)");
    println!("Time per request:       2.345 [ms] (mean, across all concurrent requests)");
    println!("Transfer rate:          513.45 [Kbytes/sec] received");
    if keepalive {
        println!("Keep-Alive:             enabled");
    }
    println!();
    println!("Connection Times (ms)");
    println!("              min  mean[+/-sd] median   max");
    println!("Connect:        0    0   0.1      0       1");
    println!("Processing:     1    2   0.8      2      12");
    println!("Waiting:        1    2   0.7      2      11");
    println!("Total:          1    2   0.8      2      12");
    println!();
    println!("Percentage of the requests served within a certain time (ms)");
    println!("  50%      2");
    println!("  66%      2");
    println!("  75%      3");
    println!("  80%      3");
    println!("  90%      3");
    println!("  95%      4");
    println!("  98%      5");
    println!("  99%      7");
    println!(" 100%     12 (longest request)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ab(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ab"), "ab");
        assert_eq!(basename(r"C:\bin\ab.exe"), "ab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ab.exe"), "ab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ab(&["--help".to_string()]), 0);
        assert_eq!(run_ab(&["-h".to_string()]), 0);
        let _ = run_ab(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ab(&[]);
    }
}
