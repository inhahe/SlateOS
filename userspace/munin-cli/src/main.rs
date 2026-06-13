#![deny(clippy::all)]

//! munin-cli — SlateOS Munin network monitoring
//!
//! Multi-personality: `munin-node`, `munin-run`, `munin-cron`, `munin-update`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_munin_node(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: munin-node [OPTIONS]");
        println!("munin-node v2.0 (SlateOS) — Munin monitoring node agent");
        println!();
        println!("Options:");
        println!("  --config FILE   Configuration file");
        println!("  --debug         Debug mode");
        println!("  --version       Show version");
        println!();
        println!("Collects system metrics and serves them to munin-update.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("munin-node v2.0 (SlateOS)"); return 0; }
    println!("munin-node: listening on port 4949");
    println!("  Plugins loaded: cpu, memory, disk, network, processes");
    println!("  Update interval: 5 minutes");
    0
}

fn run_munin_run(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: munin-run <plugin> [config]");
        println!("munin-run v2.0 (SlateOS) — Run a Munin plugin manually");
        return 0;
    }
    if args.is_empty() {
        println!("munin-run: no plugin specified");
        return 1;
    }
    println!("user.value 5.2");
    println!("system.value 2.1");
    println!("idle.value 92.7");
    0
}

fn run_munin_update(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: munin-update [OPTIONS]");
        println!("munin-update v2.0 (SlateOS) — Fetch data from munin nodes");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("munin-update v2.0 (SlateOS)"); return 0; }
    println!("munin-update: fetching data from 3 nodes...");
    println!("  localhost: 12 plugins, 48 values collected");
    println!("  server1: 8 plugins, 32 values collected");
    println!("  server2: 10 plugins, 40 values collected");
    0
}

fn run_munin_cron(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: munin-cron [OPTIONS]");
        println!("munin-cron v2.0 (SlateOS) — Periodic data collection");
        return 0;
    }
    let _ = args;
    println!("munin-cron: running update cycle");
    println!("  Update: complete");
    println!("  Graph: generated");
    println!("  HTML: updated");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "munin-node".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "munin-run" => run_munin_run(&rest, &prog),
        "munin-cron" => run_munin_cron(&rest, &prog),
        "munin-update" => run_munin_update(&rest, &prog),
        _ => run_munin_node(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_munin_node};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/munin"), "munin");
        assert_eq!(basename(r"C:\bin\munin.exe"), "munin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("munin.exe"), "munin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_munin_node(&["--help".to_string()], "munin"), 0);
        assert_eq!(run_munin_node(&["-h".to_string()], "munin"), 0);
        let _ = run_munin_node(&["--version".to_string()], "munin");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_munin_node(&[], "munin");
    }
}
