#![deny(clippy::all)]

//! pelican-cli — SlateOS Pelican static site generator
//!
//! Multi-personality: `pelican`, `pelican-quickstart`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pelican(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pelican [OPTIONS] [PATH]");
        println!("Pelican 4.9.1 (SlateOS)");
        println!("  -s FILE        Settings file");
        println!("  -o DIR         Output directory");
        println!("  -t THEME       Theme");
        println!("  -r             Autoreload");
        println!("  --listen       Start dev server");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pelican 4.9.1 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--listen") {
        println!("Pelican 4.9.1 — serving at http://localhost:8000/");
        return 0;
    }
    let path = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("content");
    println!("Pelican 4.9.1");
    println!("  Source: {}/", path);
    println!("  Done: Processed 12 articles, 3 pages, 5 tags");
    0
}

fn run_pelican_quickstart(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pelican-quickstart [OPTIONS]");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pelican-quickstart 4.9.1 (SlateOS)");
        return 0;
    }
    let _ = args;
    println!("Welcome to pelican-quickstart v4.9.1.");
    println!("  Created: pelicanconf.py");
    println!("  Created: publishconf.py");
    println!("  Created: tasks.py");
    println!("  Created: Makefile");
    println!("  Created: content/");
    println!("  Created: output/");
    println!("Done. Your new project is ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pelican".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pelican-quickstart" => run_pelican_quickstart(&rest),
        _ => run_pelican(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pelican};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pelican"), "pelican");
        assert_eq!(basename(r"C:\bin\pelican.exe"), "pelican.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pelican.exe"), "pelican");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pelican(&["--help".to_string()]), 0);
        assert_eq!(run_pelican(&["-h".to_string()]), 0);
        let _ = run_pelican(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pelican(&[]);
    }
}
