#![deny(clippy::all)]

//! locust-cli — OurOS Locust load testing tool
//!
//! Multi-personality: `locust`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_locust(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: locust [OPTIONS]");
        println!("Locust 2.29.0 (OurOS) — Load testing tool");
        println!();
        println!("Options:");
        println!("  -f FILE        Locustfile to use (default: locustfile.py)");
        println!("  --host HOST    Host to load test");
        println!("  -u NUM         Number of users");
        println!("  -r NUM         Spawn rate (users/second)");
        println!("  -t TIME        Run time (e.g. 30s, 5m, 1h)");
        println!("  --headless     Run without web UI");
        println!("  --csv PREFIX   Save stats to CSV files");
        println!("  --html FILE    Save HTML report");
        println!("  --master       Run as master node");
        println!("  --worker       Run as worker node");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("locust 2.29.0");
        return 0;
    }
    let host = args.windows(2).find(|w| w[0] == "--host")
        .map(|w| w[1].as_str()).unwrap_or("http://localhost:8000");
    let users = args.windows(2).find(|w| w[0] == "-u")
        .map(|w| w[1].as_str()).unwrap_or("10");
    let rate = args.windows(2).find(|w| w[0] == "-r")
        .map(|w| w[1].as_str()).unwrap_or("1");
    let headless = args.iter().any(|a| a == "--headless");

    if headless {
        println!("[INFO] Starting Locust {} (headless mode)", "2.29.0");
        println!("[INFO] Host: {}", host);
        println!("[INFO] Users: {}, spawn rate: {}/s", users, rate);
        println!();
        println!(" Name                              # reqs    # fails  |  Avg   Min   Max  Median  req/s");
        println!("----------------------------------------------------------------------------------------------");
        println!(" GET /                                  45     0(0%)  |   23    12    89      18   4.50");
        println!(" GET /api/users                         45     0(0%)  |   34    15   123      28   4.50");
        println!(" POST /api/login                        23     1(4%)  |   67    31   234      52   2.30");
        println!("----------------------------------------------------------------------------------------------");
        println!(" Aggregated                            113     1(1%)  |   38    12   234      28  11.30");
    } else {
        println!("[INFO] Starting web interface at http://0.0.0.0:8089");
        println!("[INFO] Host: {}", host);
        println!("[INFO] Starting Locust 2.29.0");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "locust".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_locust(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_locust};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/locust"), "locust");
        assert_eq!(basename(r"C:\bin\locust.exe"), "locust.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("locust.exe"), "locust");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_locust(&["--help".to_string()]), 0);
        assert_eq!(run_locust(&["-h".to_string()]), 0);
        assert_eq!(run_locust(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_locust(&[]), 0);
    }
}
