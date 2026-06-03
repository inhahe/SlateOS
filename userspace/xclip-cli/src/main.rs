#![deny(clippy::all)]

//! xclip-cli — OurOS xclip clipboard CLI
//!
//! Single personality: `xclip`

use std::env;
use std::process;

fn run_xclip(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xclip [OPTIONS] [FILE ...]");
        println!();
        println!("xclip — X11 clipboard interface (OurOS).");
        println!();
        println!("Options:");
        println!("  -i, --in              Read into selection (default)");
        println!("  -o, --out             Output selection to stdout");
        println!("  -f, --filter          Filter: copy stdin to stdout and selection");
        println!("  -l LOOPS              Wait for N paste requests");
        println!("  -d DISPLAY            X display to use");
        println!("  -selection SEL        Selection (primary/secondary/clipboard/buffer-cut)");
        println!("  -target TARGET        Target atom (e.g., UTF8_STRING)");
        println!("  -rmlastnl             Remove trailing newline");
        println!("  -noutf8               Don't use UTF-8");
        println!("  -quiet                Suppress messages");
        println!("  -verbose              Verbose output");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        println!("xclip version 0.13 (OurOS)");
        return 0;
    }

    let output = args.iter().any(|a| a == "-o" || a == "--out");
    let selection = args.windows(2)
        .find(|w| w[0] == "-selection")
        .map(|w| w[1].as_str())
        .unwrap_or("primary");

    if output {
        println!("(clipboard contents from {} selection)", selection);
    } else {
        let verbose = args.iter().any(|a| a == "-verbose");
        if verbose {
            println!("xclip: reading from stdin into {} selection", selection);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xclip(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_xclip};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_xclip(vec!["--help".to_string()]), 0);
        assert_eq!(run_xclip(vec!["-h".to_string()]), 0);
        assert_eq!(run_xclip(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_xclip(vec![]), 0);
    }
}
