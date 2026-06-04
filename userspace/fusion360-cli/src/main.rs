#![deny(clippy::all)]

//! fusion360-cli — OurOS Autodesk Fusion 360 cloud CAD/CAM/CAE
//!
//! Single personality: `fusion360`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_f360(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fusion360 [OPTIONS] [FILE]");
        println!("Autodesk Fusion 360 (OurOS) — Integrated cloud CAD/CAM/CAE/PCB");
        println!();
        println!("Options:");
        println!("  --open FILE            Open design or project URL");
        println!("  --script FILE          Run Python add-in/script");
        println!("  --headless             Run without UI");
        println!("  --workspace WS         Switch workspace (Design/Render/Animation/Simulation/Manufacture/Drawing)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk Fusion 360 v2.0.20294 (OurOS)"); return 0; }
    println!("Autodesk Fusion 360 v2.0.20294 (OurOS)");
    println!("  Workspaces: Design, Render, Animation, Simulation, Manufacture, Drawing");
    println!("  Cloud: Auto-saves to Autodesk cloud, version history, sharing");
    println!("  Scripting: Python (asyncio), C++ Add-in SDK");
    println!("  Generative Design: cloud-powered optimization");
    println!("  CAM: 2.5-5 axis milling, turning, additive/subtractive");
    println!("  PCB: Integrated EAGLE-derived PCB design");
    println!("  License: Free for personal, subscription for commercial");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fusion360".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_f360(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_f360};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fusion360"), "fusion360");
        assert_eq!(basename(r"C:\bin\fusion360.exe"), "fusion360.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fusion360.exe"), "fusion360");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_f360(&["--help".to_string()], "fusion360"), 0);
        assert_eq!(run_f360(&["-h".to_string()], "fusion360"), 0);
        let _ = run_f360(&["--version".to_string()], "fusion360");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_f360(&[], "fusion360");
    }
}
