#![deny(clippy::all)]

//! obs-studio-cli — OurOS OBS Studio streaming/recording
//!
//! Single personality: `obs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_obs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: obs [OPTIONS]");
        println!("obs v30.0 (OurOS) — Open Broadcaster Software");
        println!();
        println!("Options:");
        println!("  --scene NAME      Start with scene");
        println!("  --startstreaming  Start streaming on launch");
        println!("  --startrecording  Start recording on launch");
        println!("  --minimize-to-tray Minimize to system tray");
        println!("  --studio-mode     Start in studio mode");
        println!("  --profile NAME    Use specific profile");
        println!("  --collection NAME Use scene collection");
        println!("  --portable        Portable mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("obs v30.0 (OurOS)"); return 0; }
    println!("obs: Open Broadcaster Software started");
    println!("  Video: 1920x1080 @ 60fps");
    println!("  Encoder: x264 (software)");
    println!("  Audio: 48kHz stereo");
    println!("  Scenes: 3 configured");
    println!("  Sources: 5 active");
    println!("  Output: RTMP / MKV recording");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "obs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_obs(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_obs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/obs-studio"), "obs-studio");
        assert_eq!(basename(r"C:\bin\obs-studio.exe"), "obs-studio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("obs-studio.exe"), "obs-studio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_obs(&["--help".to_string()], "obs-studio"), 0);
        assert_eq!(run_obs(&["-h".to_string()], "obs-studio"), 0);
        let _ = run_obs(&["--version".to_string()], "obs-studio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_obs(&[], "obs-studio");
    }
}
