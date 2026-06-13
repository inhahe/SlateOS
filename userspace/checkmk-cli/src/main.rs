#![deny(clippy::all)]

//! checkmk-cli — SlateOS Checkmk monitoring
//!
//! Multi-personality: `cmk`, `check_mk_agent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_checkmk(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "check_mk_agent" => {
                println!("check_mk_agent (Slate OS) — Checkmk monitoring agent");
                println!("  Outputs system data in Checkmk agent format");
            }
            _ => {
                println!("cmk v2.3 (Slate OS) — Checkmk monitoring CLI");
                println!("  -I HOST        Inventory scan");
                println!("  -II HOST       Full re-inventory");
                println!("  -D HOST        Dump agent output");
                println!("  -d HOST        Debug agent output");
                println!("  --detect       Auto-detect services");
                println!("  -R             Restart monitoring core");
                println!("  -O             Reload monitoring config");
                println!("  -U             Update monitoring config");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Checkmk v2.3.0 (Slate OS)"); return 0; }
    match prog {
        "check_mk_agent" => {
            println!("<<<check_mk>>>");
            println!("Version: 2.3.0");
            println!("AgentOS: Slate OS");
            println!("<<<cpu>>>");
            println!("0.45 0.32 0.28 4/234 12345");
            println!("<<<mem>>>");
            println!("MemTotal: 16384000 kB");
            println!("MemFree: 8192000 kB");
        }
        _ => {
            println!("Checkmk v2.3.0 (Slate OS)");
            println!("  Hosts: 50 monitored");
            println!("  Services: 1,234 total");
            println!("    OK: 1,190 (96.4%)");
            println!("    WARN: 30 (2.4%)");
            println!("    CRIT: 8 (0.6%)");
            println!("    UNKNOWN: 6 (0.5%)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cmk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_checkmk(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_checkmk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/checkmk"), "checkmk");
        assert_eq!(basename(r"C:\bin\checkmk.exe"), "checkmk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("checkmk.exe"), "checkmk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_checkmk(&["--help".to_string()], "checkmk"), 0);
        assert_eq!(run_checkmk(&["-h".to_string()], "checkmk"), 0);
        let _ = run_checkmk(&["--version".to_string()], "checkmk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_checkmk(&[], "checkmk");
    }
}
