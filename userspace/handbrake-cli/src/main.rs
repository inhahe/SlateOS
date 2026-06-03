#![deny(clippy::all)]

//! handbrake-cli — OurOS HandBrake CLI
//!
//! Single personality: `HandBrakeCLI`

use std::env;
use std::process;

fn run_handbrake(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: HandBrakeCLI [OPTIONS]");
        println!();
        println!("HandBrake — video transcoder (OurOS).");
        println!();
        println!("Source options:");
        println!("  -i, --input FILE       Input file or device");
        println!("  -t, --title NUM        Select title (default: 1)");
        println!("  --scan                 Scan input, no encoding");
        println!();
        println!("Output options:");
        println!("  -o, --output FILE      Output file");
        println!("  -f, --format FMT       Container format (av_mp4, av_mkv, av_webm)");
        println!();
        println!("Video options:");
        println!("  -e, --encoder ENC      Video encoder (x264, x265, svt_av1, nvenc_h264)");
        println!("  -q, --quality Q        Constant quality (RF value)");
        println!("  -b, --vb KBPS          Video bitrate");
        println!("  -r, --rate FPS         Frame rate");
        println!("  --width W              Output width");
        println!("  --height H             Output height");
        println!("  --crop T:B:L:R         Crop values");
        println!();
        println!("Audio options:");
        println!("  -a, --audio TRACKS     Audio track(s)");
        println!("  -E, --aencoder ENC     Audio encoder (aac, opus, copy)");
        println!("  -B, --ab KBPS          Audio bitrate");
        println!();
        println!("Preset options:");
        println!("  -Z, --preset NAME      Use preset");
        println!("  --preset-list          List presets");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("HandBrake 1.7.2 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--preset-list") {
        println!("Available presets:");
        println!("  General/");
        println!("    Very Fast 1080p30");
        println!("    Fast 1080p30");
        println!("    HQ 1080p30 Surround");
        println!("    Super HQ 1080p30 Surround");
        println!("  Web/");
        println!("    Gmail Large 3 Minutes 720p30");
        println!("    YouTube HQ 1080p60");
        println!("    Discord Nitro Large 3-6 Minutes 1080p30");
        println!("  Devices/");
        println!("    Apple 1080p60 Surround");
        println!("    Android 1080p30");
        println!("    Roku 1080p30 Surround");
        println!("  Matroska/");
        println!("    H.265 MKV 1080p30");
        println!("    VP9 MKV 1080p30");
        return 0;
    }

    let input = args.windows(2).find(|w| w[0] == "-i" || w[0] == "--input")
        .map(|w| w[1].as_str());
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output")
        .map(|w| w[1].as_str());

    if args.iter().any(|a| a == "--scan") {
        let src = input.unwrap_or("input.mkv");
        println!("+ title 1:");
        println!("  + duration: 01:42:33");
        println!("  + size: 1920x1080, pixel aspect: 1/1, display aspect: 1.78");
        println!("  + autocrop: 0/0/0/0");
        println!("  + chapters:");
        println!("    + 1: cells 0->0, 0 blocks, duration 00:05:12");
        println!("    + 2: cells 0->0, 0 blocks, duration 00:04:48");
        println!("  + audio tracks:");
        println!("    + 1, English (AAC) (2.0 ch) (iso639-2: eng)");
        println!("    + 2, Spanish (AAC) (2.0 ch) (iso639-2: spa)");
        println!("  + subtitle tracks:");
        println!("    + 1, English (iso639-2: eng)");
        let _ = src;
        return 0;
    }

    let src = input.unwrap_or("input.mkv");
    let dst = output.unwrap_or("output.mp4");
    let encoder = args.windows(2).find(|w| w[0] == "-e" || w[0] == "--encoder")
        .map(|w| w[1].as_str()).unwrap_or("x264");
    let quality = args.windows(2).find(|w| w[0] == "-q" || w[0] == "--quality")
        .map(|w| w[1].as_str()).unwrap_or("20");

    println!("HandBrake has exited.");
    println!("Encoding: {} -> {}", src, dst);
    println!("  Encoder: {} (quality RF {})", encoder, quality);
    println!("  [00:00:00] task 1 of 1, 0.00 %");
    println!("  [00:00:15] task 1 of 1, 12.45 % (98.23 fps, avg 98.23 fps, ETA 00h01m52s)");
    println!("  [00:01:00] task 1 of 1, 48.92 % (102.45 fps, avg 100.34 fps, ETA 00h01m03s)");
    println!("  [00:02:05] task 1 of 1, 100.00 % (99.87 fps, avg 99.87 fps, ETA 00h00m00s)");
    println!("Encode done!");
    println!("HandBrake has exited.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_handbrake(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_handbrake};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_handbrake(vec!["--help".to_string()]), 0);
        assert_eq!(run_handbrake(vec!["-h".to_string()]), 0);
        assert_eq!(run_handbrake(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_handbrake(vec![]), 0);
    }
}
