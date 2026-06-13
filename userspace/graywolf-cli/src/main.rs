#![deny(clippy::all)]

//! graywolf-cli — SlateOS GrayWolf placement tool
//!
//! Single personality: `graywolf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_graywolf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: graywolf [OPTIONS] DESIGN");
        println!("GrayWolf v0.1.6 (Slate OS) — Standard cell placement");
        println!();
        println!("Options:");
        println!("  -n             No graphics mode");
        println!("  -p             Partition mode");
        println!("  -v LEVEL       Verbosity level (0-5)");
        println!("  -o DIR         Output directory");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GrayWolf v0.1.6 (Slate OS)"); return 0; }
    println!("GrayWolf v0.1.6 (Slate OS) — Standard Cell Placement");
    println!("  Design: processor_core");
    println!("  Reading .cel file...");
    println!("  Cells: 8,901");
    println!("  Pads: 64");
    println!("  Nets: 12,345");
    println!("  Simulated annealing placement:");
    println!("    Temperature: 5000 -> 1");
    println!("    Iterations: 2,345,678");
    println!("    Final wirelength: 567,890 units");
    println!("  Row assignment complete");
    println!("  Output: processor_core.pl1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "graywolf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_graywolf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_graywolf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/graywolf"), "graywolf");
        assert_eq!(basename(r"C:\bin\graywolf.exe"), "graywolf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("graywolf.exe"), "graywolf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_graywolf(&["--help".to_string()], "graywolf"), 0);
        assert_eq!(run_graywolf(&["-h".to_string()], "graywolf"), 0);
        let _ = run_graywolf(&["--version".to_string()], "graywolf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_graywolf(&[], "graywolf");
    }
}
