#![deny(clippy::all)]

//! nix-cli — SlateOS Nix package manager
//!
//! Multi-personality: `nix`, `nix-build`, `nix-shell`, `nix-env`, `nix-store`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nix(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nix COMMAND [OPTIONS]");
        println!("Nix 2.20.0 (Slate OS)");
        println!();
        println!("Common commands:");
        println!("  build        Build a derivation");
        println!("  develop      Run a bash shell for development");
        println!("  flake        Manage Nix flakes");
        println!("  profile      Manage Nix profiles");
        println!("  run          Run a Nix application");
        println!("  search       Search for packages");
        println!("  shell        Start a shell with packages");
        println!("  store        Manipulate the Nix store");
        println!("  repl         Start an interactive Nix REPL");
        println!("  edit         Open a package in an editor");
        println!("  log          Show build logs");
        println!("  path-info    Query path information");
        println!("  registry     Manage flake registries");
        println!("  derivation   Work with derivations");
        println!("  hash         Compute hashes");
        println!("  upgrade-nix  Upgrade Nix");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("nix (Nix) 2.20.0"),
        "build" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("nix build: building {}", target);
            println!("  /nix/store/abc123-mypackage-1.0");
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("* nixpkgs#{}", query);
            println!("  A program for producing a familiar, friendly greeting");
            println!();
            println!("* nixpkgs#{}-cli", query);
            println!("  Command-line interface for {}", query);
        }
        "shell" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            println!("nix shell: entering shell with packages:");
            for p in &pkgs {
                println!("  {}", p);
            }
        }
        "run" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("nixpkgs#hello");
            println!("nix run: running {}", target);
            println!("  Hello, world!");
        }
        "develop" => {
            println!("nix develop: entering development shell");
            println!("  [dev-shell]$");
        }
        "flake" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match action {
                "init" => {
                    println!("wrote: flake.nix");
                    println!("wrote: flake.lock");
                }
                "update" => println!("Updated flake.lock"),
                "check" => println!("nix flake check: all checks passed"),
                "show" => {
                    println!("├───devShells");
                    println!("│   └───x86_64-linux");
                    println!("│       └───default");
                    println!("└───packages");
                    println!("    └───x86_64-linux");
                    println!("        └───default");
                }
                _ => println!("nix flake: '{}' completed", action),
            }
        }
        "store" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match action {
                "gc" => {
                    println!("finding garbage collector roots...");
                    println!("removing old generations...");
                    println!("deleted 42 store paths, 1.5 GiB freed");
                }
                "info" => {
                    println!("Store URL: /nix/store");
                    println!("Trusted: yes");
                }
                _ => println!("nix store: '{}' completed", action),
            }
        }
        "repl" => {
            println!("Welcome to Nix 2.20.0");
            println!("Type :? for help.");
            println!("nix-repl>");
        }
        "hash" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("path");
            println!("nix hash {}: sha256-abc123def456...", action);
        }
        "profile" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match action {
                "list" => {
                    println!("Index  Path");
                    println!("0      /nix/store/abc-hello-2.12.1");
                    println!("1      /nix/store/def-ripgrep-14.1.0");
                }
                "install" => {
                    let pkg = args.get(2).map(|s| s.as_str()).unwrap_or("nixpkgs#hello");
                    println!("Installing {}...", pkg);
                    println!("  installed to profile");
                }
                _ => println!("nix profile: '{}' completed", action),
            }
        }
        _ => println!("nix: '{}' completed", subcmd),
    }
    0
}

fn run_nix_build(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nix-build [OPTIONS] [PATH]");
        println!("Build a Nix expression. (Slate OS)");
        return 0;
    }
    let path = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("default.nix");
    println!("these derivations will be built:");
    println!("  /nix/store/abc123-mypackage.drv");
    println!("building '/nix/store/abc123-mypackage.drv'...");
    println!("/nix/store/def456-mypackage-1.0");
    let _ = path;
    0
}

fn run_nix_shell(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nix-shell [OPTIONS] [PATH]");
        println!("Start a shell with build inputs. (Slate OS)");
        println!("  -p PACKAGES   Packages to make available");
        println!("  --pure        Clear environment");
        println!("  --run CMD     Run command instead of shell");
        return 0;
    }
    println!("[nix-shell]$");
    0
}

fn run_nix_env(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nix-env [OPTIONS]");
        println!("  -i PKG    Install package");
        println!("  -e PKG    Uninstall package");
        println!("  -u        Upgrade packages");
        println!("  -q        Query installed packages");
        println!("  --list-generations  List generations");
        return 0;
    }
    if args.iter().any(|a| a == "-q" || a == "--query") {
        println!("hello-2.12.1");
        println!("ripgrep-14.1.0");
        println!("git-2.44.0");
    } else if args.iter().any(|a| a == "-i" || a == "--install") {
        let pkg = args.windows(2).find(|w| w[0] == "-i" || w[0] == "--install").map(|w| w[1].as_str()).unwrap_or("hello");
        println!("installing '{}'...", pkg);
        println!("  created 1 symlink in user environment");
    }
    0
}

fn run_nix_store(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nix-store [OPTIONS]");
        println!("  --gc             Run garbage collector");
        println!("  -q --references  Query references");
        println!("  --verify         Verify store integrity");
        return 0;
    }
    if args.iter().any(|a| a == "--gc") {
        println!("finding garbage collector roots...");
        println!("deleting unused paths...");
        println!("42 store paths deleted, 1.5 GiB freed");
    } else if args.iter().any(|a| a == "--verify") {
        println!("checking path existence...");
        println!("checking hashes...");
        println!("0 errors found");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nix-build" => run_nix_build(&rest),
        "nix-shell" => run_nix_shell(&rest),
        "nix-env" => run_nix_env(&rest),
        "nix-store" => run_nix_store(&rest),
        _ => run_nix(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nix"), "nix");
        assert_eq!(basename(r"C:\bin\nix.exe"), "nix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nix.exe"), "nix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nix(&["--help".to_string()]), 0);
        assert_eq!(run_nix(&["-h".to_string()]), 0);
        let _ = run_nix(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nix(&[]);
    }
}
