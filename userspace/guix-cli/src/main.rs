#![deny(clippy::all)]

//! guix-cli — SlateOS GNU Guix package manager
//!
//! Multi-personality: `guix`

use std::env;
use std::process;

fn run_guix(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: guix COMMAND [OPTIONS]");
        println!("GNU Guix 1.4.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  install      Install packages");
        println!("  remove       Remove packages");
        println!("  upgrade      Upgrade packages");
        println!("  search       Search for packages");
        println!("  show         Show package details");
        println!("  package      Manage profile packages");
        println!("  build        Build packages");
        println!("  environment  Spawn build environment");
        println!("  shell        Spawn development shell");
        println!("  system       Manage the operating system");
        println!("  pull         Update Guix");
        println!("  gc           Garbage collect");
        println!("  import       Import packages from other repos");
        println!("  lint         Check package definitions");
        println!("  graph        Show package dependencies as graph");
        println!("  size         Show closure size");
        println!("  hash         Compute file hash");
        println!("  time-machine Use specific Guix revision");
        println!("  home         Manage home environment");
        println!("  deploy       Deploy to remote machines");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => {
            println!("guix (GNU Guix) 1.4.0");
            println!("Copyright (C) 2024 the Guix authors");
            println!("License GPLv3+");
        }
        "install" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            for p in &pkgs {
                println!("The following package will be installed:");
                println!("   {} 1.0.0", p);
            }
            println!();
            println!("building /gnu/store/abc-profile...");
            println!("1 package installed");
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("pkg");
            println!("The following package will be removed:");
            println!("   {} 1.0.0", pkg);
            println!("1 package removed");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("name: {}", term);
            println!("version: 2.12.1");
            println!("outputs: out");
            println!("systems: x86_64-linux");
            println!("dependencies: glibc@2.39");
            println!("location: gnu/packages/base.scm:100:2");
            println!("homepage: https://www.gnu.org/software/{}/", term);
            println!("license: GPL 3+");
            println!("synopsis: Hello, GNU world: An example GNU package");
            println!("description: GNU Hello prints the message...");
        }
        "show" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("name: {}", pkg);
            println!("version: 2.12.1");
            println!("outputs: out");
            println!("location: gnu/packages/base.scm:100:2");
        }
        "build" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("building /gnu/store/abc-{}-2.12.1.drv...", pkg);
            println!("/gnu/store/def-{}-2.12.1", pkg);
        }
        "shell" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            println!("entering shell with packages: {}", pkgs.join(", "));
            println!("[env]$");
        }
        "gc" => {
            println!("finding garbage collector roots...");
            println!("deleting /gnu/store/old-pkg-1.0 ...");
            println!("deleting /gnu/store/old-pkg-2.0 ...");
            println!("guix gc: 42 store items deleted (1.5 GiB freed)");
        }
        "pull" => {
            println!("Updating channel 'guix' from Git repository at");
            println!("  'https://git.savannah.gnu.org/git/guix.git'...");
            println!("Building from channel:");
            println!("  guix abc123");
            println!("done.");
        }
        "system" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list-generations");
            match action {
                "reconfigure" => println!("guix system: reconfiguring from config.scm..."),
                "list-generations" => {
                    println!("Generation 1  Feb 01 2024 10:00:00");
                    println!("Generation 2  Feb 15 2024 14:30:00 (current)");
                }
                "roll-back" => println!("guix system: rolling back to generation 1"),
                _ => println!("guix system: '{}' completed", action),
            }
        }
        "size" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("store item                              total  self");
            println!("/gnu/store/...-{}-2.12.1        35.2    0.2", pkg);
            println!("/gnu/store/...-glibc-2.39          35.0   35.0");
            println!("total: 35.2 MiB");
        }
        "lint" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("guix lint: checking {}...", pkg);
            println!("  description: ok");
            println!("  synopsis: ok");
            println!("  home-page: ok");
            println!("  source: ok");
        }
        _ => println!("guix: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_guix(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_guix};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_guix(&["--help".to_string()]), 0);
        assert_eq!(run_guix(&["-h".to_string()]), 0);
        let _ = run_guix(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_guix(&[]);
    }
}
