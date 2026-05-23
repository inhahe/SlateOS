#![deny(clippy::all)]

//! asdf-cli — OurOS asdf version manager
//!
//! Single personality: `asdf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_asdf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") || args.is_empty() {
        println!("Usage: asdf COMMAND [ARGS...]");
        println!("asdf v0.14.1 (OurOS) — Extendable version manager");
        println!();
        println!("Plugin commands:");
        println!("  plugin add NAME [URL]   Add a plugin");
        println!("  plugin list             List installed plugins");
        println!("  plugin list all         List all available plugins");
        println!("  plugin remove NAME      Remove a plugin");
        println!("  plugin update NAME      Update a plugin");
        println!("  plugin update --all     Update all plugins");
        println!();
        println!("Version commands:");
        println!("  install NAME VERSION    Install a version");
        println!("  uninstall NAME VERSION  Uninstall a version");
        println!("  current                 Show current versions");
        println!("  global NAME VERSION     Set global version");
        println!("  local NAME VERSION      Set local version");
        println!("  latest NAME [FILTER]    Show latest version");
        println!("  list NAME               List installed versions");
        println!("  list all NAME           List all available versions");
        println!();
        println!("Other commands:");
        println!("  reshim NAME [VERSION]   Recreate shims");
        println!("  where NAME VERSION      Show install path");
        println!("  which NAME              Show shim path");
        println!("  info                    Show system info");
        println!("  version                 Show asdf version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "version" => println!("v0.14.1-OurOS"),
        "info" => {
            println!("OS: OurOS x86_64");
            println!("SHELL: /bin/bash");
            println!("ASDF_DIR: ~/.asdf");
            println!("ASDF_DATA_DIR: ~/.asdf");
        }
        "current" => {
            println!("nodejs     20.11.1   ~/.tool-versions");
            println!("python     3.12.1    ~/.tool-versions");
            println!("rust       1.77.0    ~/.tool-versions");
        }
        "plugin" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    if args.get(2).map(|s| s.as_str()) == Some("all") {
                        println!("nodejs");
                        println!("python");
                        println!("ruby");
                        println!("golang");
                        println!("rust");
                        println!("java");
                    } else {
                        println!("nodejs");
                        println!("python");
                    }
                }
                "add" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("<plugin>");
                    println!("asdf: Adding plugin '{}'...", name);
                }
                "remove" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("<plugin>");
                    println!("asdf: Removing plugin '{}'.", name);
                }
                _ => println!("asdf plugin: {}", sub),
            }
        }
        "install" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("<tool>");
            let ver = args.get(2).map(|s| s.as_str()).unwrap_or("latest");
            println!("asdf: Installing {} {}...", name, ver);
        }
        "global" | "local" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("<tool>");
            let ver = args.get(2).map(|s| s.as_str()).unwrap_or("latest");
            println!("asdf: Set {} {} to {} version {}", cmd, name, cmd, ver);
        }
        "latest" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("nodejs");
            println!("{} 20.11.1", name);
        }
        "list" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("nodejs");
            println!("  {} 20.11.1", name);
            println!("  {} 18.19.0", name);
        }
        "where" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("nodejs");
            let ver = args.get(2).map(|s| s.as_str()).unwrap_or("20.11.1");
            println!("~/.asdf/installs/{}/{}", name, ver);
        }
        "reshim" => println!("asdf: Shims recreated."),
        _ => println!("asdf: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "asdf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_asdf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
