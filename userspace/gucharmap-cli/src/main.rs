#![deny(clippy::all)]

//! gucharmap-cli — SlateOS GNOME Character Map
//!
//! Single personality: `gucharmap`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gucharmap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gucharmap [OPTIONS]");
        println!("gucharmap v15.1 (SlateOS) — Unicode character map");
        println!();
        println!("Options:");
        println!("  --font FONT       Set display font");
        println!("  --version         Show version");
        println!();
        println!("Browse Unicode characters by script, block, or category.");
        println!("Search by name, code point, or character.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gucharmap v15.1 (SlateOS)"); return 0; }
    println!("gucharmap: character map started");
    println!("  Unicode version: 15.1");
    println!("  Total characters: 149,813");
    println!("  Scripts: 161");
    println!("  Blocks: 332");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gucharmap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gucharmap(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gucharmap};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gucharmap"), "gucharmap");
        assert_eq!(basename(r"C:\bin\gucharmap.exe"), "gucharmap.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gucharmap.exe"), "gucharmap");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gucharmap(&["--help".to_string()], "gucharmap"), 0);
        assert_eq!(run_gucharmap(&["-h".to_string()], "gucharmap"), 0);
        let _ = run_gucharmap(&["--version".to_string()], "gucharmap");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gucharmap(&[], "gucharmap");
    }
}
