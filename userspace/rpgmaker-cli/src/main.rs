#![deny(clippy::all)]

//! rpgmaker-cli — Slate OS RPG Maker (Kadokawa JRPG-style engine)
//!
//! Single personality: `rpgmaker`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rpgmaker [OPTIONS]");
        println!("RPG Maker MZ (Slate OS) — Kadokawa's classic JRPG-style maker (current 'MZ' version)");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --map                  Map editor (tile-based)");
        println!("  --database              Database (characters/items/enemies/skills/states)");
        println!("  --events                Event editor (visual scripting via Event Commands)");
        println!("  --plugin                Plugin Manager (JavaScript ES2017 plugins)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("RPG Maker MZ 1.7.0 (Slate OS)"); return 0; }
    println!("RPG Maker MZ 1.7.0 (Slate OS)");
    println!("  Vendor: Gotcha Gotcha Games (Tokyo, Japan) — part of Kadokawa Corporation");
    println!("         Originally by ASCII (1992), then Enterbrain, then Kadokawa Future Publishing");
    println!("  Series history: RPG Maker (Super Famicom 1995) → 95 → 2000 → 2003 → XP →");
    println!("                 VX → VX Ace → MV → MZ (Aug 2020, current)");
    println!("  Pricing: $79.99 base (frequent Steam sales to $20); royalty-free commercial use");
    println!("  Free trial: 30 days");
    println!("  Niche: top-down JRPG-style games — tile maps, turn-based combat, item shops, towns");
    println!("        but heavily extended via plugins to make almost any 2D genre");
    println!("  Tech: built on JavaScript (Node.js + PIXI.js) since RPG Maker MV (2015)");
    println!("        runs on browser (HTML5 export), PC/Mac/Linux/Android/iOS");
    println!("  Famous RPG Maker games:");
    println!("    - To the Moon (Freebird Games, 2011) — universally praised story game");
    println!("    - OMORI (OMOCAT, 2020) — viral indie hit");
    println!("    - LISA: The Painful (Dingaling, 2014)");
    println!("    - Yume Nikki (Kikiyama, 2004) — RPG Maker 2003 cult classic");
    println!("    - Ib (kouri, 2012), Mad Father, The Witch's House, Ao Oni (Japanese horror VNs)");
    println!("    - Hylics 1+2 (Mason Lindroth)");
    println!("  Differentiator: no programming needed for basic RPGs — event commands cover everything");
    println!("  Plugins: 1000s of community + paid plugins (Yanfly Engine, VisuStella, etc.)");
    println!("  Asset packs: huge market for character sprites / tilesets / music on the official store");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rpgmaker".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rpgmaker"), "rpgmaker");
        assert_eq!(basename(r"C:\bin\rpgmaker.exe"), "rpgmaker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rpgmaker.exe"), "rpgmaker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rm(&["--help".to_string()], "rpgmaker"), 0);
        assert_eq!(run_rm(&["-h".to_string()], "rpgmaker"), 0);
        let _ = run_rm(&["--version".to_string()], "rpgmaker");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rm(&[], "rpgmaker");
    }
}
