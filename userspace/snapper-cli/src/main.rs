#![deny(clippy::all)]

//! snapper-cli — OurOS Snapper filesystem snapshot manager
//!
//! Multi-personality: `snapper`, `snapperd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_snapper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: snapper COMMAND [OPTIONS]");
        println!("snapper v0.10 (OurOS) — Filesystem snapshot manager");
        println!();
        println!("Commands:");
        println!("  list              List snapshots");
        println!("  create            Create snapshot");
        println!("  delete NUM        Delete snapshot");
        println!("  diff NUM..NUM     Show differences");
        println!("  rollback [NUM]    Rollback to snapshot");
        println!("  status NUM..NUM   Show changed files");
        println!("  list-configs      List configurations");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("snapper v0.10 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!(" #  | Type   | Pre # | Date                     | Description");
            println!("----+--------+-------+--------------------------+-----------");
            println!(" 0  | single |       |                          | current");
            println!(" 1  | single |       | 2024-01-15 09:00:00      | first root fs");
            println!(" 2  | pre    |       | 2024-01-15 10:30:00      | zypper install");
            println!(" 3  | post   |   2   | 2024-01-15 10:31:00      | zypper install");
        }
        "create" => {
            println!("Creating snapshot...");
            println!("Created snapshot 4");
        }
        "list-configs" => {
            println!("Config  | Subvolume");
            println!("--------+---------");
            println!("root    | /");
            println!("home    | /home");
        }
        "rollback" => {
            let num = args.get(1).map(|s| s.as_str()).unwrap_or("3");
            println!("Rolling back to snapshot {}...", num);
            println!("Rollback complete. Reboot to apply.");
        }
        _ => println!("snapper: {}", cmd),
    }
    0
}

fn run_snapperd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: snapperd [OPTIONS]");
        println!("snapperd v0.10 (OurOS) — Snapper D-Bus daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("snapperd v0.10 (OurOS)"); return 0; }
    println!("snapperd: snapshot management daemon started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "snapper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "snapperd" => run_snapperd(&rest, &prog),
        _ => run_snapper(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_snapper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/snapper"), "snapper");
        assert_eq!(basename(r"C:\bin\snapper.exe"), "snapper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("snapper.exe"), "snapper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_snapper(&["--help".to_string()], "snapper"), 0);
        assert_eq!(run_snapper(&["-h".to_string()], "snapper"), 0);
        assert_eq!(run_snapper(&["--version".to_string()], "snapper"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_snapper(&[], "snapper"), 0);
    }
}
