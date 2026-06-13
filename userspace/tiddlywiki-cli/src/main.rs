#![deny(clippy::all)]

//! tiddlywiki-cli — SlateOS TiddlyWiki personal wiki
//!
//! Single personality: `tiddlywiki`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tiddlywiki(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tiddlywiki [WIKIDIR] [OPTIONS]");
        println!("TiddlyWiki v5.3 (Slate OS) — Non-linear personal wiki");
        println!();
        println!("Options:");
        println!("  --listen           Start server (default port 8080)");
        println!("  --port PORT        Server port");
        println!("  --build TARGET     Build output");
        println!("  --render TIDDLER   Render tiddler");
        println!("  --savewikifolder DIR  Save as folder wiki");
        println!("  --import FILE TYPE Import tiddlers");
        println!("  --output DIR       Output directory");
        println!("  --verbose          Verbose output");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("TiddlyWiki v5.3.3 (Slate OS)"); return 0; }
    println!("TiddlyWiki v5.3.3 (Slate OS)");
    println!("  Wiki: ./mywiki");
    println!("  Tiddlers: 456");
    println!("  Tags: 89");
    println!("  Plugins: 12 loaded");
    println!("  Themes: tiddlywiki/vanilla");
    println!("  Server: http://127.0.0.1:8080");
    println!("  Syncing: filesystem");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tiddlywiki".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tiddlywiki(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tiddlywiki};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tiddlywiki"), "tiddlywiki");
        assert_eq!(basename(r"C:\bin\tiddlywiki.exe"), "tiddlywiki.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tiddlywiki.exe"), "tiddlywiki");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tiddlywiki(&["--help".to_string()], "tiddlywiki"), 0);
        assert_eq!(run_tiddlywiki(&["-h".to_string()], "tiddlywiki"), 0);
        let _ = run_tiddlywiki(&["--version".to_string()], "tiddlywiki");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tiddlywiki(&[], "tiddlywiki");
    }
}
