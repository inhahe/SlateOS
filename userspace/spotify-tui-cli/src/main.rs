#![deny(clippy::all)]

//! spotify-tui-cli — SlateOS spotify-tui client
//!
//! Single personality: `spt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spt [OPTIONS] [COMMAND]");
        println!("spotify-tui 0.25.0 (Slate OS) — Terminal Spotify client");
        println!();
        println!("Commands:");
        println!("  playback         Interact with playback");
        println!("  play             Resume playback");
        println!("  search QUERY     Search");
        println!("  list TYPE        List (devices, playlists)");
        println!();
        println!("Options:");
        println!("  -t, --tick-rate MS    Tick rate (default 250)");
        println!("  -c, --config DIR      Config directory");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("spotify-tui 0.25.0 (Slate OS)");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    match cmd {
        Some("playback") => {
            println!("Now playing: Artist - Song Title");
            println!("Progress: 1:23 / 4:00");
            println!("Volume: 80%");
            println!("Shuffle: off  Repeat: off");
        }
        Some("play") => println!("spt: Resuming playback"),
        Some("search") => {
            let query = args.iter().skip_while(|a| a.as_str() != "search").nth(1)
                .map(|s| s.as_str()).unwrap_or("");
            println!("spt search '{}': (results)", query);
        }
        Some("list") => {
            let what = args.iter().skip_while(|a| a.as_str() != "list").nth(1)
                .map(|s| s.as_str()).unwrap_or("playlists");
            match what {
                "devices" => println!("  1. Desktop (active)"),
                "playlists" => {
                    println!("  1. Liked Songs (128 tracks)");
                    println!("  2. Discover Weekly (30 tracks)");
                }
                _ => println!("spt list: {}", what),
            }
        }
        _ => println!("spt: Opening Spotify TUI..."),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spotify-tui"), "spotify-tui");
        assert_eq!(basename(r"C:\bin\spotify-tui.exe"), "spotify-tui.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spotify-tui.exe"), "spotify-tui");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spt(&["--help".to_string()], "spotify-tui"), 0);
        assert_eq!(run_spt(&["-h".to_string()], "spotify-tui"), 0);
        let _ = run_spt(&["--version".to_string()], "spotify-tui");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spt(&[], "spotify-tui");
    }
}
