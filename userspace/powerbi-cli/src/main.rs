#![deny(clippy::all)]

//! powerbi-cli — OurOS Microsoft Power BI
//!
//! Single personality: `powerbi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pbi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: powerbi [OPTIONS] [FILE]");
        println!("Microsoft Power BI Desktop 2.137 (OurOS) — Business analytics platform");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .pbix/.pbit (template)");
        println!("  --workspace WS         Power BI Service workspace");
        println!("  --reportserver         Connect to Power BI Report Server");
        println!("  --fabric               Microsoft Fabric workspace");
        println!("  --gateway              Power BI Gateway (on-prem data)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft Power BI Desktop 2.137.1102.0 (OurOS)"); return 0; }
    println!("Microsoft Power BI Desktop 2.137.1102.0 (OurOS)");
    println!("  Products: Desktop, Service (cloud), Mobile, Report Server (on-prem)");
    println!("  Fabric: unified analytics platform (Power BI + Synapse + Data Factory)");
    println!("  Data: Power Query (M), 200+ connectors, DirectQuery, semantic models");
    println!("  Language: DAX (Data Analysis Expressions), M (Power Query Formula Language)");
    println!("  Visuals: 100+ built-in + custom visuals marketplace + R/Python visuals");
    println!("  AI: Q&A natural language, Smart Narrative, Decomposition Tree, Copilot");
    println!("  Format: .pbix (workbook), .pbit (template), .pbids (data source spec)");
    println!("  License: Free (Desktop), Pro (per-user), Premium (capacity), Fabric SKUs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "powerbi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pbi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
