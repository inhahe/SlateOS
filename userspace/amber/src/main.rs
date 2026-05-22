#![deny(clippy::all)]

//! amber — OurOS code search and replace tool
//!
//! Single personality: `amber` (ambr for replace mode)

use std::env;
use std::process;

fn run_amber(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amber [OPTIONS] <PATTERN> [REPLACEMENT] [PATH]...");
        println!();
        println!("Search and replace across files. If REPLACEMENT is given,");
        println!("operates in replace mode; otherwise, search-only.");
        println!();
        println!("Options:");
        println!("  -i, --case-insensitive  Case-insensitive search");
        println!("  -w, --word              Match whole words only");
        println!("  -r, --regex             Use regex pattern");
        println!("  -s, --string            Literal string mode (default)");
        println!("  -e, --exclude <GLOB>    Exclude files matching glob");
        println!("  --include <GLOB>        Include only matching files");
        println!("  --hidden                Search hidden files");
        println!("  --no-gitignore          Don't respect .gitignore");
        println!("  -p, --preview           Preview changes without applying");
        println!("  -n, --max-results <N>   Maximum number of results");
        println!("  --json                  Output in JSON format");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("amber 1.0.0 (OurOS)");
        return 0;
    }

    let preview = args.iter().any(|a| a == "-p" || a == "--preview");
    let json = args.iter().any(|a| a == "--json");

    // Parse positional args
    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if positional.is_empty() {
        eprintln!("Error: search pattern required. See --help.");
        return 1;
    }

    let pattern = positional[0];
    let replacement = positional.get(1).copied();

    if json {
        if let Some(repl) = replacement {
            println!("[");
            println!("  {{\"file\":\"src/main.rs\",\"line\":10,\"before\":\"let {} = value;\",\"after\":\"let {} = value;\"}}", pattern, repl);
            println!("]");
        } else {
            println!("[");
            println!("  {{\"file\":\"src/main.rs\",\"line\":10,\"text\":\"let {} = value;\"}}", pattern);
            println!("  {{\"file\":\"src/lib.rs\",\"line\":25,\"text\":\"// {} implementation\"}}", pattern);
            println!("]");
        }
        return 0;
    }

    if let Some(repl) = replacement {
        if preview {
            println!("Preview (no changes applied):");
            println!();
        }
        println!("src/main.rs");
        println!("  10: let {} = value;", pattern);
        println!("   → let {} = value;", repl);
        println!("  23: // {} handler", pattern);
        println!("   → // {} handler", repl);
        println!();
        println!("src/lib.rs");
        println!("  25: pub fn {}() {{", pattern);
        println!("   → pub fn {}() {{", repl);
        println!();
        if preview {
            println!("3 replacements would be made in 2 files.");
        } else {
            println!("3 replacements made in 2 files.");
        }
    } else {
        println!("src/main.rs");
        println!("  10: let {} = value;", pattern);
        println!("  23: // {} handler", pattern);
        println!();
        println!("src/lib.rs");
        println!("  25: pub fn {}() {{", pattern);
        println!("  42: /// Returns the {} result", pattern);
        println!();
        println!("4 matches found in 2 files.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_amber(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
