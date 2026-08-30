#![deny(clippy::all)]

//! twitchstudio-cli — Slate OS Twitch Studio streaming app
//!
//! Single personality: `twitchstudio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ts(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: twitchstudio [OPTIONS]");
        println!("Twitch Studio (Slate OS) — Built-for-beginner streaming app from Twitch");
        println!();
        println!("Options:");
        println!("  --setup                Run setup wizard");
        println!("  --scenes               Open scene editor");
        println!("  --quality PROFILE      auto/720p30/720p60/1080p60");
        println!("  --start                Start streaming");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Twitch Studio 0.40.0 (Slate OS)"); return 0; }
    println!("Twitch Studio 0.40.0 (Slate OS)");
    println!("  Features: Guided setup, alerts, chat overlay, scenes, layouts");
    println!("  Encoders: x264 (CPU), NVENC (NVIDIA), AMF (AMD), QuickSync (Intel)");
    println!("  Quality presets: auto-adjust based on bandwidth + hardware");
    println!("  Integrations: Discord, IRL camera, screen + window capture");
    println!("  License: Free (Twitch account required)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "twitchstudio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ts(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ts};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/twitchstudio"), "twitchstudio");
        assert_eq!(basename(r"C:\bin\twitchstudio.exe"), "twitchstudio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("twitchstudio.exe"), "twitchstudio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ts(&["--help".to_string()], "twitchstudio"), 0);
        assert_eq!(run_ts(&["-h".to_string()], "twitchstudio"), 0);
        let _ = run_ts(&["--version".to_string()], "twitchstudio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ts(&[], "twitchstudio");
    }
}
