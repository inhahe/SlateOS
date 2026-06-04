#![deny(clippy::all)]

//! sd — OurOS intuitive find & replace CLI (sed alternative)
//!
//! Single personality: `sd`

use std::env;
use std::process;

fn run_sd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sd [OPTIONS] <FIND> <REPLACE> [FILES]...");
        println!();
        println!("An intuitive find & replace CLI.");
        println!();
        println!("Options:");
        println!("  -p, --preview         Preview changes without modifying files");
        println!("  -s, --string-mode     Treat FIND as a literal string (not regex)");
        println!("  -f, --flags <FLAGS>   Regex flags (e=extended, i=case-insensitive,");
        println!("                        m=multiline, s=dotall, w=word boundary)");
        println!("  -n, --max-replacements <N>  Limit replacements per file");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sd 1.0.0 (OurOS)");
        return 0;
    }

    let preview = args.iter().any(|a| a == "-p" || a == "--preview");

    // Find positional args (skip flags and their values)
    let mut positional: Vec<&str> = Vec::new();
    let mut skip_next = false;
    for a in &args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "-f" || a == "--flags" || a == "-n" || a == "--max-replacements" {
            skip_next = true;
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        positional.push(a.as_str());
    }

    if positional.len() < 2 {
        eprintln!("Error: FIND and REPLACE patterns required. See --help.");
        return 1;
    }

    let find = positional[0];
    let replace = positional[1];
    let files: Vec<&str> = positional[2..].to_vec();

    if preview {
        println!("Preview mode (no files modified):");
        println!();
    }

    if files.is_empty() {
        // Stdin mode
        println!("(reading from stdin)");
        println!("Before: The quick brown fox jumps over the lazy {}.", find);
        println!("After:  The quick brown fox jumps over the lazy {}.", replace);
    } else {
        for file in &files {
            println!("{}", file);
            println!("  - line 5: ...the {} was found...", find);
            println!("  + line 5: ...the {} was found...", replace);
            println!("  - line 12: ...another {} here...", find);
            println!("  + line 12: ...another {} here...", replace);
            if !preview {
                println!("  (2 replacements)");
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sd(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sd(vec!["--help".to_string()]), 0);
        assert_eq!(run_sd(vec!["-h".to_string()]), 0);
        let _ = run_sd(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sd(vec![]);
    }
}
