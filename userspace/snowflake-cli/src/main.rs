#![deny(clippy::all)]

//! snowflake-cli — OurOS Snowflake cloud data platform
//!
//! Single personality: `snowflake`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: snowflake [OPTIONS] [SUBCMD]");
        println!("Snowflake Data Cloud (OurOS) — Cloud data warehouse + lakehouse + AI");
        println!();
        println!("Options:");
        println!("  --account ACCT         Account locator");
        println!("  --user USER            Username");
        println!("  --warehouse WH         Compute warehouse");
        println!("  --database DB          Database context");
        println!("  sql -q \"QUERY\"        Run SQL query");
        println!("  --snowsight            Open Snowsight web UI");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Snowflake CLI 2.7.0 / Account 8.27 (OurOS)"); return 0; }
    println!("Snowflake Data Cloud (OurOS)");
    println!("  Architecture: separated compute/storage; multi-cluster shared data");
    println!("  Languages: SQL (ANSI), Snowpark (Python/Java/Scala), JavaScript UDFs");
    println!("  Clouds: AWS, Azure, GCP — cross-cloud replication");
    println!("  Features: Time Travel, Zero-Copy Cloning, Data Sharing (Snowflake Marketplace)");
    println!("  AI/ML: Cortex (LLM functions, vector search, ML.FORECAST/ANOMALY)");
    println!("  Streaming: Snowpipe, dynamic tables, Kafka connector");
    println!("  Apps: Native Apps Framework, Streamlit-in-Snowflake, container services");
    println!("  License: pay-as-you-go (credits) by warehouse size + storage");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "snowflake".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/snowflake"), "snowflake");
        assert_eq!(basename(r"C:\bin\snowflake.exe"), "snowflake.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("snowflake.exe"), "snowflake");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sf(&["--help".to_string()], "snowflake"), 0);
        assert_eq!(run_sf(&["-h".to_string()], "snowflake"), 0);
        let _ = run_sf(&["--version".to_string()], "snowflake");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sf(&[], "snowflake");
    }
}
