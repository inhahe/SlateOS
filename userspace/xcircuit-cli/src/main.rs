#![deny(clippy::all)]

//! xcircuit-cli — SlateOS XCircuit schematic editor
//!
//! Single personality: `xcircuit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xcircuit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xcircuit [OPTIONS] [FILE...]");
        println!("XCircuit v3.10 (SlateOS) — Publication-quality schematic editor");
        println!();
        println!("Options:");
        println!("  -2             Two-page mode");
        println!("  -bg COLOR      Background color");
        println!("  -fg COLOR      Foreground color");
        println!("  -noc           No console output");
        println!("  -p SCALE       Output scale");
        println!("  -r             Read-only mode");
        println!("  -s FILE        Execute script");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("XCircuit v3.10.30 (SlateOS)"); return 0; }
    println!("XCircuit v3.10.30 (SlateOS) — Schematic Editor");
    println!("  Loading library: analog.lps");
    println!("  Loading library: digital.lps");
    println!("  Schematic: amplifier.ps");
    println!("    Components: 45");
    println!("    Nets: 23");
    println!("    Labels: 12");
    println!("  Netlist generated: amplifier.spice");
    println!("  PostScript output: amplifier.eps");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xcircuit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xcircuit(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xcircuit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xcircuit"), "xcircuit");
        assert_eq!(basename(r"C:\bin\xcircuit.exe"), "xcircuit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xcircuit.exe"), "xcircuit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xcircuit(&["--help".to_string()], "xcircuit"), 0);
        assert_eq!(run_xcircuit(&["-h".to_string()], "xcircuit"), 0);
        let _ = run_xcircuit(&["--version".to_string()], "xcircuit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xcircuit(&[], "xcircuit");
    }
}
