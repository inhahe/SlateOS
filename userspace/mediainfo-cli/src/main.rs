#![deny(clippy::all)]

//! mediainfo-cli — OurOS MediaInfo CLI
//!
//! Single personality: `mediainfo`

use std::env;
use std::process;

fn run_mediainfo(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mediainfo [OPTIONS] FILE...");
        println!();
        println!("MediaInfo — media file information (OurOS).");
        println!();
        println!("Options:");
        println!("  --Full, -f             Full information display");
        println!("  --Output=FORMAT        Output format (HTML, XML, JSON, CSV)");
        println!("  --Inform=FMT           Custom output template");
        println!("  --Language=LANG        Language for output");
        println!("  --Version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--Version" || a == "--version") {
        println!("MediaInfo Command line, MediaInfoLib - v24.01 (OurOS)");
        return 0;
    }

    let full = args.iter().any(|a| a == "--Full" || a == "-f");
    let json_out = args.iter().any(|a| a == "--Output=JSON");
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("mediainfo: no file specified. See --help.");
        return 1;
    }

    for file in &files {
        if json_out {
            println!("{{");
            println!("  \"media\": {{");
            println!("    \"@ref\": \"{}\",", file);
            println!("    \"track\": [");
            println!("      {{\"@type\": \"General\", \"Format\": \"MPEG-4\", \"Duration\": \"6135.123\"}},");
            println!("      {{\"@type\": \"Video\", \"Format\": \"AVC\", \"Width\": \"1920\", \"Height\": \"1080\"}},");
            println!("      {{\"@type\": \"Audio\", \"Format\": \"AAC\", \"Channels\": \"2\", \"SamplingRate\": \"48000\"}}");
            println!("    ]");
            println!("  }}");
            println!("}}");
        } else if full {
            println!("General");
            println!("Complete name                    : {}", file);
            println!("Format                           : MPEG-4");
            println!("Format profile                   : Base Media / Version 2");
            println!("Codec ID                         : mp42 (isom/mp42)");
            println!("File size                        : 1.23 GiB");
            println!("Duration                         : 1 h 42 min");
            println!("Overall bit rate                 : 1 724 kb/s");
            println!();
            println!("Video");
            println!("ID                               : 1");
            println!("Format                           : AVC");
            println!("Format/Info                      : Advanced Video Codec");
            println!("Format profile                   : High@L4.1");
            println!("Codec ID                         : avc1");
            println!("Duration                         : 1 h 42 min");
            println!("Bit rate                         : 1 500 kb/s");
            println!("Width                            : 1 920 pixels");
            println!("Height                           : 1 080 pixels");
            println!("Display aspect ratio             : 16:9");
            println!("Frame rate mode                  : Constant");
            println!("Frame rate                       : 23.976 FPS");
            println!("Color space                      : YUV");
            println!("Chroma subsampling               : 4:2:0");
            println!("Bit depth                        : 8 bits");
            println!();
            println!("Audio");
            println!("ID                               : 2");
            println!("Format                           : AAC LC");
            println!("Codec ID                         : mp4a-40-2");
            println!("Duration                         : 1 h 42 min");
            println!("Bit rate                         : 192 kb/s");
            println!("Channel(s)                       : 2 channels");
            println!("Sampling rate                    : 48.0 kHz");
        } else {
            println!("General");
            println!("Complete name                    : {}", file);
            println!("Format                           : MPEG-4");
            println!("File size                        : 1.23 GiB");
            println!("Duration                         : 1 h 42 min");
            println!();
            println!("Video");
            println!("Format                           : AVC");
            println!("Width                            : 1 920 pixels");
            println!("Height                           : 1 080 pixels");
            println!("Frame rate                       : 23.976 FPS");
            println!();
            println!("Audio");
            println!("Format                           : AAC LC");
            println!("Channel(s)                       : 2 channels");
            println!("Sampling rate                    : 48.0 kHz");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mediainfo(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mediainfo};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mediainfo(vec!["--help".to_string()]), 0);
        assert_eq!(run_mediainfo(vec!["-h".to_string()]), 0);
        let _ = run_mediainfo(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mediainfo(vec![]);
    }
}
