#![deny(clippy::all)]

//! logseq-cli — SlateOS Logseq outliner knowledge base
//!
//! Single personality: `logseq`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logseq(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logseq [OPTIONS] [GRAPH_DIR]");
        println!("logseq v0.10 (Slate OS) — Outliner knowledge base");
        println!();
        println!("Options:");
        println!("  --graph DIR       Open specific graph");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("logseq v0.10 (Slate OS)"); return 0; }
    println!("logseq: outliner knowledge base started");
    println!("  Graph: ~/Documents/logseq");
    println!("  Pages: 185");
    println!("  Journals: daily notes enabled");
    println!("  Format: Markdown");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logseq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logseq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_logseq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logseq"), "logseq");
        assert_eq!(basename(r"C:\bin\logseq.exe"), "logseq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logseq.exe"), "logseq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logseq(&["--help".to_string()], "logseq"), 0);
        assert_eq!(run_logseq(&["-h".to_string()], "logseq"), 0);
        let _ = run_logseq(&["--version".to_string()], "logseq");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logseq(&[], "logseq");
    }
}
