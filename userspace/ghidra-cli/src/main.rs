#![deny(clippy::all)]

//! ghidra-cli — Slate OS Ghidra reverse engineering suite
//!
//! Multi-personality: `ghidra`, `analyzeHeadless`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ghidra(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "analyzeHeadless" => {
                println!("analyzeHeadless (Slate OS) — Ghidra headless analyzer");
                println!("  analyzeHeadless PROJECT_DIR PROJECT_NAME");
                println!("  -import FILE       Import binary");
                println!("  -process FILE      Process existing program");
                println!("  -postScript SCRIPT Run script after analysis");
                println!("  -scriptPath DIR    Script search path");
                println!("  -deleteProject     Delete project after analysis");
                println!("  -overwrite         Overwrite existing program");
            }
            _ => {
                println!("Ghidra v11.0 (Slate OS) — Software Reverse Engineering Suite");
                println!("  --project DIR    Project directory");
                println!("  --import FILE    Import binary");
                println!("  --script FILE    Run Ghidra script");
                println!("  --nogui          Headless mode");
            }
        }
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ghidra v11.0.3 (Slate OS)"); return 0; }
    match prog {
        "analyzeHeadless" => {
            println!("Ghidra analyzeHeadless v11.0.3 (Slate OS)");
            println!("  Project: /tmp/ghidra_project");
            println!("  Importing: target.exe");
            println!("  Language: x86:LE:64:default");
            println!("  Auto-analysis running...");
            println!("    Disassembly: 45,678 functions");
            println!("    Decompilation: 45,678 functions");
            println!("    Data type analysis: complete");
            println!("    Reference analysis: 123,456 xrefs");
            println!("  Analysis complete in 4m 23s");
        }
        _ => {
            println!("Ghidra v11.0.3 (Slate OS)");
            println!("  CodeBrowser: loaded target.exe");
            println!("  Program: x86_64, PE32+, Windows");
            println!("  Functions: 45,678");
            println!("  Defined data: 12,345");
            println!("  Cross-references: 123,456");
            println!("  Decompiler: C output available");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ghidra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ghidra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ghidra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ghidra"), "ghidra");
        assert_eq!(basename(r"C:\bin\ghidra.exe"), "ghidra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ghidra.exe"), "ghidra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ghidra(&["--help".to_string()], "ghidra"), 0);
        assert_eq!(run_ghidra(&["-h".to_string()], "ghidra"), 0);
        let _ = run_ghidra(&["--version".to_string()], "ghidra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ghidra(&[], "ghidra");
    }
}
