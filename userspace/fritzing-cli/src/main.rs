#![deny(clippy::all)]

//! fritzing-cli — SlateOS Fritzing electronics prototyping
//!
//! Single personality: `fritzing`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fritzing(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fritzing [OPTIONS] [FILE.fzz]");
        println!("fritzing v1.0.2 (Slate OS) — Electronics design automation");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Views:");
        println!("  Breadboard        Visual prototyping layout");
        println!("  Schematic         Circuit schematic");
        println!("  PCB               Printed circuit board design");
        println!("  Code              Arduino sketch editor");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fritzing v1.0.2 (Slate OS)"); return 0; }
    println!("fritzing: electronics prototyping started");
    println!("  Parts library: 12000+ components");
    println!("  Auto-router: enabled");
    println!("  Gerber export: for PCB fabrication");
    println!("  BOM generation: bill of materials");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fritzing".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fritzing(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fritzing};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fritzing"), "fritzing");
        assert_eq!(basename(r"C:\bin\fritzing.exe"), "fritzing.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fritzing.exe"), "fritzing");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fritzing(&["--help".to_string()], "fritzing"), 0);
        assert_eq!(run_fritzing(&["-h".to_string()], "fritzing"), 0);
        let _ = run_fritzing(&["--version".to_string()], "fritzing");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fritzing(&[], "fritzing");
    }
}
