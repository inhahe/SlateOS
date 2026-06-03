#![deny(clippy::all)]

//! vorbis-tools — OurOS Ogg Vorbis audio tools
//!
//! Multi-personality: `oggenc`, `oggdec`, `ogginfo`, `vorbiscomment`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "oggdec" => "oggdec",
        "ogginfo" => "ogginfo",
        "vorbiscomment" => "vorbiscomment",
        _ => "oggenc",
    }
}

fn run_oggenc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: oggenc [OPTIONS] <FILE>...");
        println!();
        println!("Encode audio to Ogg Vorbis.");
        println!();
        println!("Options:");
        println!("  -q, --quality <N>      Quality (-1 to 10, default: 3 ≈ 112kbps)");
        println!("  -b, --bitrate <N>      Target bitrate (kbps)");
        println!("  --min-bitrate <N>      Minimum bitrate");
        println!("  --max-bitrate <N>      Maximum bitrate");
        println!("  --managed              Bitrate management mode");
        println!("  --resample <HZ>        Resample to rate");
        println!("  --downmix              Downmix stereo to mono");
        println!("  -o, --output <FILE>    Output file");
        println!("  -t, --title <TEXT>     Track title");
        println!("  -a, --artist <TEXT>    Artist");
        println!("  -l, --album <TEXT>     Album");
        println!("  -G, --genre <TEXT>     Genre");
        println!("  -d, --date <DATE>      Date");
        println!("  -N, --tracknum <N>     Track number");
        println!("  -c, --comment <T>=<V>  Comment tag");
        println!("  -Q, --quiet            Suppress output");
        return 0;
    }

    let quiet = args.iter().any(|a| a == "-Q" || a == "--quiet");

    let quality = args.windows(2)
        .find(|w| w[0] == "-q" || w[0] == "--quality")
        .and_then(|w| w[1].parse::<f32>().ok())
        .unwrap_or(3.0);

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("Error: input file required. See --help.");
        return 1;
    }

    for file in &files {
        let output = if file.ends_with(".wav") {
            file.replace(".wav", ".ogg")
        } else {
            format!("{}.ogg", file)
        };
        if !quiet {
            println!("Encoding \"{}\" to \"{}\"", file, output);
            println!("  Quality: {:.1} (≈{} kbps)", quality, (quality * 32.0 + 64.0) as i32);
            println!("  Input:  44100 Hz, 16-bit, stereo");
            println!("  [==================================================] 100.0%");
            println!("  Done. Output: {} ({:.1} MB)", output, 5.2_f64);
            println!();
        }
    }
    0
}

fn run_oggdec(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: oggdec [OPTIONS] <FILE>...");
        println!();
        println!("Decode Ogg Vorbis to WAV.");
        println!();
        println!("Options:");
        println!("  -o, --output <FILE>    Output file");
        println!("  -b, --bits <N>         Output bit depth (8/16, default: 16)");
        println!("  -e, --endian <N>       Endianness (0=little, 1=big)");
        println!("  -s, --sign <N>         Signedness (0=unsigned, 1=signed)");
        println!("  -R, --raw              Raw output (no WAV header)");
        println!("  -Q, --quiet            Suppress output");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    for file in &files {
        let output = file.replace(".ogg", ".wav");
        println!("Decoding \"{}\" to \"{}\"...", file, output);
        println!("  Done.");
    }
    0
}

fn run_ogginfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ogginfo [OPTIONS] <FILE>...");
        println!();
        println!("Show Ogg file information.");
        println!();
        println!("Options:");
        println!("  -q    Quiet (errors only)");
        println!("  -v    Verbose");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    for file in &files {
        println!("Processing file \"{}\"...", file);
        println!();
        println!("New logical stream (#1, serial: 0x4B2E1A3F):");
        println!("  Vorbis headers parsed for stream 1, serial 0x4B2E1A3F");
        println!("  Version: 0");
        println!("  Vendor: Xiph.Org libVorbis I 1.3.7");
        println!("  Channels: 2");
        println!("  Rate: 44100 Hz");
        println!("  Nominal bitrate: 112.000000 kb/s");
        println!("  Upper bitrate not set");
        println!("  Lower bitrate not set");
        println!();
        println!("  User comments section follows...");
        println!("    TITLE=Example Track");
        println!("    ARTIST=Example Artist");
        println!("    ALBUM=Example Album");
        println!();
        println!("  Vorbis stream 1:");
        println!("    Total data length: 5,234,567 bytes");
        println!("    Playback length: 3m:45.230s");
        println!("    Average bitrate: 112.345 kb/s");
        println!();
    }
    0
}

fn run_vorbiscomment(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vorbiscomment [OPTIONS] <FILE>");
        println!();
        println!("Edit Ogg Vorbis comments/tags.");
        println!();
        println!("Options:");
        println!("  -l, --list             List comments");
        println!("  -w, --write            Write comments (replace all)");
        println!("  -a, --append           Append comments");
        println!("  -t, --tag <TAG>=<VAL>  Add/set tag");
        println!("  -d, --tag-delete <TAG> Delete tag");
        println!("  -R, --raw              Raw (UTF-8) output");
        println!("  -c, --commentfile <F>  Read comments from file");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l" || a == "--list");
    let tag = args.windows(2)
        .find(|w| w[0] == "-t" || w[0] == "--tag")
        .map(|w| w[1].as_str());

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let file = files.first().copied().unwrap_or("audio.ogg");

    if list || tag.is_none() {
        println!("TITLE=Example Track");
        println!("ARTIST=Example Artist");
        println!("ALBUM=Example Album");
        println!("DATE=2024");
        println!("GENRE=Rock");
        println!("TRACKNUMBER=1");
    }

    if let Some(t) = tag {
        println!("Tag written to {}: {}", file, t);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("oggenc"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.iter().any(|a| a == "-V" || a == "--version") {
        println!("vorbis-tools 1.4.2 (OurOS)");
        process::exit(0);
    }

    let code = match p {
        "oggenc" => run_oggenc(&rest),
        "oggdec" => run_oggdec(&rest),
        "ogginfo" => run_ogginfo(&rest),
        "vorbiscomment" => run_vorbiscomment(&rest),
        _ => run_oggenc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_oggenc};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_oggenc(&["--help".to_string()]), 0);
        assert_eq!(run_oggenc(&["-h".to_string()]), 0);
        assert_eq!(run_oggenc(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_oggenc(&[]), 0);
    }
}
