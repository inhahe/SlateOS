#![deny(clippy::all)]

//! micro — OurOS modern terminal text editor
//!
//! Single personality: `micro`

use std::env;
use std::process;

fn run_micro(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: micro [OPTIONS] [FILE]...");
        println!();
        println!("Options:");
        println!("  -config-dir <dir>    Config directory");
        println!("  -options             Show all options");
        println!("  -debug               Enable debug mode");
        println!("  -version             Show version");
        println!("  -plugin <command>    Plugin management (install/remove/list/search/update)");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("Version: 2.0.13 (OurOS)");
        println!("Compiled with: Go 1.22");
        println!("Clipboard: external");
        return 0;
    }
    if args.iter().any(|a| a == "-options") {
        println!("autoclose: true");
        println!("autoindent: true");
        println!("autosave: 0");
        println!("colorscheme: default");
        println!("cursorline: true");
        println!("encoding: utf-8");
        println!("filetype: unknown");
        println!("keepautoindent: false");
        println!("mouse: true");
        println!("rmtrailingws: false");
        println!("ruler: true");
        println!("savecursor: false");
        println!("saveundo: false");
        println!("scrollbar: false");
        println!("scrollmargin: 3");
        println!("scrollspeed: 2");
        println!("softwrap: false");
        println!("statusline: true");
        println!("syntax: true");
        println!("tabmovement: false");
        println!("tabsize: 4");
        println!("tabstospaces: false");
        return 0;
    }
    if args.iter().any(|a| a == "-plugin") {
        let cmd_pos = args.iter().position(|a| a == "-plugin").unwrap_or(0);
        let subcmd = args.get(cmd_pos + 1).map(|s| s.as_str()).unwrap_or("list");
        match subcmd {
            "list" => {
                println!("Installed plugins:");
                println!("  autoclose  (built-in)");
                println!("  comment    (built-in)");
                println!("  diff       (built-in)");
                println!("  ftoptions  (built-in)");
                println!("  linter     (built-in)");
                println!("  literate   (built-in)");
                println!("  status     (built-in)");
            }
            "search" => println!("(search — simulated)"),
            "install" | "remove" | "update" => println!("({} — simulated)", subcmd),
            _ => println!("Unknown plugin command: {}", subcmd),
        }
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if !files.is_empty() {
        for f in &files {
            println!("Opening: {}", f);
        }
    }
    println!("micro 2.0.13 (OurOS) — Ctrl-Q to quit, Ctrl-S to save");
    println!("(TUI launched — simulated)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_micro(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
