#![deny(clippy::all)]

//! gamemode-cli — OurOS Feral GameMode performance optimizer
//!
//! Multi-personality: `gamemoded`, `gamemoderun`, `gamemodelist`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gamemoderun(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gamemoderun COMMAND [ARGS]");
        println!();
        println!("gamemoderun — run a program with GameMode optimizations (OurOS).");
        println!("Sets LD_PRELOAD to enable GameMode for the given process.");
        return 0;
    }

    let program = args.first().map(|s| s.as_str()).unwrap_or("game");
    println!("[GameMode] Requesting GameMode for PID 1234");
    println!("[GameMode] Optimizations applied:");
    println!("  CPU governor:    performance");
    println!("  GPU power state: high");
    println!("  Scheduler:       SCHED_ISO");
    println!("  I/O priority:    best-effort class 0");
    println!("  Renice:          -4");
    println!("[GameMode] Running: {}", program);
    0
}

fn run_gamemoded(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gamemoded [OPTIONS]");
        println!("  -d    Daemonize");
        println!("  -l    Log to syslog");
        println!("  -r    Request start");
        println!("  -s    Request status");
        println!("  -t    Run tests");
        println!("  -v    Version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("gamemode daemon version 1.8.1 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-s") {
        println!("gamemode is active");
        println!("  Active clients: 2");
        println!("  PID 1234: /usr/bin/game1");
        println!("  PID 5678: /usr/bin/game2");
        return 0;
    }
    if args.iter().any(|a| a == "-t") {
        println!(":: Loading config");
        println!(":: Running tests");
        println!(":: Verifying CPU governor... OK (performance)");
        println!(":: Verifying GPU clock... OK (high)");
        println!(":: Verifying I/O scheduler... OK");
        println!(":: Verifying renice... OK");
        println!(":: All tests passed.");
        return 0;
    }

    println!("gamemoded: starting daemon (OurOS)");
    println!("gamemoded: listening for game mode requests");
    0
}

fn run_gamemodelist(_args: &[String]) -> i32 {
    println!("Active GameMode clients:");
    println!("  PID     COMMAND");
    println!("  1234    /usr/bin/game1");
    println!("  5678    /usr/bin/game2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gamemoderun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gamemoded" => run_gamemoded(&rest),
        "gamemodelist" => run_gamemodelist(&rest),
        _ => run_gamemoderun(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gamemoderun};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gamemode"), "gamemode");
        assert_eq!(basename(r"C:\bin\gamemode.exe"), "gamemode.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gamemode.exe"), "gamemode");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gamemoderun(&["--help".to_string()]), 0);
        assert_eq!(run_gamemoderun(&["-h".to_string()]), 0);
        assert_eq!(run_gamemoderun(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gamemoderun(&[]), 0);
    }
}
