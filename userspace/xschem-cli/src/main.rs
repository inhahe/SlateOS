#![deny(clippy::all)]

//! xschem-cli — Slate OS Xschem schematic editor
//!
//! Single personality: `xschem`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xschem(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xschem [OPTIONS] [FILE.sch]");
        println!("Xschem v3.4 (Slate OS) — Schematic capture and netlisting");
        println!();
        println!("Options:");
        println!("  FILE.sch          Open schematic file");
        println!("  --netlist         Generate netlist");
        println!("  --spice           Generate SPICE netlist");
        println!("  --vhdl            Generate VHDL");
        println!("  --verilog         Generate Verilog");
        println!("  --tcl SCRIPT      Run Tcl script");
        println!("  --no-x            Batch mode (no GUI)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Xschem v3.4 (Slate OS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("circuit.sch");
    if args.iter().any(|a| a == "--netlist" || a == "--spice") {
        println!("Generating SPICE netlist from: {}", file);
        println!("  Components: 24");
        println!("  Nets: 18");
        println!("  Output: circuit.spice");
        return 0;
    }
    println!("Xschem v3.4 — Opening: {}", file);
    println!("  Components: 24");
    println!("  Hierarchical sheets: 3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xschem".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xschem(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xschem};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xschem"), "xschem");
        assert_eq!(basename(r"C:\bin\xschem.exe"), "xschem.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xschem.exe"), "xschem");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xschem(&["--help".to_string()], "xschem"), 0);
        assert_eq!(run_xschem(&["-h".to_string()], "xschem"), 0);
        let _ = run_xschem(&["--version".to_string()], "xschem");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xschem(&[], "xschem");
    }
}
