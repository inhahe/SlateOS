#![deny(clippy::all)]

//! xbox-cli — SlateOS Xbox app (Microsoft Game Pass + xCloud)
//!
//! Single personality: `xbox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xbox(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xbox [OPTIONS]");
        println!("Xbox app 2411 (Slate OS) — Microsoft Game Pass / xCloud / Xbox Network");
        println!();
        println!("Options:");
        println!("  --library              Game library (installed + Game Pass + ready-to-install)");
        println!("  --store                Microsoft Store games");
        println!("  --gamepass             Xbox Game Pass Ultimate ($16.99/mo)");
        println!("  --cloud-gaming         xCloud — stream from Microsoft's cloud (no download)");
        println!("  --remote               Remote Play from your Xbox console");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xbox app 2411.1001.27.0 (Slate OS)"); return 0; }
    println!("Xbox app 2411.1001.27.0 (Slate OS)");
    println!("  Vendor: Microsoft Corporation (Microsoft Gaming division)");
    println!("  CEO Microsoft Gaming: Phil Spencer (since 2017)");
    println!("  Brand: Xbox launched Nov 15 2001 (original Xbox console)");
    println!("  Console lineage: Xbox → 360 → One → Series X|S (current, Nov 2020)");
    println!("  Game Pass tiers (PC):");
    println!("    Core $9.99/mo (was Live Gold) — online MP + small game catalog");
    println!("    PC Game Pass $11.99/mo — 200+ PC games, day 1 first-party releases");
    println!("    Game Pass Ultimate $16.99/mo — PC + console + xCloud + EA Play");
    println!("    Standard $14.99/mo — no day-1 first-party");
    println!("  Studios under Xbox Game Studios (XGS):");
    println!("    Bethesda Softworks (Mar 2021 acq, $7.5B) — Doom, Fallout, Elder Scrolls, Starfield");
    println!("    Activision Blizzard King (Oct 2023, $68.7B) — CoD, Diablo, WoW, Candy Crush");
    println!("    343 Industries (Halo), The Coalition (Gears of War), Rare (Sea of Thieves),");
    println!("    Mojang (Minecraft), Obsidian, inXile, Ninja Theory, Playground (Forza), Turn 10");
    println!("  Key features in app: Game Pass discovery, cloud saves, achievements, friends,");
    println!("                       xCloud beta (no install — stream 1080p60), remote console play");
    println!("  Differentiator: Game Pass — Netflix-of-games at $11.99/mo PC = massive value");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xbox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xbox(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xbox};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xbox"), "xbox");
        assert_eq!(basename(r"C:\bin\xbox.exe"), "xbox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xbox.exe"), "xbox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xbox(&["--help".to_string()], "xbox"), 0);
        assert_eq!(run_xbox(&["-h".to_string()], "xbox"), 0);
        let _ = run_xbox(&["--version".to_string()], "xbox");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xbox(&[], "xbox");
    }
}
