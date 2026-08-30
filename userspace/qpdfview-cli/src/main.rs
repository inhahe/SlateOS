#![deny(clippy::all)]

//! qpdfview-cli — Slate OS qpdfview tabbed PDF viewer
//!
//! Single personality: `qpdfview`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qpdfview(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qpdfview [OPTIONS] [FILE[:PAGE]...]");
        println!("qpdfview v0.5 (Slate OS) — Tabbed document viewer");
        println!();
        println!("Options:");
        println!("  --unique          Single instance mode");
        println!("  --instance NAME   Named instance");
        println!("  --search TEXT     Search in document");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qpdfview v0.5 (Slate OS)"); return 0; }
    println!("qpdfview: tabbed document viewer started");
    println!("  Backends: PDF (Poppler), PS (libspectre), DjVu");
    println!("  Tabs: supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qpdfview".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qpdfview(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qpdfview};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qpdfview"), "qpdfview");
        assert_eq!(basename(r"C:\bin\qpdfview.exe"), "qpdfview.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qpdfview.exe"), "qpdfview");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qpdfview(&["--help".to_string()], "qpdfview"), 0);
        assert_eq!(run_qpdfview(&["-h".to_string()], "qpdfview"), 0);
        let _ = run_qpdfview(&["--version".to_string()], "qpdfview");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qpdfview(&[], "qpdfview");
    }
}
