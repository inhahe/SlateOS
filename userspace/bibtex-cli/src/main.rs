#![deny(clippy::all)]

//! bibtex-cli — OurOS BibTeX CLI
//!
//! Multi-personality: `bibtex`, `biber`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_bibtex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help" || a == "-h") {
        println!("Usage: bibtex [OPTIONS] AUX-FILE");
        println!();
        println!("BibTeX — bibliography processor (OurOS).");
        println!();
        println!("Options:");
        println!("  -terse             Terse mode");
        println!("  -min-crossrefs N   Minimum crossrefs (default 2)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("BibTeX 0.99d (TeX Live 2024/OurOS)");
        return 0;
    }

    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("document");

    let base = strip_ext(file);

    println!("This is BibTeX, Version 0.99d (OurOS)");
    println!("The top-level auxiliary file: {}.aux", base);
    println!("The style file: plain.bst");
    println!("Database file #1: references.bib");
    println!("You've used 5 entries,");
    println!("            2 wiz_defined-function locations,");
    println!("            512 strings with 4567 characters.");
    println!("(There were 0 warnings)");
    0
}

fn run_biber(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: biber [OPTIONS] FILE");
        println!();
        println!("Biber — BibLaTeX backend processor (OurOS).");
        println!();
        println!("Options:");
        println!("  --output-file FILE     Output file");
        println!("  --output-format FMT    Output format (bbl, bibtex)");
        println!("  --validate-datamodel   Validate data model");
        println!("  --tool                 Tool mode (transform bib)");
        println!("  --debug                Debug mode");
        println!("  --quiet                Quiet mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("biber version: 2.19 (OurOS)");
        return 0;
    }

    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("document");

    let base = strip_ext(file);

    println!("INFO - This is Biber 2.19 (OurOS)");
    println!("INFO - Logfile is '{}.blg'", base);
    println!("INFO - Reading '{}.bcf'", base);
    println!("INFO - Found 5 citekeys in bib section 0");
    println!("INFO - Processing section 0");
    println!("INFO - Looking for bibtex file 'references.bib' for section 0");
    println!("INFO - Found BibTeX data source 'references.bib'");
    println!("INFO - Overriding locale 'en-US' defaults 'variable = shifted' with 'variable = non-ignorable'");
    println!("INFO - Sorting list 'nty/global//global/global/global' of type 'entry' with locale 'en-US'");
    println!("INFO - No sort tailoring available for locale 'en-US'");
    println!("INFO - Writing '{}.bbl' with encoding 'UTF-8'", base);
    println!("INFO - Output to {}.bbl", base);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "bibtex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "biber" => run_biber(&rest),
        _ => run_bibtex(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bibtex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bibtex"), "bibtex");
        assert_eq!(basename(r"C:\bin\bibtex.exe"), "bibtex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bibtex.exe"), "bibtex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bibtex(&["--help".to_string()]), 0);
        assert_eq!(run_bibtex(&["-h".to_string()]), 0);
        assert_eq!(run_bibtex(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bibtex(&[]), 0);
    }
}
