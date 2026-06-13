#![deny(clippy::all)]

//! ncmpcpp — SlateOS NCurses Music Player Client (Plus Plus)
//!
//! Multi-personality: `ncmpcpp`, `mpc`

use std::env;
use std::process;

fn run_ncmpcpp(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ncmpcpp [options]");
        println!();
        println!("Options:");
        println!("  -h, --host <host>   MPD host (default: localhost)");
        println!("  -p, --port <port>   MPD port (default: 6600)");
        println!("  -c, --config <file> Config file");
        println!("  -s, --screen <s>    Startup screen");
        println!("  -S, --slave-screen  Slave screen");
        println!("  --current-song      Print current song info");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ncmpcpp 0.9.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--current-song") {
        println!("Pink Floyd - Time");
        return 0;
    }

    println!("ncmpcpp 0.9.2 (Slate OS) — NCurses MPD client");
    println!("(TUI interface — simulated)");
    0
}

fn run_mpc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mpc [command] [args...]");
        println!();
        println!("Commands:");
        println!("  play [N]       Start/resume playback");
        println!("  pause          Pause playback");
        println!("  stop           Stop playback");
        println!("  next           Next track");
        println!("  prev           Previous track");
        println!("  toggle         Toggle play/pause");
        println!("  status         Show status");
        println!("  current        Show current song");
        println!("  playlist       Show playlist");
        println!("  ls [dir]       List directory");
        println!("  add <uri>      Add to playlist");
        println!("  clear          Clear playlist");
        println!("  volume [+/-]N  Set volume");
        println!("  repeat [on|off] Toggle repeat");
        println!("  random [on|off] Toggle random");
        println!("  search <type> <q> Search");
        println!("  update [path]  Update database");
        println!("  stats          Show database stats");
        println!("  version        Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "version" => {
            println!("mpc version: 0.35 (Slate OS)");
            println!("mpd version: 0.23.15");
        }
        "status" => {
            println!("Pink Floyd - The Dark Side of the Moon - Time");
            println!("[playing] #4/12   0:42/4:53 (14%)");
            println!("volume: 85%   repeat: off   random: on   single: off   consume: off");
        }
        "current" => println!("Pink Floyd - Time"),
        "playlist" => {
            println!(" 1) Pink Floyd - Speak to Me");
            println!(" 2) Pink Floyd - Breathe");
            println!(" 3) Pink Floyd - On the Run");
            println!(">4) Pink Floyd - Time");
            println!(" 5) Pink Floyd - The Great Gig in the Sky");
        }
        "stats" => {
            println!("Artists:    156");
            println!("Albums:      89");
            println!("Songs:     1234");
            println!("DB Playtime: 3 days, 12:45:30");
            println!("DB Updated:  Thu May 22 10:00:00 2025");
        }
        "play" | "pause" | "stop" | "next" | "prev" | "toggle" => {
            println!("({} — sent to MPD)", cmd);
        }
        "ls" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if dir.is_empty() {
                println!("Pink Floyd");
                println!("Radiohead");
                println!("Led Zeppelin");
            } else {
                println!("{}/Album1", dir);
                println!("{}/Album2", dir);
            }
        }
        "search" => {
            println!("Pink Floyd - Time");
            println!("Pink Floyd - Money");
        }
        _ => {
            println!("({} — simulated)", cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("ncmpcpp");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "mpc" => run_mpc(rest),
        _ => run_ncmpcpp(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ncmpcpp};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ncmpcpp(vec!["--help".to_string()]), 0);
        assert_eq!(run_ncmpcpp(vec!["-h".to_string()]), 0);
        let _ = run_ncmpcpp(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ncmpcpp(vec![]);
    }
}
