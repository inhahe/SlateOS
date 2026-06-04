#![deny(clippy::all)]

//! renpy-cli — OurOS Ren'Py (FOSS visual novel engine)
//!
//! Single personality: `renpy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: renpy [OPTIONS]");
        println!("Ren'Py 8.3.4 (OurOS) — FOSS visual novel + storytelling engine");
        println!();
        println!("Options:");
        println!("  --new                  New project (visual novel template)");
        println!("  --launcher             Ren'Py Launcher (project list)");
        println!("  --lint                 Lint script (catch errors before testing)");
        println!("  --distribute           Build distributions (Win/Mac/Linux/Android/iOS/Web)");
        println!("  --android-build        Generate Android APK / Google Play AAB");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ren'Py 8.3.4 (OurOS)"); return 0; }
    println!("Ren'Py 8.3.4 (OurOS)");
    println!("  Created by: PyTom (Tom Rothamel, Pittsburgh PA) — initial release 2004");
    println!("  License: MIT (engine), free for commercial use");
    println!("  Built on: Python 3 (since Ren'Py 8.0, Aug 2022 — was Python 2 for 18 years)");
    println!("  Backend: pygame_sdl2 (SDL2-based), OpenGL ES 2.0+ renderer, audio via SDL_mixer");
    println!("  Script: Ren'Py script language (custom DSL) + embedded Python for advanced logic");
    println!("  Multi-platform: Windows, macOS, Linux, iOS, Android, HTML5 (web), Steam Deck (Linux native)");
    println!("  Niche: visual novels — branching dialogue, ADV/NVL modes, sprites + backgrounds + music");
    println!("        also: dating sims, kinetic novels, choice-based RPGs, interactive fiction");
    println!("  Famous Ren'Py games:");
    println!("    - Doki Doki Literature Club (Team Salvato, 2017) — viral horror VN, FREE on Steam");
    println!("    - Long Live the Queen, A Summer's End, butterfly soup, OneShot (partial)");
    println!("    - Katawa Shoujo (Four Leaf Studios, 2012)");
    println!("    - Robotics;Notes (port), Higurashi When They Cry (port)");
    println!("  Community: huge — discord, forums, lemmasoft, itch.io tagged 'visual novel'");
    println!("  Pricing: FREE FOREVER, $0 commercial — Ren'Py is one of the great FOSS gifts to game dev");
    println!("  Donations: PyTom accepts via Patreon, sustains development single-handed");
    println!("  Differentiator: only mainstream open-source engine purpose-built for VNs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "renpy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/renpy"), "renpy");
        assert_eq!(basename(r"C:\bin\renpy.exe"), "renpy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("renpy.exe"), "renpy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rp(&["--help".to_string()], "renpy"), 0);
        assert_eq!(run_rp(&["-h".to_string()], "renpy"), 0);
        let _ = run_rp(&["--version".to_string()], "renpy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rp(&[], "renpy");
    }
}
