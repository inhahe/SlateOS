#![deny(clippy::all)]

//! xvkbd-cli — Slate OS virtual keyboard
//!
//! Single personality: `xvkbd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xvkbd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xvkbd [OPTIONS]");
        println!("xvkbd v4.1 (Slate OS) — Virtual keyboard for accessibility");
        println!();
        println!("Options:");
        println!("  -text STRING     Send keystrokes");
        println!("  -window TITLE    Target window");
        println!("  -widget NAME     Target widget");
        println!("  -delay N         Delay between keys (ms)");
        println!("  -no-repeat       Disable key repeat");
        println!("  -compact         Compact keyboard layout");
        println!("  -keypad          Show numeric keypad");
        println!("  -modifiers       Show modifier keys panel");
        println!("  -geometry WxH    Window geometry");
        println!("  -xsendevent      Use XSendEvent");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xvkbd v4.1 (Slate OS)"); return 0; }
    if let Some(text) = args.windows(2).find(|w| w[0] == "-text").map(|w| w[1].as_str()) {
        println!("xvkbd: sending keystrokes: {}", text);
        println!("xvkbd: {} characters sent", text.len());
        return 0;
    }
    println!("xvkbd v4.1 (Slate OS) — Virtual Keyboard");
    println!("  Layout: US English (QWERTY)");
    println!("  Mode: on-screen keyboard");
    println!("  Geometry: 640x200");
    println!("  Status: displayed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xvkbd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xvkbd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xvkbd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xvkbd"), "xvkbd");
        assert_eq!(basename(r"C:\bin\xvkbd.exe"), "xvkbd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xvkbd.exe"), "xvkbd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xvkbd(&["--help".to_string()], "xvkbd"), 0);
        assert_eq!(run_xvkbd(&["-h".to_string()], "xvkbd"), 0);
        let _ = run_xvkbd(&["--version".to_string()], "xvkbd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xvkbd(&[], "xvkbd");
    }
}
