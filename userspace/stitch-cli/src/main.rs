#![deny(clippy::all)]

//! stitch-cli — OurOS Stitch data loader
//!
//! Single personality: `stitch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stitch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stitch [COMMAND] [OPTIONS]");
        println!("Stitch v4.0 (OurOS) — Simple data pipeline / ELT");
        println!();
        println!("Commands:");
        println!("  source list|create|check     Manage sources");
        println!("  destination list|create       Manage destinations");
        println!("  replication list|start|pause  Manage replications");
        println!("  extraction list|logs          Extraction history");
        println!("  loading list|logs             Loading history");
        println!("  notification list|create      Alert notifications");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API access token");
        println!("  --account-id ID    Account ID");
        println!("  --format json|csv  Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Stitch v4.0.0 (OurOS)"); return 0; }
    println!("Stitch v4.0.0 (OurOS)");
    println!("  Sources: 12 connected");
    println!("  Destinations: 2 (Snowflake, BigQuery)");
    println!("  Replications: 12 active");
    println!("  Rows loaded: 45.6M (last 24h)");
    println!("  Frequency: every 30min");
    println!("  Alerts: 0 active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stitch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stitch(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stitch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stitch"), "stitch");
        assert_eq!(basename(r"C:\bin\stitch.exe"), "stitch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stitch.exe"), "stitch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stitch(&["--help".to_string()], "stitch"), 0);
        assert_eq!(run_stitch(&["-h".to_string()], "stitch"), 0);
        let _ = run_stitch(&["--version".to_string()], "stitch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stitch(&[], "stitch");
    }
}
