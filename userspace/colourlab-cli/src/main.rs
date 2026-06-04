#![deny(clippy::all)]

//! colourlab-cli — OurOS FilmLight ColourLab Ai look management
//!
//! Single personality: `colourlab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: colourlab [OPTIONS] [SHOT]");
        println!("ColourLab Ai 3 (OurOS) — AI-assisted shot matching & look transfer");
        println!();
        println!("Options:");
        println!("  --shot-match REFERENCE Match shot to reference");
        println!("  --look LOOK_FILE       Apply look (Look files from cinematographers)");
        println!("  --auto-match           Automatic shot-to-shot matching");
        println!("  --emulator             Camera/film emulator");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ColourLab Ai 3.5.0 (OurOS)"); return 0; }
    println!("ColourLab Ai 3.5.0 (OurOS)");
    println!("  AI engine: Trained on Hollywood-grade reference content");
    println!("  Workflow: Match every shot to a reference, transfer look across project");
    println!("  Integration: BLG export to Baselight, CDL/LUT to any system");
    println!("  Cinematographer profiles: Roger Deakins, Greig Fraser, Hoyte van Hoytema, ...");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "colourlab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/colourlab"), "colourlab");
        assert_eq!(basename(r"C:\bin\colourlab.exe"), "colourlab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("colourlab.exe"), "colourlab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cl(&["--help".to_string()], "colourlab"), 0);
        assert_eq!(run_cl(&["-h".to_string()], "colourlab"), 0);
        let _ = run_cl(&["--version".to_string()], "colourlab");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cl(&[], "colourlab");
    }
}
