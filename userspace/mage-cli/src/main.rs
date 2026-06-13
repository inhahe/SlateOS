#![deny(clippy::all)]

//! mage-cli — Slate OS Mage AI data pipeline
//!
//! Single personality: `mage`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mage [COMMAND] [OPTIONS]");
        println!("Mage v0.9 (Slate OS) — Open-source data pipeline tool");
        println!();
        println!("Commands:");
        println!("  start              Start Mage server");
        println!("  init PROJECT       Initialize new project");
        println!("  run PIPELINE       Run pipeline");
        println!("  test               Run tests");
        println!("  clean              Clean cached data");
        println!("  create_spark_cluster  Create Spark cluster");
        println!();
        println!("Options:");
        println!("  --host ADDR        Server host");
        println!("  --port PORT        Server port (default: 6789)");
        println!("  --project DIR      Project directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mage v0.9.73 (Slate OS)"); return 0; }
    println!("Mage v0.9.73 (Slate OS)");
    println!("  Server: http://0.0.0.0:6789");
    println!("  Pipelines: 12 (8 batch, 3 streaming, 1 integration)");
    println!("  Blocks: 67 total");
    println!("  Triggers: 5 active");
    println!("  Variables: 23 configured");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mage(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mage"), "mage");
        assert_eq!(basename(r"C:\bin\mage.exe"), "mage.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mage.exe"), "mage");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mage(&["--help".to_string()], "mage"), 0);
        assert_eq!(run_mage(&["-h".to_string()], "mage"), 0);
        let _ = run_mage(&["--version".to_string()], "mage");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mage(&[], "mage");
    }
}
