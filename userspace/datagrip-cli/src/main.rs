#![deny(clippy::all)]

//! datagrip-cli — Slate OS DataGrip database IDE
//!
//! Single personality: `datagrip`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_datagrip(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: datagrip [OPTIONS] [PROJECT]");
        println!("DataGrip v2024.1 (Slate OS) — Multi-engine database IDE");
        println!();
        println!("Options:");
        println!("  --project DIR      Open project directory");
        println!("  --nosplash         Skip splash screen");
        println!("  --disableNonBundledPlugins  Disable plugins");
        println!("  --wait             Wait for files to be closed");
        println!("  diff FILE1 FILE2   Compare files");
        println!("  inspect DIR        Run code inspections");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DataGrip v2024.1.5 (Slate OS)"); return 0; }
    println!("DataGrip v2024.1.5 (Slate OS)");
    println!("  Datasources: 7 configured");
    println!("  Drivers: PostgreSQL, MySQL, Oracle, MSSQL, MongoDB, Cassandra, Redis");
    println!("  Consoles: 4 open");
    println!("  Schemas: 23 introspected");
    println!("  Recent queries: 156");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "datagrip".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_datagrip(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_datagrip};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/datagrip"), "datagrip");
        assert_eq!(basename(r"C:\bin\datagrip.exe"), "datagrip.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("datagrip.exe"), "datagrip");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_datagrip(&["--help".to_string()], "datagrip"), 0);
        assert_eq!(run_datagrip(&["-h".to_string()], "datagrip"), 0);
        let _ = run_datagrip(&["--version".to_string()], "datagrip");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_datagrip(&[], "datagrip");
    }
}
