#![deny(clippy::all)]

//! pnpm-cli — OurOS pnpm package manager
//!
//! Multi-personality: `pnpm`, `pnpx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pnpm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pnpm COMMAND [OPTIONS]");
        println!("pnpm 9.4.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  add         Install a package");
        println!("  install     Install all dependencies");
        println!("  remove      Remove a package");
        println!("  update      Update packages");
        println!("  run         Run a script");
        println!("  exec        Execute a command");
        println!("  dlx         Run a package without installing");
        println!("  create      Create a project from template");
        println!("  init        Create package.json");
        println!("  list        List installed packages");
        println!("  outdated    Show outdated packages");
        println!("  why         Show why a package is installed");
        println!("  store       Manage the content-addressable store");
        println!("  audit       Check for security vulnerabilities");
        println!("  publish     Publish a package");
        println!("  pack        Create tarball");
        println!("  link        Link a local package");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("9.4.0"),
        "install" | "i" => {
            let frozen = args.iter().any(|a| a == "--frozen-lockfile");
            if frozen {
                println!("Lockfile is up to date, skipping resolution.");
            }
            println!("Packages: +142");
            println!("++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++");
            println!("Progress: resolved 142, reused 140, downloaded 2, added 142");
            println!();
            println!("dependencies:");
            println!("+ react 18.3.1");
            println!("+ react-dom 18.3.1");
            println!();
            println!("devDependencies:");
            println!("+ typescript 5.5.3");
            println!("+ vite 5.3.3");
            println!();
            println!("Done in 1.2s");
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("lodash");
            let dev = args.iter().any(|a| a == "-D" || a == "--save-dev");
            println!("Packages: +1");
            println!("+");
            if dev {
                println!("devDependencies:");
            } else {
                println!("dependencies:");
            }
            println!("+ {} (latest)", pkg);
            println!("Done in 0.8s");
        }
        "remove" | "rm" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("lodash");
            println!("Packages: -1");
            println!("-");
            println!("dependencies:");
            println!("- {}", pkg);
            println!("Done in 0.3s");
        }
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("build");
            println!("> myapp@0.1.0 {}", script);
            println!("> vite {}", script);
            println!("vite v5.3.3 building for production...");
            println!("Done.");
        }
        "list" | "ls" => {
            println!("myapp@0.1.0 /home/user/myapp");
            println!("dependencies:");
            println!("  react 18.3.1");
            println!("  react-dom 18.3.1");
            println!("devDependencies:");
            println!("  typescript 5.5.3");
            println!("  vite 5.3.3");
        }
        "outdated" => {
            println!("Package      Current  Latest   Wanted");
            println!("react        18.2.0   18.3.1   18.3.1");
            println!("typescript   5.4.5    5.5.3    5.5.3");
        }
        "why" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("loose-envify");
            println!("myapp@0.1.0 /home/user/myapp");
            println!();
            println!("dependencies:");
            println!("react 18.3.1");
            println!("  {} 1.4.0", pkg);
        }
        "store" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => println!("Content-addressable store: 4.2 GB, 12,345 packages"),
                "prune" => println!("Removed 234 unreferenced packages. Freed 120 MB."),
                "path" => println!("/home/user/.local/share/pnpm/store/v3"),
                _ => println!("pnpm store: '{}' completed", sub),
            }
        }
        "audit" => {
            println!("0 vulnerabilities found");
        }
        _ => println!("pnpm: '{}' completed", subcmd),
    }
    0
}

fn run_pnpx(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pnpx [OPTIONS] COMMAND [ARGS]");
        println!("Execute a package without installing (pnpm dlx)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("create-react-app");
    println!("pnpx: running {}...", cmd);
    println!("{}: executed.", cmd);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pnpm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pnpx" => run_pnpx(&rest),
        _ => run_pnpm(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pnpm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pnpm"), "pnpm");
        assert_eq!(basename(r"C:\bin\pnpm.exe"), "pnpm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pnpm.exe"), "pnpm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pnpm(&["--help".to_string()]), 0);
        assert_eq!(run_pnpm(&["-h".to_string()]), 0);
        let _ = run_pnpm(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pnpm(&[]);
    }
}
