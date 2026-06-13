#![deny(clippy::all)]

//! diptrace-cli — SlateOS Novarm DipTrace PCB EDA
//!
//! Single personality: `diptrace`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: diptrace [OPTIONS] [FILE]");
        println!("Novarm DipTrace 5.0 (Slate OS) — Affordable schematic + PCB EDA");
        println!();
        println!("Options:");
        println!("  --schematic FILE       Open .dch schematic");
        println!("  --pcb FILE             Open .dip PCB layout");
        println!("  --component-editor     Launch Component Editor");
        println!("  --pattern-editor       Launch Pattern Editor");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Novarm DipTrace 5.0 (Slate OS)"); return 0; }
    println!("Novarm DipTrace 5.0 (Slate OS)");
    println!("  Tools: Schematic, PCB Layout, Component Editor, Pattern Editor, 3D Viewer");
    println!("  Format: .dch (schematic), .dip (PCB) + DXF, IDF, STEP, Gerber, IPC-2581");
    println!("  Routing: ShapeRouter (autorouter) + manual with rule checking");
    println!("  Editions: Free (300-pin/2-layer), Starter, Lite, Standard, Extended, Full");
    println!("  3D: real-time 3D PCB preview, STEP export to MCAD");
    println!("  Beginner-friendly: hobbyist-priced, low learning curve");
    println!("  License: perpetual (one-time, no maintenance required)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "diptrace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/diptrace"), "diptrace");
        assert_eq!(basename(r"C:\bin\diptrace.exe"), "diptrace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("diptrace.exe"), "diptrace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dt(&["--help".to_string()], "diptrace"), 0);
        assert_eq!(run_dt(&["-h".to_string()], "diptrace"), 0);
        let _ = run_dt(&["--version".to_string()], "diptrace");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dt(&[], "diptrace");
    }
}
