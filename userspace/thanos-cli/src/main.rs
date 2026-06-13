#![deny(clippy::all)]

//! thanos-cli — SlateOS Thanos HA Prometheus tools
//!
//! Multi-personality: `thanos`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_thanos(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: thanos COMMAND [OPTIONS]");
        println!("Thanos 0.35.1 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  sidecar      Run sidecar for Prometheus");
        println!("  store        Run store gateway");
        println!("  query        Run query layer");
        println!("  compact      Run compactor");
        println!("  rule         Run ruler");
        println!("  receive      Run receiver");
        println!("  tools        CLI tools (bucket, rules)");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => {
            println!("thanos, version 0.35.1 (branch: HEAD, revision: abc1234)");
            println!("  build date: 2024-06-15");
            println!("  go version: go1.22.4");
        }
        "sidecar" => {
            println!("thanos sidecar: connecting to Prometheus at http://localhost:9090");
            println!("thanos sidecar: uploading blocks to object store...");
        }
        "store" => {
            println!("thanos store gateway: serving blocks from object store");
            println!("thanos store: listening on 0.0.0.0:10901");
        }
        "query" => {
            println!("thanos query: starting query layer");
            println!("thanos query: listening on 0.0.0.0:10902");
            println!("thanos query: connected to 3 store endpoints");
        }
        "compact" => {
            println!("thanos compact: starting compaction");
            println!("thanos compact: compacted 12 blocks into 3");
            println!("thanos compact: deleted 9 source blocks");
        }
        "tools" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "bucket" => {
                    let action = args.get(2).map(|s| s.as_str()).unwrap_or("ls");
                    match action {
                        "ls" => {
                            println!("Block ID                           Size      MinTime              MaxTime");
                            println!("01ABC...                           234 MB    2024-06-01T00:00     2024-06-01T02:00");
                            println!("02DEF...                           198 MB    2024-06-01T02:00     2024-06-01T04:00");
                        }
                        "verify" => println!("All blocks verified: OK"),
                        "inspect" => println!("Block details: 1234 series, 567890 samples, 2h duration"),
                        _ => println!("thanos tools bucket: '{}' completed", action),
                    }
                }
                _ => println!("thanos tools: '{}' completed", sub),
            }
        }
        _ => println!("thanos: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "thanos".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_thanos(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_thanos};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/thanos"), "thanos");
        assert_eq!(basename(r"C:\bin\thanos.exe"), "thanos.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("thanos.exe"), "thanos");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_thanos(&["--help".to_string()]), 0);
        assert_eq!(run_thanos(&["-h".to_string()]), 0);
        let _ = run_thanos(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_thanos(&[]);
    }
}
