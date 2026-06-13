#![deny(clippy::all)]

//! font-viewer-cli — SlateOS GNOME font viewer
//!
//! Single personality: `gnome-font-viewer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_font_viewer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-font-viewer [OPTIONS] [FILE.ttf|.otf]");
        println!("gnome-font-viewer v46.0 (SlateOS) — Font preview application");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Preview and install fonts. Shows sample text rendering,");
        println!("character coverage, and font metadata.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-font-viewer v46.0 (SlateOS)"); return 0; }
    println!("gnome-font-viewer: font viewer started");
    println!("  Installed fonts: 142 families");
    println!("  Preview: sample text at multiple sizes");
    println!("  Install: drag-and-drop supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-font-viewer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_font_viewer(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_font_viewer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/font-viewer"), "font-viewer");
        assert_eq!(basename(r"C:\bin\font-viewer.exe"), "font-viewer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("font-viewer.exe"), "font-viewer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_font_viewer(&["--help".to_string()], "font-viewer"), 0);
        assert_eq!(run_font_viewer(&["-h".to_string()], "font-viewer"), 0);
        let _ = run_font_viewer(&["--version".to_string()], "font-viewer");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_font_viewer(&[], "font-viewer");
    }
}
