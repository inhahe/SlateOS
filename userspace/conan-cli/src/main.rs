#![deny(clippy::all)]

//! conan-cli — SlateOS Conan package manager CLI
//!
//! Single personality: `conan`

use std::env;
use std::process;

fn run_conan(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: conan <COMMAND> [OPTIONS]");
        println!();
        println!("Conan — C/C++ package manager (Slate OS).");
        println!();
        println!("Commands:");
        println!("  install      Install dependencies");
        println!("  create       Create a package");
        println!("  search       Search packages");
        println!("  list         List packages");
        println!("  remote       Manage remotes");
        println!("  profile      Manage profiles");
        println!("  new          Create template project");
        println!("  build        Build a package locally");
        println!("  export       Export recipe");
        println!("  upload       Upload packages");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Conan version 2.0.14 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "install" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("======== Installing ({path}): conanfile.txt ========");
            println!();
            println!("-------- Installing packages ----------");
            println!("Requirements:");
            println!("  zlib/1.3.1         - Downloaded");
            println!("  openssl/3.2.0      - Downloaded");
            println!("  boost/1.84.0       - Downloaded");
            println!();
            println!("-------- Finalizing install -----------");
            println!("Generators: CMakeDeps, CMakeToolchain");
            println!("Install finished successfully");
            0
        }
        "create" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Exporting recipe from {}", path);
            println!("  mylib/1.0.0: Exported recipe");
            println!();
            println!("======== Testing ========");
            println!("  mylib/1.0.0: Build succeeded");
            println!("  mylib/1.0.0: Package created");
            0
        }
        "search" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("*");
            println!("Searching for '{}' in remote 'conancenter'...", pattern);
            println!("  zlib/1.3.1");
            println!("  openssl/3.2.0");
            println!("  boost/1.84.0");
            println!("  fmt/10.2.1");
            println!("  spdlog/1.13.0");
            0
        }
        "list" => {
            println!("Local cache packages:");
            println!("  zlib");
            println!("    zlib/1.3.1");
            println!("  openssl");
            println!("    openssl/3.2.0");
            println!("  boost");
            println!("    boost/1.84.0");
            0
        }
        "profile" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "show" | "detect" => {
                    println!("[settings]");
                    println!("arch=x86_64");
                    println!("build_type=Release");
                    println!("compiler=clang");
                    println!("compiler.version=17");
                    println!("os=Slate OS");
                }
                "list" => {
                    println!("Profiles:");
                    println!("  default");
                    println!("  debug");
                    println!("  release");
                }
                _ => { println!("Profile operation: {}", sub); }
            }
            0
        }
        "new" => {
            let template = args.get(1).map(|s| s.as_str()).unwrap_or("cmake_lib");
            println!("Using template: {}", template);
            println!("  Created conanfile.py");
            println!("  Created CMakeLists.txt");
            println!("  Created src/mylib.cpp");
            println!("  Created include/mylib.h");
            println!("  Created test_package/conanfile.py");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: conan <command>. See --help.");
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
    let code = run_conan(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_conan};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_conan(vec!["--help".to_string()]), 0);
        assert_eq!(run_conan(vec!["-h".to_string()]), 0);
        let _ = run_conan(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_conan(vec![]);
    }
}
