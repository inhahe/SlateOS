#![deny(clippy::all)]

//! calligra-cli — SlateOS Calligra KDE office suite
//!
//! Multi-personality: `calligrawords`, `calligrasheets`, `calligrastage`, `karbon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_calligra(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE]", prog);
        println!("calligra v3.2 (SlateOS) — KDE office suite");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("calligra v3.2 (SlateOS)"); return 0; }
    let component = match prog {
        "calligrawords" => "Words (word processor)",
        "calligrasheets" => "Sheets (spreadsheet)",
        "calligrastage" => "Stage (presentation)",
        "karbon" => "Karbon (vector graphics)",
        _ => "Words (word processor)",
    };
    println!("calligra: {} started", component);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "calligrawords".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_calligra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_calligra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/calligra"), "calligra");
        assert_eq!(basename(r"C:\bin\calligra.exe"), "calligra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("calligra.exe"), "calligra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_calligra(&["--help".to_string()], "calligra"), 0);
        assert_eq!(run_calligra(&["-h".to_string()], "calligra"), 0);
        let _ = run_calligra(&["--version".to_string()], "calligra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_calligra(&[], "calligra");
    }
}
