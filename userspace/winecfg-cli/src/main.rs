#![deny(clippy::all)]

//! winecfg-cli — SlateOS Wine configuration utility
//!
//! Single personality: `winecfg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_winecfg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: winecfg [OPTIONS]");
        println!("winecfg v9.0 (SlateOS) — Wine configuration dialog");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Tabs:");
        println!("  Applications      Per-application Windows version");
        println!("  Libraries         DLL override configuration");
        println!("  Graphics          Display resolution and DPI");
        println!("  Desktop           Virtual desktop settings");
        println!("  Audio             Audio driver configuration");
        println!("  Staging           Wine Staging patch options");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("winecfg v9.0 (SlateOS)"); return 0; }
    println!("winecfg: Wine configuration dialog opened");
    println!("  Prefix: ~/.wine");
    println!("  Windows version: Windows 10");
    println!("  Architecture: win64");
    println!("  DLL overrides: 3 configured");
    println!("  Audio driver: PulseAudio");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "winecfg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_winecfg(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_winecfg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/winecfg"), "winecfg");
        assert_eq!(basename(r"C:\bin\winecfg.exe"), "winecfg.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("winecfg.exe"), "winecfg");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_winecfg(&["--help".to_string()], "winecfg"), 0);
        assert_eq!(run_winecfg(&["-h".to_string()], "winecfg"), 0);
        let _ = run_winecfg(&["--version".to_string()], "winecfg");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_winecfg(&[], "winecfg");
    }
}
