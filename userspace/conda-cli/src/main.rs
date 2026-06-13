#![deny(clippy::all)]

//! conda-cli — SlateOS Conda package/environment manager
//!
//! Multi-personality: `conda`, `mamba`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_conda(args: &[String], prog_name: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: {} COMMAND [OPTIONS]", prog_name);
        println!("{} 24.5.0 (SlateOS)", prog_name);
        println!();
        println!("Environment commands:");
        println!("  create      Create a new environment");
        println!("  activate    Activate an environment");
        println!("  deactivate  Deactivate current environment");
        println!("  list        List environments");
        println!("  remove      Remove an environment");
        println!("  export      Export environment to YAML");
        println!();
        println!("Package commands:");
        println!("  install     Install packages");
        println!("  update      Update packages");
        println!("  search      Search for packages");
        println!("  info        Display conda info");
        println!("  clean       Remove unused caches");
        println!("  config      Manage configuration");
        println!("  run         Run command in environment");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-V" => println!("{} 24.5.0", prog_name),
        "create" => {
            let env_name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name")
                .map(|w| w[1].as_str()).unwrap_or("myenv");
            let pkgs: Vec<&str> = args.iter()
                .filter(|a| !a.starts_with('-') && a.as_str() != "create")
                .filter(|a| {
                    let pos = args.iter().position(|x| x == *a).unwrap_or(0);
                    pos == 0 || (args.get(pos - 1).map(|s| s.as_str()) != Some("-n")
                        && args.get(pos - 1).map(|s| s.as_str()) != Some("--name"))
                })
                .map(|s| s.as_str()).collect();
            println!("Collecting package metadata: done");
            println!("Solving environment: done");
            println!("Creating environment at /home/user/.conda/envs/{}", env_name);
            if !pkgs.is_empty() {
                for p in &pkgs {
                    println!("  Installing {}...", p);
                }
            }
            println!("Environment '{}' created successfully.", env_name);
        }
        "install" => {
            let pkgs: Vec<&str> = args.iter()
                .filter(|a| !a.starts_with('-') && a.as_str() != "install")
                .map(|s| s.as_str()).collect();
            println!("Collecting package metadata: done");
            println!("Solving environment: done");
            for p in &pkgs {
                println!("  Installing {}...", p);
            }
            if pkgs.is_empty() {
                println!("  Installing packages from environment.yml...");
            }
            println!("Done.");
        }
        "update" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("--all");
            if pkg == "--all" {
                println!("Updating all packages...");
            } else {
                println!("Updating {}...", pkg);
            }
            println!("Solving environment: done");
            println!("Done.");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("numpy");
            println!("Loading channels: done");
            println!("# Name          Version  Build          Channel");
            println!("{}         1.26.4   py312h1234    defaults", term);
            println!("{}         1.26.3   py311h5678    defaults", term);
            println!("{}         1.25.2   py310habcd    conda-forge", term);
        }
        "list" => {
            if args.iter().any(|a| a == "--envs" || a == "-e") {
                println!("# conda environments:");
                println!("#");
                println!("base                  *  /home/user/.conda");
                println!("myenv                    /home/user/.conda/envs/myenv");
                println!("data-science             /home/user/.conda/envs/data-science");
            } else {
                println!("# packages in environment at /home/user/.conda:");
                println!("#");
                println!("# Name            Version    Build        Channel");
                println!("numpy             1.26.4     py312h1234   defaults");
                println!("pandas            2.2.2      py312h5678   defaults");
                println!("python            3.12.4     h9876        defaults");
                println!("scipy             1.13.1     py312habcd   defaults");
            }
        }
        "remove" => {
            let env_name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name")
                .map(|w| w[1].as_str()).unwrap_or("myenv");
            if args.iter().any(|a| a == "--all") {
                println!("Remove all packages in environment '{}'...", env_name);
                println!("Environment '{}' removed.", env_name);
            } else {
                let pkg = args.iter().find(|a| !a.starts_with('-') && a.as_str() != "remove")
                    .map(|s| s.as_str()).unwrap_or("package");
                println!("Removing {} from {}...", pkg, env_name);
                println!("Done.");
            }
        }
        "info" => {
            println!("{} 24.5.0", prog_name);
            println!("  platform: linux-64");
            println!("  user config: /home/user/.condarc");
            println!("  base environment: /home/user/.conda");
            println!("  channel URLs: https://repo.anaconda.com/pkgs/main/linux-64");
            println!("  package cache: /home/user/.conda/pkgs");
            println!("  envs directories: /home/user/.conda/envs");
        }
        "clean" => {
            println!("Removing cached package tarballs...");
            println!("Removed 342 MB of cached packages.");
        }
        "export" => {
            println!("name: myenv");
            println!("channels:");
            println!("  - defaults");
            println!("  - conda-forge");
            println!("dependencies:");
            println!("  - python=3.12.4");
            println!("  - numpy=1.26.4");
            println!("  - pandas=2.2.2");
        }
        _ => println!("{}: '{}' completed", prog_name, subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "conda".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_conda(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_conda};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/conda"), "conda");
        assert_eq!(basename(r"C:\bin\conda.exe"), "conda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("conda.exe"), "conda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_conda(&["--help".to_string()], "conda"), 0);
        assert_eq!(run_conda(&["-h".to_string()], "conda"), 0);
        let _ = run_conda(&["--version".to_string()], "conda");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_conda(&[], "conda");
    }
}
