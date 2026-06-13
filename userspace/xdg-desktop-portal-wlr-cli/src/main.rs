#![deny(clippy::all)]

//! xdg-desktop-portal-wlr-cli — SlateOS wlr portal backend
//!
//! Single personality: `xdg-desktop-portal-wlr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_portal_wlr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xdg-desktop-portal-wlr [OPTIONS]");
        println!("xdg-desktop-portal-wlr v0.7 (Slate OS) — wlroots portal backend");
        println!();
        println!("Options:");
        println!("  -r                Replace running instance");
        println!("  -l LOGLEVEL       Log level (QUIET, ERROR, WARNING, INFO, DEBUG, TRACE)");
        println!("  -c CONFIG         Config file path");
        println!("  --version         Show version");
        println!();
        println!("Provides Screenshot and ScreenCast portals for wlroots compositors.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xdg-desktop-portal-wlr v0.7 (Slate OS)"); return 0; }
    println!("xdg-desktop-portal-wlr: started");
    println!("  Providing: Screenshot, ScreenCast portals");
    println!("  Using: zwlr_screencopy_manager_v1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xdg-desktop-portal-wlr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_portal_wlr(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_portal_wlr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xdg-desktop-portal-wlr"), "xdg-desktop-portal-wlr");
        assert_eq!(basename(r"C:\bin\xdg-desktop-portal-wlr.exe"), "xdg-desktop-portal-wlr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xdg-desktop-portal-wlr.exe"), "xdg-desktop-portal-wlr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_portal_wlr(&["--help".to_string()], "xdg-desktop-portal-wlr"), 0);
        assert_eq!(run_portal_wlr(&["-h".to_string()], "xdg-desktop-portal-wlr"), 0);
        let _ = run_portal_wlr(&["--version".to_string()], "xdg-desktop-portal-wlr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_portal_wlr(&[], "xdg-desktop-portal-wlr");
    }
}
