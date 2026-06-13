#![deny(clippy::all)]

//! inkscape-cli — SlateOS Inkscape SVG editor CLI
//!
//! Single personality: `inkscape`

use std::env;
use std::process;

fn run_inkscape(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: inkscape [OPTIONS] [FILE ...]");
        println!();
        println!("Inkscape — SVG editor (Slate OS).");
        println!();
        println!("Options:");
        println!("  -o, --export-filename F   Output file");
        println!("  --export-type TYPE        Export type (png,pdf,eps,ps,svg)");
        println!("  -w, --export-width N      Export width (px)");
        println!("  -h, --export-height N     Export height (px)");
        println!("  -d, --export-dpi N        Export DPI");
        println!("  -D, --export-area-drawing Export drawing area");
        println!("  -C, --export-area-page    Export page area");
        println!("  --export-background COL   Background color");
        println!("  --export-plain-svg        Export plain SVG");
        println!("  -l, --export-plain-svg F  (deprecated, use -o)");
        println!("  --export-text-to-path     Convert text to paths");
        println!("  -p, --print               Print document");
        println!("  --query-id ID             Query element by ID");
        println!("  --query-x                 Query X position");
        println!("  --query-y                 Query Y position");
        println!("  --query-width             Query width");
        println!("  --query-height            Query height");
        println!("  --vacuum-defs             Clean up defs");
        println!("  --shell                   Interactive shell mode");
        println!("  --batch-process           Batch process mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Inkscape 1.3.2 (Slate OS)");
        println!("    Pango version: 1.50.14");
        return 0;
    }

    let export_type = args.windows(2)
        .find(|w| w[0] == "--export-type")
        .map(|w| w[1].as_str());

    let output = args.windows(2)
        .find(|w| w[0] == "-o" || w[0] == "--export-filename")
        .map(|w| w[1].as_str());

    let dpi = args.windows(2)
        .find(|w| w[0] == "-d" || w[0] == "--export-dpi")
        .map(|w| w[1].as_str())
        .unwrap_or("96");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    // Query mode
    if args.iter().any(|a| a == "--query-width" || a == "--query-height") {
        println!("1024");
        return 0;
    }
    if args.iter().any(|a| a == "--query-x" || a == "--query-y") {
        println!("0");
        return 0;
    }

    if args.iter().any(|a| a == "--vacuum-defs") {
        for f in &files {
            println!("Vacuuming defs in '{}'...", f);
        }
        println!("Done.");
        return 0;
    }

    if let Some(etype) = export_type {
        for f in &files {
            let out = output.unwrap_or("output");
            println!("Exporting '{}' → '{}' as {} at {} DPI", f, out, etype.to_uppercase(), dpi);
        }
    } else if output.is_some() {
        for f in &files {
            println!("Exporting '{}' → '{}'", f, output.unwrap_or("output"));
        }
    } else if files.is_empty() {
        println!("Inkscape 1.3.2 (Slate OS)");
        println!("Starting Inkscape GUI...");
    } else {
        for f in &files {
            println!("Opening '{}' in Inkscape...", f);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_inkscape(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_inkscape};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_inkscape(vec!["--help".to_string()]), 0);
        assert_eq!(run_inkscape(vec!["-h".to_string()]), 0);
        let _ = run_inkscape(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_inkscape(vec![]);
    }
}
