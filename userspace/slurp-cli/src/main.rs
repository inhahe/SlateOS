#![deny(clippy::all)]

//! slurp-cli — SlateOS slurp region selector
//!
//! Single personality: `slurp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_slurp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: slurp [OPTIONS]");
        println!("slurp v1.5 (Slate OS) — Select a region in Wayland compositor");
        println!();
        println!("Options:");
        println!("  -d                Show display dimensions");
        println!("  -b COLOR          Background color");
        println!("  -c COLOR          Border color");
        println!("  -s COLOR          Selection color");
        println!("  -B COLOR          Border color");
        println!("  -w N              Border width");
        println!("  -f FORMAT         Output format");
        println!("  -p                Select a single point");
        println!("  -o                Select entire output");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("3840x2160");
        return 0;
    }
    if args.iter().any(|a| a == "-p") {
        println!("960,540");
        return 0;
    }
    // Default: output selected region
    println!("100,200 800x600");
    if args.is_empty() {
        // Interactive selection simulation
        println!("100,200 800x600");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "slurp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_slurp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_slurp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/slurp"), "slurp");
        assert_eq!(basename(r"C:\bin\slurp.exe"), "slurp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("slurp.exe"), "slurp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_slurp(&["--help".to_string()], "slurp"), 0);
        assert_eq!(run_slurp(&["-h".to_string()], "slurp"), 0);
        let _ = run_slurp(&["--version".to_string()], "slurp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_slurp(&[], "slurp");
    }
}
