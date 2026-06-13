#![deny(clippy::all)]

//! openshot-cli — SlateOS OpenShot video editor
//!
//! Single personality: `openshot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_os(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openshot [OPTIONS] [PROJECT]");
        println!("OpenShot Video Editor 3.2 (Slate OS) — Easy-to-use cross-platform NLE");
        println!();
        println!("Options:");
        println!("  --debug                Debug logging");
        println!("  --version              Show version");
        println!("  --lang LANG            UI language");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OpenShot 3.2.0 (Slate OS)"); return 0; }
    println!("OpenShot 3.2.0 (Slate OS)");
    println!("  Engine: libopenshot (C++ with Python bindings)");
    println!("  Features: Drag-and-drop editing, keyframe animation, 3D titles");
    println!("  Effects: Watermarks, transparency, color shifts");
    println!("  Audio: Waveform display, per-clip audio mixing");
    println!("  Formats: All FFmpeg formats");
    println!("  License: GNU GPLv3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "openshot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_os(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_os};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openshot"), "openshot");
        assert_eq!(basename(r"C:\bin\openshot.exe"), "openshot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openshot.exe"), "openshot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_os(&["--help".to_string()], "openshot"), 0);
        assert_eq!(run_os(&["-h".to_string()], "openshot"), 0);
        let _ = run_os(&["--version".to_string()], "openshot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_os(&[], "openshot");
    }
}
