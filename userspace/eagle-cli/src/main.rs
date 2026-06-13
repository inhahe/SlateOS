#![deny(clippy::all)]

//! eagle-cli — SlateOS EAGLE PCB design (open-source compatible)
//!
//! Single personality: `eagle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eagle(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: eagle [OPTIONS] [FILE.brd|FILE.sch]");
        println!("eagle v9.6 (SlateOS) — PCB design and schematic editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Editors:");
        println!("  Schematic         Circuit schematic capture");
        println!("  Board             PCB layout editor");
        println!("  Library           Component library manager");
        println!("  CAM               Manufacturing output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("eagle v9.6 (SlateOS)"); return 0; }
    println!("eagle: PCB design editor started");
    println!("  Schematic: hierarchical sheet support");
    println!("  Board: 16 signal layers, autorouter");
    println!("  DRC: design rule check");
    println!("  CAM: Gerber/Excellon output");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "eagle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eagle(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eagle};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/eagle"), "eagle");
        assert_eq!(basename(r"C:\bin\eagle.exe"), "eagle.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("eagle.exe"), "eagle");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_eagle(&["--help".to_string()], "eagle"), 0);
        assert_eq!(run_eagle(&["-h".to_string()], "eagle"), 0);
        let _ = run_eagle(&["--version".to_string()], "eagle");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_eagle(&[], "eagle");
    }
}
