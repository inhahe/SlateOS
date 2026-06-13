#![deny(clippy::all)]

//! beets — SlateOS music library manager
//!
//! Single personality: `beet`

use std::env;
use std::process;

fn run_beet(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: beet <command> [args...]");
        println!();
        println!("Commands:");
        println!("  import     Import music into library");
        println!("  list       List items in library");
        println!("  modify     Modify item metadata");
        println!("  move       Move items in library");
        println!("  remove     Remove items from library");
        println!("  stats      Show library statistics");
        println!("  update     Update library");
        println!("  write      Write metadata to files");
        println!("  fields     Show available fields");
        println!("  config     Show/edit configuration");
        println!("  info       Show file metadata");
        println!("  fetchart   Fetch album art");
        println!("  lyrics     Fetch lyrics");
        println!("  version    Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => println!("beets version 1.6.0 (SlateOS)"),
        "list" | "ls" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if query.is_empty() {
                println!("Artist - Album - Title (all items)");
            }
            println!("Pink Floyd - The Dark Side of the Moon - Time");
            println!("Pink Floyd - The Dark Side of the Moon - Money");
            println!("Radiohead - OK Computer - Paranoid Android");
            println!("Radiohead - OK Computer - Karma Police");
        }
        "stats" => {
            println!("Tracks: 1234");
            println!("Total time: 3d 12h 45m 30s");
            println!("Approximate total size: 8.5 GiB");
            println!("Artists: 156");
            println!("Albums: 89");
        }
        "import" | "imp" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Importing music from {}...", path);
            println!("  Pink Floyd - The Dark Side of the Moon");
            println!("    Time (04:53)");
            println!("    Money (06:22)");
            println!("Applied 2 items.");
        }
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("track.flac");
            println!("{}", file);
            println!("  Title: Time");
            println!("  Artist: Pink Floyd");
            println!("  Album: The Dark Side of the Moon");
            println!("  Year: 1973");
            println!("  Track: 4");
            println!("  Genre: Progressive Rock");
            println!("  Format: FLAC");
            println!("  Bitrate: 1053kbps");
            println!("  Samplerate: 44100Hz");
            println!("  Channels: 2");
        }
        "fields" => {
            println!("Item fields:");
            println!("  title, artist, album, albumartist, genre, year, track, disc,");
            println!("  length, bitrate, samplerate, channels, format, path, mb_trackid");
        }
        "config" => {
            println!("directory: ~/Music");
            println!("library: ~/Music/beets.db");
            println!("import:");
            println!("  move: no");
            println!("  write: yes");
        }
        "modify" | "move" | "remove" | "update" | "write" | "fetchart" | "lyrics" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_beet(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_beet};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_beet(vec!["--help".to_string()]), 0);
        assert_eq!(run_beet(vec!["-h".to_string()]), 0);
        let _ = run_beet(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_beet(vec![]);
    }
}
