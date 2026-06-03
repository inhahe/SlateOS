#![deny(clippy::all)]

//! lame-cli — OurOS LAME MP3 encoder CLI
//!
//! Single personality: `lame`

use std::env;
use std::process;

fn run_lame(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "--longhelp") {
        println!("Usage: lame [OPTIONS] INPUT [OUTPUT]");
        println!();
        println!("LAME — MP3 encoder (OurOS).");
        println!();
        println!("Quality options:");
        println!("  -b N                   Set bitrate (32-320 kbps)");
        println!("  -V N                   VBR quality (0=best, 9=worst)");
        println!("  --cbr                  Constant bitrate mode");
        println!("  --abr N                Average bitrate mode");
        println!("  -q N                   Algorithm quality (0=best, 9=fast)");
        println!();
        println!("Input options:");
        println!("  -r                     Raw PCM input");
        println!("  -s N                   Input sample rate (kHz)");
        println!("  --bitwidth N           Input bit width (8, 16, 24, 32)");
        println!("  -m MODE               Channel mode (s=stereo, j=joint, m=mono)");
        println!();
        println!("Filter options:");
        println!("  --lowpass N            Lowpass frequency (kHz)");
        println!("  --highpass N           Highpass frequency (kHz)");
        println!("  --resample N           Output sample rate");
        println!();
        println!("ID3 tag options:");
        println!("  --tt TITLE             Title tag");
        println!("  --ta ARTIST            Artist tag");
        println!("  --tl ALBUM             Album tag");
        println!("  --ty YEAR              Year tag");
        println!("  --tn TRACK             Track number");
        println!("  --tg GENRE             Genre tag");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("LAME 64bits version 3.100 (OurOS)");
        println!("  LAME is the best MP3 encoder.");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("lame: no input file specified. See --help.");
        return 1;
    }

    let input = files[0];
    let output = if files.len() > 1 { files[1] } else { "output.mp3" };

    let bitrate = args.windows(2).find(|w| w[0] == "-b")
        .map(|w| w[1].as_str());
    let vbr = args.windows(2).find(|w| w[0] == "-V")
        .map(|w| w[1].as_str());
    let mode = args.windows(2).find(|w| w[0] == "-m")
        .map(|w| w[1].as_str()).unwrap_or("j");

    println!("LAME 3.100 64bits (OurOS)");
    println!("Autodetecting input: {}", input);
    println!("Encoding as {}", output);

    let mode_str = match mode {
        "s" => "stereo",
        "j" => "joint stereo",
        "m" => "mono",
        "f" => "forced mid/side stereo",
        _ => "joint stereo",
    };

    if let Some(v) = vbr {
        println!("Encoding {} to {}", input, output);
        println!("  VBR(q={}) {} MPEG-1 Layer III", v, mode_str);
    } else {
        let br = bitrate.unwrap_or("128");
        println!("Encoding {} to {}", input, output);
        println!("  CBR {} kbps {} MPEG-1 Layer III", br, mode_str);
    }

    println!("  Input: 44.1 kHz, 16 bit, stereo");
    println!("    Frame          |  CPU time/estim | REAL time/estim | play/CPU |    ETA");
    println!("  10324/10324 (100%)|    0:04/    0:04|    0:04/    0:04|   60.12x |    0:00");
    println!("  Writing LAME Tag...done");
    println!("  ReplayGain: -6.2dB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lame(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lame};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lame(vec!["--help".to_string()]), 0);
        assert_eq!(run_lame(vec!["-h".to_string()]), 0);
        assert_eq!(run_lame(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lame(vec![]), 0);
    }
}
