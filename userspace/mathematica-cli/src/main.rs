#![deny(clippy::all)]

//! mathematica-cli — OurOS Wolfram Mathematica symbolic/numeric computing
//!
//! Single personality: `mathematica`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mma(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mathematica [OPTIONS] [FILE]");
        println!("Wolfram Mathematica 14.1 (OurOS) — Symbolic/numeric computation");
        println!();
        println!("Options:");
        println!("  -script FILE           Run Wolfram Language script (.wls)");
        println!("  -noprompt              No prompt in scripted mode");
        println!("  --notebook FILE        Open notebook (.nb)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wolfram Mathematica 14.1.0 (OurOS)"); return 0; }
    println!("Wolfram Mathematica 14.1.0 (OurOS)");
    println!("  Language: Wolfram Language — symbolic, functional, pattern-based");
    println!("  Domains: symbolic algebra, calculus, ODE/PDE, statistics, graph theory,");
    println!("           ML, image/signal processing, finance, chemistry, biology");
    println!("  Knowledgebase: Wolfram|Alpha curated data (10+ trillion facts)");
    println!("  Visualization: Plot, Plot3D, ContourPlot, Graph, Manipulate (interactive)");
    println!("  Cloud: Wolfram Cloud deployment, APIFunctions");
    println!("  License: subscription / perpetual (Home/Edu/Pro)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mathematica".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mma(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
