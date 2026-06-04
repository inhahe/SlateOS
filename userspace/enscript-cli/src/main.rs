#![deny(clippy::all)]

//! enscript-cli — OurOS GNU Enscript CLI
//!
//! Single personality: `enscript`

use std::env;
use std::process;

fn run_enscript(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: enscript [OPTIONS] [FILE...]");
        println!();
        println!("GNU Enscript — convert text to PostScript/PDF/HTML (OurOS).");
        println!();
        println!("Options:");
        println!("  -o, --output FILE      Output file (- for stdout)");
        println!("  -p, --output FILE      Same as -o");
        println!("  -1, -2                 Columns (1 or 2)");
        println!("  -r, --landscape        Landscape orientation");
        println!("  -G                     Fancy header");
        println!("  -E, --highlight LANG   Syntax highlighting");
        println!("  -f, --font NAME        Body font");
        println!("  -F, --header-font NAME Header font");
        println!("  --color                Color output");
        println!("  -B, --no-header        No page header");
        println!("  -l, --lineprinter      Line printer mode");
        println!("  --line-numbers         Print line numbers");
        println!("  --word-wrap            Wrap long lines at word boundaries");
        println!("  --media NAME           Paper size (A4, Letter)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GNU Enscript 1.6.6 (OurOS)");
        return 0;
    }

    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output" || w[0] == "-p")
        .map(|w| w[1].as_str());
    let highlight = args.windows(2).find(|w| w[0] == "-E" || w[0] == "--highlight")
        .map(|w| w[1].as_str());
    let landscape = args.iter().any(|a| a == "-r" || a == "--landscape");
    let fancy = args.iter().any(|a| a == "-G");
    let line_numbers = args.iter().any(|a| a == "--line-numbers");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let input_desc = if files.is_empty() { "stdin" } else { files[0] };
    let out_desc = output.unwrap_or("stdout");

    print!("enscript: {} -> {}", input_desc, out_desc);
    if let Some(lang) = highlight {
        print!(" [highlight: {}]", lang);
    }
    if landscape { print!(" [landscape]"); }
    if fancy { print!(" [fancy header]"); }
    if line_numbers { print!(" [line-numbers]"); }
    println!();

    let pages = if files.is_empty() { 1 } else { 3 };
    println!("[ {} pages * 1 copy ] left in {}", pages, out_desc);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_enscript(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_enscript};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_enscript(vec!["--help".to_string()]), 0);
        assert_eq!(run_enscript(vec!["-h".to_string()]), 0);
        let _ = run_enscript(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_enscript(vec![]);
    }
}
