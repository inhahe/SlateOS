#![deny(clippy::all)]

//! cmus — OurOS console music player
//!
//! Multi-personality: `cmus`, `cmus-remote`

use std::env;
use std::process;

fn run_cmus(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cmus [options]");
        println!();
        println!("Options:");
        println!("  --listen <addr>   Listen address for remote");
        println!("  --plugins         List plugins");
        println!("  --show-cursor     Show cursor");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cmus v2.11.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--plugins") {
        println!("Input plugins:  flac, mad (mp3), vorbis, opus, wavpack, wav, aac, ffmpeg");
        println!("Output plugins: pulse, alsa, jack, oss, null");
        return 0;
    }

    println!("cmus v2.11.0 (OurOS) — press :help for commands");
    println!("(TUI music player — simulated)");
    0
}

fn run_cmus_remote(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cmus-remote [options]");
        println!();
        println!("Options:");
        println!("  -p, --play          Play");
        println!("  -u, --pause         Toggle pause");
        println!("  -s, --stop          Stop");
        println!("  -n, --next          Next track");
        println!("  -r, --prev          Previous track");
        println!("  -R, --repeat        Toggle repeat");
        println!("  -S, --shuffle       Toggle shuffle");
        println!("  -v, --volume VOL    Set volume (+/-/absolute)");
        println!("  -k, --seek OFFSET   Seek (+/-/absolute seconds)");
        println!("  -Q, --query         Show current status");
        println!("  -l, --library       Modify library");
        println!("  -P, --playlist      Modify playlist");
        println!("  -q, --queue FILE    Queue file");
        println!("  -C, --raw CMD       Send raw command");
        return 0;
    }

    if args.iter().any(|a| a == "-Q" || a == "--query") {
        println!("status playing");
        println!("file /home/user/Music/song.flac");
        println!("duration 225");
        println!("position 42");
        println!("tag artist Pink Floyd");
        println!("tag album The Dark Side of the Moon");
        println!("tag title Time");
        println!("tag tracknumber 4");
        println!("set repeat false");
        println!("set shuffle true");
        println!("set vol_left 100");
        println!("set vol_right 100");
        return 0;
    }

    // Execute command
    let cmd_name = if args.iter().any(|a| a == "-p" || a == "--play") { "play" }
        else if args.iter().any(|a| a == "-u" || a == "--pause") { "pause" }
        else if args.iter().any(|a| a == "-s" || a == "--stop") { "stop" }
        else if args.iter().any(|a| a == "-n" || a == "--next") { "next" }
        else if args.iter().any(|a| a == "-r" || a == "--prev") { "prev" }
        else { "unknown" };
    if cmd_name != "unknown" {
        println!("(sent '{}' command to cmus)", cmd_name);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("cmus");
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
        "cmus-remote" => run_cmus_remote(rest),
        _ => run_cmus(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cmus};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cmus(vec!["--help".to_string()]), 0);
        assert_eq!(run_cmus(vec!["-h".to_string()]), 0);
        assert_eq!(run_cmus(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cmus(vec![]), 0);
    }
}
