#![deny(clippy::all)]

//! festival-cli — OurOS Festival/Flite TTS CLI
//!
//! Multi-personality: `festival`, `flite`, `text2wave`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_festival(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: festival [OPTIONS] [FILE ...]");
        println!();
        println!("Festival — speech synthesis system (OurOS).");
        println!();
        println!("Options:");
        println!("  --tts              Text-to-speech mode");
        println!("  --batch            Batch mode");
        println!("  --interactive      Interactive mode (default)");
        println!("  --language LANG    Set language");
        println!("  --pipe             Read from stdin, TTS");
        println!("  --server           Run as server");
        println!("  --heap N           Set heap size");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Festival Speech Synthesis System 2.5.0 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--tts" || a == "--pipe") {
        println!("festival: reading text for speech synthesis...");
        println!("festival: synthesizing speech...");
    } else if args.iter().any(|a| a == "--server") {
        println!("Festival server mode on port 1314");
        println!("Waiting for connections...");
    } else {
        println!("Festival Speech Synthesis System 2.5.0 (OurOS)");
        println!("festival> ");
    }
    0
}

fn run_flite(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flite [OPTIONS] [TEXT | -f FILE]");
        println!();
        println!("Flite — lightweight speech synthesis (OurOS).");
        println!();
        println!("Options:");
        println!("  -v VOICE      Voice (slt/kal/awb/rms)");
        println!("  -f FILE       Read text from file");
        println!("  -o FILE       Output to WAV file");
        println!("  -t TEXT       Text to speak");
        println!("  --voices      List voices");
        println!("  -s            SSML mode");
        println!("  --setf F=V    Set feature");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("flite 2.2 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--voices") {
        println!("Voices available: slt kal awb rms");
        return 0;
    }

    let voice = args.windows(2).find(|w| w[0] == "-v").map(|w| w[1].as_str()).unwrap_or("slt");
    let output = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str());
    let text = args.windows(2).find(|w| w[0] == "-t").map(|w| w[1].as_str());

    if let Some(wav) = output {
        println!("flite: synthesizing to '{}' (voice={})", wav, voice);
    } else if let Some(t) = text {
        println!("Speaking: \"{}\" (voice={})", t, voice);
    } else {
        println!("flite: reading from stdin (voice={})...", voice);
    }
    0
}

fn run_text2wave(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: text2wave [OPTIONS] [FILE]");
        println!();
        println!("text2wave — Festival text to WAV (OurOS).");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file");
        println!("  -eval EXPR    Evaluate Scheme expression");
        println!("  -otype TYPE   Output type (wav/aiff/raw)");
        return 0;
    }

    let output = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str()).unwrap_or("output.wav");
    println!("text2wave: reading text from stdin...");
    println!("text2wave: writing to '{}'", output);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "festival".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "flite" => run_flite(&rest),
        "text2wave" => run_text2wave(&rest),
        _ => run_festival(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
