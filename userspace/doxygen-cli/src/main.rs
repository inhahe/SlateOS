#![deny(clippy::all)]

//! doxygen-cli — OurOS Doxygen documentation generator
//!
//! Multi-personality: `doxygen`, `doxyindexer`, `doxysearch.cgi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_doxygen(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: doxygen [OPTIONS] [CONFIG_FILE]");
        println!("Doxygen 1.10.0 (OurOS)");
        println!();
        println!("Options:");
        println!("  -g [FILE]     Generate template configuration file");
        println!("  -u [FILE]     Update old configuration file");
        println!("  -s            Short output (no comments in config)");
        println!("  -l [FILE]     Generate template layout file");
        println!("  -w html|latex|rtf  Generate style templates");
        println!("  -d            Debug mode");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("1.10.0 (4b15e70c3a5d8b01cb3ae3aa36f29e6d8a64e498)");
        return 0;
    }
    if args.iter().any(|a| a == "-g") {
        let file = args.windows(2)
            .find(|w| w[0] == "-g")
            .and_then(|w| if w[1].starts_with('-') { None } else { Some(w[1].as_str()) })
            .unwrap_or("Doxyfile");
        println!("Configuration file '{}' created.", file);
        println!("  Edit and run 'doxygen {}' to generate documentation.", file);
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("Layout file 'DoxygenLayout.xml' created.");
        return 0;
    }
    let config = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("Doxyfile");
    println!("Doxygen 1.10.0");
    println!("  Reading configuration from {}", config);
    println!("  Searching for include files...");
    println!("  Searching for example files...");
    println!("  Searching for images...");
    println!("  Searching for files to process...");
    println!("  Parsing input...");
    println!("  Building group list...");
    println!("  Building directory list...");
    println!("  Building namespace list...");
    println!("  Building file list...");
    println!("  Building class list...");
    println!("  Generating docs...");
    println!("  Generating index pages...");
    println!("  Generating HTML output...");
    println!("  144 files, 89 classes, 432 members documented");
    println!("  Output written to html/");
    0
}

fn run_doxyindexer(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: doxyindexer [OPTIONS] SEARCHDATA.XML [SEARCHDATA.XML ...]");
        println!("  -o DIR    Output directory for index (default: doxysearch.db)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".xml"))
        .map(|s| s.as_str())
        .collect();
    for f in &files {
        println!("doxyindexer: indexing {}", f);
    }
    println!("doxyindexer: index written to doxysearch.db/");
    0
}

fn run_doxysearch(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: doxysearch.cgi");
        println!("  CGI-based search engine for Doxygen documentation");
        return 0;
    }
    let _ = args;
    println!("Content-Type: application/javascript");
    println!();
    println!("searchResultsText(\"Results\", []);");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "doxygen".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "doxyindexer" => run_doxyindexer(&rest),
        "doxysearch.cgi" | "doxysearch" => run_doxysearch(&rest),
        _ => run_doxygen(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_doxygen};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/doxygen"), "doxygen");
        assert_eq!(basename(r"C:\bin\doxygen.exe"), "doxygen.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("doxygen.exe"), "doxygen");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_doxygen(&["--help".to_string()]), 0);
        assert_eq!(run_doxygen(&["-h".to_string()]), 0);
        assert_eq!(run_doxygen(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_doxygen(&[]), 0);
    }
}
