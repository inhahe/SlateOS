#![deny(clippy::all)]

//! easyeda-cli — OurOS JLC EasyEDA cloud schematic + PCB
//!
//! Single personality: `easyeda`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_easyeda(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: easyeda [OPTIONS] [URL]");
        println!("JLC EasyEDA Pro v2 (OurOS) — Cloud-based PCB design");
        println!();
        println!("Options:");
        println!("  --project URL          Open project URL");
        println!("  --jlcpcb               Order PCB via JLCPCB");
        println!("  --lcsc-import PART     Import LCSC library part");
        println!("  --offline              Use offline edition");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("JLC EasyEDA Pro v2.2.32 (OurOS)"); return 0; }
    println!("JLC EasyEDA Pro v2.2.32 (OurOS)");
    println!("  Architecture: Cloud (web) or Desktop (Pro/Std editions)");
    println!("  Library: 1M+ LCSC parts pre-loaded + community library (3M+ parts)");
    println!("  Manufacturing: One-click JLCPCB fab order + JLCPCB SMT assembly");
    println!("  Simulation: integrated SPICE simulator");
    println!("  Format: .epro/.json (native) + Altium/Eagle/KiCad import");
    println!("  Collaboration: real-time multi-user editing (cloud)");
    println!("  License: Free (most features), Pro subscription for advanced");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "easyeda".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_easyeda(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_easyeda};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/easyeda"), "easyeda");
        assert_eq!(basename(r"C:\bin\easyeda.exe"), "easyeda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("easyeda.exe"), "easyeda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_easyeda(&["--help".to_string()], "easyeda"), 0);
        assert_eq!(run_easyeda(&["-h".to_string()], "easyeda"), 0);
        let _ = run_easyeda(&["--version".to_string()], "easyeda");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_easyeda(&[], "easyeda");
    }
}
