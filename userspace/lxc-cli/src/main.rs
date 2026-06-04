#![deny(clippy::all)]

//! lxc-cli — OurOS LXC/LXD container CLI
//!
//! Multi-personality: `lxc`, `lxc-create`, `lxc-start`, `lxc-stop`, `lxc-ls`, `lxc-info`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_lxc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: lxc COMMAND [OPTIONS]");
        println!();
        println!("LXD — system container and VM manager (OurOS).");
        println!();
        println!("Commands:");
        println!("  launch IMAGE [NAME]  Create and start");
        println!("  init IMAGE [NAME]    Create container");
        println!("  start NAME           Start container");
        println!("  stop NAME            Stop container");
        println!("  delete NAME          Delete container");
        println!("  list                 List containers");
        println!("  info NAME            Container info");
        println!("  exec NAME -- CMD     Execute command");
        println!("  file push/pull       Transfer files");
        println!("  image list           List images");
        println!("  network list         List networks");
        println!("  storage list         List storage pools");
        println!("  snapshot             Manage snapshots");
        println!("  config               Manage configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("lxc 5.21 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    match cmd {
        "launch" => {
            let image = rest.first().unwrap_or(&"ubuntu:22.04");
            let name = rest.get(1).unwrap_or(&"container1");
            println!("Creating {} (image: {})", name, image);
            println!("Starting {}", name);
        }
        "init" => {
            let name = rest.get(1).unwrap_or(&"container1");
            println!("Creating {}", name);
        }
        "start" => {
            let name = rest.first().unwrap_or(&"container1");
            println!("Starting {}", name);
        }
        "stop" => {
            let name = rest.first().unwrap_or(&"container1");
            println!("Stopping {}", name);
        }
        "delete" => {
            let name = rest.first().unwrap_or(&"container1");
            println!("Deleting {}", name);
        }
        "list" => {
            println!("+------------+---------+---------------------+------+-----------+");
            println!("| NAME       | STATE   | IPV4                | TYPE | SNAPSHOTS |");
            println!("+------------+---------+---------------------+------+-----------+");
            println!("| web-server | RUNNING | 10.0.0.2 (eth0)     | CONT | 0         |");
            println!("| db-server  | RUNNING | 10.0.0.3 (eth0)     | CONT | 1         |");
            println!("| test-env   | STOPPED |                     | CONT | 0         |");
            println!("+------------+---------+---------------------+------+-----------+");
        }
        "info" => {
            let name = rest.first().unwrap_or(&"web-server");
            println!("Name: {}", name);
            println!("Status: RUNNING");
            println!("Type: container");
            println!("Architecture: x86_64");
            println!("PID: 12345");
            println!("Resources:");
            println!("  Processes: 42");
            println!("  Memory usage:");
            println!("    Memory (current): 256.00MiB");
            println!("    Memory (peak):    512.00MiB");
            println!("  Network usage:");
            println!("    eth0:");
            println!("      Bytes received: 1.2GiB");
            println!("      Bytes sent: 890.5MiB");
        }
        "exec" => {
            let name = rest.first().unwrap_or(&"container1");
            println!("Executing in {}...", name);
        }
        "image" => {
            if rest.first() == Some(&"list") {
                println!("+-------+--------+--------+------+-------+");
                println!("| ALIAS | FINGER | PUBLIC | DESC | SIZE  |");
                println!("+-------+--------+--------+------+-------+");
                println!("| u2204 | abc123 | no     | U22  | 350MB |");
                println!("+-------+--------+--------+------+-------+");
            }
        }
        "network" => {
            if rest.first() == Some(&"list") {
                println!("+---------+----------+---------+");
                println!("| NAME    | TYPE     | MANAGED |");
                println!("+---------+----------+---------+");
                println!("| lxdbr0  | bridge   | YES     |");
                println!("+---------+----------+---------+");
            }
        }
        _ => {
            eprintln!("lxc: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_lxc_legacy(args: &[String], cmd_name: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", cmd_name);
        println!();
        println!("{} — LXC legacy command (OurOS).", cmd_name);
        return 0;
    }
    match cmd_name {
        "lxc-create" => println!("Container created."),
        "lxc-start" => println!("Container started."),
        "lxc-stop" => println!("Container stopped."),
        "lxc-ls" => {
            println!("web-server");
            println!("db-server");
            println!("test-env");
        }
        "lxc-info" => {
            let name = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("container1");
            println!("Name:           {}", name);
            println!("State:          RUNNING");
            println!("PID:            12345");
            println!("IP:             10.0.0.2");
            println!("Memory use:     256.00 MiB");
        }
        _ => println!("{}: done", cmd_name),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "lxc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lxc-create" | "lxc-start" | "lxc-stop" | "lxc-ls" | "lxc-info" => run_lxc_legacy(&rest, &prog),
        _ => run_lxc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lxc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lxc"), "lxc");
        assert_eq!(basename(r"C:\bin\lxc.exe"), "lxc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lxc.exe"), "lxc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lxc(&["--help".to_string()]), 0);
        assert_eq!(run_lxc(&["-h".to_string()]), 0);
        let _ = run_lxc(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lxc(&[]);
    }
}
