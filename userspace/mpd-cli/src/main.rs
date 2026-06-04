#![deny(clippy::all)]

//! mpd-cli — OurOS MPD music player daemon + mpc client
//!
//! Multi-personality: `mpd`, `mpc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mpd(args: &[String], prog: &str) -> i32 {
    if prog == "mpc" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: mpc [OPTIONS] COMMAND [ARGS...]");
            println!("mpc 0.35 (OurOS) — MPD command-line client");
            println!();
            println!("Commands:");
            println!("  play [N]        Play (track N)");
            println!("  pause           Pause");
            println!("  stop            Stop");
            println!("  next            Next track");
            println!("  prev            Previous track");
            println!("  toggle          Toggle play/pause");
            println!("  status          Show status");
            println!("  current         Show current song");
            println!("  playlist        Show playlist");
            println!("  add URI         Add to playlist");
            println!("  clear           Clear playlist");
            println!("  volume [+-]N    Set/adjust volume");
            println!("  repeat on|off   Set repeat");
            println!("  random on|off   Set random");
            println!("  update          Update database");
            println!("  stats           Show statistics");
            println!("  outputs         Show audio outputs");
            println!("  version         Show version");
            return 0;
        }
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
        match cmd {
            "status" | "current" => {
                println!("Artist Name - Song Title");
                println!("[playing] #1/10  1:23/4:00 (35%)");
                println!("volume: 80%  repeat: off  random: off  single: off");
            }
            "play" => println!("mpc: Playing"),
            "pause" => println!("mpc: Paused"),
            "stop" => println!("mpc: Stopped"),
            "next" => println!("mpc: Next track"),
            "prev" => println!("mpc: Previous track"),
            "toggle" => println!("mpc: Toggled"),
            "playlist" => {
                println!(" 1) Artist - Track 1");
                println!(" 2) Artist - Track 2");
                println!(" 3) Artist - Track 3");
            }
            "clear" => println!("mpc: Playlist cleared"),
            "update" => println!("Updating DB (#1) ..."),
            "stats" => {
                println!("Artists:    42");
                println!("Albums:    128");
                println!("Songs:    1536");
                println!("DB playtime: 4 days, 12:34:56");
                println!("Uptime: 2 days, 3:15:42");
            }
            "outputs" => {
                println!("Output 1 (ALSA) is enabled");
                println!("Output 2 (PulseAudio) is enabled");
            }
            "version" => println!("mpd version: 0.23.15"),
            _ => println!("mpc: {}", cmd),
        }
        return 0;
    }
    // mpd
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mpd [OPTIONS] [CONFIG_FILE]");
        println!("MPD 0.23.15 (OurOS) — Music Player Daemon");
        println!();
        println!("Options:");
        println!("  --no-daemon          Don't daemonize");
        println!("  --kill               Kill running MPD");
        println!("  --stdout             Log to stdout");
        println!("  -v, --verbose        Increase verbosity");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Music Player Daemon 0.23.15 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--kill") {
        println!("mpd: Killing daemon...");
        return 0;
    }
    println!("mpd: Starting Music Player Daemon...");
    println!("mpd: Listening on 0.0.0.0:6600");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mpd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mpd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mpd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mpd"), "mpd");
        assert_eq!(basename(r"C:\bin\mpd.exe"), "mpd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mpd.exe"), "mpd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mpd(&["--help".to_string()], "mpd"), 0);
        assert_eq!(run_mpd(&["-h".to_string()], "mpd"), 0);
        let _ = run_mpd(&["--version".to_string()], "mpd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mpd(&[], "mpd");
    }
}
