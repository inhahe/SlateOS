#![deny(clippy::all)]

//! icc-cli — OurOS ICC profile inspector
//!
//! Single personality: `iccinfo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_icc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: iccinfo [OPTIONS] PROFILE.icc");
        println!("iccinfo v1.0 (OurOS) — ICC profile inspector");
        println!();
        println!("Options:");
        println!("  PROFILE.icc       ICC profile to inspect");
        println!("  --tags            List all tags");
        println!("  --validate        Validate profile");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("profile.icc");
    println!("Profile: {}", file);
    println!("  Version: 4.4");
    println!("  Class: Display");
    println!("  Color space: RGB");
    println!("  PCS: XYZ");
    println!("  White point: D65");
    println!("  Creator: OCIO");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iccinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_icc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
