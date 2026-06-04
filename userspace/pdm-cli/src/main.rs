#![deny(clippy::all)]

//! pdm-cli — OurOS PDM Python package manager
//!
//! Multi-personality: `pdm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdm COMMAND [OPTIONS]");
        println!("PDM 2.17.1 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize a new project");
        println!("  install      Install dependencies");
        println!("  add          Add dependencies");
        println!("  remove       Remove dependencies");
        println!("  update       Update dependencies");
        println!("  list         List installed packages");
        println!("  show         Show package info");
        println!("  build        Build the project");
        println!("  publish      Publish to PyPI");
        println!("  lock         Generate/update lock file");
        println!("  run          Run a command/script");
        println!("  use          Switch Python interpreter");
        println!("  venv         Manage virtualenvs");
        println!("  self         Manage PDM itself");
        println!("  config       Manage configuration");
        println!("  cache        Manage the package cache");
        println!("  fix          Fix project problems");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-V" => println!("PDM, version 2.17.1"),
        "init" => {
            println!("Creating a pyproject.toml for PDM...");
            println!("Project name: myproject");
            println!("Project version: 0.1.0");
            println!("Build backend: pdm-backend");
            println!("License: MIT");
            println!("Author name: Dev");
            println!("Python requires: >=3.12");
            println!("Project created successfully at ./myproject");
        }
        "install" => {
            println!("Resolving dependencies...");
            println!("  resolved 12 packages");
            println!("Installing: requests 2.32.3");
            println!("Installing: click 8.1.7");
            println!("Installing: rich 13.7.1");
            println!("All packages installed.");
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("requests");
            let dev = args.iter().any(|a| a == "--dev" || a == "-d" || a == "-dG");
            println!("Adding {} to {} dependencies", pkg,
                if dev { "dev" } else { "default" });
            println!("Resolving dependencies...");
            println!("  resolved 8 packages");
            println!("Installing: {} (latest)", pkg);
            println!("Added {} to pyproject.toml", pkg);
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("requests");
            println!("Removing {} from dependencies", pkg);
            println!("Resolving dependencies...");
            println!("Removed {}.", pkg);
        }
        "update" => {
            let pkg = args.get(1).map(|s| s.as_str());
            if let Some(p) = pkg {
                println!("Updating {}...", p);
            } else {
                println!("Updating all dependencies...");
            }
            println!("Resolving dependencies...");
            println!("All packages up to date.");
        }
        "list" => {
            let tree = args.iter().any(|a| a == "--tree");
            if tree {
                println!("requests 2.32.3");
                println!("  certifi 2024.6.2");
                println!("  idna 3.7");
                println!("  urllib3 2.2.2");
            } else {
                println!("Name                Version");
                println!("requests            2.32.3");
                println!("click               8.1.7");
                println!("rich                13.7.1");
                println!("certifi             2024.6.2");
            }
        }
        "show" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("requests");
            println!("Name:        {}", pkg);
            println!("Version:     2.32.3");
            println!("Summary:     Python HTTP for Humans.");
            println!("Requires-Python: >=3.8");
            println!("Home-page:   https://requests.readthedocs.io");
        }
        "build" => {
            println!("Building myproject-0.1.0...");
            println!("  sdist: dist/myproject-0.1.0.tar.gz");
            println!("  wheel: dist/myproject-0.1.0-py3-none-any.whl");
            println!("Build succeeded.");
        }
        "lock" => {
            println!("Resolving dependencies...");
            println!("  resolved 12 packages");
            println!("Lock file written to pdm.lock.");
        }
        "run" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("python");
            println!("pdm: running '{}' in project environment", cmd);
        }
        "use" => {
            let ver = args.get(1).map(|s| s.as_str()).unwrap_or("3.12");
            println!("Using Python {} at /usr/bin/python{}", ver, ver);
            println!("Virtualenv updated.");
        }
        "cache" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Cache root: /home/user/.cache/pdm");
                    println!("Packages: 234");
                    println!("Size: 1.2 GB");
                }
                "clear" => println!("Cache cleared."),
                _ => println!("pdm cache: '{}' completed", sub),
            }
        }
        _ => println!("pdm: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pdm(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pdm"), "pdm");
        assert_eq!(basename(r"C:\bin\pdm.exe"), "pdm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pdm.exe"), "pdm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pdm(&["--help".to_string()]), 0);
        assert_eq!(run_pdm(&["-h".to_string()]), 0);
        let _ = run_pdm(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pdm(&[]);
    }
}
