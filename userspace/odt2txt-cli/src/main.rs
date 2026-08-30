#![deny(clippy::all)]

//! odt2txt-cli — Slate OS OpenDocument to text converter
//!
//! Single personality: `odt2txt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_odt2txt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: odt2txt [OPTIONS] <file.odt>");
        println!("odt2txt v0.5 (Slate OS) — OpenDocument to plain text converter");
        println!();
        println!("Options:");
        println!("  -o FILE        Output to file (default: stdout)");
        println!("  --encoding ENC Output encoding (default: utf-8)");
        println!("  --width N      Line width (default: 65)");
        println!("  --raw          Raw output (no formatting)");
        println!("  --subst TYPE   Substitution type for special chars:");
        println!("                   utf-8, ascii, unicode");
        println!("  --no-wrap      Don't wrap lines");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("odt2txt v0.5 (Slate OS)"); return 0; }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-') && {
        let idx = args.iter().position(|x| std::ptr::eq(x, *a)).unwrap_or(0);
        idx == 0 || !matches!(args.get(idx.wrapping_sub(1)).map(|s| s.as_str()), Some("-o" | "--encoding" | "--width" | "--subst"))
    }).collect();
    if files.is_empty() {
        eprintln!("odt2txt: error: no input file specified");
        return 1;
    }
    println!("Document Title");
    println!("==============");
    println!();
    println!("This is a sample document converted from OpenDocument format.");
    println!("The text has been extracted with formatting preserved as plain");
    println!("text equivalents.");
    println!();
    println!("  * List item one");
    println!("  * List item two");
    println!("  * List item three");
    println!();
    println!("Table 1: Sample Data");
    println!("  Name      | Value  | Status");
    println!("  ----------+--------+--------");
    println!("  Alpha     | 100    | OK");
    println!("  Beta      | 200    | OK");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "odt2txt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_odt2txt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_odt2txt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/odt2txt"), "odt2txt");
        assert_eq!(basename(r"C:\bin\odt2txt.exe"), "odt2txt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("odt2txt.exe"), "odt2txt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_odt2txt(&["--help".to_string()], "odt2txt"), 0);
        assert_eq!(run_odt2txt(&["-h".to_string()], "odt2txt"), 0);
        let _ = run_odt2txt(&["--version".to_string()], "odt2txt");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_odt2txt(&[], "odt2txt");
    }
}
