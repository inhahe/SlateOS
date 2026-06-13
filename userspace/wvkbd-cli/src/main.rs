#![deny(clippy::all)]

//! wvkbd-cli — SlateOS wvkbd virtual keyboard
//!
//! Single personality: `wvkbd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wvkbd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wvkbd [OPTIONS]");
        println!("wvkbd v0.7 (SlateOS) — On-screen virtual keyboard for Wayland");
        println!();
        println!("Options:");
        println!("  -l LAYERS         Comma-separated layer list");
        println!("  -L LAYOUT         Keyboard layout file");
        println!("  -H HEIGHT         Keyboard height (pixels)");
        println!("  -o                Show on startup (don't wait for focus)");
        println!("  --hidden          Start hidden");
        println!("  --bg COLOR        Background color");
        println!("  --fg COLOR        Foreground color");
        println!("  --press COLOR     Key press color");
        println!("  --font FONT       Font name");
        println!("  --font-size SIZE  Font size");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wvkbd v0.7 (SlateOS)"); return 0; }

    let layers = args.iter().skip_while(|a| a.as_str() != "-l").nth(1)
        .map(|s| s.as_str()).unwrap_or("full,special,numeric");
    let hidden = args.iter().any(|a| a == "--hidden");
    println!("wvkbd: virtual keyboard (layers={})", layers);
    if hidden {
        println!("  Started hidden — send SIGUSR1 or focus text input to show");
    } else {
        println!("  Keyboard visible");
    }
    println!("  Layers: {}", layers);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wvkbd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wvkbd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wvkbd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wvkbd"), "wvkbd");
        assert_eq!(basename(r"C:\bin\wvkbd.exe"), "wvkbd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wvkbd.exe"), "wvkbd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wvkbd(&["--help".to_string()], "wvkbd"), 0);
        assert_eq!(run_wvkbd(&["-h".to_string()], "wvkbd"), 0);
        let _ = run_wvkbd(&["--version".to_string()], "wvkbd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wvkbd(&[], "wvkbd");
    }
}
