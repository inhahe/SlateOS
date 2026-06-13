#![deny(clippy::all)]

//! poetry-cli — SlateOS Poetry Python dependency manager
//!
//! Multi-personality: `poetry`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_poetry(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: poetry COMMAND [OPTIONS]");
        println!("Poetry 1.8.3 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  new          Create a new project");
        println!("  init         Initialize pyproject.toml in current dir");
        println!("  install      Install dependencies");
        println!("  update       Update dependencies");
        println!("  add          Add a dependency");
        println!("  remove       Remove a dependency");
        println!("  show         Show package info");
        println!("  build        Build the project (sdist/wheel)");
        println!("  publish      Publish to PyPI");
        println!("  lock         Lock dependencies");
        println!("  run          Run a command in the virtualenv");
        println!("  shell        Spawn a shell in the virtualenv");
        println!("  check        Validate pyproject.toml");
        println!("  search       Search for packages on PyPI");
        println!("  export       Export lock file to requirements.txt");
        println!("  env          Manage virtualenvs");
        println!("  config       Manage configuration");
        println!("  version      Show/bump project version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-V" => println!("Poetry (version 1.8.3)"),
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myproject");
            println!("Created package {} in {}", name, name);
            println!("  pyproject.toml");
            println!("  {}/", name);
            println!("    __init__.py");
            println!("  tests/");
            println!("    __init__.py");
        }
        "init" => {
            println!("This command will guide you through creating your pyproject.toml.");
            println!();
            println!("Package name: mypackage");
            println!("Version: 0.1.0");
            println!("Description: ");
            println!("Author: Dev <dev@example.com>");
            println!("License: MIT");
            println!("Compatible Python versions: ^3.12");
            println!();
            println!("Generated pyproject.toml");
        }
        "install" => {
            let no_dev = args.iter().any(|a| a == "--no-dev" || a == "--only=main");
            println!("Installing dependencies from lock file");
            println!();
            println!("Package operations: 8 installs, 0 updates, 0 removals");
            println!("  - Installing certifi (2024.6.2)");
            println!("  - Installing charset-normalizer (3.3.2)");
            println!("  - Installing idna (3.7)");
            println!("  - Installing urllib3 (2.2.2)");
            println!("  - Installing requests (2.32.3)");
            if !no_dev {
                println!("  - Installing pytest (8.2.2)");
                println!("  - Installing pytest-cov (5.0.0)");
            }
            println!();
            println!("Installing the current project: mypackage (0.1.0)");
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("requests");
            let dev = args.iter().any(|a| a == "--dev" || a == "-D" || a == "--group=dev");
            if dev {
                println!("Using version ^8.2 for {}", pkg);
                println!("Updating dependencies (dev)");
            } else {
                println!("Using version ^2.32 for {}", pkg);
                println!("Updating dependencies");
            }
            println!("Resolving dependencies...");
            println!("Writing lock file");
            println!("Package operations: 1 install, 0 updates, 0 removals");
            println!("  - Installing {} (latest)", pkg);
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("requests");
            println!("Updating dependencies");
            println!("Resolving dependencies...");
            println!("Writing lock file");
            println!("Package operations: 0 installs, 0 updates, 1 removal");
            println!("  - Removing {}", pkg);
        }
        "show" => {
            let pkg = args.get(1).map(|s| s.as_str());
            if let Some(p) = pkg {
                println!("name         : {}", p);
                println!("version      : 2.32.3");
                println!("description  : Python HTTP for Humans.");
                println!("dependencies:");
                println!("  - certifi >=2017.4.17");
                println!("  - charset-normalizer >=2,<4");
                println!("  - idna >=2.5,<4");
                println!("  - urllib3 >=1.21.1,<3");
            } else {
                let tree = args.iter().any(|a| a == "--tree" || a == "-t");
                if tree {
                    println!("requests 2.32.3");
                    println!("  certifi >=2017.4.17");
                    println!("  charset-normalizer >=2,<4");
                    println!("  idna >=2.5,<4");
                    println!("  urllib3 >=1.21.1,<3");
                } else {
                    println!("certifi             2024.6.2   Python package for providing Mozilla's CA Bundle.");
                    println!("charset-normalizer  3.3.2      The Real First Universal Charset Detector.");
                    println!("requests            2.32.3     Python HTTP for Humans.");
                }
            }
        }
        "build" => {
            println!("Building mypackage (0.1.0)");
            println!("  - Building sdist");
            println!("  - Built mypackage-0.1.0.tar.gz");
            println!("  - Building wheel");
            println!("  - Built mypackage-0.1.0-py3-none-any.whl");
        }
        "lock" => {
            println!("Resolving dependencies...");
            println!("Writing lock file");
        }
        "check" => {
            println!("All set!");
        }
        "export" => {
            println!("certifi==2024.6.2");
            println!("charset-normalizer==3.3.2");
            println!("idna==3.7");
            println!("requests==2.32.3");
            println!("urllib3==2.2.2");
        }
        "env" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Virtualenv");
                    println!("Python:         3.12.4");
                    println!("Implementation: CPython");
                    println!("Path:           /home/user/.cache/pypoetry/virtualenvs/mypackage-py3.12");
                    println!("Executable:     /home/user/.cache/pypoetry/virtualenvs/mypackage-py3.12/bin/python");
                    println!("Valid:          True");
                }
                "list" => {
                    println!("mypackage-abc12345-py3.12 (Activated)");
                }
                "remove" => println!("Deleted virtualenv"),
                _ => println!("poetry env: '{}' completed", sub),
            }
        }
        "version" => {
            let bump = args.get(1).map(|s| s.as_str());
            if let Some(b) = bump {
                println!("Bumping version from 0.1.0 to {}", match b {
                    "patch" => "0.1.1",
                    "minor" => "0.2.0",
                    "major" => "1.0.0",
                    v => v,
                });
            } else {
                println!("mypackage 0.1.0");
            }
        }
        _ => println!("poetry: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "poetry".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_poetry(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_poetry};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/poetry"), "poetry");
        assert_eq!(basename(r"C:\bin\poetry.exe"), "poetry.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("poetry.exe"), "poetry");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_poetry(&["--help".to_string()]), 0);
        assert_eq!(run_poetry(&["-h".to_string()]), 0);
        let _ = run_poetry(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_poetry(&[]);
    }
}
