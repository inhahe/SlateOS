#![deny(clippy::all)]

//! camtasia-cli — OurOS TechSmith Camtasia (screen-recording + editor)
//!
//! Single personality: `camtasia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cam(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: camtasia [OPTIONS]");
        println!("TechSmith Camtasia 2024 (OurOS) — Screen recording + video editor");
        println!();
        println!("Options:");
        println!("  --record               Camtasia Recorder (screen + webcam + mic)");
        println!("  --new                  New project");
        println!("  --quizzes              Interactive Quizzes (SCORM/LMS export)");
        println!("  --captions             Auto-Captions (speech-to-text)");
        println!("  --snagit               Companion: Snagit (screenshot tool, separate)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Camtasia 2024 (24.1.5) (OurOS)"); return 0; }
    println!("Camtasia 2024 (24.1.5) (OurOS)");
    println!("  Vendor: TechSmith Corporation (HQ Okemos, MI — founded 1987 by Bill Hamilton)");
    println!("  Platforms: Windows, macOS (cross-platform parity ~mid-2010s)");
    println!("  Pricing: Camtasia Pro $179.88/yr (was $299 perpetual until 2022 model shift)");
    println!("          Education $144/yr, Business $215.88/yr per user");
    println!("  Use case #1: corporate training videos / e-learning (the dominant niche)");
    println!("  Use case #2: software tutorials, product demos, YouTube how-to creators");
    println!("  Engine: GPU accelerated, native 4K, animated annotations, callouts, transitions");
    println!("  Signature features:");
    println!("    - Smart Focus (auto-zoom-pan to cursor activity)");
    println!("    - SmartTracks (audio levelling)");
    println!("    - Quizzes (interactive Q&A for LMS, SCORM 1.2/2004 export)");
    println!("    - Cursor effects (highlight cursor, click ripples, magnifier)");
    println!("    - Webcam picture-in-picture with shape masks");
    println!("    - Library of free music + sound effects + animated assets");
    println!("  Captions: AI speech-to-text (built in, no subscription), edit + style + burn-in");
    println!("  Companion: Snagit ($62.99/yr) — screenshot/screen-capture tool, sister app");
    println!("  Other TechSmith: TechSmith Capture, Audiate (audio editor), Knowmia (video LMS)");
    println!("  Differentiator: easiest screen-record-to-polished-edit pipeline in market");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "camtasia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cam(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cam};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/camtasia"), "camtasia");
        assert_eq!(basename(r"C:\bin\camtasia.exe"), "camtasia.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("camtasia.exe"), "camtasia");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cam(&["--help".to_string()], "camtasia"), 0);
        assert_eq!(run_cam(&["-h".to_string()], "camtasia"), 0);
        assert_eq!(run_cam(&["--version".to_string()], "camtasia"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cam(&[], "camtasia"), 0);
    }
}
