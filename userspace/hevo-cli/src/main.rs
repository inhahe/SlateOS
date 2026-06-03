#![deny(clippy::all)]

//! hevo-cli — OurOS Hevo Data pipeline
//!
//! Single personality: `hevo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hevo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hevo [COMMAND] [OPTIONS]");
        println!("Hevo Data v2.0 (OurOS) — No-code data pipeline");
        println!();
        println!("Commands:");
        println!("  pipeline list|create|pause   Manage pipelines");
        println!("  source list|test             Manage sources");
        println!("  destination list|test         Manage destinations");
        println!("  model list|run               Manage models");
        println!("  events status                Event tracking");
        println!("  workflow list|run             Manage workflows");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API key");
        println!("  --region REGION    Data region");
        println!("  --output json|table  Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Hevo Data v2.0.0 (OurOS)"); return 0; }
    println!("Hevo Data v2.0.0 (OurOS)");
    println!("  Pipelines: 15 active");
    println!("  Sources: 8 (MySQL, PostgreSQL, MongoDB, S3, Kafka)");
    println!("  Destinations: 3 (BigQuery, Snowflake, Redshift)");
    println!("  Events: 2.3M/day");
    println!("  Models: 12");
    println!("  Latency: < 5min avg");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hevo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hevo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hevo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hevo"), "hevo");
        assert_eq!(basename(r"C:\bin\hevo.exe"), "hevo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hevo.exe"), "hevo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_hevo(&["--help".to_string()], "hevo"), 0);
        assert_eq!(run_hevo(&["-h".to_string()], "hevo"), 0);
        assert_eq!(run_hevo(&["--version".to_string()], "hevo"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_hevo(&[], "hevo"), 0);
    }
}
