#![deny(clippy::all)]

//! tuned-cli — Slate OS TuneD system tuning daemon
//!
//! Multi-personality: `tuned`, `tuned-adm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tuned(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tuned [OPTIONS]");
        println!("tuned v2.22 (Slate OS) — System tuning daemon");
        println!();
        println!("Options:");
        println!("  -d                Run as daemon");
        println!("  -D                Don't daemonize");
        println!("  -l LOGLEVEL       Log level");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("tuned v2.22 (Slate OS)"); return 0; }
    println!("tuned: system tuning daemon started");
    println!("  Active profile: balanced");
    0
}

fn run_adm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tuned-adm COMMAND [OPTIONS]");
        println!("tuned-adm v2.22 (Slate OS) — TuneD administration");
        println!();
        println!("Commands:");
        println!("  active            Show active profile");
        println!("  list              List available profiles");
        println!("  profile NAME      Switch profile");
        println!("  recommend         Recommend profile");
        println!("  off               Disable tuning");
        println!("  verify            Verify current settings");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("active");
    match cmd {
        "active" => println!("Current active profile: balanced"),
        "list" => {
            println!("Available profiles:");
            println!("- balanced");
            println!("- desktop");
            println!("- latency-performance");
            println!("- network-latency");
            println!("- network-throughput");
            println!("- powersave");
            println!("- throughput-performance");
            println!("- virtual-guest");
            println!("- virtual-host");
        }
        "recommend" => println!("Recommended: desktop"),
        "profile" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("balanced");
            println!("Switched to profile: {}", name);
        }
        _ => println!("tuned-adm: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tuned".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "tuned-adm" => run_adm(&rest, &prog),
        _ => run_tuned(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tuned};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tuned"), "tuned");
        assert_eq!(basename(r"C:\bin\tuned.exe"), "tuned.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tuned.exe"), "tuned");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tuned(&["--help".to_string()], "tuned"), 0);
        assert_eq!(run_tuned(&["-h".to_string()], "tuned"), 0);
        let _ = run_tuned(&["--version".to_string()], "tuned");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tuned(&[], "tuned");
    }
}
