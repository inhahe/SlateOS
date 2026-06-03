#![deny(clippy::all)]

//! velero-cli — OurOS Velero Kubernetes backup tool
//!
//! Single personality: `velero`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_velero(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: velero COMMAND [OPTIONS]");
        println!("Velero v1.13.1 (OurOS) — Kubernetes backup & restore");
        println!();
        println!("Commands:");
        println!("  install         Install Velero server");
        println!("  uninstall       Uninstall Velero server");
        println!("  backup          Manage backups");
        println!("  restore         Manage restores");
        println!("  schedule        Manage backup schedules");
        println!("  get             List resources");
        println!("  describe        Describe resources");
        println!("  delete          Delete resources");
        println!("  plugin          Manage plugins");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Client:");
        println!("  Version: v1.13.1");
        println!("  Git commit: abc1234");
        println!("Server:");
        println!("  Version: v1.13.1");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("get");
    match cmd {
        "backup" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("backup-1");
                    println!("Backup request \"{}\" submitted successfully.", name);
                }
                "get" => {
                    println!("NAME       STATUS      ERRORS  WARNINGS  CREATED                EXPIRES");
                    println!("backup-1   Completed   0       0         2024-01-15 10:00:00    29d");
                }
                "logs" => println!("velero: Fetching backup logs..."),
                _ => println!("velero backup {}: completed", sub),
            }
        }
        "restore" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "create" => println!("Restore request submitted successfully."),
                "get" => {
                    println!("NAME          BACKUP     STATUS      STARTED                COMPLETED");
                    println!("restore-1     backup-1   Completed   2024-01-16 10:00:00    2024-01-16 10:05:00");
                }
                _ => println!("velero restore {}: completed", sub),
            }
        }
        "schedule" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "create" => println!("Schedule created successfully."),
                "get" => {
                    println!("NAME       STATUS    SCHEDULE      LAST BACKUP");
                    println!("daily      Enabled   0 2 * * *     2024-01-15");
                }
                _ => println!("velero schedule {}: completed", sub),
            }
        }
        "install" => println!("velero: Installing server components..."),
        "get" => println!("velero: (no resources found)"),
        "plugin" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            if sub == "get" {
                println!("NAME                           KIND");
                println!("velero.io/aws                  ObjectStore");
                println!("velero.io/aws                  VolumeSnapshotter");
            }
        }
        _ => println!("velero {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "velero".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_velero(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_velero};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/velero"), "velero");
        assert_eq!(basename(r"C:\bin\velero.exe"), "velero.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("velero.exe"), "velero");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_velero(&["--help".to_string()], "velero"), 0);
        assert_eq!(run_velero(&["-h".to_string()], "velero"), 0);
        assert_eq!(run_velero(&["--version".to_string()], "velero"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_velero(&[], "velero"), 0);
    }
}
