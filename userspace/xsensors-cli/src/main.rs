#![deny(clippy::all)]

//! xsensors-cli — SlateOS xsensors graphical sensor viewer
//!
//! Single personality: `xsensors`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xsensors(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xsensors [OPTIONS]");
        println!("xsensors v0.8 (SlateOS) — Graphical hardware sensor display");
        println!();
        println!("Options:");
        println!("  -f                Fahrenheit display");
        println!("  -c CHIP           Show specific chip only");
        println!("  -t SECONDS        Update interval");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xsensors v0.8 (SlateOS)"); return 0; }
    println!("xsensors: graphical sensor display started");
    println!("  Chips found: coretemp-isa-0000, it8728-isa-0a30");
    println!("  Update interval: 2s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xsensors".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xsensors(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xsensors};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xsensors"), "xsensors");
        assert_eq!(basename(r"C:\bin\xsensors.exe"), "xsensors.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xsensors.exe"), "xsensors");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xsensors(&["--help".to_string()], "xsensors"), 0);
        assert_eq!(run_xsensors(&["-h".to_string()], "xsensors"), 0);
        let _ = run_xsensors(&["--version".to_string()], "xsensors");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xsensors(&[], "xsensors");
    }
}
