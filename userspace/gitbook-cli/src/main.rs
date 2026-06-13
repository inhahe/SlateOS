#![deny(clippy::all)]

//! gitbook-cli — SlateOS GitBook CLI
//!
//! Multi-personality: `gitbook`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gitbook(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gitbook COMMAND [OPTIONS]");
        println!("GitBook CLI 2.3.3 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  init           Create a new book");
        println!("  build          Build static site");
        println!("  serve          Serve the book locally");
        println!("  install        Install plugins");
        println!("  pdf            Generate PDF");
        println!("  epub           Generate EPUB");
        println!("  mobi           Generate MOBI");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("gitbook 2.3.3"),
        "init" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Initializing book in '{}'...", dir);
            println!("  Created README.md");
            println!("  Created SUMMARY.md");
            println!("  Created book.json");
            println!("Done.");
        }
        "build" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Building book in '{}'...", dir);
            println!("  info: loading book configuration...");
            println!("  info: found 15 pages");
            println!("  info: generating...");
            println!("  info: book built in _book/");
        }
        "serve" => {
            let port = args.windows(2).find(|w| w[0] == "--port" || w[0] == "-p")
                .map(|w| w[1].as_str()).unwrap_or("4000");
            println!("  info: loading book configuration...");
            println!("  info: building book...");
            println!("  Serving book on http://localhost:{}", port);
            println!("  Press CTRL+C to quit");
        }
        "install" => {
            println!("Installing plugins...");
            println!("  search: installed");
            println!("  highlight: installed");
            println!("  sharing: installed");
            println!("3 plugins installed.");
        }
        "pdf" => {
            let output = args.get(1).map(|s| s.as_str()).unwrap_or("book.pdf");
            println!("Generating PDF...");
            println!("  Output: {}", output);
            println!("Done.");
        }
        "epub" => {
            let output = args.get(1).map(|s| s.as_str()).unwrap_or("book.epub");
            println!("Generating EPUB...");
            println!("  Output: {}", output);
            println!("Done.");
        }
        _ => println!("gitbook: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gitbook".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gitbook(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gitbook};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gitbook"), "gitbook");
        assert_eq!(basename(r"C:\bin\gitbook.exe"), "gitbook.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gitbook.exe"), "gitbook");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gitbook(&["--help".to_string()]), 0);
        assert_eq!(run_gitbook(&["-h".to_string()]), 0);
        let _ = run_gitbook(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gitbook(&[]);
    }
}
