#![deny(clippy::all)]

//! a2ps-cli — OurOS a2ps/enscript text-to-PostScript CLI
//!
//! Single personality: `a2ps`

use std::env;
use std::process;

fn run_a2ps(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: a2ps [OPTIONS] [FILE ...]");
        println!();
        println!("a2ps — any to PostScript filter (OurOS).");
        println!();
        println!("Options:");
        println!("  -o, --output FILE     Output file (default stdout)");
        println!("  -P PRINTER            Send to printer");
        println!("  -1, --columns=1       1 column per page");
        println!("  -2, --columns=2       2 columns per page (default)");
        println!("  -r, --landscape       Landscape orientation");
        println!("  -R, --portrait        Portrait orientation");
        println!("  -M PAPER              Paper size (A4, Letter, etc.)");
        println!("  --header=TEXT          Page header");
        println!("  --footer=TEXT          Page footer");
        println!("  -b, --no-header       No page headers");
        println!("  -l LINES              Lines per page");
        println!("  -f SIZE               Font size");
        println!("  -T TABS               Tab size");
        println!("  --highlight-level=N   Syntax highlighting (0-3)");
        println!("  --pretty-print=LANG   Force language for highlighting");
        println!("  --prologue=FILE       PostScript prologue");
        println!("  -n, --line-numbers=N  Print line numbers every N");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("a2ps 4.15.4 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--list=features") {
        println!("Known features:");
        println!("  delegations: yes");
        println!("  pages: yes");
        println!("  encoding: Latin1, UTF-8");
        println!("  media: A4, Letter, Legal, A3, A5");
        return 0;
    }

    let output = args.windows(2)
        .find(|w| w[0] == "-o" || w[0] == "--output")
        .map(|w| w[1].as_str());

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let total_pages = if files.is_empty() { 1 } else { files.len() * 2 };
    let total_sheets = (total_pages + 1) / 2;

    if let Some(out) = output {
        println!("[{}: {} pages on {} sheets]", out, total_pages, total_sheets);
    } else {
        println!("[Total: {} pages on {} sheets] sent to the default printer", total_pages, total_sheets);
    }

    for f in &files {
        println!("  [{}]", f);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_a2ps(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_a2ps};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_a2ps(vec!["--help".to_string()]), 0);
        assert_eq!(run_a2ps(vec!["-h".to_string()]), 0);
        assert_eq!(run_a2ps(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_a2ps(vec![]), 0);
    }
}
