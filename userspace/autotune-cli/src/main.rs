#![deny(clippy::all)]

//! autotune-cli — OurOS Antares Auto-Tune pitch correction
//!
//! Single personality: `autotune`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_at(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: autotune [OPTIONS] [VOCAL]");
        println!("Antares Auto-Tune Pro X (OurOS) — Real-time and graphical pitch correction");
        println!();
        println!("Options:");
        println!("  --key KEY              Target key (e.g. C, Am, F#m)");
        println!("  --scale SCALE          Scale (chromatic/major/minor/custom)");
        println!("  --retune-speed N       Retune speed (0 = hard tune)");
        println!("  --humanize N           Humanize amount");
        println!("  --vibrato CMD          Vibrato controls");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Antares Auto-Tune Pro X v10.5.0 (OurOS)"); return 0; }
    println!("Antares Auto-Tune Pro X v10.5.0 (OurOS)");
    println!("  Modes: Auto Mode (real-time), Graph Mode (offline edit)");
    println!("  Engine: Antares Spectral Shift");
    println!("  Features: Classic Mode (retro \"5 hardware\"), Humanize, Throat Modeling");
    println!("  Companion: Auto-Key (key/scale detection), Articulator, Mic Mod");
    println!("  Plug-in formats: VST3, AU, AAX, ARA2 (with Cubase/Studio One/Logic)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "autotune".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_at(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_at};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/autotune"), "autotune");
        assert_eq!(basename(r"C:\bin\autotune.exe"), "autotune.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("autotune.exe"), "autotune");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_at(&["--help".to_string()], "autotune"), 0);
        assert_eq!(run_at(&["-h".to_string()], "autotune"), 0);
        assert_eq!(run_at(&["--version".to_string()], "autotune"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_at(&[], "autotune"), 0);
    }
}
