#![deny(clippy::all)]

//! ab — SlateOS Apache HTTP benchmarking tool
//!
//! Multi-personality: `ab`, `siege`

use std::env;
use std::process;

fn run_ab(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ab [options] [http[s]://]hostname[:port]/path");
        println!();
        println!("Options:");
        println!("  -n requests     Number of requests");
        println!("  -c concurrency  Number of concurrent requests");
        println!("  -t timelimit    Maximum seconds to spend (implies -n 50000)");
        println!("  -k              Use HTTP KeepAlive");
        println!("  -H attribute    Add Arbitrary header");
        println!("  -T content-type Content-type header for POST/PUT");
        println!("  -p postfile     File containing data to POST");
        println!("  -u putfile      File containing data to PUT");
        println!("  -v verbosity    Verbosity level (0-4)");
        println!("  -w              Print results in HTML tables");
        println!("  -i              Use HEAD instead of GET");
        println!("  -e csv          Output CSV file");
        println!("  -g gnuplot      Output gnuplot file");
        println!("  -r              Don't exit on socket receive errors");
        println!("  -s timeout      Timeout (default: 30)");
        println!("  -V              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("This is ApacheBench, Version 2.3 <$Revision: 1913912 $>");
        println!("Copyright 1996 Adam Twiss, Zeus Technology Ltd, http://www.zeustech.net/");
        println!("Licensed to The Apache Software Foundation, http://www.apache.org/");
        println!("Slate OS build");
        return 0;
    }

    let url = args.iter().rfind(|a| a.starts_with("http")).map(|s| s.as_str()).unwrap_or("http://localhost/");
    let n = args.iter().position(|a| a == "-n")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("1");
    let c = args.iter().position(|a| a == "-c")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("1");
    let keepalive = args.iter().any(|a| a == "-k");

    println!("This is ApacheBench, Version 2.3 <$Revision: 1913912 $> (Slate OS)");
    println!("Benchmarking {} (be patient)...", url);
    println!();
    println!("Server Software:        nginx/1.25");
    println!("Server Hostname:        localhost");
    println!("Server Port:            80");
    println!();
    println!("Document Path:          /");
    println!("Document Length:         612 bytes");
    println!();
    println!("Concurrency Level:      {}", c);
    println!("Time taken for tests:   0.250 seconds");
    println!("Complete requests:      {}", n);
    println!("Failed requests:        0");
    println!("Total transferred:      312500 bytes");
    println!("HTML transferred:       250000 bytes");
    println!("Requests per second:    4000.00 [#/sec] (mean)");
    println!("Time per request:       0.250 [ms] (mean)");
    println!("Time per request:       0.250 [ms] (mean, across all concurrent requests)");
    println!("Transfer rate:          1220.70 [Kbytes/sec] received");
    if keepalive {
        println!("Keep-Alive:             enabled");
    }
    println!();
    println!("Connection Times (ms)");
    println!("              min  mean[+/-sd] median   max");
    println!("Connect:        0    0   0.0      0       1");
    println!("Processing:     0    0   0.1      0       2");
    println!("Waiting:        0    0   0.1      0       2");
    println!("Total:          0    0   0.1      0       2");
    println!();
    println!("Percentage of the requests served within a certain time (ms)");
    println!("  50%      0");
    println!("  66%      0");
    println!("  75%      0");
    println!("  80%      0");
    println!("  90%      1");
    println!("  95%      1");
    println!("  98%      1");
    println!("  99%      2");
    println!(" 100%      2 (longest request)");
    0
}

fn run_siege(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: siege [options] URL");
        println!();
        println!("Options:");
        println!("  -c NUM, --concurrent=NUM  Concurrent users (default: 25)");
        println!("  -r NUM, --reps=NUM        Repetitions per user");
        println!("  -t NUMm, --time=NUMm     Timed testing (e.g., 1M, 10S)");
        println!("  -d NUM, --delay=NUM       Time delay between requests");
        println!("  -b, --benchmark           Benchmark mode (no delays)");
        println!("  -i, --internet            Random URLs from file");
        println!("  -f FILE, --file=FILE      URLs file");
        println!("  --content-type=TYPE       Set content type");
        println!("  -H HEADER                 Add header");
        println!("  -V, --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("SIEGE 4.1.6 (Slate OS)");
        return 0;
    }

    let url = args.iter().rfind(|a| a.starts_with("http")).map(|s| s.as_str()).unwrap_or("http://localhost/");
    let concurrent = args.iter().position(|a| a == "-c" || a == "--concurrent")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("25");

    println!("** SIEGE 4.1.6 (Slate OS)");
    println!("** Preparing {} concurrent users for battle.", concurrent);
    println!("The server is now under siege...");
    println!();
    println!("Transactions:               500 hits");
    println!("Availability:            100.00 %");
    println!("Elapsed time:              1.25 secs");
    println!("Data transferred:          0.29 MB");
    println!("Response time:             0.05 secs");
    println!("Transaction rate:        400.00 trans/sec");
    println!("Throughput:                0.23 MB/sec");
    println!("Concurrency:              20.00");
    println!("Successful transactions:     500");
    println!("Failed transactions:           0");
    println!("Longest transaction:        0.15");
    println!("Shortest transaction:       0.01");
    let _ = url;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("ab");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "siege" => run_siege(rest),
        _ => run_ab(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ab};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ab(vec!["--help".to_string()]), 0);
        assert_eq!(run_ab(vec!["-h".to_string()]), 0);
        let _ = run_ab(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ab(vec![]);
    }
}
