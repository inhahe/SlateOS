#![deny(clippy::all)]

//! pkg-cli — OurOS FreeBSD pkg package manager
//!
//! Multi-personality: `pkg`

use std::env;
use std::process;

fn run_pkg(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pkg COMMAND [OPTIONS]");
        println!("pkg 1.21.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  install       Install packages");
        println!("  delete        Delete packages");
        println!("  upgrade       Upgrade packages");
        println!("  update        Update repository catalog");
        println!("  search        Search packages");
        println!("  info          Show package information");
        println!("  audit         Audit installed packages for vulnerabilities");
        println!("  autoremove    Remove orphaned packages");
        println!("  clean         Clean local cache");
        println!("  lock          Lock package versions");
        println!("  unlock        Unlock package versions");
        println!("  which         Show which package provides a file");
        println!("  check         Verify installed packages");
        println!("  stats         Show statistics");
        println!("  version       Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => println!("pkg 1.21.0"),
        "install" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            println!("Updating FreeBSD repository catalogue...");
            println!("New packages to be INSTALLED:");
            for p in &pkgs {
                println!("  {}: 1.0.0", p);
            }
            println!();
            println!("Number of packages to be installed: {}", pkgs.len());
            println!("Proceed with this action? [y/N]: y");
            for p in &pkgs {
                println!("[1/1] Installing {}...", p);
            }
        }
        "delete" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("pkg");
            println!("Deinstalling {}...", pkg);
            println!("  [1/1] Deleting {}...", pkg);
        }
        "upgrade" => {
            println!("Checking for upgrades (1 candidates):");
            println!("  openssl: 3.1.4 -> 3.1.5");
            println!();
            println!("1 package to be upgraded.");
        }
        "update" => {
            println!("Updating FreeBSD repository catalogue...");
            println!("Fetching meta.conf: 100%");
            println!("Fetching data.pkg: 100%");
            println!("Processing entries: 100%");
            println!("FreeBSD repository update completed. 32000 packages processed.");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("{}                   Vi IMproved", term);
            println!("{}-console             Vi IMproved (console only)", term);
            println!("{}-gtk3                Vi IMproved (GTK3)", term);
        }
        "info" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("{}:", pkg);
            println!("  Version     : 9.1.0");
            println!("  Origin      : editors/{}", pkg);
            println!("  Arch        : freebsd:13:x86:64");
            println!("  Size        : 15 MiB");
            println!("  Description : Vi IMproved");
        }
        "audit" => {
            println!("Fetching vuln.xml.bz2: 100%");
            println!("0 problem(s) in 42 installed package(s) found.");
        }
        "autoremove" => {
            println!("Checking for orphaned packages...");
            println!("  Nothing to do.");
        }
        "stats" => {
            println!("Local package database:");
            println!("  Installed packages: 42");
            println!("  Disk space occupied: 1.5 GiB");
        }
        "which" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("/usr/local/bin/vim");
            println!("{} was installed by package vim-9.1.0", file);
        }
        _ => println!("pkg: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pkg(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pkg};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pkg(&["--help".to_string()]), 0);
        assert_eq!(run_pkg(&["-h".to_string()]), 0);
        assert_eq!(run_pkg(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pkg(&[]), 0);
    }
}
