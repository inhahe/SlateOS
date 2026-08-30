#![deny(clippy::all)]

//! onlyoffice-cli — Slate OS ONLYOFFICE desktop editors
//!
//! Single personality: `onlyoffice-desktopeditors`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_onlyoffice(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: onlyoffice-desktopeditors [OPTIONS] [FILE]");
        println!("onlyoffice v8.0 (Slate OS) — Desktop document editors");
        println!();
        println!("Options:");
        println!("  --new:word        New document");
        println!("  --new:cell        New spreadsheet");
        println!("  --new:slide       New presentation");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("onlyoffice v8.0 (Slate OS)"); return 0; }
    println!("onlyoffice: desktop editors started");
    println!("  Document editor: ready");
    println!("  Spreadsheet editor: ready");
    println!("  Presentation editor: ready");
    println!("  Recent files: 3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "onlyoffice-desktopeditors".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_onlyoffice(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_onlyoffice};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/onlyoffice"), "onlyoffice");
        assert_eq!(basename(r"C:\bin\onlyoffice.exe"), "onlyoffice.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("onlyoffice.exe"), "onlyoffice");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_onlyoffice(&["--help".to_string()], "onlyoffice"), 0);
        assert_eq!(run_onlyoffice(&["-h".to_string()], "onlyoffice"), 0);
        let _ = run_onlyoffice(&["--version".to_string()], "onlyoffice");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_onlyoffice(&[], "onlyoffice");
    }
}
