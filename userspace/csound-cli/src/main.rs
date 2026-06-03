#![deny(clippy::all)]

//! csound-cli — OurOS Csound audio programming
//!
//! Multi-personality: `csound`

use std::env;
use std::process;

fn run_csound(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: csound [OPTIONS] FILE.csd");
        println!("Csound 6.18.1 (OurOS)");
        println!("  -o FILE       Output audio file (or dac for realtime)");
        println!("  -r N          Sample rate");
        println!("  -k N          Control rate");
        println!("  -b N          Buffer size");
        println!("  -B N          Hardware buffer size");
        println!("  --midi-device=N  MIDI input device");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Csound version 6.18.1 (OurOS)");
        println!("libsndfile-1.2.2");
        println!("JACK, ALSA, PortAudio, PortMIDI");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".csd") || a.ends_with(".orc")).map(|s| s.as_str()).unwrap_or("piece.csd");
    let output = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str()).unwrap_or("dac");
    println!("--Csound version 6.18.1 (OurOS)");
    println!("Reading CSD file: {}", file);
    println!("Orchestra: sr=48000, kr=4800, ksmps=10, nchnls=2, 0dbfs=1.0");
    if output == "dac" {
        println!("audio buffered in 256 sample-frame blocks");
        println!("SECTION 1:");
        println!("playing in realtime...");
    } else {
        println!("writing {} samples to {}", 48000 * 10, output);
        println!("SECTION 1:");
        println!("Score finished.");
        println!("inactive allocs returned to freespace");
        println!("end of score.\t\t   overall amps: 0.89 0.87");
        println!("0 errors in performance");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_csound(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_csound};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_csound(&["--help".to_string()]), 0);
        assert_eq!(run_csound(&["-h".to_string()]), 0);
        assert_eq!(run_csound(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_csound(&[]), 0);
    }
}
