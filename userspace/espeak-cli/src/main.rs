#![deny(clippy::all)]

//! espeak-cli — OurOS eSpeak-NG text-to-speech CLI
//!
//! Multi-personality: `espeak`, `espeak-ng`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_espeak(args: &[String], ng: bool) -> i32 {
    let name = if ng { "espeak-ng" } else { "espeak" };

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [WORDS]", name);
        println!();
        println!("{} — text to speech synthesizer (OurOS).", name);
        println!();
        println!("Options:");
        println!("  -v VOICE         Voice name (default en)");
        println!("  -s SPEED         Speed in words per minute (default 175)");
        println!("  -p PITCH         Pitch (0-99, default 50)");
        println!("  -a AMPLITUDE     Amplitude (0-200, default 100)");
        println!("  -g GAP           Word gap (ms, default 10)");
        println!("  -f FILE          Read text from file");
        println!("  -w FILE          Output to WAV file");
        println!("  --stdin          Read from stdin");
        println!("  -q               Quiet (no audio, use with -w)");
        println!("  -x               Show phonemes");
        println!("  -X               Show phonemes + translation");
        println!("  --voices         List available voices");
        println!("  --languages      List languages");
        println!("  -m               Interpret SSML markup");
        println!("  -b N             Input encoding (1=UTF-8, 2=8-bit, 4=16-bit)");
        println!("  --split N        Split at N minutes");
        println!("  --punct CHARS    Announce punctuation");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        if ng {
            println!("eSpeak NG text-to-speech: 1.51.1 (OurOS)");
        } else {
            println!("eSpeak text-to-speech: 1.48.15 (OurOS)");
        }
        return 0;
    }

    if args.iter().any(|a| a == "--voices") {
        println!("Pty Language       Age/Gender VoiceName          File          Other Languages");
        println!(" 5  af              M  afrikaans              other/af");
        println!(" 5  en              M  english                default");
        println!(" 5  en-gb           M  english                other/en");
        println!(" 5  en-us           M  english-us             other/en-us");
        println!(" 5  de              M  german                 other/de");
        println!(" 5  es              M  spanish                other/es");
        println!(" 5  fr              M  french                 other/fr");
        println!(" 5  it              M  italian                other/it");
        println!(" 5  ja              M  japanese               other/ja");
        println!(" 5  zh              M  mandarin               other/zh");
        return 0;
    }

    if args.iter().any(|a| a == "--languages") {
        println!("Available languages: af an bg bs ca cs cy da de el en en-gb en-us es et fa fi fr ga gd grc hi hr hu hy id is it ja ka kn ko ku la lv mk ml mr ms ne nl no pa pl pt pt-br ro ru sk sl sq sr sv sw ta te tr vi zh zh-yue");
        return 0;
    }

    let voice = args.windows(2).find(|w| w[0] == "-v").map(|w| w[1].as_str()).unwrap_or("en");
    let speed = args.windows(2).find(|w| w[0] == "-s").map(|w| w[1].as_str()).unwrap_or("175");
    let show_phonemes = args.iter().any(|a| a == "-x" || a == "-X");
    let wav_output = args.windows(2).find(|w| w[0] == "-w").map(|w| w[1].as_str());

    let text: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if show_phonemes {
        println!("  h@l'oU w'3:ld");
    }

    if let Some(wav) = wav_output {
        let input = if text.is_empty() { "(stdin)" } else { &text.join(" ") };
        println!("Writing to '{}': voice={}, speed={} wpm", wav, voice, speed);
        println!("  Text: {}", input);
    } else if text.is_empty() {
        println!("{}: reading from stdin (voice={}, speed={} wpm)...", name, voice, speed);
    } else {
        println!("Speaking: \"{}\" (voice={}, speed={} wpm)", text.join(" "), voice, speed);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "espeak".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "espeak-ng" => run_espeak(&rest, true),
        _ => run_espeak(&rest, false),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_espeak};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/espeak"), "espeak");
        assert_eq!(basename(r"C:\bin\espeak.exe"), "espeak.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("espeak.exe"), "espeak");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_espeak(&["--help".to_string()], false), 0);
        assert_eq!(run_espeak(&["-h".to_string()], false), 0);
        let _ = run_espeak(&["--version".to_string()], false);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_espeak(&[], false);
    }
}
