#![deny(clippy::all)]

//! graylog-cli — Slate OS Graylog log management
//!
//! Multi-personality: `graylog-server`, `graylog-ctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_graylog(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "graylog-ctl" => {
                println!("graylog-ctl (Slate OS) — Graylog management CLI");
                println!("  status         Show server status");
                println!("  start          Start Graylog");
                println!("  stop           Stop Graylog");
                println!("  restart        Restart Graylog");
                println!("  backup         Create backup");
                println!("  restore FILE   Restore from backup");
            }
            _ => {
                println!("graylog-server v5.2 (Slate OS) — Log management server");
                println!("  -f FILE        Config file");
                println!("  -p FILE        PID file");
                println!("  -np            No PID file");
                println!("  -d             Debug mode");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Graylog v5.2.5 (Slate OS)"); return 0; }
    println!("Graylog v5.2.5 (Slate OS)");
    println!("  Inputs: 5 (Syslog UDP, GELF TCP, Beats, Raw TCP, JSON)");
    println!("  Messages/sec: 12,345");
    println!("  Total messages: 456,789,012");
    println!("  Indices: 12 (30-day retention)");
    println!("  Streams: 8");
    println!("  Alerts: 3 active");
    println!("  Extractors: 23");
    println!("  OpenSearch: connected (3 nodes, green)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "graylog-server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_graylog(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_graylog};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/graylog"), "graylog");
        assert_eq!(basename(r"C:\bin\graylog.exe"), "graylog.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("graylog.exe"), "graylog");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_graylog(&["--help".to_string()], "graylog"), 0);
        assert_eq!(run_graylog(&["-h".to_string()], "graylog"), 0);
        let _ = run_graylog(&["--version".to_string()], "graylog");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_graylog(&[], "graylog");
    }
}
