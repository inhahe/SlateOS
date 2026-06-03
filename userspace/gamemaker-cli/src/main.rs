#![deny(clippy::all)]

//! gamemaker-cli — OurOS GameMaker Studio (Opera-owned indie 2D engine)
//!
//! Single personality: `gamemaker`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gamemaker [OPTIONS]");
        println!("GameMaker 2024.11 (OurOS) — Opera GameMaker Studio (indie 2D engine)");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --gml                  GameMaker Language (GML) script editor");
        println!("  --drag-and-drop        Drag-and-Drop (DnD) visual programming");
        println!("  --runtime              YYRuntime (executable runtime, target many platforms)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GameMaker 2024.11.0.180 (OurOS)"); return 0; }
    println!("GameMaker 2024.11.0.180 (OurOS)");
    println!("  Vendor: Opera Limited (Norway) — acquired YoYo Games (GameMaker creators) Jan 2021");
    println!("  Created by: Mark Overmars (Utrecht Univ., Netherlands, 1999 — initially 'Animo')");
    println!("  Renamed: Animo → Game Maker → GameMaker Studio → GameMaker Studio 2 → GameMaker");
    println!("  Pricing: FREE for non-commercial / educational");
    println!("          Pro $79.99/yr (publish to PC/Mac/Linux)");
    println!("          Console export +$799/yr per platform (Nintendo/PlayStation/Xbox)");
    println!("  Languages: GML (GameMaker Language — C-like), Drag-and-Drop visual scripting");
    println!("  Specialty: 2D games (sprites, rooms, objects, events) — easiest 2D engine in market");
    println!("  Multi-platform: Windows, macOS, Linux, iOS, Android, HTML5, PS4/5, Xbox One/Series, Switch");
    println!("  Famous GameMaker games:");
    println!("    - Undertale (Toby Fox) — 2015 indie smash");
    println!("    - Hotline Miami 1+2 (Dennaton)");
    println!("    - Hyper Light Drifter (Heart Machine)");
    println!("    - Risk of Rain 1 (Hopoo Games)");
    println!("    - Spelunky (original — by Derek Yu, 2008)");
    println!("    - Nuclear Throne (Vlambeer)");
    println!("    - Nidhogg, Crashlands, Forager, Loop Hero");
    println!("  Differentiator: lowest barrier to ship a polished 2D PC/console game — true success stories");
    println!("  Companion tools: GameMaker IDE, Image Editor, Sound Editor, Particle Editor");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gamemaker".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gamemaker"), "gamemaker");
        assert_eq!(basename(r"C:\bin\gamemaker.exe"), "gamemaker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gamemaker.exe"), "gamemaker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gm(&["--help".to_string()], "gamemaker"), 0);
        assert_eq!(run_gm(&["-h".to_string()], "gamemaker"), 0);
        assert_eq!(run_gm(&["--version".to_string()], "gamemaker"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gm(&[], "gamemaker"), 0);
    }
}
