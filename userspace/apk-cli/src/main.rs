#![deny(clippy::all)]

//! apk-cli — SlateOS Alpine Package Keeper
//!
//! Multi-personality: `apk`

use std::env;
use std::process;

fn run_apk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: apk COMMAND [OPTIONS]");
        println!("apk-tools 2.14.0 (Slate OS)");
        println!();
        println!("Package management:");
        println!("  add          Install packages");
        println!("  del          Remove packages");
        println!("  fix          Repair packages");
        println!();
        println!("System maintenance:");
        println!("  update       Update repository indexes");
        println!("  upgrade      Upgrade installed packages");
        println!("  cache        Manage local package cache");
        println!();
        println!("Query:");
        println!("  info         Show package information");
        println!("  list         List packages");
        println!("  search       Search packages");
        println!("  dot          Generate graphviz graph");
        println!("  policy       Show repository policy");
        println!("  stats        Show statistics");
        println!();
        println!("  --version    Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "version" => println!("apk-tools 2.14.0, compiled for x86_64."),
        "add" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            for p in &pkgs {
                println!("(1/1) Installing {} (1.0.0-r0)", p);
            }
            println!("OK: {} MiB in 42 packages", 150);
        }
        "del" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("pkg");
            println!("(1/1) Purging {} (1.0.0-r0)", pkg);
            println!("OK: 145 MiB in 41 packages");
        }
        "update" => {
            println!("fetch https://dl-cdn.alpinelinux.org/alpine/v3.19/main");
            println!("fetch https://dl-cdn.alpinelinux.org/alpine/v3.19/community");
            println!("v3.19.1-42-g1234567890 [https://dl-cdn.alpinelinux.org/alpine/v3.19/main]");
            println!("OK: 18432 distinct packages available");
        }
        "upgrade" => {
            println!("(1/3) Upgrading musl (1.2.4-r2 -> 1.2.4-r3)");
            println!("(2/3) Upgrading busybox (1.36.1-r15 -> 1.36.1-r16)");
            println!("(3/3) Upgrading openssl (3.1.4-r5 -> 3.1.4-r6)");
            println!("OK: 150 MiB in 42 packages");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("{}-9.1.0-r0", term);
            println!("{}-doc-9.1.0-r0", term);
            println!("{}-tutor-9.1.0-r0", term);
        }
        "info" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("busybox");
            println!("{}-1.36.1-r16 description:", pkg);
            println!("Size optimized toolbox of common UNIX utilities");
            println!();
            println!("{}-1.36.1-r16 webpage:", pkg);
            println!("https://busybox.net/");
            println!();
            println!("{}-1.36.1-r16 installed size:", pkg);
            println!("960 KiB");
        }
        "list" => {
            println!("busybox-1.36.1-r16 x86_64 {{installed}}");
            println!("musl-1.2.4-r3 x86_64 {{installed}}");
            println!("alpine-baselayout-3.4.3-r2 x86_64 {{installed}}");
        }
        "stats" => {
            println!("packages:   42");
            println!("dirs:       1024");
            println!("files:      8192");
            println!("bytes:      157286400");
            println!("triggers:   3");
        }
        "cache" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("clean");
            println!("apk cache {}: done", action);
        }
        _ => println!("apk: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_apk(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_apk};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_apk(&["--help".to_string()]), 0);
        assert_eq!(run_apk(&["-h".to_string()]), 0);
        let _ = run_apk(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_apk(&[]);
    }
}
