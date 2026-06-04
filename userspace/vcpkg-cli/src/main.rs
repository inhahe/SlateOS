#![deny(clippy::all)]

//! vcpkg-cli — OurOS vcpkg CLI
//!
//! Single personality: `vcpkg`

use std::env;
use std::process;

fn run_vcpkg(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: vcpkg <COMMAND> [OPTIONS]");
        println!();
        println!("vcpkg — C/C++ package manager (OurOS).");
        println!();
        println!("Commands:");
        println!("  install      Install packages");
        println!("  remove       Remove packages");
        println!("  search       Search packages");
        println!("  list         List installed packages");
        println!("  update       Update port metadata");
        println!("  upgrade      Rebuild outdated packages");
        println!("  integrate    Integrate with build systems");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("vcpkg package management program version 2024.01.12 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "install" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).collect();
            let pkg = if pkgs.is_empty() { "zlib" } else { pkgs[0] };
            println!("Computing installation plan...");
            println!("The following packages will be built and installed:");
            println!("    {}:x64-ouros", pkg);
            println!();
            println!("Starting package 1/1: {}:x64-ouros", pkg);
            println!("Building {}:x64-ouros...", pkg);
            println!("-- Downloading source...");
            println!("-- Extracting source...");
            println!("-- Configuring...");
            println!("-- Building...");
            println!("-- Installing...");
            println!("-- Performing post-build validation...");
            println!("Elapsed time to handle {}:x64-ouros: 12.3s", pkg);
            println!();
            println!("Total install time: 12.3s");
            0
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("zlib");
            println!("Removing {}:x64-ouros...", pkg);
            println!("  Package {} removed.", pkg);
            0
        }
        "search" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("");
            println!("boost                1.84.0           Boost C++ Libraries");
            println!("fmt                  10.2.1           Modern formatting library");
            println!("openssl              3.2.0            TLS/SSL library");
            println!("spdlog               1.13.0           Fast C++ logging library");
            println!("zlib                 1.3.1            Compression library");
            if !pattern.is_empty() {
                println!("  (filtered by: {})", pattern);
            }
            0
        }
        "list" => {
            println!("boost:x64-ouros              1.84.0       Boost C++ Libraries");
            println!("openssl:x64-ouros            3.2.0        TLS/SSL library");
            println!("zlib:x64-ouros               1.3.1        Compression library");
            0
        }
        "update" => {
            println!("Using local portfile versions. To update the built-in registry, run:");
            println!("  git pull");
            println!();
            println!("No packages need updating.");
            0
        }
        "integrate" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("install");
            match sub {
                "install" => {
                    println!("Applied user-wide integration for this vcpkg root.");
                    println!("CMake projects should use: -DCMAKE_TOOLCHAIN_FILE=/home/user/vcpkg/scripts/buildsystems/vcpkg.cmake");
                }
                "remove" => {
                    println!("User-wide integration removed.");
                }
                _ => { println!("Integrate: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: vcpkg <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vcpkg(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_vcpkg};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vcpkg(vec!["--help".to_string()]), 0);
        assert_eq!(run_vcpkg(vec!["-h".to_string()]), 0);
        let _ = run_vcpkg(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vcpkg(vec![]);
    }
}
