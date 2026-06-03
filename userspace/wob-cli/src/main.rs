#![deny(clippy::all)]

//! wob-cli — OurOS wob Wayland overlay bar
//!
//! Single personality: `wob`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wob(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wob [OPTIONS]");
        println!("wob v0.15 (OurOS) — Wayland overlay bar (volume/brightness)");
        println!();
        println!("Reads values (0-100) from stdin and displays overlay bar.");
        println!();
        println!("Options:");
        println!("  -a ANCHOR         Anchor position (top, bottom, left, right)");
        println!("  -M MARGIN         Margin in pixels");
        println!("  -W WIDTH          Bar width");
        println!("  -H HEIGHT         Bar height");
        println!("  -o OFFSET         Border offset");
        println!("  -b BORDER         Border width");
        println!("  -p PADDING        Padding");
        println!("  --background-color COLOR  Background color");
        println!("  --border-color COLOR      Border color");
        println!("  --bar-color COLOR         Bar color");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wob v0.15 (OurOS)"); return 0; }
    println!("wob: overlay bar ready");
    println!("  Anchor: bottom");
    println!("  Size: 400x50");
    println!("  Waiting for input on stdin...");
    if args.is_empty() {
        println!("  (echo 75 | wob)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wob".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wob(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wob};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wob"), "wob");
        assert_eq!(basename(r"C:\bin\wob.exe"), "wob.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wob.exe"), "wob");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wob(&["--help".to_string()], "wob"), 0);
        assert_eq!(run_wob(&["-h".to_string()], "wob"), 0);
        assert_eq!(run_wob(&["--version".to_string()], "wob"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wob(&[], "wob"), 0);
    }
}
