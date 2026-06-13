#![deny(clippy::all)]

//! yt-dlp-cli — Slate OS yt-dlp video downloader CLI
//!
//! Single personality: `yt-dlp`

use std::env;
use std::process;

fn run_ytdlp(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: yt-dlp [OPTIONS] URL...");
        println!();
        println!("yt-dlp — video downloader (Slate OS).");
        println!();
        println!("Options:");
        println!("  -f, --format FORMAT    Video format selector");
        println!("  -o, --output TMPL      Output filename template");
        println!("  -x, --extract-audio    Extract audio only");
        println!("  --audio-format FMT     Audio format (mp3, m4a, wav, opus)");
        println!("  --audio-quality Q      Audio quality (0-10)");
        println!("  -F, --list-formats     List available formats");
        println!("  --cookies FILE         Cookie file");
        println!("  --flat-playlist        Do not download playlist videos");
        println!("  --write-subs           Write subtitles");
        println!("  --sub-lang LANG        Subtitle language");
        println!("  --embed-thumbnail      Embed thumbnail in file");
        println!("  --embed-metadata       Embed metadata");
        println!("  --sponsorblock-mark    Mark sponsor segments");
        println!("  --limit-rate RATE      Download rate limit");
        println!("  -U, --update           Update yt-dlp");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("yt-dlp 2024.01.12 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "-U" || a == "--update") {
        println!("Current version: 2024.01.12");
        println!("yt-dlp is up to date (2024.01.12)");
        return 0;
    }

    let urls: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && (a.contains("http") || a.contains("youtu") || a.contains("www.")))
        .map(|s| s.as_str())
        .collect();

    if urls.is_empty() && !args.iter().any(|a| a == "-F" || a == "--list-formats") {
        eprintln!("yt-dlp: error: no URL provided. See --help.");
        return 1;
    }

    let extract_audio = args.iter().any(|a| a == "-x" || a == "--extract-audio");
    let list_formats = args.iter().any(|a| a == "-F" || a == "--list-formats");

    if list_formats {
        println!("ID  EXT  RESOLUTION FPS |  FILESIZE   TBR PROTO | VCODEC       ACODEC");
        println!("--- ---- ---------- --- + --------- ----- ----- + ------------ ------");
        println!("140 m4a  audio only     |   3.45MiB  128k https | audio only   aac");
        println!("251 webm audio only     |   3.89MiB  160k https | audio only   opus");
        println!("136 mp4  1280x720   30  |  12.34MiB  800k https | avc1.64001f  none");
        println!("137 mp4  1920x1080  30  |  35.67MiB 2500k https | avc1.640028  none");
        println!("313 webm 3840x2160  30  | 156.78MiB 8000k https | vp9          none");
        println!("22  mp4  1280x720   30  |  15.78MiB  950k https | avc1.64001f  aac");
        return 0;
    }

    let url = urls.first().copied().unwrap_or("https://example.com/video");
    let format = args.windows(2).find(|w| w[0] == "-f" || w[0] == "--format")
        .map(|w| w[1].as_str());

    println!("[yt-dlp] Extracting URL: {}", url);
    println!("[info] Video ID: dQw4w9WgXcQ");
    println!("[info] Title: Example Video Title");
    println!("[info] Duration: 3:32");

    if let Some(fmt) = format {
        println!("[info] Selected format: {}", fmt);
    }

    if extract_audio {
        let audio_fmt = args.windows(2).find(|w| w[0] == "--audio-format")
            .map(|w| w[1].as_str()).unwrap_or("mp3");
        println!("[download] Destination: Example Video Title.{}", audio_fmt);
        println!("[download] 100% of 3.45MiB in 00:02");
        println!("[ExtractAudio] Destination: Example Video Title.{}", audio_fmt);
    } else {
        println!("[download] Destination: Example Video Title.mp4");
        println!("[download]   0.0% of 35.67MiB at  2.34MiB/s ETA 00:15");
        println!("[download]  50.2% of 35.67MiB at  4.56MiB/s ETA 00:04");
        println!("[download] 100.0% of 35.67MiB in 00:08");
    }

    if args.iter().any(|a| a == "--embed-thumbnail") {
        println!("[EmbedThumbnail] Adding thumbnail...");
    }
    if args.iter().any(|a| a == "--embed-metadata") {
        println!("[Metadata] Adding metadata...");
    }

    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ytdlp(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ytdlp};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ytdlp(vec!["--help".to_string()]), 0);
        assert_eq!(run_ytdlp(vec!["-h".to_string()]), 0);
        let _ = run_ytdlp(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ytdlp(vec![]);
    }
}
