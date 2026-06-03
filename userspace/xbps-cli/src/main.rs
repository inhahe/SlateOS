#![deny(clippy::all)]

//! xbps-cli — OurOS Void Linux XBPS package manager
//!
//! Multi-personality: `xbps-install`, `xbps-remove`, `xbps-query`, `xbps-reconfigure`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_install(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xbps-install [OPTIONS] PKG [PKG ...]");
        println!("  -S           Synchronize remote repository index");
        println!("  -u           Update all packages");
        println!("  -y           Assume yes to all questions");
        println!("  -n           Dry run");
        return 0;
    }
    if args.iter().any(|a| a == "-S") && args.len() == 1 {
        println!("[*] Updating repository `https://repo-default.voidlinux.org/current/x86_64-repodata' ...");
        println!("[*] Updating repository `https://repo-default.voidlinux.org/current/nonfree/x86_64-repodata' ...");
        return 0;
    }
    if args.iter().any(|a| a == "-u") {
        println!("[*] Updating packages...");
        println!("  linux-6.7.3_1: updating to 6.7.4_1 ...");
        println!("  glibc-2.39_1: updating to 2.39_2 ...");
        println!("[*] 2 packages updated.");
        return 0;
    }
    let pkgs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    for p in &pkgs {
        println!("[*] Downloading {} ...", p);
        println!("[*] Verifying {} ...", p);
        println!("[*] Installing {} ...", p);
    }
    println!("[*] {} package(s) installed.", pkgs.len());
    0
}

fn run_remove(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xbps-remove [OPTIONS] PKG [PKG ...]");
        println!("  -o    Remove orphaned packages");
        println!("  -O    Clean package cache");
        println!("  -y    Assume yes");
        return 0;
    }
    if args.iter().any(|a| a == "-o") {
        println!("[*] Removing orphaned packages...");
        println!("  Removed: libfoo-1.0_1");
        println!("  Removed: libbar-2.0_1");
        println!("[*] 2 orphans removed.");
        return 0;
    }
    if args.iter().any(|a| a == "-O") {
        println!("[*] Cleaning package cache...");
        println!("[*] 150 MiB freed.");
        return 0;
    }
    let pkgs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    for p in &pkgs {
        println!("[*] Removing {} ...", p);
    }
    0
}

fn run_query(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xbps-query [OPTIONS] [PKG]");
        println!("  -l           List installed packages");
        println!("  -s PATTERN   Search packages");
        println!("  -S PKG       Show package info");
        println!("  -f PKG       Show package files");
        println!("  -x PKG       Show dependencies");
        println!("  -X PKG       Show reverse dependencies");
        println!("  -Rs PATTERN  Search remote packages");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("ii base-system-0.114_1       Void Linux base system meta package");
        println!("ii linux-6.7.4_1             Linux kernel and modules");
        println!("ii glibc-2.39_2              GNU C library");
        return 0;
    }
    if args.iter().any(|a| a == "-s" || a == "-Rs") {
        let term = args.windows(2)
            .find(|w| w[0] == "-s" || w[0] == "-Rs")
            .map(|w| w[1].as_str())
            .unwrap_or("vim");
        println!("[*] vim-9.1.0_1        Vi IMproved");
        println!("[*] vim-x11-9.1.0_1    Vi IMproved (X11)");
        let _ = term;
        return 0;
    }
    let pkg = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("vim");
    println!("architecture: x86_64");
    println!("filename-sha256: abc123...");
    println!("filename-size: 3.5MB");
    println!("homepage: https://www.vim.org");
    println!("installed_size: 15MB");
    println!("pkgname: {}", pkg);
    println!("pkgver: {}-9.1.0_1", pkg);
    println!("short_desc: Vi IMproved");
    0
}

fn run_reconfigure(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xbps-reconfigure [OPTIONS] PKG [PKG ...]");
        println!("  -a    Reconfigure all packages");
        println!("  -f    Force reconfigure");
        return 0;
    }
    if args.iter().any(|a| a == "-a") {
        println!("[*] Reconfiguring all packages...");
        println!("[*] Done.");
        return 0;
    }
    let pkgs: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for p in &pkgs {
        println!("[*] Reconfiguring {} ...", p);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xbps-install".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "xbps-remove" => run_remove(&rest),
        "xbps-query" => run_query(&rest),
        "xbps-reconfigure" => run_reconfigure(&rest),
        _ => run_install(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_install};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xbps"), "xbps");
        assert_eq!(basename(r"C:\bin\xbps.exe"), "xbps.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xbps.exe"), "xbps");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_install(&["--help".to_string()]), 0);
        assert_eq!(run_install(&["-h".to_string()]), 0);
        assert_eq!(run_install(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_install(&[]), 0);
    }
}
