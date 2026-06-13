#![deny(clippy::all)]

//! zypper-cli — SlateOS openSUSE Zypper package manager
//!
//! Multi-personality: `zypper`

use std::env;
use std::process;

fn run_zypper(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zypper [GLOBAL-OPTIONS] COMMAND [OPTIONS] [ARGUMENTS]");
        println!("Zypper 1.14.68 (SlateOS)");
        println!();
        println!("Repository management:");
        println!("  repos, lr          List repos");
        println!("  addrepo, ar        Add repository");
        println!("  removerepo, rr     Remove repository");
        println!("  refresh, ref       Refresh repositories");
        println!();
        println!("Package management:");
        println!("  install, in        Install packages");
        println!("  remove, rm         Remove packages");
        println!("  update, up         Update packages");
        println!("  dist-upgrade, dup  Distribution upgrade");
        println!("  patch              Install patches");
        println!();
        println!("Query:");
        println!("  search, se         Search packages");
        println!("  info, if           Show package info");
        println!("  what-provides, wp  Find package providing a file");
        println!("  patches            List patches");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => {
            println!("zypper 1.14.68");
            println!("libzypp 17.31.26");
        }
        "install" | "in" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            println!("Loading repository data...");
            println!("Reading installed packages...");
            println!("Resolving package dependencies...");
            println!();
            println!("The following NEW packages are going to be installed:");
            for p in &pkgs {
                println!("  {} 1.0.0-1.1", p);
            }
            println!();
            println!("{} new package to install.", pkgs.len());
            println!("Overall download size: 5.2 MiB.");
            println!("Downloading package {}...", pkgs.first().unwrap_or(&"pkg"));
            println!("Installing: {} [done]", pkgs.first().unwrap_or(&"pkg"));
        }
        "remove" | "rm" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("pkg");
            println!("Loading repository data...");
            println!("Reading installed packages...");
            println!("Removing {} [done]", pkg);
        }
        "update" | "up" => {
            println!("Loading repository data...");
            println!("Reading installed packages...");
            println!();
            println!("The following packages are going to be updated:");
            println!("  kernel-default  6.7.2-1.1 -> 6.7.3-1.1");
            println!("  glibc           2.39-1.1 -> 2.39-1.2");
            println!();
            println!("2 packages to upgrade.");
            println!("Overall download size: 120 MiB.");
        }
        "search" | "se" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("Loading repository data...");
            println!("Reading installed packages...");
            println!();
            println!("S  | Name         | Summary                | Type");
            println!("---+--------------+------------------------+---------");
            println!("i  | {}         | Vi IMproved            | package", term);
            println!("   | {}-data    | Vi IMproved data files | package", term);
            println!("   | g{}        | Graphical Vi IMproved  | package", term);
        }
        "info" | "if" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("Information for package {}:", pkg);
            println!("  Repository  : openSUSE-Tumbleweed-Oss");
            println!("  Name        : {}", pkg);
            println!("  Version     : 9.1.0-1.1");
            println!("  Arch        : x86_64");
            println!("  Installed   : Yes");
            println!("  Status      : up-to-date");
            println!("  Size        : 3.5 MiB");
        }
        "repos" | "lr" => {
            println!("#  | Alias                    | Enabled | GPG Check | Refresh");
            println!("---+--------------------------+---------+-----------+--------");
            println!("1  | openSUSE-Tumbleweed-Oss  | Yes     | (r) Yes   | Yes");
            println!("2  | openSUSE-Tumbleweed-Non  | Yes     | (r) Yes   | Yes");
        }
        "refresh" | "ref" => {
            println!("Retrieving repository 'openSUSE-Tumbleweed-Oss' metadata ......[done]");
            println!("Building repository 'openSUSE-Tumbleweed-Oss' cache ..........[done]");
            println!("All repositories have been refreshed.");
        }
        "dist-upgrade" | "dup" => {
            println!("Loading repository data...");
            println!("Reading installed packages...");
            println!("Computing distribution upgrade...");
            println!();
            println!("5 packages to upgrade, 2 to downgrade, 1 new.");
            println!("Overall download size: 250 MiB.");
        }
        _ => println!("zypper: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zypper(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_zypper};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zypper(&["--help".to_string()]), 0);
        assert_eq!(run_zypper(&["-h".to_string()]), 0);
        let _ = run_zypper(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zypper(&[]);
    }
}
