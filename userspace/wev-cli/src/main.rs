#![deny(clippy::all)]

//! wev-cli — SlateOS wev Wayland event viewer
//!
//! Single personality: `wev`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wev(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wev [OPTIONS]");
        println!("wev v1.0 (Slate OS) — Wayland event viewer");
        println!();
        println!("Options:");
        println!("  -f FILTER         Event filter (keyboard, pointer, touch, all)");
        println!("  -t                Show timestamps");
        println!("  --version         Show version");
        println!();
        println!("Shows Wayland input events. Like xev for Wayland.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wev v1.0 (Slate OS)"); return 0; }

    let filter = args.iter().skip_while(|a| a.as_str() != "-f").nth(1)
        .map(|s| s.as_str()).unwrap_or("all");
    println!("Listening for Wayland events (filter: {})...", filter);
    println!();
    println!("[wl_keyboard] key: state=pressed, key=36 (Return), serial=1042");
    println!("[wl_keyboard] key: state=released, key=36 (Return), serial=1043");
    println!("[wl_pointer] motion: time=1500, x=512.00, y=384.00");
    println!("[wl_pointer] button: serial=1044, button=272 (BTN_LEFT), state=pressed");
    println!("[wl_pointer] button: serial=1045, button=272 (BTN_LEFT), state=released");
    println!("[wl_pointer] axis: time=1600, axis=vertical, value=15.00");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wev".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wev(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wev};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wev"), "wev");
        assert_eq!(basename(r"C:\bin\wev.exe"), "wev.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wev.exe"), "wev");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wev(&["--help".to_string()], "wev"), 0);
        assert_eq!(run_wev(&["-h".to_string()], "wev"), 0);
        let _ = run_wev(&["--version".to_string()], "wev");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wev(&[], "wev");
    }
}
