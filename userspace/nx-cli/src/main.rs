#![deny(clippy::all)]

//! nx-cli — SlateOS Siemens NX integrated CAD/CAM/CAE
//!
//! Single personality: `nx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nx [OPTIONS] [FILE]");
        println!("Siemens NX 2406 (SlateOS) — Integrated CAD/CAM/CAE (Unigraphics successor)");
        println!();
        println!("Options:");
        println!("  -part FILE             Open .prt part file");
        println!("  -grip SCRIPT           Run GRIP automation script");
        println!("  -nxopen SCRIPT         Run NXOpen Python/Java/C++/.NET script");
        println!("  -journal FILE          Replay journal");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Siemens NX 2406 (SlateOS)"); return 0; }
    println!("Siemens NX 2406 (SlateOS)");
    println!("  Industries: Automotive, aerospace, machinery, electronics");
    println!("  Modeling: Synchronous Technology (direct + parametric hybrid)");
    println!("  CAM: 2.5-5 axis milling, turning, multi-axis, additive");
    println!("  CAE: NX Nastran, NX CFD, multiphysics, motion");
    println!("  Convergent Modeling: facet + B-Rep + procedural in one model");
    println!("  Scripting: NXOpen (Python/Java/C++/.NET), GRIP, KF");
    println!("  PLM: Teamcenter integration");
    println!("  License: enterprise subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nx"), "nx");
        assert_eq!(basename(r"C:\bin\nx.exe"), "nx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nx.exe"), "nx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nx(&["--help".to_string()], "nx"), 0);
        assert_eq!(run_nx(&["-h".to_string()], "nx"), 0);
        let _ = run_nx(&["--version".to_string()], "nx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nx(&[], "nx");
    }
}
