#![deny(clippy::all)]

//! mpv-cli — OurOS mpv media player CLI
//!
//! Single personality: `mpv`

use std::env;
use std::process;

fn run_mpv(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mpv [OPTIONS] <FILE|URL>...");
        println!();
        println!("Free, open-source, cross-platform media player.");
        println!();
        println!("Playback options:");
        println!("  --start=<TIME>           Seek to start position");
        println!("  --end=<TIME>             Stop at position");
        println!("  --length=<SEC>           Duration to play");
        println!("  --speed=<FACTOR>         Playback speed (0.01-100)");
        println!("  --pause                  Start paused");
        println!("  --loop-file=<N|inf>      Loop file N times");
        println!("  --loop-playlist=<N|inf>  Loop playlist N times");
        println!("  --shuffle                Shuffle playlist");
        println!();
        println!("Audio options:");
        println!("  --volume=<N>             Volume (0-100, default: 100)");
        println!("  --mute=<yes|no>          Mute audio");
        println!("  --audio-device=<NAME>    Audio device");
        println!("  --audio-channels=<SPEC>  Audio channels");
        println!("  --no-audio               Disable audio");
        println!();
        println!("Video options:");
        println!("  --fullscreen             Fullscreen mode");
        println!("  --fs-screen=<N>          Fullscreen monitor");
        println!("  --geometry=<WxH+X+Y>     Window geometry");
        println!("  --autofit=<WxH>          Auto fit window size");
        println!("  --no-video               Disable video");
        println!("  --vo=<DRIVER>            Video output driver");
        println!("  --hwdec=<API>            Hardware decoding (auto/vaapi/nvdec)");
        println!();
        println!("Subtitle options:");
        println!("  --sub-file=<FILE>        External subtitle file");
        println!("  --sub-auto=<MODE>        Auto-load subtitles (exact/fuzzy/all/no)");
        println!("  --sub-font-size=<N>      Subtitle font size");
        println!("  --sub-delay=<SEC>        Subtitle delay");
        println!();
        println!("Screenshot:");
        println!("  --screenshot-format=<F>  Format (png/jpg/webp)");
        println!("  --screenshot-directory=<D>  Save directory");
        println!();
        println!("  --profile=<NAME>         Use configuration profile");
        println!("  --list-options           List all options");
        println!("  -V, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mpv 0.38.0 (OurOS)");
        return 0;
    }

    let list_options = args.iter().any(|a| a == "--list-options");
    if list_options {
        println!("  --start=<time>            default: (none)");
        println!("  --end=<time>              default: (none)");
        println!("  --speed=<double>          default: 1.0");
        println!("  --volume=<float>          default: 100");
        println!("  --fullscreen=<flag>       default: no");
        println!("  --hwdec=<string>          default: no");
        println!("  ... (500+ options available)");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("Error: no file or URL specified. See --help.");
        return 1;
    }

    let no_video = args.iter().any(|a| a == "--no-video");

    for file in &files {
        println!("Playing: {}", file);
        if no_video {
            println!(" (+) Audio --aid=1 (aac 2ch 48000Hz)");
            println!("AO: [pulse] 48000Hz stereo 2ch float");
        } else {
            println!(" (+) Video --vid=1 (*) (h264 1920x1080 30.000fps)");
            println!(" (+) Audio --aid=1 (*) (aac 2ch 48000Hz)");
            println!("VO: [gpu] 1920x1080 yuv420p");
            println!("AO: [pulse] 48000Hz stereo 2ch float");
        }
        println!("A-V: 0.000 ct: 0.023 cache: 10s+2MB");
    }
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
    fn help_and_version_exit_zero() {
        assert_eq!(run_mpv(vec!["--help".to_string()]), 0);
        assert_eq!(run_mpv(vec!["-h".to_string()]), 0);
        assert_eq!(run_mpv(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mpv(vec![]), 0);
    }
}
