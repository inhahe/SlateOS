#![deny(clippy::all)]

//! druid-cli — OurOS Apache Druid analytics database
//!
//! Single personality: `druid`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_druid(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: druid [COMMAND] [OPTIONS]");
        println!("Apache Druid v29.0 (OurOS) — Real-time analytics database");
        println!();
        println!("Commands:");
        println!("  server             Start server (single-server mode)");
        println!("  coordinator        Start coordinator node");
        println!("  historical         Start historical node");
        println!("  broker             Start broker node");
        println!("  middlemanager      Start middle manager");
        println!("  router             Start router node");
        println!("  overlord           Start overlord node");
        println!();
        println!("Options:");
        println!("  --config DIR       Config directory");
        println!("  --classpath PATH   Classpath");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache Druid v29.0.1 (OurOS)"); return 0; }
    println!("Apache Druid v29.0.1 (OurOS)");
    println!("  Mode: single-server");
    println!("  Datasources: 8");
    println!("  Segments: 12,345");
    println!("  Rows: 5.6 billion");
    println!("  Console: http://0.0.0.0:8888");
    println!("  Broker: 0.0.0.0:8082");
    println!("  Deep storage: local (/var/druid/segments)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "druid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_druid(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_druid};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/druid"), "druid");
        assert_eq!(basename(r"C:\bin\druid.exe"), "druid.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("druid.exe"), "druid");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_druid(&["--help".to_string()], "druid"), 0);
        assert_eq!(run_druid(&["-h".to_string()], "druid"), 0);
        assert_eq!(run_druid(&["--version".to_string()], "druid"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_druid(&[], "druid"), 0);
    }
}
