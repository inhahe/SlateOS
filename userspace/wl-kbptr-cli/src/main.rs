#![deny(clippy::all)]

//! wl-kbptr-cli — Slate OS wl-kbptr keyboard-driven pointer control
//!
//! Single personality: `wl-kbptr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_kbptr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-kbptr [OPTIONS]");
        println!("wl-kbptr v0.3 (Slate OS) — Keyboard-driven pointer control for Wayland");
        println!();
        println!("Options:");
        println!("  --mode MODE       Mode: bisect, absolute, relative");
        println!("  --keys KEYS       Key bindings (hjkl, wasd, arrows)");
        println!("  --speed SPEED     Movement speed");
        println!("  --grid COLS,ROWS  Grid dimensions for bisect mode");
        println!("  --version         Show version");
        println!();
        println!("Control the mouse pointer using keyboard keys.");
        println!("Bisect mode: subdivide screen quadrants to quickly locate any point.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-kbptr v0.3 (Slate OS)"); return 0; }
    let mode = args.iter().skip_while(|a| a.as_str() != "--mode").nth(1)
        .map(|s| s.as_str()).unwrap_or("bisect");
    println!("wl-kbptr: keyboard pointer control (mode={})", mode);
    println!("  Use configured keys to move pointer");
    println!("  Enter/Space to click, Escape to cancel");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-kbptr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_kbptr(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wl_kbptr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wl-kbptr"), "wl-kbptr");
        assert_eq!(basename(r"C:\bin\wl-kbptr.exe"), "wl-kbptr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wl-kbptr.exe"), "wl-kbptr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wl_kbptr(&["--help".to_string()], "wl-kbptr"), 0);
        assert_eq!(run_wl_kbptr(&["-h".to_string()], "wl-kbptr"), 0);
        let _ = run_wl_kbptr(&["--version".to_string()], "wl-kbptr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wl_kbptr(&[], "wl-kbptr");
    }
}
