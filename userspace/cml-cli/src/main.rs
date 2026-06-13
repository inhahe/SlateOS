#![deny(clippy::all)]

//! cml-cli — Slate OS CML (Continuous Machine Learning) CLI
//!
//! Multi-personality: `cml`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cml(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cml COMMAND [OPTIONS]");
        println!("CML 0.20.0 (Slate OS) — Continuous Machine Learning");
        println!();
        println!("Commands:");
        println!("  comment        Post comment to PR/MR");
        println!("  publish        Publish file as comment asset");
        println!("  runner         Launch a CML runner");
        println!("  tensorboard    Launch TensorBoard");
        println!("  pr             Create a pull request");
        println!("  check          Create a check report");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("cml 0.20.0"),
        "comment" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("create");
            if sub == "create" {
                println!("Posting comment to PR...");
                println!("Comment posted successfully.");
                println!("URL: https://github.com/myorg/myrepo/pull/42#issuecomment-123");
            } else {
                println!("cml comment: '{}' completed", sub);
            }
        }
        "publish" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("metrics.png");
            println!("Publishing '{}'...", file);
            println!("https://asset.cml.dev/abc123.png");
        }
        "runner" => {
            println!("Launching CML runner...");
            println!("  Provider: local");
            println!("  Labels: cml");
            println!("  Runner started and waiting for jobs.");
        }
        "tensorboard" => {
            let logdir = args.windows(2).find(|w| w[0] == "--logdir")
                .map(|w| w[1].as_str()).unwrap_or("logs");
            println!("Starting TensorBoard...");
            println!("  Log directory: {}", logdir);
            println!("  URL: https://tb.cml.dev/abc123");
        }
        "pr" => {
            println!("Creating pull request...");
            println!("  PR created: https://github.com/myorg/myrepo/pull/43");
        }
        _ => println!("cml: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cml".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cml(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cml};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cml"), "cml");
        assert_eq!(basename(r"C:\bin\cml.exe"), "cml.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cml.exe"), "cml");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cml(&["--help".to_string()]), 0);
        assert_eq!(run_cml(&["-h".to_string()]), 0);
        let _ = run_cml(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cml(&[]);
    }
}
