#![deny(clippy::all)]

//! lf-cli — OurOS lf file manager
//!
//! Single personality: `lf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lf [OPTIONS] [PATH]");
        println!("lf r32 (OurOS) — Terminal file manager");
        println!();
        println!("Options:");
        println!("  -command CMD      Execute command");
        println!("  -config FILE      Config file path");
        println!("  -cpuprofile FILE  CPU profile output");
        println!("  -doc              Show documentation");
        println!("  -last-dir-path F  Output last dir to file");
        println!("  -log FILE         Log file path");
        println!("  -remote CMD       Send remote command");
        println!("  -selection-path F Selection output file");
        println!("  -server           Start server");
        println!("  -single           Single pane mode");
        println!("  -version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("lf r32 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-doc") {
        println!("lf - Terminal File Manager");
        println!();
        println!("lf is a terminal file manager written in Go with a heavy");
        println!("focus on configuration and scripting.");
        return 0;
    }
    if args.iter().any(|a| a == "-server") {
        println!("lf: Starting server...");
        return 0;
    }
    if let Some(pos) = args.iter().position(|a| a == "-remote") {
        let cmd = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("quit");
        println!("lf remote: {}", cmd);
        return 0;
    }
    let path = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("lf: Opening '{}'", path);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lf"), "lf");
        assert_eq!(basename(r"C:\bin\lf.exe"), "lf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lf.exe"), "lf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lf(&["--help".to_string()], "lf"), 0);
        assert_eq!(run_lf(&["-h".to_string()], "lf"), 0);
        let _ = run_lf(&["--version".to_string()], "lf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lf(&[], "lf");
    }
}
