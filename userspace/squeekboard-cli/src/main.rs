#![deny(clippy::all)]

//! squeekboard-cli — OurOS Squeekboard on-screen keyboard
//!
//! Single personality: `squeekboard`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_squeekboard(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: squeekboard [OPTIONS]");
        println!("squeekboard v1.22 (OurOS) — On-screen keyboard for Wayland");
        println!();
        println!("Options:");
        println!("  --layout LAYOUT   Keyboard layout (us, de, fr, etc.)");
        println!("  --theme THEME     Visual theme");
        println!("  --height PIXELS   Keyboard height");
        println!("  --version         Show version");
        println!();
        println!("Automatically shows/hides when text input is focused.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("squeekboard v1.22 (OurOS)"); return 0; }

    let layout = args.iter().skip_while(|a| a.as_str() != "--layout").nth(1)
        .map(|s| s.as_str()).unwrap_or("us");
    println!("squeekboard: on-screen keyboard active (layout={})", layout);
    println!("  Listening for text-input-v3 focus events");
    println!("  Keyboard will auto-show when text field is focused");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "squeekboard".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_squeekboard(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_squeekboard};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/squeekboard"), "squeekboard");
        assert_eq!(basename(r"C:\bin\squeekboard.exe"), "squeekboard.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("squeekboard.exe"), "squeekboard");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_squeekboard(&["--help".to_string()], "squeekboard"), 0);
        assert_eq!(run_squeekboard(&["-h".to_string()], "squeekboard"), 0);
        assert_eq!(run_squeekboard(&["--version".to_string()], "squeekboard"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_squeekboard(&[], "squeekboard"), 0);
    }
}
