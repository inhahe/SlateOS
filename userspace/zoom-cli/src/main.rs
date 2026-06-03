#![deny(clippy::all)]

//! zoom-cli — OurOS Zoom video conferencing
//!
//! Single personality: `zoom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zoom(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zoom [OPTIONS]");
        println!("zoom v5.17 (OurOS) — Video conferencing client");
        println!();
        println!("Options:");
        println!("  --url=URL         Join meeting by URL");
        println!("  --minimized       Start minimized");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zoom v5.17 (OurOS)"); return 0; }
    println!("zoom: video conferencing client started");
    println!("  Account: signed in");
    println!("  Virtual background: available");
    println!("  Screen sharing: ready");
    println!("  Recording: local/cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zoom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zoom(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zoom};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zoom"), "zoom");
        assert_eq!(basename(r"C:\bin\zoom.exe"), "zoom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zoom.exe"), "zoom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_zoom(&["--help".to_string()], "zoom"), 0);
        assert_eq!(run_zoom(&["-h".to_string()], "zoom"), 0);
        assert_eq!(run_zoom(&["--version".to_string()], "zoom"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_zoom(&[], "zoom"), 0);
    }
}
