#![deny(clippy::all)]

//! lutris-cli — Slate OS Lutris game manager
//!
//! Single personality: `lutris`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lutris(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: lutris [OPTIONS] [URI]");
        println!("lutris v0.5 (Slate OS) — Open source game manager");
        println!();
        println!("Options:");
        println!("  -l                List installed games");
        println!("  -i SLUG           Install game");
        println!("  -e SLUG           Execute game");
        println!("  --list-runners    List available runners");
        println!("  -d                Debug output");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lutris v0.5 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Installed games:");
        println!("  Portal 2          (wine-ge-8-26)  [steam]");
        println!("  Celeste           (native)        [gog]");
        println!("  Hollow Knight     (wine-ge-8-26)  [epic]");
        return 0;
    }
    if args.iter().any(|a| a == "--list-runners") {
        println!("Runners:");
        println!("  wine (wine-ge-8-26, wine-9.0)");
        println!("  linux (native)");
        println!("  steam");
        println!("  dosbox");
        println!("  retroarch");
        return 0;
    }
    println!("lutris: game manager started");
    println!("  Games: 3 installed");
    println!("  Runners: 5 available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lutris".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lutris(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lutris};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lutris"), "lutris");
        assert_eq!(basename(r"C:\bin\lutris.exe"), "lutris.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lutris.exe"), "lutris");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lutris(&["--help".to_string()], "lutris"), 0);
        assert_eq!(run_lutris(&["-h".to_string()], "lutris"), 0);
        let _ = run_lutris(&["--version".to_string()], "lutris");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lutris(&[], "lutris");
    }
}
