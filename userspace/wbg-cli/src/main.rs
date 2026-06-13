#![deny(clippy::all)]

//! wbg-cli — Slate OS wbg minimal wallpaper setter
//!
//! Single personality: `wbg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wbg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wbg IMAGE");
        println!("wbg v1.1 (Slate OS) — Minimal wallpaper setter for Wayland");
        println!();
        println!("Arguments:");
        println!("  IMAGE             Path to wallpaper image");
        println!();
        println!("Extremely minimal — no scaling options, no config file.");
        println!("Just set a wallpaper and stay running.");
        return 0;
    }
    let image = args.first().map(|s| s.as_str()).unwrap_or("wallpaper.png");
    println!("wbg: {}", image);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wbg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wbg(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wbg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wbg"), "wbg");
        assert_eq!(basename(r"C:\bin\wbg.exe"), "wbg.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wbg.exe"), "wbg");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wbg(&["--help".to_string()], "wbg"), 0);
        assert_eq!(run_wbg(&["-h".to_string()], "wbg"), 0);
        let _ = run_wbg(&["--version".to_string()], "wbg");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wbg(&[], "wbg");
    }
}
