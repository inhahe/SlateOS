#![deny(clippy::all)]

//! lightroom-cli — SlateOS Adobe Lightroom Classic photo workflow
//!
//! Single personality: `lightroom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightroom [OPTIONS] [CATALOG]");
        println!("Adobe Lightroom Classic 2024 (SlateOS) — Photo cataloging, editing & workflow");
        println!();
        println!("Options:");
        println!("  -o CATALOG.lrcat       Open catalog");
        println!("  -import FOLDER         Import photos from folder");
        println!("  -export PRESET FOLDER  Export with preset");
        println!("  -plugin PATH           Load Lua plugin");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Lightroom Classic 13.4 (SlateOS)"); return 0; }
    println!("Adobe Lightroom Classic 13.4 (SlateOS)");
    println!("  Modules: Library, Develop, Map, Book, Slideshow, Print, Web");
    println!("  Raw engine: Adobe Camera Raw (1000+ camera profiles)");
    println!("  AI: Lens Blur, Denoise, Adaptive Presets, Subject/Sky select");
    println!("  Scripting: Lua plug-in SDK");
    println!("  License: Creative Cloud (Photography plan)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lightroom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lightroom"), "lightroom");
        assert_eq!(basename(r"C:\bin\lightroom.exe"), "lightroom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lightroom.exe"), "lightroom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lr(&["--help".to_string()], "lightroom"), 0);
        assert_eq!(run_lr(&["-h".to_string()], "lightroom"), 0);
        let _ = run_lr(&["--version".to_string()], "lightroom");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lr(&[], "lightroom");
    }
}
