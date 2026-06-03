#![deny(clippy::all)]

//! nifi-cli — OurOS Apache NiFi data flow
//!
//! Single personality: `nifi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nifi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nifi [COMMAND] [OPTIONS]");
        println!("Apache NiFi v2.0 (OurOS) — Data flow automation");
        println!();
        println!("Commands:");
        println!("  start              Start NiFi");
        println!("  stop               Stop NiFi");
        println!("  restart            Restart NiFi");
        println!("  status             Show NiFi status");
        println!("  run                Run NiFi in foreground");
        println!("  install            Install as service");
        println!("  set-single-user-credentials  Set credentials");
        println!();
        println!("Options:");
        println!("  --nifi-home DIR    NiFi home directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache NiFi v2.0.0 (OurOS)"); return 0; }
    println!("Apache NiFi v2.0.0 (OurOS)");
    println!("  Web UI: https://0.0.0.0:8443/nifi");
    println!("  Processors: 45 running, 3 stopped");
    println!("  Process groups: 8");
    println!("  Connections: 56");
    println!("  Flowfiles queued: 1,234");
    println!("  Throughput: 567 events/s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nifi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nifi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nifi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nifi"), "nifi");
        assert_eq!(basename(r"C:\bin\nifi.exe"), "nifi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nifi.exe"), "nifi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nifi(&["--help".to_string()], "nifi"), 0);
        assert_eq!(run_nifi(&["-h".to_string()], "nifi"), 0);
        assert_eq!(run_nifi(&["--version".to_string()], "nifi"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nifi(&[], "nifi"), 0);
    }
}
