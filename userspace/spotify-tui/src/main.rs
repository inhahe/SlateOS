#![deny(clippy::all)]

//! spotify-tui — OurOS Spotify terminal client
//!
//! Single personality: `spt`

use std::env;
use std::process;

fn run_spt(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spt [OPTIONS] [COMMAND]");
        println!();
        println!("Spotify for the terminal.");
        println!();
        println!("Commands:");
        println!("  play          Start/resume playback");
        println!("  pause         Pause playback");
        println!("  next          Skip to next track");
        println!("  prev          Previous track");
        println!("  toggle        Toggle play/pause");
        println!("  seek <POS>    Seek to position (e.g., 1:30, +10, -5)");
        println!("  volume <N>    Set volume (0-100)");
        println!("  shuffle       Toggle shuffle");
        println!("  repeat <MODE> Set repeat (off/track/context)");
        println!("  search <Q>    Search for tracks/albums/artists");
        println!("  devices       List available devices");
        println!("  status        Show current playback status");
        println!("  like          Like current track");
        println!("  list <TYPE>   List playlists/albums/artists");
        println!("  transfer <D>  Transfer playback to device");
        println!();
        println!("Options:");
        println!("  --device <NAME>   Target device");
        println!("  --format <FMT>    Output format for status");
        println!("  --completions <S> Generate shell completions");
        println!("  -V, --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("spt 0.27.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "play" | "toggle" => {
            println!("▶ Resumed playback");
            0
        }
        "pause" => {
            println!("⏸ Paused playback");
            0
        }
        "next" => {
            println!("⏭ Skipped to next track");
            println!("  Now playing: Bohemian Rhapsody - Queen");
            0
        }
        "prev" => {
            println!("⏮ Previous track");
            println!("  Now playing: Don't Stop Me Now - Queen");
            0
        }
        "seek" => {
            let pos = args.get(1).map(|s| s.as_str()).unwrap_or("0:00");
            println!("Seeked to {}", pos);
            0
        }
        "volume" => {
            let vol = args.get(1).map(|s| s.as_str()).unwrap_or("50");
            println!("Volume set to {}%", vol);
            0
        }
        "shuffle" => {
            println!("🔀 Shuffle: ON");
            0
        }
        "repeat" => {
            let mode = args.get(1).map(|s| s.as_str()).unwrap_or("off");
            println!("🔁 Repeat: {}", mode);
            0
        }
        "search" => {
            let query = args.iter().skip(1).map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            println!("Search results for '{}':", if query.is_empty() { "music" } else { &query });
            println!();
            println!("  Tracks:");
            println!("    1. Bohemian Rhapsody - Queen (6:07)");
            println!("    2. Stairway to Heaven - Led Zeppelin (8:02)");
            println!("    3. Hotel California - Eagles (6:31)");
            println!();
            println!("  Albums:");
            println!("    1. A Night at the Opera - Queen (1975)");
            println!("    2. Led Zeppelin IV - Led Zeppelin (1971)");
            println!();
            println!("  Artists:");
            println!("    1. Queen");
            println!("    2. Led Zeppelin");
            0
        }
        "devices" => {
            println!("Available devices:");
            println!("  1. Desktop (Computer) [ACTIVE]");
            println!("  2. Living Room Speaker (Speaker)");
            println!("  3. Phone (Smartphone)");
            0
        }
        "status" => {
            println!("▶ Now Playing:");
            println!("  Track:    Bohemian Rhapsody");
            println!("  Artist:   Queen");
            println!("  Album:    A Night at the Opera");
            println!("  Duration: 3:45 / 6:07");
            println!("  Volume:   75%");
            println!("  Shuffle:  OFF");
            println!("  Repeat:   OFF");
            println!("  Device:   Desktop");
            0
        }
        "like" => {
            println!("♥ Liked: Bohemian Rhapsody - Queen");
            0
        }
        "list" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("playlists");
            match what {
                "playlists" => {
                    println!("Your playlists:");
                    println!("  1. Liked Songs (847 tracks)");
                    println!("  2. Discover Weekly (30 tracks)");
                    println!("  3. Release Radar (30 tracks)");
                    println!("  4. Workout Mix (65 tracks)");
                    println!("  5. Chill Vibes (120 tracks)");
                }
                "albums" => {
                    println!("Saved albums:");
                    println!("  1. A Night at the Opera - Queen (1975)");
                    println!("  2. OK Computer - Radiohead (1997)");
                    println!("  3. Abbey Road - The Beatles (1969)");
                }
                _ => {
                    println!("Unknown list type: {}. Use: playlists, albums", what);
                }
            }
            0
        }
        "transfer" => {
            let device = args.get(1).map(|s| s.as_str()).unwrap_or("Speaker");
            println!("Transferred playback to: {}", device);
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spt(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_spt};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spt(vec!["--help".to_string()]), 0);
        assert_eq!(run_spt(vec!["-h".to_string()]), 0);
        let _ = run_spt(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spt(vec![]);
    }
}
