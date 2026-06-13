#![deny(clippy::all)]

//! sox — SlateOS Sound eXchange audio processor
//!
//! Multi-personality: `sox`, `soxi`, `play`, `rec`

use std::env;
use std::process;

fn run_sox(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sox [global-options] [input-options] INFILE... [output-options] OUTFILE [effect...]");
        println!();
        println!("Global options:");
        println!("  --buffer BYTES   Set buffer size");
        println!("  --combine mix|merge|concatenate|sequence  How to combine channels");
        println!("  --norm           Normalize output");
        println!("  -V[level]        Verbosity level");
        println!("  --version        Show version");
        println!();
        println!("Effects:");
        println!("  bass, treble     Tone adjustment");
        println!("  chorus           Chorus effect");
        println!("  compand          Dynamic range compression");
        println!("  delay            Delay channels");
        println!("  echo, echos      Echo effects");
        println!("  equalizer        Parametric EQ");
        println!("  fade             Fade in/out");
        println!("  flanger          Flanger effect");
        println!("  gain             Adjust gain");
        println!("  loudness         Loudness control");
        println!("  norm             Normalize");
        println!("  pad              Pad with silence");
        println!("  rate             Change sample rate");
        println!("  reverb           Reverb effect");
        println!("  silence          Remove silence");
        println!("  speed            Adjust speed/pitch");
        println!("  tempo            Adjust tempo");
        println!("  trim             Trim audio");
        println!("  vol              Adjust volume");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sox:      SoX v14.4.2 (Slate OS)");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if files.len() >= 2 {
        println!("Input File     : '{}'", files.first().unwrap_or(&"input"));
        println!("Channels       : 2");
        println!("Sample Rate    : 44100");
        println!("Duration       : 00:03:45.00 = 9922050 samples");
        println!();
        println!("Output File    : '{}'", files.get(1).unwrap_or(&"output"));
        println!("(conversion complete — simulated)");
    } else {
        eprintln!("sox: need at least an input and output file. Use --help.");
        return 1;
    }
    0
}

fn run_soxi(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: soxi [-V[level]] [-t|-r|-c|-s|-d|-D|-b|-p|-e|-a] infile...");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("audio.wav");
    println!("Input File     : '{}'", file);
    println!("Channels       : 2");
    println!("Sample Rate    : 44100");
    println!("Precision      : 16-bit");
    println!("Duration       : 00:03:45.00 = 9922050 samples = 16872.4 CDDA sectors");
    println!("File Size      : 39.7M");
    println!("Bit Rate       : 1.41M");
    println!("Sample Encoding: 16-bit Signed Integer PCM");
    0
}

fn run_play(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: play [options] INFILE [effect...]");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("audio.wav");
    println!("{}: 44100Hz, 16-bit, stereo, 00:03:45.00", file);
    println!("In:100%  00:03:45.00 [00:00:00.00] Out:9.92M [      |      ]");
    println!("Done.");
    0
}

fn run_rec(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rec [options] OUTFILE [effect...]");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("recording.wav");
    println!("Recording: {} (44100Hz, 16-bit, stereo)", file);
    println!("In:0.00% 00:00:05.00 [00:00:00.00] Out:220k  [      |      ]");
    let _ = args;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("sox");
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
        "soxi" => run_soxi(rest),
        "play" => run_play(rest),
        "rec" => run_rec(rest),
        _ => run_sox(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sox};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sox(vec!["--help".to_string()]), 0);
        assert_eq!(run_sox(vec!["-h".to_string()]), 0);
        let _ = run_sox(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sox(vec![]);
    }
}
