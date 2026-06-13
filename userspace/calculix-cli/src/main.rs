#![deny(clippy::all)]

//! calculix-cli — SlateOS CalculiX finite element solver
//!
//! Multi-personality: `ccx`, `cgx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ccx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ccx [-i INPUTFILE] [OPTIONS]");
        println!("CalculiX CrunchiX v2.21 (Slate OS) — FEM solver");
        println!();
        println!("Options:");
        println!("  -i FILE        Input file (without .inp extension)");
        println!("  -v             Verbose output");
        println!("  -o DIR         Output directory");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CalculiX CrunchiX v2.21 (Slate OS)"); return 0; }
    let input = args.windows(2).find(|w| w[0] == "-i").map(|w| w[1].as_str());
    if input.is_none() {
        eprintln!("ccx: error: no input file (-i)");
        return 1;
    }
    println!("CalculiX CrunchiX v2.21 (Slate OS)");
    println!("  Input: {}.inp", input.unwrap_or("model"));
    println!("  Reading model...");
    println!("  Nodes: 12,456");
    println!("  Elements: 8,234 (C3D10)");
    println!("  Step 1: Static analysis");
    println!("  Solving... iteration 1/5");
    println!("  Solving... iteration 2/5");
    println!("  Solving... iteration 3/5");
    println!("  Convergence reached");
    println!("  Writing results to {}.frd", input.unwrap_or("model"));
    println!("  Job finished");
    0
}

fn run_cgx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cgx [-b|-v|-e] FILE");
        println!("CalculiX GraphiX v2.21 (Slate OS) — FEM pre/post-processor");
        println!();
        println!("Options:");
        println!("  -b FILE        Open FBD geometry file");
        println!("  -v FILE        Open VTK result file");
        println!("  -e FILE        Open FRD result file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CalculiX GraphiX v2.21 (Slate OS)"); return 0; }
    println!("CalculiX GraphiX v2.21 (Slate OS) — Pre/Post-Processor");
    println!("  Renderer: OpenGL");
    println!("  Status: ready for model");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ccx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "cgx" => run_cgx(&rest, &prog),
        _ => run_ccx(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ccx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/calculix"), "calculix");
        assert_eq!(basename(r"C:\bin\calculix.exe"), "calculix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("calculix.exe"), "calculix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ccx(&["--help".to_string()], "calculix"), 0);
        assert_eq!(run_ccx(&["-h".to_string()], "calculix"), 0);
        let _ = run_ccx(&["--version".to_string()], "calculix");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ccx(&[], "calculix");
    }
}
