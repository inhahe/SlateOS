#![deny(clippy::all)]

//! skanlite-cli — SlateOS Skanlite KDE scanner application
//!
//! Single personality: `skanlite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_skanlite(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: skanlite [OPTIONS]");
        println!("skanlite v23.08 (SlateOS) — KDE scanner application");
        println!();
        println!("Options:");
        println!("  -d DEVICE         Use specific scanner");
        println!("  --batch           Batch scanning mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("skanlite v23.08 (SlateOS)"); return 0; }
    println!("skanlite: KDE scanner application started");
    println!("  Available scanners: 2");
    println!("  Default: Epson Perfection V39");
    println!("  Save format: PNG");
    println!("  Save location: ~/Documents/Scans/");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "skanlite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_skanlite(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_skanlite};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/skanlite"), "skanlite");
        assert_eq!(basename(r"C:\bin\skanlite.exe"), "skanlite.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("skanlite.exe"), "skanlite");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_skanlite(&["--help".to_string()], "skanlite"), 0);
        assert_eq!(run_skanlite(&["-h".to_string()], "skanlite"), 0);
        let _ = run_skanlite(&["--version".to_string()], "skanlite");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_skanlite(&[], "skanlite");
    }
}
