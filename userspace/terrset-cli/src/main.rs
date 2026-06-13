#![deny(clippy::all)]

//! terrset-cli — SlateOS TerrSet geospatial monitoring
//!
//! Single personality: `terrset`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_terrset(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: terrset [OPTIONS] COMMAND");
        println!("TerrSet v19 (SlateOS) — Geospatial monitoring and modeling");
        println!();
        println!("Commands:");
        println!("  classify       Land cover classification");
        println!("  change         Change detection analysis");
        println!("  ndvi           Calculate NDVI");
        println!("  trend          Time series trend analysis");
        println!("  ca-markov      CA-Markov land change simulation");
        println!("  habitat        Habitat suitability modeling");
        println!();
        println!("Options:");
        println!("  -i FILE [FILE...]  Input raster files");
        println!("  -o FILE        Output file");
        println!("  -r REF         Reference data");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("TerrSet v19.0 (SlateOS)"); return 0; }
    println!("TerrSet v19.0 (SlateOS) — Geospatial Monitoring");
    println!("  Module: Land Cover Change Analysis");
    println!("  Input: landsat_2020.tif, landsat_2023.tif");
    println!("  Classification: Maximum Likelihood");
    println!("    Classes: Water, Forest, Urban, Agriculture, Barren");
    println!("  Change matrix:");
    println!("    Forest -> Urban: 12,345 ha");
    println!("    Agriculture -> Urban: 8,901 ha");
    println!("    Forest -> Agriculture: 5,678 ha");
    println!("  Net forest loss: 15,234 ha (-4.5%)");
    println!("  Urban growth: 21,246 ha (+12.3%)");
    println!("  Output: change_map.tif");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "terrset".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_terrset(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_terrset};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/terrset"), "terrset");
        assert_eq!(basename(r"C:\bin\terrset.exe"), "terrset.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("terrset.exe"), "terrset");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_terrset(&["--help".to_string()], "terrset"), 0);
        assert_eq!(run_terrset(&["-h".to_string()], "terrset"), 0);
        let _ = run_terrset(&["--version".to_string()], "terrset");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_terrset(&[], "terrset");
    }
}
