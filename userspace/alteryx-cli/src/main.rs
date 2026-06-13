#![deny(clippy::all)]

//! alteryx-cli — SlateOS Alteryx Analytics automation
//!
//! Single personality: `alteryx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_aterix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alteryx [OPTIONS] [FILE]");
        println!("Alteryx Designer 2024.2 (SlateOS) — Self-service analytics automation");
        println!();
        println!("Options:");
        println!("  --open FILE            Open workflow (.yxmd / .yxwz)");
        println!("  --runworkflow FILE     Run workflow headlessly");
        println!("  --server URL           Alteryx Server (formerly Gallery)");
        println!("  --auto-insights        Auto Insights AI exploration");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Alteryx Designer 2024.2.1.4 (SlateOS)"); return 0; }
    println!("Alteryx Designer 2024.2.1.4 (SlateOS)");
    println!("  Products: Designer, Server, AAS (Auto Insights / formerly ClearStory)");
    println!("  Workflow: drag-drop data prep + blending + analytics + automation");
    println!("  Tools: 250+ built-in tools (filter, join, transform, predictive, spatial)");
    println!("  Languages: built-in formulas, R, Python (Code Tool), SQL pushdown");
    println!("  Format: .yxmd (workflow), .yxwz (wizard), .yxmc (macro)");
    println!("  Predictive: regression, decision trees, clustering, time series, AutoML");
    println!("  Spatial: geocoding, drive-time, routing, trade area analytics");
    println!("  Integration: SAP, Salesforce, Marketo, Workday, Snowflake, Databricks");
    println!("  License: Designer seat + Server capacity (private equity owned 2024)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "alteryx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aterix(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aterix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/alteryx"), "alteryx");
        assert_eq!(basename(r"C:\bin\alteryx.exe"), "alteryx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("alteryx.exe"), "alteryx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_aterix(&["--help".to_string()], "alteryx"), 0);
        assert_eq!(run_aterix(&["-h".to_string()], "alteryx"), 0);
        let _ = run_aterix(&["--version".to_string()], "alteryx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_aterix(&[], "alteryx");
    }
}
