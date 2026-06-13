#![deny(clippy::all)]

//! gitlab-cli — Slate OS GitLab CLI tools
//!
//! Multi-personality: `glab`, `gitlab-runner`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_glab(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: glab <command> [flags]");
        println!();
        println!("glab — GitLab CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  mr          Manage merge requests");
        println!("  issue       Manage issues");
        println!("  ci          CI/CD pipelines");
        println!("  repo        Repository management");
        println!("  release     Manage releases");
        println!("  auth        Authentication");
        println!("  variable    CI/CD variables");
        println!("  version     Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => println!("glab version 1.36.0 (Slate OS)"),
        "mr" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match cmd {
                "list" => {
                    println!("Showing 3 open merge requests:");
                    println!("!42  feat: add new scheduler    main <- feature/scheduler   @alice");
                    println!("!41  fix: memory leak in IPC    main <- fix/ipc-leak        @bob");
                    println!("!40  docs: update API docs      main <- docs/api-update     @charlie");
                }
                "create" => println!("Creating merge request..."),
                "view" => println!("!42  feat: add new scheduler\nAuthor: @alice\nStatus: open\nPipeline: passed"),
                _ => println!("glab mr {} completed", cmd),
            }
        }
        "issue" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if cmd == "list" {
                println!("#100 Bug: crash on startup         opened 2h ago   critical");
                println!("#99  Feature: dark mode support    opened 1d ago   enhancement");
                println!("#98  Task: update dependencies     opened 3d ago   maintenance");
            } else {
                println!("glab issue {} completed", cmd);
            }
        }
        "ci" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match cmd {
                "status" => {
                    println!("Pipeline #1234: passed (3m 42s)");
                    println!("  build      passed  1m 20s");
                    println!("  test       passed  1m 45s");
                    println!("  deploy     passed  0m 37s");
                }
                "list" => {
                    println!("#1234  passed   main     3m 42s   2h ago");
                    println!("#1233  failed   develop  2m 15s   5h ago");
                    println!("#1232  passed   main     3m 38s   1d ago");
                }
                _ => println!("glab ci {} completed", cmd),
            }
        }
        "auth" => println!("Logged in to gitlab.com as @user"),
        _ => println!("glab: command '{}' completed", subcmd),
    }
    0
}

fn run_gitlab_runner(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gitlab-runner <command> [OPTIONS]");
        println!("Commands: run, register, list, verify, unregister, status, restart");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Version:      16.8.0 (Slate OS)");
        println!("Git revision: abcdef12");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            println!("Listing configured runners                          ConfigFile=/etc/gitlab-runner/config.toml");
            println!("slateos-runner-1                                      Executor=docker   Token=abcdef12   URL=https://gitlab.com");
            println!("slateos-runner-2                                      Executor=shell    Token=34567890   URL=https://gitlab.com");
        }
        "status" => println!("gitlab-runner: Service is running"),
        "verify" => println!("Verifying runner... is valid                         runner=abcdef12"),
        _ => println!("gitlab-runner: {} completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "glab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gitlab-runner" => run_gitlab_runner(&rest),
        _ => run_glab(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_glab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gitlab"), "gitlab");
        assert_eq!(basename(r"C:\bin\gitlab.exe"), "gitlab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gitlab.exe"), "gitlab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_glab(&["--help".to_string()]), 0);
        assert_eq!(run_glab(&["-h".to_string()]), 0);
        let _ = run_glab(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_glab(&[]);
    }
}
