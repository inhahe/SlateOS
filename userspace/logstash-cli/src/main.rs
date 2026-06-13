#![deny(clippy::all)]

//! logstash-cli — Slate OS Logstash data processing pipeline
//!
//! Single personality: `logstash`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logstash(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logstash [OPTIONS]");
        println!("Logstash v8.14 (Slate OS) — Server-side data processing pipeline");
        println!();
        println!("Options:");
        println!("  -f, --config FILE     Config file or directory");
        println!("  -e, --config.string S Inline config string");
        println!("  --config.test_and_exit Validate config and exit");
        println!("  -w, --pipeline.workers N  Pipeline workers");
        println!("  -b, --pipeline.batch.size N  Batch size");
        println!("  --path.data DIR       Data directory");
        println!("  --path.logs DIR       Log directory");
        println!("  --path.plugins DIR    Plugin directory");
        println!("  --log.level LEVEL     Log level (fatal..trace)");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("logstash v8.14.3 (Slate OS)"); return 0; }
    println!("Logstash v8.14.3 (Slate OS)");
    println!("  Pipelines: 2 running");
    println!("  Workers: 8 per pipeline");
    println!("  Batch size: 125 events");
    println!("  Inputs: beats, tcp, kafka");
    println!("  Outputs: elasticsearch, file");
    println!("  Events in: 45,678/s");
    println!("  Events out: 45,201/s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logstash".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logstash(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_logstash};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logstash"), "logstash");
        assert_eq!(basename(r"C:\bin\logstash.exe"), "logstash.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logstash.exe"), "logstash");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logstash(&["--help".to_string()], "logstash"), 0);
        assert_eq!(run_logstash(&["-h".to_string()], "logstash"), 0);
        let _ = run_logstash(&["--version".to_string()], "logstash");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logstash(&[], "logstash");
    }
}
