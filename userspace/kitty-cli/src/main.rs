#![deny(clippy::all)]

//! kitty-cli — SlateOS Kitty terminal emulator tools
//!
//! Multi-personality: `kitty`, `kitten`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kitty(args: &[String], prog: &str) -> i32 {
    if prog == "kitten" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: kitten COMMAND [ARGS...]");
            println!("Kitten — Kitty extensions");
            println!();
            println!("Commands:");
            println!("  icat             Display images in terminal");
            println!("  diff             Diff files with syntax highlighting");
            println!("  clipboard        Interact with clipboard");
            println!("  unicode_input    Unicode character picker");
            println!("  themes           Browse and apply themes");
            println!("  ssh              SSH with Kitty features");
            println!("  transfer         Transfer files over SSH");
            println!("  hyperlinked_grep Grep with clickable results");
            return 0;
        }
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("icat");
        match cmd {
            "icat" => {
                let file = args.get(1).map(|s| s.as_str()).unwrap_or("<image>");
                println!("kitten icat: Displaying '{}'", file);
            }
            "diff" => println!("kitten diff: (diff viewer)"),
            "clipboard" => println!("kitten clipboard: (clipboard access)"),
            "themes" => println!("kitten themes: (theme browser)"),
            "ssh" => println!("kitten ssh: (enhanced SSH)"),
            _ => println!("kitten {}: (running)", cmd),
        }
        return 0;
    }
    // kitty
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kitty [OPTIONS] [COMMAND...]");
        println!("kitty 0.35.2 (Slate OS) — GPU-accelerated terminal emulator");
        println!();
        println!("Options:");
        println!("  --config FILE, -c FILE     Config file");
        println!("  --override KEY=VAL, -o K=V Override config option");
        println!("  --directory DIR, -d DIR    Working directory");
        println!("  --session FILE             Session file");
        println!("  --title TEXT, -T TEXT       Window title");
        println!("  --class CLASS              Window class");
        println!("  --start-as STATE           normal/fullscreen/maximized/minimized");
        println!("  --single-instance, -1      Single instance mode");
        println!("  --detach                   Detach from terminal");
        println!("  --version                  Show version");
        println!("  --dump-theme               Dump current theme");
        println!("  --debug-config             Debug config");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("kitty 0.35.2 created by Kovid Goyal (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--dump-theme") {
        println!("foreground #dddddd");
        println!("background #000000");
        println!("cursor #cccccc");
        return 0;
    }
    if args.iter().any(|a| a == "--debug-config") {
        println!("font_family: monospace");
        println!("font_size: 12.0");
        println!("scrollback_lines: 2000");
        return 0;
    }
    println!("kitty: Starting GPU-accelerated terminal...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kitty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kitty(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kitty};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kitty"), "kitty");
        assert_eq!(basename(r"C:\bin\kitty.exe"), "kitty.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kitty.exe"), "kitty");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kitty(&["--help".to_string()], "kitty"), 0);
        assert_eq!(run_kitty(&["-h".to_string()], "kitty"), 0);
        let _ = run_kitty(&["--version".to_string()], "kitty");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kitty(&[], "kitty");
    }
}
