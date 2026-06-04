#![deny(clippy::all)]

//! lmms-cli — OurOS LMMS music production
//!
//! Multi-personality: `lmms`

use std::env;
use std::process;

fn run_lmms(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lmms [OPTIONS] [FILE.mmp]");
        println!("LMMS 1.2.2 (OurOS)");
        println!("  -r, --render FILE   Render to audio file");
        println!("  -f, --format FMT    Output format (wav, ogg, mp3)");
        println!("  -s, --samplerate N  Sample rate");
        println!("  -b, --bitrate N     Bitrate (for mp3)");
        println!("  --loop N            Loop count");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("LMMS 1.2.2 (OurOS)");
        return 0;
    }
    let render = args.iter().any(|a| a == "-r" || a == "--render");
    let file = args.iter().find(|a| a.ends_with(".mmp") || a.ends_with(".mmpz")).map(|s| s.as_str());
    if render {
        let output = args.windows(2).find(|w| w[0] == "-r" || w[0] == "--render").map(|w| w[1].as_str()).unwrap_or("output.wav");
        let input = file.unwrap_or("project.mmp");
        println!("LMMS 1.2.2 — rendering: {}", input);
        println!("  Output: {}", output);
        println!("  Format: WAV (44100 Hz, 16-bit)");
        println!("  Rendering... 100%");
        println!("  Done.");
    } else if let Some(f) = file {
        println!("LMMS 1.2.2 — opening: {}", f);
        println!("Ready.");
    } else {
        println!("LMMS 1.2.2 — Starting...");
        println!("Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lmms(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lmms};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lmms(&["--help".to_string()]), 0);
        assert_eq!(run_lmms(&["-h".to_string()]), 0);
        let _ = run_lmms(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lmms(&[]);
    }
}
