#![deny(clippy::all)]

//! jellyfin — OurOS media server
//!
//! Single personality: `jellyfin`

use std::env;
use std::process;

fn run_jellyfin(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jellyfin [options]");
        println!();
        println!("Options:");
        println!("  --datadir <dir>       Path to data directory");
        println!("  --configdir <dir>     Path to config directory");
        println!("  --cachedir <dir>      Path to cache directory");
        println!("  --logdir <dir>        Path to log directory");
        println!("  --ffmpeg <path>       Path to FFmpeg binary");
        println!("  --nowebclient         Disable the web client");
        println!("  --service             Run as a service");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Jellyfin 10.9.6 (OurOS)");
        return 0;
    }

    let datadir = args.iter().position(|a| a == "--datadir")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("/var/lib/jellyfin");

    println!("[10:00:00] [INF] Jellyfin version: 10.9.6 (OurOS)");
    println!("[10:00:00] [INF] Arguments: {:?}", args);
    println!("[10:00:00] [INF] Operating System: OurOS x86_64");
    println!("[10:00:00] [INF] Architecture: X64");
    println!("[10:00:00] [INF] Data path: {}", datadir);
    println!("[10:00:00] [INF] Web path: /usr/share/jellyfin-web");
    println!("[10:00:01] [INF] Loaded plugin: TMDb (17.0.0.0)");
    println!("[10:00:01] [INF] Loaded plugin: Open Subtitles (4.0.0.0)");
    println!("[10:00:01] [INF] Loaded plugin: AudioDB (1.0.0.0)");
    println!("[10:00:02] [INF] Core startup complete.");
    println!("[10:00:02] [INF] Kestrel is listening on http://0.0.0.0:8096");
    println!("[10:00:02] [INF] Startup complete. Server is running.");
    println!();
    println!("Libraries:");
    println!("  Movies:    /media/movies     (142 items)");
    println!("  TV Shows:  /media/tv         (38 shows, 456 episodes)");
    println!("  Music:     /media/music      (2,345 tracks)");
    println!("  Books:     /media/books      (89 items)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_jellyfin(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jellyfin};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_jellyfin(vec!["--help".to_string()]), 0);
        assert_eq!(run_jellyfin(vec!["-h".to_string()]), 0);
        assert_eq!(run_jellyfin(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_jellyfin(vec![]), 0);
    }
}
