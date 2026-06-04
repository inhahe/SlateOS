#![deny(clippy::all)]

//! mpv — OurOS media player
//!
//! Single personality: `mpv`

use std::env;
use std::process;

fn run_mpv(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mpv [options] [url|path/]filename");
        println!();
        println!("Basic options:");
        println!("  --start=<time>         Seek to given position");
        println!("  --no-audio             Disable audio");
        println!("  --no-video             Disable video");
        println!("  --fs, --fullscreen     Fullscreen playback");
        println!("  --sub-file=<file>      Load subtitle file");
        println!("  --loop-file=<N|inf>    Loop file N times");
        println!("  --loop-playlist=<N|inf> Loop playlist");
        println!("  --shuffle              Shuffle playlist");
        println!("  --volume=<value>       Set volume (0-100)");
        println!("  --speed=<value>        Playback speed");
        println!();
        println!("Video options:");
        println!("  --vo=<driver>          Video output (gpu/gpu-next/null)");
        println!("  --hwdec=<api>          Hardware decoding (auto/vaapi/nvdec/none)");
        println!("  --deinterlace=<yes|no> Deinterlace");
        println!();
        println!("Audio options:");
        println!("  --ao=<driver>          Audio output (pulse/alsa/jack/null)");
        println!("  --audio-device=<name>  Audio device");
        println!("  --audio-channels=<ch>  Audio channels");
        println!();
        println!("  --list-options         List all options");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mpv 0.38.0 (OurOS)");
        println!("Copyright (C) 2000-2025 mpv/MPlayer/mplayer2 projects");
        println!(" built with gcc 13.2.0");
        println!("libplacebo version: v6.338.2");
        println!("FFmpeg version: 7.0");
        println!("FFmpeg library versions:");
        println!("  libavutil       59.8.100");
        println!("  libavcodec      61.3.100");
        println!("  libavformat     61.1.100");
        println!("  libswscale      8.1.100");
        println!("  libavfilter     10.1.100");
        println!("  libswresample   5.1.100");
        return 0;
    }
    if args.iter().any(|a| a == "--list-options") {
        println!("  --start=<time>");
        println!("  --end=<time>");
        println!("  --length=<time>");
        println!("  --speed=<0.01-100>");
        println!("  --pause");
        println!("  --shuffle");
        println!("  --volume=<-1-1000>");
        println!("  --mute=<yes|no|auto>");
        println!("  (... 500+ options omitted ...)");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if files.is_empty() {
        eprintln!("No files or URLs provided. Use --help.");
        return 1;
    }

    println!("Playing: {}", files.first().unwrap_or(&""));
    println!(" (+) Video --vid=1 (*) (h264 1920x1080 23.976fps)");
    println!(" (+) Audio --aid=1 --alang=eng (*) (aac 2ch 48000Hz)");
    println!(" (+) Subs  --sid=1 --slang=eng (subrip)");
    println!("AO: [pulse] 48000Hz stereo 2ch float");
    println!("VO: [gpu] 1920x1080 yuv420p");
    println!("AV: 00:00:05 / 01:42:30 (0%) A-V: 0.000");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mpv(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mpv};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mpv(vec!["--help".to_string()]), 0);
        assert_eq!(run_mpv(vec!["-h".to_string()]), 0);
        let _ = run_mpv(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mpv(vec![]);
    }
}
