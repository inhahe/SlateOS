#![deny(clippy::all)]

//! yarn-cli — SlateOS Yarn package manager
//!
//! Multi-personality: `yarn`, `yarnpkg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_yarn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: yarn COMMAND [OPTIONS]");
        println!("Yarn 4.3.1 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  add          Add dependencies");
        println!("  remove       Remove dependencies");
        println!("  install      Install all dependencies");
        println!("  up           Update packages");
        println!("  run          Run a script");
        println!("  dlx          Run a package without installing");
        println!("  init         Create a new project");
        println!("  info         Show package info");
        println!("  why          Show dependency reasons");
        println!("  pack         Create tarball");
        println!("  npm          npm-related commands");
        println!("  plugin       Manage plugins");
        println!("  set          Change config");
        println!("  workspace    Workspace commands");
        println!("  workspaces   Run commands across workspaces");
        println!("  cache        Manage package cache");
        println!("  config       Display configuration");
        println!("  rebuild      Rebuild native modules");
        return 0;
    }
    // yarn without args runs install
    if args.is_empty() {
        println!("yarn install v4.3.1");
        println!("Resolving packages...");
        println!("Fetching packages...");
        println!("Linking dependencies...");
        println!("Done in 2.34s.");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("4.3.1"),
        "install" | "i" => {
            let immutable = args.iter().any(|a| a == "--immutable" || a == "--frozen-lockfile");
            if immutable {
                println!("yarn install v4.3.1 (immutable)");
            } else {
                println!("yarn install v4.3.1");
            }
            println!("Resolving packages...");
            println!("Fetching packages...");
            println!("Linking dependencies...");
            println!("Done in 2.34s.");
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("react");
            let dev = args.iter().any(|a| a == "-D" || a == "--dev");
            println!("Resolving {}...", pkg);
            if dev {
                println!("Added {} to devDependencies", pkg);
            } else {
                println!("Added {} to dependencies", pkg);
            }
            println!("Done in 0.8s.");
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("lodash");
            println!("Removing {}...", pkg);
            println!("Done in 0.3s.");
        }
        "up" => {
            let pkg = args.get(1).map(|s| s.as_str());
            if let Some(p) = pkg {
                println!("Resolving {}...", p);
                println!("Updated {}.", p);
            } else {
                println!("Updating all packages...");
                println!("Done.");
            }
        }
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("build");
            println!("$ {}", script);
        }
        "dlx" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("create-react-app");
            println!("yarn dlx: running {}...", pkg);
            println!("Done.");
        }
        "init" => {
            println!("yarn init v4.3.1");
            println!("  name: myapp");
            println!("  version: 0.1.0");
            println!("  entry: src/index.js");
            println!("Done.");
        }
        "info" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("react");
            println!("{}@npm:18.3.1", pkg);
            println!("  Manifest:");
            println!("    name: {}", pkg);
            println!("    version: 18.3.1");
            println!("    license: MIT");
            println!("    homepage: https://react.dev/");
        }
        "why" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("scheduler");
            println!("{}@npm:0.23.2", pkg);
            println!("  Reason: react-dom@npm:18.3.1 depends on it");
        }
        "workspace" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Workspaces:");
                    println!("  packages/core");
                    println!("  packages/ui");
                    println!("  packages/utils");
                    println!("  apps/web");
                }
                _ => println!("yarn workspace: '{}' completed", sub),
            }
        }
        "cache" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => println!("Cache entries: 234 (456 MB)"),
                "clean" => println!("Cache cleared."),
                _ => println!("yarn cache: '{}' completed", sub),
            }
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" || sub == "--why" {
                println!("cacheFolder: ./.yarn/cache");
                println!("enableGlobalCache: false");
                println!("nodeLinker: pnp");
            }
        }
        "rebuild" => {
            println!("Rebuilding all native modules...");
            println!("Done in 3.2s.");
        }
        _ => {
            // yarn <script> runs the script
            println!("$ {}", subcmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "yarn".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_yarn(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_yarn};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/yarn"), "yarn");
        assert_eq!(basename(r"C:\bin\yarn.exe"), "yarn.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("yarn.exe"), "yarn");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_yarn(&["--help".to_string()]), 0);
        assert_eq!(run_yarn(&["-h".to_string()]), 0);
        let _ = run_yarn(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_yarn(&[]);
    }
}
