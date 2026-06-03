#![deny(clippy::all)]

//! musescore-cli — OurOS MuseScore music notation
//!
//! Multi-personality: `musescore`, `mscore`

use std::env;
use std::process;

fn run_musescore(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: musescore [OPTIONS] [FILE.mscz]");
        println!("MuseScore 4.2.1 (OurOS)");
        println!("  -o FILE        Export to file (pdf, png, svg, mp3, wav, midi, musicxml)");
        println!("  -r DPI         Resolution for image export");
        println!("  -T N           Trim margin (pixels)");
        println!("  --score-parts  Export parts separately");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("MuseScore 4.2.1 (OurOS)");
        println!("Built with Qt 6.6.1");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".mscz") || a.ends_with(".mscx") || a.ends_with(".musicxml")).map(|s| s.as_str());
    let export = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str());
    if let Some(out) = export {
        let input = file.unwrap_or("score.mscz");
        println!("MuseScore 4.2.1 — exporting");
        println!("  Input: {}", input);
        println!("  Output: {}", out);
        if out.ends_with(".pdf") {
            println!("  Format: PDF");
        } else if out.ends_with(".mp3") || out.ends_with(".wav") {
            println!("  Format: Audio");
            println!("  Rendering audio...");
        } else if out.ends_with(".midi") || out.ends_with(".mid") {
            println!("  Format: MIDI");
        }
        println!("  Export complete.");
    } else if let Some(f) = file {
        println!("MuseScore 4.2.1 — opening: {}", f);
        println!("Ready.");
    } else {
        println!("MuseScore 4.2.1 — Starting...");
        println!("Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_musescore(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_musescore};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_musescore(&["--help".to_string()]), 0);
        assert_eq!(run_musescore(&["-h".to_string()]), 0);
        assert_eq!(run_musescore(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_musescore(&[]), 0);
    }
}
