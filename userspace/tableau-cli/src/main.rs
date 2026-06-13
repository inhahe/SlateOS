#![deny(clippy::all)]

//! tableau-cli — SlateOS Salesforce Tableau data visualization
//!
//! Single personality: `tableau`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tableau [OPTIONS] [FILE]");
        println!("Salesforce Tableau Desktop 2024.3 (Slate OS) — Data visualization");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .twb/.twbx/.hyper");
        println!("  --server URL           Connect to Tableau Server / Cloud");
        println!("  --prep                 Tableau Prep Builder");
        println!("  --bridge               Tableau Bridge (private network connector)");
        println!("  --tabcmd CMD           Server admin tabcmd command");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Salesforce Tableau Desktop 2024.3.0 (Slate OS)"); return 0; }
    println!("Salesforce Tableau Desktop 2024.3.0 (Slate OS)");
    println!("  Products: Desktop, Prep, Server, Cloud (was Online), Public, Mobile");
    println!("  Data sources: 80+ native connectors (DBs, files, cloud apps, REST APIs)");
    println!("  Storage: Hyper (columnar in-memory engine) — fast extracts");
    println!("  Workbooks: .twb (XML) / .twbx (packaged with extracts)");
    println!("  Languages: VizQL (visual query), Calc formulas, Table Calcs, LOD expressions");
    println!("  AI: Tableau Pulse, Einstein Discovery, Ask Data, Explain Data");
    println!("  Extensions: REST API, Hyper API, Tableau JavaScript API, Extensions API");
    println!("  License: Creator (full), Explorer, Viewer — per-user subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tableau".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tab(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tableau"), "tableau");
        assert_eq!(basename(r"C:\bin\tableau.exe"), "tableau.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tableau.exe"), "tableau");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tab(&["--help".to_string()], "tableau"), 0);
        assert_eq!(run_tab(&["-h".to_string()], "tableau"), 0);
        let _ = run_tab(&["--version".to_string()], "tableau");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tab(&[], "tableau");
    }
}
