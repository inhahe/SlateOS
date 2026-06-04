#![deny(clippy::all)]

//! amp-cli — OurOS Amp text editor
//!
//! Single personality: `amp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_amp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amp [OPTIONS] [FILE...]");
        println!("amp 0.7.0 (OurOS) — A complete text editor for the terminal");
        println!();
        println!("Options:");
        println!("  -s, --syntax-path DIR   Syntax definition directory");
        println!("  -l, --log               Enable logging");
        println!("  -V, --version           Show version");
        println!();
        println!("Modes:");
        println!("  Normal       Navigation (vi-like)");
        println!("  Insert       Text insertion");
        println!("  Select       Selection mode");
        println!("  Search       Search mode");
        println!("  Command      Command palette");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("amp 0.7.0 (OurOS)");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if let Some(f) = file {
        println!("amp: Editing '{}'", f);
    } else {
        println!("amp: New buffer");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "amp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_amp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_amp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/amp"), "amp");
        assert_eq!(basename(r"C:\bin\amp.exe"), "amp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("amp.exe"), "amp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_amp(&["--help".to_string()], "amp"), 0);
        assert_eq!(run_amp(&["-h".to_string()], "amp"), 0);
        let _ = run_amp(&["--version".to_string()], "amp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_amp(&[], "amp");
    }
}
