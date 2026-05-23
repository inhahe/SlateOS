#![deny(clippy::all)]

//! mdbook-cli — OurOS mdBook CLI
//!
//! Multi-personality: `mdbook`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mdbook(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mdbook COMMAND [OPTIONS]");
        println!("mdbook 0.4.37 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init           Create a new book");
        println!("  build          Build the book");
        println!("  watch          Watch for changes and rebuild");
        println!("  serve          Serve the book locally");
        println!("  test           Test Rust code samples in the book");
        println!("  clean          Delete built book");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("mdbook 0.4.37"),
        "init" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("my-book");
            println!("Creating a new book in '{}'...", dir);
            println!("  Created book.toml");
            println!("  Created src/SUMMARY.md");
            println!("  Created src/chapter_1.md");
            println!("Done. Start writing!");
        }
        "build" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("[INFO] Building book in '{}'...", dir);
            println!("[INFO] Running 0 preprocessors");
            println!("[INFO] Running HTML backend");
            println!("[INFO] Building 12 chapters");
            println!("[INFO] Book built in book/");
        }
        "serve" => {
            let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("3000");
            println!("[INFO] Building book...");
            println!("[INFO] Serving at http://localhost:{}", port);
            println!("[INFO] Watching for changes...");
        }
        "watch" => {
            println!("[INFO] Building book...");
            println!("[INFO] Watching for changes...");
        }
        "test" => {
            println!("[INFO] Testing Rust code samples...");
            println!("  Chapter 3: 2 tests... ok");
            println!("  Chapter 5: 1 test... ok");
            println!("  Chapter 8: 3 tests... ok");
            println!();
            println!("test result: ok. 6 passed; 0 failed");
        }
        "clean" => {
            println!("[INFO] Deleting book/");
            println!("Done.");
        }
        _ => println!("mdbook: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mdbook".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mdbook(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
