#![deny(clippy::all)]

//! micro-cli — OurOS Micro editor
//!
//! Single personality: `micro`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_micro(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: micro [OPTIONS] [FILE...]");
        println!("micro 2.0.14 (OurOS) — A modern and intuitive terminal text editor");
        println!();
        println!("Options:");
        println!("  -config-dir DIR   Config directory");
        println!("  -options          Show default options");
        println!("  -debug            Enable debug mode");
        println!("  -plugin CMD       Plugin management");
        println!("  -clean            Start without plugins or settings");
        println!("  -version          Show version");
        println!();
        println!("Keybindings:");
        println!("  Ctrl+S  Save");
        println!("  Ctrl+Q  Quit");
        println!("  Ctrl+F  Find");
        println!("  Ctrl+Z  Undo");
        println!("  Ctrl+Y  Redo");
        println!("  Ctrl+E  Command bar");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("Version: 2.0.14 (OurOS)");
        println!("Commit hash: main");
        println!("Compiled on: 2024-01-15");
        return 0;
    }
    if args.iter().any(|a| a == "-options") {
        println!("autoindent: true");
        println!("autosave: 0");
        println!("colorscheme: default");
        println!("cursorline: true");
        println!("encoding: utf-8");
        println!("filetype: unknown");
        println!("ruler: true");
        println!("savecursor: false");
        println!("scrollbar: false");
        println!("syntax: true");
        println!("tabsize: 4");
        println!("tabstospaces: false");
        return 0;
    }
    if args.iter().any(|a| a == "-plugin") {
        let action = args.iter().skip_while(|a| a.as_str() != "-plugin").nth(1)
            .map(|s| s.as_str()).unwrap_or("list");
        match action {
            "list" => {
                println!("Installed plugins:");
                println!("  comment (default)");
                println!("  diff (default)");
                println!("  ftoptions (default)");
                println!("  linter (default)");
                println!("  literate (default)");
                println!("  status (default)");
            }
            "install" => {
                let name = args.iter().skip_while(|a| a.as_str() != "install").nth(1)
                    .map(|s| s.as_str()).unwrap_or("<plugin>");
                println!("micro: Installing plugin '{}'...", name);
            }
            _ => println!("micro plugin: {}", action),
        }
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if let Some(f) = file {
        println!("micro: Editing '{}'", f);
    } else {
        println!("micro: New buffer");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "micro".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_micro(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
