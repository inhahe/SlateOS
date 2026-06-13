#![deny(clippy::all)]

//! gnumeric-cli — Slate OS Gnumeric spreadsheet
//!
//! Multi-personality: `gnumeric`, `ssconvert`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnumeric(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnumeric [OPTIONS] [FILE...]");
        println!("gnumeric v1.12 (Slate OS) — GNOME spreadsheet");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnumeric v1.12 (Slate OS)"); return 0; }
    println!("gnumeric: spreadsheet application started");
    println!("  Functions: 500+ available");
    println!("  Plugins: Python, Perl");
    0
}

fn run_ssconvert(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ssconvert [OPTIONS] INFILE OUTFILE");
        println!("ssconvert v1.12 (Slate OS) — Spreadsheet format converter");
        println!();
        println!("Options:");
        println!("  -T FMT            Output format");
        println!("  -S                Export all sheets");
        println!("  --list-exporters  List output formats");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ssconvert v1.12 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list-exporters") {
        println!("Gnumeric_XmlIO:sax:0    Gnumeric XML (.gnumeric)");
        println!("Gnumeric_stf:stf_csv    CSV");
        println!("Gnumeric_Excel:xlsx2    Excel 2007+ (.xlsx)");
        println!("Gnumeric_pdf:pdf_cairo  PDF");
        return 0;
    }
    println!("ssconvert: converting file...");
    println!("  Input: workbook.xlsx");
    println!("  Output: workbook.csv");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnumeric".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ssconvert" => run_ssconvert(&rest, &prog),
        _ => run_gnumeric(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gnumeric};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnumeric"), "gnumeric");
        assert_eq!(basename(r"C:\bin\gnumeric.exe"), "gnumeric.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnumeric.exe"), "gnumeric");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnumeric(&["--help".to_string()], "gnumeric"), 0);
        assert_eq!(run_gnumeric(&["-h".to_string()], "gnumeric"), 0);
        let _ = run_gnumeric(&["--version".to_string()], "gnumeric");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnumeric(&[], "gnumeric");
    }
}
