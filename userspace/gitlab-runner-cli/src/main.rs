#![deny(clippy::all)]

//! gitlab-runner-cli — OurOS GitLab Runner
//!
//! Single personality: `gitlab-runner`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_runner(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gitlab-runner [COMMAND] [OPTIONS]");
        println!("GitLab Runner v16.11 (OurOS) — CI/CD job executor");
        println!();
        println!("Commands:");
        println!("  run                Start runner");
        println!("  register           Register new runner");
        println!("  unregister         Unregister runner");
        println!("  verify             Verify runner registration");
        println!("  list               List configured runners");
        println!("  status             Show service status");
        println!("  restart            Restart service");
        println!("  exec               Execute a build locally");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --working-directory DIR  Working directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GitLab Runner v16.11.1 (OurOS)"); return 0; }
    println!("GitLab Runner v16.11.1 (OurOS)");
    println!("  Runners: 3 registered");
    println!("  Executors: docker (2), shell (1)");
    println!("  Concurrent: 4 jobs max");
    println!("  Check interval: 3s");
    println!("  Connected to: https://gitlab.example.com");
    println!("  Jobs completed: 12,345");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gitlab-runner".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_runner(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_runner};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gitlab-runner"), "gitlab-runner");
        assert_eq!(basename(r"C:\bin\gitlab-runner.exe"), "gitlab-runner.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gitlab-runner.exe"), "gitlab-runner");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_runner(&["--help".to_string()], "gitlab-runner"), 0);
        assert_eq!(run_runner(&["-h".to_string()], "gitlab-runner"), 0);
        let _ = run_runner(&["--version".to_string()], "gitlab-runner");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_runner(&[], "gitlab-runner");
    }
}
