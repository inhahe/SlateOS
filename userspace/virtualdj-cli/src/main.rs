#![deny(clippy::all)]

//! virtualdj-cli — OurOS VirtualDJ (Atomix Productions DJ software)
//!
//! Single personality: `virtualdj`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vdj(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virtualdj [OPTIONS]");
        println!("VirtualDJ 2024 (OurOS) — Atomix Productions DJ mixing software");
        println!();
        println!("Options:");
        println!("  --new                  New session (2/4/6 decks)");
        println!("  --stem-separation      Real-Time Stem Separation (vocals/instr/bass/drums live)");
        println!("  --controller           Connect MIDI/HID DJ controller (Pioneer/Numark/Denon/Hercules)");
        println!("  --video                Video mixing (audio + visual sync)");
        println!("  --karaoke              Karaoke mode (CD+G, .lrc)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VirtualDJ 2024 (build 8189) (OurOS)"); return 0; }
    println!("VirtualDJ 2024 (build 8189) (OurOS)");
    println!("  Vendor: Atomix Productions (HQ Paris, France — founded 1997)");
    println!("  Founder: Stéphane Clavel");
    println!("  Origin: AtomixMP3 (1999) → VirtualDJ 1.0 (2003) → present");
    println!("  Platforms: Windows, macOS (Intel + Apple Silicon native), iOS, Android");
    println!("  Pricing: VirtualDJ Home FREE (full features, non-commercial use), Pro $19/mo or $299 lifetime");
    println!("  Killer feature (VDJ 2024+):");
    println!("    Real-Time Stem Separation — separates ANY track into vocals / instruments / bass /");
    println!("    drums in real time during a mix, enabling acapella + instrumental on the fly");
    println!("    (Serato/rekordbox followed; VDJ shipped it first as 'STEMS 2.0')");
    println!("  Decks: 2/4/6 deck modes, each with hot cues, loops, beat grid, key detection");
    println!("  Sample decks: 8 sampler slots per session");
    println!("  Controller support: 300+ MIDI/HID controllers natively (more than any competitor)");
    println!("  Video DJ: mix videos in sync with audio (clubs/weddings — strong niche)");
    println!("  Library: BPM/key/energy autoanalysis, smart playlists, cloud sync via Cloud Library");
    println!("  Effects: 90+ built-in effects (reverb, echo, filter, BeatGrid, FlangeR)");
    println!("  Skinning: deeply skinnable UI, community theme marketplace");
    println!("  Use cases: club DJs, mobile DJs (weddings), KJ (karaoke), bedroom mixing");
    println!("  Competitors: rekordbox (Pioneer DJ), Serato DJ Pro, Traktor Pro 3");
    println!("  Differentiator: largest free tier in DJ software, widest controller compatibility");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "virtualdj".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vdj(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vdj};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virtualdj"), "virtualdj");
        assert_eq!(basename(r"C:\bin\virtualdj.exe"), "virtualdj.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virtualdj.exe"), "virtualdj");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_vdj(&["--help".to_string()], "virtualdj"), 0);
        assert_eq!(run_vdj(&["-h".to_string()], "virtualdj"), 0);
        assert_eq!(run_vdj(&["--version".to_string()], "virtualdj"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_vdj(&[], "virtualdj"), 0);
    }
}
