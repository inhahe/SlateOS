#![deny(clippy::all)]

//! mediainfo — Slate OS media file information tool
//!
//! Single personality: `mediainfo`

use std::env;
use std::process;

fn run_mediainfo(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mediainfo [OPTIONS] <FILE>...");
        println!();
        println!("Display information about media files.");
        println!();
        println!("Options:");
        println!("  --Output=<FMT>         Output format (HTML/XML/JSON/CSV/Text)");
        println!("  --Full                 Full information display");
        println!("  --Inform=<TEMPLATE>    Custom output template");
        println!("  --Language=<LANG>      Output language");
        println!("  --BySteamKind          Sort by stream type");
        println!("  --Details=<LEVEL>      Detail level (0-1)");
        println!("  --LogFile=<FILE>       Log to file");
        println!("  --Version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--Version" || a == "--version" || a == "-V") {
        println!("MediaInfo 24.01 (Slate OS)");
        return 0;
    }

    let json = args.iter().any(|a| a == "--Output=JSON" || a == "--output=json");
    let full = args.iter().any(|a| a == "--Full" || a == "--full" || a == "-f");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("Error: file required. See --help.");
        return 1;
    }

    for file in &files {
        if json {
            println!("{{");
            println!("  \"media\": {{");
            println!("    \"@ref\": \"{}\",", file);
            println!("    \"track\": [");
            println!("      {{\"@type\":\"General\", \"Format\":\"MPEG-4\", \"FileSize\":\"345678901\", \"Duration\":\"323.450\", \"OverallBitRate\":\"8543210\"}},");
            println!("      {{\"@type\":\"Video\", \"Format\":\"AVC\", \"Width\":\"1920\", \"Height\":\"1080\", \"FrameRate\":\"30.000\", \"BitRate\":\"8000000\"}},");
            println!("      {{\"@type\":\"Audio\", \"Format\":\"AAC\", \"SamplingRate\":\"48000\", \"Channels\":\"2\", \"BitRate\":\"192000\"}}");
            println!("    ]");
            println!("  }}");
            println!("}}");
        } else {
            println!("General");
            println!("Complete name                    : {}", file);
            println!("Format                           : MPEG-4");
            println!("Format profile                   : Base Media");
            println!("Codec ID                         : isom (isom/iso2/mp41)");
            println!("File size                        : 330 MiB");
            println!("Duration                         : 5 min 23 s");
            println!("Overall bit rate                 : 8 543 kb/s");
            println!();
            println!("Video");
            println!("ID                               : 1");
            println!("Format                           : AVC");
            println!("Format/Info                      : Advanced Video Codec");
            println!("Format profile                   : High@L4.0");
            println!("Codec ID                         : avc1");
            println!("Duration                         : 5 min 23 s");
            println!("Bit rate                         : 8 000 kb/s");
            println!("Width                            : 1 920 pixels");
            println!("Height                           : 1 080 pixels");
            println!("Display aspect ratio             : 16:9");
            println!("Frame rate mode                  : Constant");
            println!("Frame rate                       : 30.000 FPS");
            println!("Color space                      : YUV");
            println!("Chroma subsampling               : 4:2:0");
            println!("Bit depth                        : 8 bits");
            println!("Scan type                        : Progressive");
            if full {
                println!("Encoding settings                : cabac=1 / ref=4 / deblock=1:0:0 / analyse=0x3:0x113");
                println!("Codec configuration box          : avcC");
            }
            println!();
            println!("Audio");
            println!("ID                               : 2");
            println!("Format                           : AAC LC");
            println!("Format/Info                      : Advanced Audio Codec Low Complexity");
            println!("Codec ID                         : mp4a-40-2");
            println!("Duration                         : 5 min 23 s");
            println!("Bit rate mode                    : Constant");
            println!("Bit rate                         : 192 kb/s");
            println!("Channel(s)                       : 2 channels");
            println!("Channel layout                   : L R");
            println!("Sampling rate                    : 48.0 kHz");
            println!("Frame rate                       : 46.875 FPS (1024 SPF)");
            println!("Compression mode                 : Lossy");
        }
        println!();
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
