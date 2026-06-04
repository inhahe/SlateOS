#![deny(clippy::all)]

//! ffmpeg — OurOS multimedia framework
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `ffmpeg` (default) — media converter/transcoder
//! - `ffprobe` — media analyzer
//! - `ffplay` — media player

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_ffmpeg(args: Vec<String>) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("ffmpeg version 7.0 (OurOS) — Hyper fast Audio and Video encoder");
        println!("usage: ffmpeg [options] [[infile options] -i infile]... {{[outfile options] outfile}}...");
        println!();
        println!("Common options:");
        println!("  -i filename      Input file");
        println!("  -f fmt           Force format");
        println!("  -c codec         Codec name (-c:v video, -c:a audio)");
        println!("  -b:v bitrate     Video bitrate");
        println!("  -b:a bitrate     Audio bitrate");
        println!("  -r rate          Frame rate");
        println!("  -s WxH           Frame size");
        println!("  -t duration      Duration");
        println!("  -ss position     Start time offset");
        println!("  -to position     Stop time");
        println!("  -vn              Disable video");
        println!("  -an              Disable audio");
        println!("  -y               Overwrite output");
        println!("  -n               Never overwrite");
        println!("  -threads N       Thread count");
        println!("  -version         Show version");
        println!("  -codecs          Show available codecs");
        println!("  -formats         Show available formats");
        println!("  -filters         Show available filters");
        return 0;
    }

    if args.iter().any(|a| a == "-version") {
        println!("ffmpeg version 7.0 (OurOS)");
        println!("built with gcc 13.2.0");
        println!("configuration: --enable-gpl --enable-nonfree --enable-libx264 --enable-libx265 --enable-libvpx --enable-libopus");
        println!("libavutil      59. 2.100");
        println!("libavcodec     61. 3.100");
        println!("libavformat    61. 1.100");
        println!("libavdevice    61. 1.100");
        println!("libavfilter    10. 1.100");
        println!("libswscale      8. 1.100");
        println!("libswresample   5. 1.100");
        return 0;
    }

    if args.iter().any(|a| a == "-codecs") {
        println!("Codecs:");
        println!(" DEV.LS h264        H.264 / AVC / MPEG-4 AVC");
        println!(" DEV.LS hevc        H.265 / HEVC");
        println!(" DEV.LS vp9         Google VP9");
        println!(" DEV.LS av1         Alliance for Open Media AV1");
        println!(" DEA.LS aac         AAC (Advanced Audio Coding)");
        println!(" DEA.LS opus        Opus");
        println!(" DEA.LS mp3         MP3 (MPEG audio layer 3)");
        println!(" DEA.LS flac        FLAC (Free Lossless Audio Codec)");
        println!(" DEA.LS vorbis      Vorbis");
        return 0;
    }

    if args.iter().any(|a| a == "-formats") {
        println!("File formats:");
        println!(" DE mp4         MP4 (MPEG-4 Part 14)");
        println!(" DE mkv         Matroska / WebM");
        println!(" DE avi         AVI (Audio Video Interleaved)");
        println!(" DE mov         QuickTime / MOV");
        println!(" DE webm        WebM");
        println!(" DE flv         FLV (Flash Video)");
        println!(" DE wav         WAV / WAVE");
        println!(" DE mp3         MP3 (MPEG audio layer 3)");
        println!(" DE ogg         Ogg");
        return 0;
    }

    if args.iter().any(|a| a == "-filters") {
        println!("Filters:");
        println!("  T.. scale         Scale the input video size");
        println!("  T.. crop          Crop the input video");
        println!("  T.. overlay       Overlay one video on another");
        println!("  T.. fade          Apply fade-in/out effect");
        println!("  T.. volume        Change input volume");
        println!("  T.. aresample     Resample audio");
        println!("  T.. eq            Set brightness, contrast, saturation");
        println!("  T.. drawtext      Draw text on video");
        return 0;
    }

    // Simulated encoding
    let input = args.iter().position(|a| a == "-i")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("input.mp4");
    let output = args.last().map(|s| s.as_str()).unwrap_or("output.mp4");

    println!("Input #0, mov,mp4, from '{}':", input);
    println!("  Duration: 00:05:32.40, start: 0.000000, bitrate: 8500 kb/s");
    println!("  Stream #0:0: Video: h264 (High), yuv420p, 1920x1080, 8000 kb/s, 30 fps");
    println!("  Stream #0:1: Audio: aac (LC), 48000 Hz, stereo, 192 kb/s");
    println!("Stream mapping:");
    println!("  Stream #0:0 -> #0:0 (h264 -> h264)");
    println!("  Stream #0:1 -> #0:1 (aac -> aac)");
    println!("Output #0, mp4, to '{}':", output);
    println!("  Stream #0:0: Video: h264, 1920x1080, 8000 kb/s");
    println!("  Stream #0:1: Audio: aac, 48000 Hz, stereo, 192 kb/s");
    println!("frame= 9972 fps=120 q=28.0 size=  262144kB time=00:05:32.40 bitrate=6450.3kbits/s speed=4.0x");
    println!("video:240000kB audio:7500kB subtitle:0kB global headers:0kB muxing overhead: 0.5%");
    0
}

fn run_ffprobe(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("ffprobe version 7.0 (OurOS) — Multimedia stream analyzer");
        println!("usage: ffprobe [OPTIONS] INPUT_FILE");
        println!();
        println!("Options:");
        println!("  -show_format     Show format/container info");
        println!("  -show_streams    Show stream info");
        println!("  -show_entries    Show specific entries");
        println!("  -of format       Set output format (default, json, xml, csv)");
        println!("  -v level         Set logging level");
        return 0;
    }

    let input = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.mp4");
    let json_output = args.iter().any(|a| a == "json") ||
        args.iter().position(|a| a == "-of").and_then(|i| args.get(i + 1)).map(|s| s == "json").unwrap_or(false);

    if json_output {
        println!("{{");
        println!("  \"format\": {{");
        println!("    \"filename\": \"{}\",", input);
        println!("    \"format_name\": \"mov,mp4\",");
        println!("    \"duration\": \"332.400000\",");
        println!("    \"bit_rate\": \"8500000\"");
        println!("  }},");
        println!("  \"streams\": [");
        println!("    {{\"codec_type\": \"video\", \"codec_name\": \"h264\", \"width\": 1920, \"height\": 1080, \"r_frame_rate\": \"30/1\"}},");
        println!("    {{\"codec_type\": \"audio\", \"codec_name\": \"aac\", \"sample_rate\": \"48000\", \"channels\": 2}}");
        println!("  ]");
        println!("}}");
    } else {
        println!("Input #0, mov,mp4, from '{}':", input);
        println!("  Metadata:");
        println!("    major_brand     : isom");
        println!("    encoder         : Lavf61.1.100");
        println!("  Duration: 00:05:32.40, start: 0.000000, bitrate: 8500 kb/s");
        println!("  Stream #0:0(und): Video: h264 (High), yuv420p(tv), 1920x1080 [SAR 1:1 DAR 16:9], 8000 kb/s, 30 fps");
        println!("  Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo, fltp, 192 kb/s");
    }
    0
}

fn run_ffplay(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("ffplay version 7.0 (OurOS) — Simple media player");
        println!("usage: ffplay [options] input_file");
        println!();
        println!("Options:");
        println!("  -fs              Start in fullscreen");
        println!("  -an              Disable audio");
        println!("  -vn              Disable video");
        println!("  -ss pos          Seek to position");
        println!("  -t duration      Play for duration seconds");
        println!("  -loop N          Loop N times (0=infinite)");
        println!("  -autoexit        Exit at the end");
        println!("  -volume N        Set volume (0-100)");
        return 0;
    }

    let input = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.mp4");
    println!("Playing: {}", input);
    println!("  Duration: 00:05:32.40");
    println!("  Video: h264, 1920x1080, 30 fps");
    println!("  Audio: aac, 48000 Hz, stereo");
    println!("(playback simulated)");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("ffmpeg");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "ffprobe" => run_ffprobe(rest),
        "ffplay" => run_ffplay(rest),
        _ => run_ffmpeg(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_ffmpeg};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ffmpeg(vec!["--help".to_string()]), 0);
        assert_eq!(run_ffmpeg(vec!["-h".to_string()]), 0);
        let _ = run_ffmpeg(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ffmpeg(vec![]);
    }
}
