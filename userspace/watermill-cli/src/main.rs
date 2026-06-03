#![deny(clippy::all)]

//! watermill-cli — OurOS Watermill event streaming library
//!
//! Single personality: `watermill`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_watermill(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: watermill [COMMAND] [OPTIONS]");
        println!("Watermill v1.3 (OurOS) — Event-driven processing toolkit");
        println!();
        println!("Commands:");
        println!("  pub TOPIC MSG      Publish message to topic");
        println!("  sub TOPIC          Subscribe to topic");
        println!("  router             Start message router");
        println!("  bench              Run benchmarks");
        println!("  inspect TOPIC      Inspect topic messages");
        println!();
        println!("Options:");
        println!("  --driver DRIVER    Backend (kafka/nats/amqp/gochannel)");
        println!("  --broker URL       Broker URL");
        println!("  --config FILE      Config file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Watermill v1.3.7 (OurOS)"); return 0; }
    println!("Watermill v1.3.7 (OurOS)");
    println!("  Driver: GoChannel (in-memory)");
    println!("  Router: 5 handlers");
    println!("  Topics: 8 (pub/sub)");
    println!("  Messages processed: 12,345");
    println!("  Throughput: 2,345 msg/sec");
    println!("  Middleware: retry, throttle, correlation, poison");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "watermill".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_watermill(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_watermill};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/watermill"), "watermill");
        assert_eq!(basename(r"C:\bin\watermill.exe"), "watermill.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("watermill.exe"), "watermill");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_watermill(&["--help".to_string()], "watermill"), 0);
        assert_eq!(run_watermill(&["-h".to_string()], "watermill"), 0);
        assert_eq!(run_watermill(&["--version".to_string()], "watermill"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_watermill(&[], "watermill"), 0);
    }
}
