#![deny(clippy::all)]

//! power-profiles-daemon-cli — SlateOS power-profiles-daemon
//!
//! Multi-personality: `power-profiles-daemon`, `powerprofilesctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_daemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: power-profiles-daemon [OPTIONS]");
        println!("power-profiles-daemon v0.21 (Slate OS) — Power profile management");
        println!();
        println!("Options:");
        println!("  --replace         Replace running daemon");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("power-profiles-daemon v0.21 (Slate OS)"); return 0; }
    println!("power-profiles-daemon: started");
    println!("  D-Bus: net.hadess.PowerProfiles");
    0
}

fn run_ctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: powerprofilesctl COMMAND");
        println!("powerprofilesctl v0.21 (Slate OS) — Control power profiles");
        println!();
        println!("Commands:");
        println!("  list              List available profiles");
        println!("  get               Get active profile");
        println!("  set PROFILE       Set active profile");
        println!("  launch PROFILE CMD Launch command with profile");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("get");
    match cmd {
        "list" => {
            println!("  power-saver");
            println!("* balanced");
            println!("  performance");
        }
        "get" => println!("balanced"),
        "set" => {
            let profile = args.get(1).map(|s| s.as_str()).unwrap_or("balanced");
            println!("Set profile: {}", profile);
        }
        _ => println!("powerprofilesctl: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "power-profiles-daemon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "powerprofilesctl" => run_ctl(&rest, &prog),
        _ => run_daemon(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_daemon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/power-profiles-daemon"), "power-profiles-daemon");
        assert_eq!(basename(r"C:\bin\power-profiles-daemon.exe"), "power-profiles-daemon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("power-profiles-daemon.exe"), "power-profiles-daemon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_daemon(&["--help".to_string()], "power-profiles-daemon"), 0);
        assert_eq!(run_daemon(&["-h".to_string()], "power-profiles-daemon"), 0);
        let _ = run_daemon(&["--version".to_string()], "power-profiles-daemon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_daemon(&[], "power-profiles-daemon");
    }
}
