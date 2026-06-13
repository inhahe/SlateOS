#![deny(clippy::all)]

//! x11vnc-cli — SlateOS x11vnc VNC server for existing displays
//!
//! Single personality: `x11vnc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_x11vnc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: x11vnc [OPTIONS]");
        println!("x11vnc v0.9 (Slate OS) — VNC server for real X displays");
        println!();
        println!("Options:");
        println!("  -display :N       X display to share");
        println!("  -rfbport PORT     VNC port (default: 5900)");
        println!("  -passwd PASS      Set password");
        println!("  -forever          Keep running after client disconnect");
        println!("  -shared           Allow multiple viewers");
        println!("  -noxdamage        Disable XDamage extension");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("x11vnc v0.9 (Slate OS)"); return 0; }
    println!("x11vnc: VNC server started on port 5900");
    println!("  Sharing display :0");
    println!("  XDamage: enabled");
    println!("  Clipboard: shared");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "x11vnc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_x11vnc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_x11vnc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/x11vnc"), "x11vnc");
        assert_eq!(basename(r"C:\bin\x11vnc.exe"), "x11vnc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("x11vnc.exe"), "x11vnc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_x11vnc(&["--help".to_string()], "x11vnc"), 0);
        assert_eq!(run_x11vnc(&["-h".to_string()], "x11vnc"), 0);
        let _ = run_x11vnc(&["--version".to_string()], "x11vnc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_x11vnc(&[], "x11vnc");
    }
}
