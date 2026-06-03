#![deny(clippy::all)]

//! wf-recorder-cli — OurOS wf-recorder screen recorder
//!
//! Single personality: `wf-recorder`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wf_recorder(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wf-recorder [OPTIONS]");
        println!("wf-recorder v0.4 (OurOS) — Screen recording for Wayland");
        println!();
        println!("Options:");
        println!("  -f FILE           Output file");
        println!("  -g GEOMETRY       Record region");
        println!("  -o OUTPUT         Record specific output");
        println!("  -c CODEC          Video codec (h264, vp9, etc.)");
        println!("  -C CODEC          Audio codec");
        println!("  -a [DEVICE]       Record audio");
        println!("  -r FPS            Framerate");
        println!("  -d DEVICE         DRM device");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wf-recorder v0.4 (OurOS)"); return 0; }
    let file = args.iter().skip_while(|a| a.as_str() != "-f").nth(1).map(|s| s.as_str()).unwrap_or("recording.mp4");
    println!("Recording to: {}", file);
    println!("  Codec: h264 (vaapi)");
    println!("  Framerate: 30 fps");
    if args.iter().any(|a| a == "-a") {
        println!("  Audio: enabled");
    }
    println!("  Press Ctrl+C to stop");
    if args.is_empty() {
        println!("  Full screen recording");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wf-recorder".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wf_recorder(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wf_recorder};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wf-recorder"), "wf-recorder");
        assert_eq!(basename(r"C:\bin\wf-recorder.exe"), "wf-recorder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wf-recorder.exe"), "wf-recorder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wf_recorder(&["--help".to_string()], "wf-recorder"), 0);
        assert_eq!(run_wf_recorder(&["-h".to_string()], "wf-recorder"), 0);
        assert_eq!(run_wf_recorder(&["--version".to_string()], "wf-recorder"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wf_recorder(&[], "wf-recorder"), 0);
    }
}
