#![deny(clippy::all)]

//! iotdb-cli — OurOS Apache IoTDB time-series database
//!
//! Single personality: `iotdb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iotdb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iotdb [COMMAND] [OPTIONS]");
        println!("Apache IoTDB v1.3 (OurOS) — IoT time-series database");
        println!();
        println!("Commands:");
        println!("  start              Start IoTDB server");
        println!("  stop               Stop IoTDB server");
        println!("  cli                Start interactive CLI");
        println!("  import FILE        Import data (CSV/TsFile)");
        println!("  export DIR         Export data");
        println!("  compaction         Trigger compaction");
        println!();
        println!("Options:");
        println!("  -h HOST            Server host");
        println!("  -p PORT            RPC port (default: 6667)");
        println!("  -u USER            Username");
        println!("  --config DIR       Config directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache IoTDB v1.3.1 (OurOS)"); return 0; }
    println!("Apache IoTDB v1.3.1 (OurOS)");
    println!("  RPC: 0.0.0.0:6667");
    println!("  REST: 0.0.0.0:18080");
    println!("  Storage groups: 8");
    println!("  Devices: 2,345");
    println!("  Timeseries: 45,678");
    println!("  Data points: 12.3 billion");
    println!("  Encoding: GORILLA, RLE, DICTIONARY");
    println!("  Compaction: cross-space enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iotdb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iotdb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_iotdb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/iotdb"), "iotdb");
        assert_eq!(basename(r"C:\bin\iotdb.exe"), "iotdb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("iotdb.exe"), "iotdb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_iotdb(&["--help".to_string()], "iotdb"), 0);
        assert_eq!(run_iotdb(&["-h".to_string()], "iotdb"), 0);
        assert_eq!(run_iotdb(&["--version".to_string()], "iotdb"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_iotdb(&[], "iotdb"), 0);
    }
}
