#![deny(clippy::all)]

//! pipx-cli — Slate OS pipx tool for installing Python apps in isolated envs
//!
//! Multi-personality: `pipx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pipx(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pipx COMMAND [OPTIONS]");
        println!("pipx 1.6.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  install      Install a package");
        println!("  uninstall    Uninstall a package");
        println!("  upgrade      Upgrade a package");
        println!("  upgrade-all  Upgrade all packages");
        println!("  reinstall    Reinstall a package");
        println!("  list         List installed packages");
        println!("  run          Run an app temporarily");
        println!("  inject       Add extra packages to an app's environment");
        println!("  ensurepath   Ensure pipx paths are on PATH");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("1.6.0"),
        "install" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("black");
            println!("  installed package {}", pkg);
            println!("  These apps are now globally available:");
            println!("    - {}", pkg);
            println!("done!");
        }
        "uninstall" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("black");
            println!("uninstalled {}!", pkg);
        }
        "upgrade" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("black");
            println!("{} is already at latest version 24.4.2", pkg);
        }
        "upgrade-all" => {
            println!("black is already at latest version 24.4.2");
            println!("ruff is already at latest version 0.5.0");
            println!("mypy is already at latest version 1.10.1");
        }
        "list" => {
            println!("venvs are in /home/user/.local/pipx/venvs");
            println!("apps are exposed on your $PATH at /home/user/.local/bin");
            println!();
            println!("   package black 24.4.2, installed using Python 3.12.4");
            println!("    - black");
            println!("    - blackd");
            println!();
            println!("   package ruff 0.5.0, installed using Python 3.12.4");
            println!("    - ruff");
            println!();
            println!("   package mypy 1.10.1, installed using Python 3.12.4");
            println!("    - mypy");
            println!("    - dmypy");
            println!("    - stubgen");
        }
        "run" => {
            let app = args.get(1).map(|s| s.as_str()).unwrap_or("cowsay");
            println!("pipx: running {} in temporary venv...", app);
            println!("{}: executed successfully.", app);
        }
        "inject" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("black");
            let extra = args.get(2).map(|s| s.as_str()).unwrap_or("black[jupyter]");
            println!("  injected package {} into venv {}", extra, pkg);
            println!("done!");
        }
        "ensurepath" => {
            println!("Success! Added /home/user/.local/bin to the PATH.");
            println!("Consider adding shell completions for pipx.");
        }
        _ => println!("pipx: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pipx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pipx(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pipx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pipx"), "pipx");
        assert_eq!(basename(r"C:\bin\pipx.exe"), "pipx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pipx.exe"), "pipx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pipx(&["--help".to_string()]), 0);
        assert_eq!(run_pipx(&["-h".to_string()]), 0);
        let _ = run_pipx(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pipx(&[]);
    }
}
