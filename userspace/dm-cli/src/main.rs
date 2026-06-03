#![deny(clippy::all)]

//! dm-cli — OurOS device-mapper tools
//!
//! Multi-personality: `dmsetup`, `dmstats`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dm(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} COMMAND [OPTIONS]", prog);
        match prog {
            "dmstats" => {
                println!("dmstats (OurOS) — Device-mapper statistics");
                println!("  create     Create statistics region");
                println!("  delete     Delete statistics region");
                println!("  list       List statistics regions");
                println!("  print      Print statistics");
                println!("  report     Report statistics");
            }
            _ => {
                println!("dmsetup (OurOS) — Device-mapper management");
                println!("  create NAME TABLE  Create device");
                println!("  remove NAME        Remove device");
                println!("  table NAME         Show table");
                println!("  status NAME        Show status");
                println!("  info NAME          Show info");
                println!("  ls                 List devices");
                println!("  deps NAME          Show dependencies");
                println!("  targets            List target types");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dmsetup v1.02.196 (OurOS)"); return 0; }
    match prog {
        "dmstats" => {
            println!("dmstats (OurOS)");
            println!("  Region  Start        Length       Step       Read/s    Write/s");
            println!("  0       0            1048576      65536      1234.5    567.8");
            println!("  1       1048576      2097152      65536      890.1     234.5");
        }
        _ => {
            println!("dmsetup (OurOS) — Device-Mapper");
            println!("  Devices:");
            println!("    vg_root-root (253:0)");
            println!("    vg_root-home (253:1)");
            println!("    vg_root-swap (253:2)");
            println!("    vg_data-data (253:3)");
            println!("  Targets: linear, striped, mirror, snapshot, thin, cache, crypt");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dmsetup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dm"), "dm");
        assert_eq!(basename(r"C:\bin\dm.exe"), "dm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dm.exe"), "dm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dm(&["--help".to_string()], "dm"), 0);
        assert_eq!(run_dm(&["-h".to_string()], "dm"), 0);
        assert_eq!(run_dm(&["--version".to_string()], "dm"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dm(&[], "dm"), 0);
    }
}
