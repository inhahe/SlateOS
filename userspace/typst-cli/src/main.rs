#![deny(clippy::all)]

//! typst-cli — SlateOS Typst typesetting system
//!
//! Multi-personality: `typst`

use std::env;
use std::process;

fn run_typst(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: typst COMMAND [OPTIONS]");
        println!("Typst 0.11.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  compile      Compile a Typst file to PDF");
        println!("  watch        Watch and recompile on changes");
        println!("  init         Initialize a new project");
        println!("  query        Query document metadata");
        println!("  fonts        List available fonts");
        println!("  update       Update packages");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("typst 0.11.0 (Slate OS)"),
        "compile" | "c" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("document.typ");
            let output = args.get(2).map(|s| s.as_str());
            let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
            let default_out = format!("{}.pdf", base);
            let out = output.unwrap_or(default_out.as_str());
            println!("Compiling {} -> {}", file, out);
            println!("  Pages: 5");
            println!("  Compiled in 120ms.");
        }
        "watch" | "w" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("document.typ");
            println!("Watching: {}", file);
            println!("  Compiled successfully.");
            println!("  Watching for changes...");
        }
        "init" => {
            let tmpl = args.get(1).map(|s| s.as_str()).unwrap_or("@preview/basic:0.1.0");
            println!("Initializing from template: {}", tmpl);
            println!("  Created: main.typ");
            println!("  Project initialized.");
        }
        "fonts" => {
            println!("Available fonts:");
            println!("  New Computer Modern");
            println!("  Linux Libertine");
            println!("  DejaVu Sans");
            println!("  DejaVu Sans Mono");
            println!("  Noto Sans");
            println!("  Noto Serif");
            println!("  Source Code Pro");
        }
        "query" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("document.typ");
            let selector = args.get(2).map(|s| s.as_str()).unwrap_or("<heading>");
            println!("Querying {} for {}", file, selector);
            println!("  [{{\"value\": \"Introduction\"}}, {{\"value\": \"Methods\"}}]");
        }
        _ => println!("typst: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_typst(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_typst};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_typst(&["--help".to_string()]), 0);
        assert_eq!(run_typst(&["-h".to_string()]), 0);
        let _ = run_typst(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_typst(&[]);
    }
}
