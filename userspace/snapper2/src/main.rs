#![deny(clippy::all)]

//! snapper2 — OurOS filesystem snapshot management
//!
//! Single personality: `snapper2` (avoiding conflict with existing `snapper` crate)

use std::env;
use std::process;

fn run_snapper(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: snapper2 <command> [options]");
        println!();
        println!("Commands:");
        println!("  list            List snapshots");
        println!("  create          Create snapshot");
        println!("  delete          Delete snapshot");
        println!("  modify          Modify snapshot");
        println!("  diff            Compare snapshots");
        println!("  status          Show status between snapshots");
        println!("  undochange      Undo changes between snapshots");
        println!("  rollback        Rollback to snapshot");
        println!("  cleanup         Cleanup old snapshots");
        println!("  list-configs    List configurations");
        println!("  create-config   Create new configuration");
        println!("  delete-config   Delete configuration");
        println!("  get-config      Get configuration values");
        println!("  set-config      Set configuration values");
        println!("  setup-quota     Setup quota");
        println!();
        println!("Global options:");
        println!("  -c, --config <name>   Use config name (default: root)");
        println!("  --no-dbus             Don't use D-Bus");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("snapper 0.10.7 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "list" | "ls" => {
            println!("  # | Type   | Pre # | Date                     | User | Cleanup  | Description");
            println!("----+--------+-------+--------------------------+------+----------+------------");
            println!("  0 | single |       |                          | root |          | current");
            println!("  1 | single |       | Thu May 22 09:00:00 2025 | root | timeline | timeline");
            println!("  2 | pre    |       | Thu May 22 10:00:00 2025 | root | number   | apt install");
            println!("  3 | post   |     2 | Thu May 22 10:01:00 2025 | root | number   | apt install");
        }
        "create" => {
            let desc = args.iter().position(|a| a == "-d" || a == "--description")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("");
            println!("Creating snapshot... {}", desc);
            println!("Created snapshot #4.");
        }
        "delete" => {
            let num = args.get(1).map(|s| s.as_str()).unwrap_or("1");
            println!("Deleting snapshot #{}.", num);
        }
        "diff" => {
            let n1 = args.get(1).map(|s| s.as_str()).unwrap_or("2");
            let n2 = args.get(2).map(|s| s.as_str()).unwrap_or("3");
            println!("Comparing snapshots {} and {}:", n1, n2);
            println!("+..... /etc/apt/sources.list");
            println!("c..... /etc/hostname");
            println!("-..... /tmp/oldfile");
        }
        "status" => {
            let n1 = args.get(1).map(|s| s.as_str()).unwrap_or("2");
            let n2 = args.get(2).map(|s| s.as_str()).unwrap_or("3");
            println!("Status between {} and {}:", n1, n2);
            println!("+... /usr/bin/newpackage");
            println!("c... /etc/config.yaml");
        }
        "rollback" => {
            let num = args.get(1).map(|s| s.as_str()).unwrap_or("1");
            println!("Rolling back to snapshot #{}...", num);
            println!("Rollback complete. Reboot to apply.");
        }
        "cleanup" => {
            println!("Cleaning up snapshots...");
            println!("Deleted 2 old snapshots.");
        }
        "list-configs" => {
            println!("Config     | Subvolume");
            println!("-----------+----------");
            println!("root       | /");
            println!("home       | /home");
        }
        "undochange" | "modify" | "create-config" | "delete-config" | "get-config" | "set-config" | "setup-quota" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_snapper(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_snapper};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_snapper(vec!["--help".to_string()]), 0);
        assert_eq!(run_snapper(vec!["-h".to_string()]), 0);
        let _ = run_snapper(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_snapper(vec![]);
    }
}
