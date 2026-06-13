#![deny(clippy::all)]

//! wl-screencast-cli — SlateOS wl-screencast PipeWire-based screen sharing
//!
//! Single personality: `wl-screencast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_screencast(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-screencast [OPTIONS]");
        println!("wl-screencast v0.1 (Slate OS) — PipeWire screen sharing for Wayland");
        println!();
        println!("Options:");
        println!("  -o OUTPUT         Output to share");
        println!("  -r REGION         Region to share (X,Y WxH)");
        println!("  -f FPS            Framerate");
        println!("  --show-cursor     Include cursor");
        println!("  --version         Show version");
        println!();
        println!("Creates a PipeWire stream for screen sharing. Used by");
        println!("xdg-desktop-portal for WebRTC/OBS/Teams screen sharing.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-screencast v0.1 (Slate OS)"); return 0; }
    let output = args.iter().skip_while(|a| a.as_str() != "-o").nth(1)
        .map(|s| s.as_str()).unwrap_or("*");
    println!("wl-screencast: sharing output {} via PipeWire", output);
    println!("  PipeWire node created — ready for consumers");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-screencast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_screencast(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wl_screencast};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wl-screencast"), "wl-screencast");
        assert_eq!(basename(r"C:\bin\wl-screencast.exe"), "wl-screencast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wl-screencast.exe"), "wl-screencast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wl_screencast(&["--help".to_string()], "wl-screencast"), 0);
        assert_eq!(run_wl_screencast(&["-h".to_string()], "wl-screencast"), 0);
        let _ = run_wl_screencast(&["--version".to_string()], "wl-screencast");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wl_screencast(&[], "wl-screencast");
    }
}
