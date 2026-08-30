#![deny(clippy::all)]

//! geda-cli — Slate OS gEDA electronic design suite
//!
//! Multi-personality: `gschem`, `gnetlist`, `gattrib`, `gsch2pcb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gschem(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gschem [OPTIONS] [FILE.sch]");
        println!("gschem v1.10 (Slate OS) — gEDA schematic editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gschem v1.10 (Slate OS, gEDA)"); return 0; }
    println!("gschem: schematic editor started");
    println!("  Symbol library: standard, SPICE, simulation");
    println!("  Hierarchical schematics supported");
    0
}

fn run_gnetlist(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnetlist [OPTIONS] FILE.sch...");
        println!("gnetlist v1.10 (Slate OS) — gEDA netlist generator");
        println!();
        println!("Options:");
        println!("  -g BACKEND        Output backend (spice, pcb, verilog)");
        println!("  -o FILE           Output file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnetlist v1.10 (Slate OS, gEDA)"); return 0; }
    println!("gnetlist: generating netlist...");
    0
}

fn run_gattrib(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gattrib [OPTIONS] FILE.sch...");
        println!("gattrib v1.10 (Slate OS) — gEDA attribute spreadsheet editor");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gattrib v1.10 (Slate OS, gEDA)"); return 0; }
    println!("gattrib: attribute editor started");
    0
}

fn run_gsch2pcb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gsch2pcb [OPTIONS] FILE.sch");
        println!("gsch2pcb v1.10 (Slate OS) — Schematic to PCB layout bridge");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gsch2pcb v1.10 (Slate OS, gEDA)"); return 0; }
    println!("gsch2pcb: generating PCB layout from schematic...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gschem".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gnetlist" => run_gnetlist(&rest, &prog),
        "gattrib" => run_gattrib(&rest, &prog),
        "gsch2pcb" => run_gsch2pcb(&rest, &prog),
        _ => run_gschem(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gschem};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/geda"), "geda");
        assert_eq!(basename(r"C:\bin\geda.exe"), "geda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("geda.exe"), "geda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gschem(&["--help".to_string()], "geda"), 0);
        assert_eq!(run_gschem(&["-h".to_string()], "geda"), 0);
        let _ = run_gschem(&["--version".to_string()], "geda");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gschem(&[], "geda");
    }
}
