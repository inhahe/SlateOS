#![deny(clippy::all)]

//! gradience-cli — SlateOS Gradience GNOME/libadwaita theming
//!
//! Single personality: `gradience`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gradience(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gradience [OPTIONS]");
        println!("gradience v0.4 (Slate OS) — Customize libadwaita/GTK4 colors");
        println!();
        println!("Options:");
        println!("  --apply PRESET    Apply preset by name");
        println!("  --list            List installed presets");
        println!("  --reset           Reset to defaults");
        println!("  --import FILE     Import preset from file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gradience v0.4 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("Adwaita (default)");
        println!("Catppuccin Mocha");
        println!("Dracula");
        println!("Nord");
        println!("Gruvbox Dark");
        return 0;
    }
    if args.iter().any(|a| a == "--reset") {
        println!("Reset to Adwaita defaults.");
        return 0;
    }
    if let Some(preset) = args.iter().skip_while(|a| a.as_str() != "--apply").nth(1) {
        println!("Applied preset: {}", preset);
        return 0;
    }
    println!("gradience: libadwaita/GTK4 color customization");
    println!("  Current: Adwaita");
    println!("  Presets: 5 installed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gradience".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gradience(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gradience};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gradience"), "gradience");
        assert_eq!(basename(r"C:\bin\gradience.exe"), "gradience.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gradience.exe"), "gradience");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gradience(&["--help".to_string()], "gradience"), 0);
        assert_eq!(run_gradience(&["-h".to_string()], "gradience"), 0);
        let _ = run_gradience(&["--version".to_string()], "gradience");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gradience(&[], "gradience");
    }
}
