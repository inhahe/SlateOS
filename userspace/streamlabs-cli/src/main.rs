#![deny(clippy::all)]

//! streamlabs-cli — OurOS Streamlabs Desktop streaming app
//!
//! Single personality: `streamlabs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: streamlabs [OPTIONS]");
        println!("Streamlabs Desktop 1.18 (OurOS) — All-in-one streaming + alerts");
        println!();
        println!("Options:");
        println!("  --import-obs           Import OBS scenes");
        println!("  --alerts               Open alert box editor");
        println!("  --themes               Browse stream themes");
        println!("  --multistream          Multistream to multiple platforms");
        println!("  --start                Start streaming");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Streamlabs Desktop 1.18.0 (OurOS)"); return 0; }
    println!("Streamlabs Desktop 1.18.0 (OurOS)");
    println!("  Core: Built on OBS Studio fork (Streamlabs OBS)");
    println!("  Alerts: Follow/Sub/Cheer/Donation/Raid overlays");
    println!("  Multistream: Twitch + YouTube + Facebook + Trovo simultaneously");
    println!("  Cloudbot: Chat moderation, mini-games, song requests");
    println!("  License: Free (Ultra subscription for premium themes/widgets)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "streamlabs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/streamlabs"), "streamlabs");
        assert_eq!(basename(r"C:\bin\streamlabs.exe"), "streamlabs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("streamlabs.exe"), "streamlabs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sl(&["--help".to_string()], "streamlabs"), 0);
        assert_eq!(run_sl(&["-h".to_string()], "streamlabs"), 0);
        assert_eq!(run_sl(&["--version".to_string()], "streamlabs"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sl(&[], "streamlabs"), 0);
    }
}
