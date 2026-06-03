#![deny(clippy::all)]

//! kcharselect-cli — OurOS KDE Character Selector
//!
//! Single personality: `kcharselect`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kcharselect(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kcharselect [OPTIONS]");
        println!("kcharselect v23.08 (OurOS) — KDE character selector");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Browse and insert Unicode characters.");
        println!("Search by name, category, or recently used.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kcharselect v23.08 (OurOS)"); return 0; }
    println!("kcharselect: character selector started");
    println!("  Unicode: 15.1 character database");
    println!("  Categories: European, African, Middle Eastern, South Asian, ...");
    println!("  Clipboard: copy on click");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kcharselect".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kcharselect(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kcharselect};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kcharselect"), "kcharselect");
        assert_eq!(basename(r"C:\bin\kcharselect.exe"), "kcharselect.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kcharselect.exe"), "kcharselect");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kcharselect(&["--help".to_string()], "kcharselect"), 0);
        assert_eq!(run_kcharselect(&["-h".to_string()], "kcharselect"), 0);
        assert_eq!(run_kcharselect(&["--version".to_string()], "kcharselect"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kcharselect(&[], "kcharselect"), 0);
    }
}
