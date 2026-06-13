#![deny(clippy::all)]

//! xsplit-cli — SlateOS XSplit Broadcaster streaming app
//!
//! Single personality: `xsplit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xsplit [OPTIONS]");
        println!("XSplit Broadcaster 4.6 (Slate OS) — Pro streaming & recording");
        println!();
        println!("Options:");
        println!("  --gamecaster           Launch XSplit Gamecaster (game streaming)");
        println!("  --vcam                 Launch XSplit VCam (virtual webcam)");
        println!("  --presenter            Launch XSplit Presenter (presentations)");
        println!("  --scene NAME           Switch to scene");
        println!("  --start                Start broadcasting");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("XSplit Broadcaster 4.6.2402 (Slate OS)"); return 0; }
    println!("XSplit Broadcaster 4.6.2402 (Slate OS)");
    println!("  Apps: Broadcaster, Gamecaster, VCam, Presenter, Connect");
    println!("  Sources: Game, Window, Display, Webcam, Image, Video, Browser");
    println!("  Outputs: YouTube/Twitch/Facebook/Custom RTMP, NDI, multi-stream");
    println!("  Plugins: Stinger transitions, lookups, chroma key, scene presets");
    println!("  License: Free / Premium subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xsplit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xsplit"), "xsplit");
        assert_eq!(basename(r"C:\bin\xsplit.exe"), "xsplit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xsplit.exe"), "xsplit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xs(&["--help".to_string()], "xsplit"), 0);
        assert_eq!(run_xs(&["-h".to_string()], "xsplit"), 0);
        let _ = run_xs(&["--version".to_string()], "xsplit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xs(&[], "xsplit");
    }
}
