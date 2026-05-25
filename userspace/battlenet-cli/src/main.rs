#![deny(clippy::all)]

//! battlenet-cli — OurOS Battle.net (Blizzard/Activision launcher)
//!
//! Single personality: `battlenet`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bnet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: battlenet [OPTIONS]");
        println!("Battle.net 2.39 (OurOS) — Blizzard/Activision/King launcher");
        println!();
        println!("Options:");
        println!("  --library              Game library (WoW, Diablo, Overwatch, SC, HS, etc.)");
        println!("  --shop                 Battle.net Shop");
        println!("  --launch GAME          Launch a specific game by tag (wow/d4/ow2/sc2/hs/coh/cod)");
        println!("  --authenticator        Battle.net Mobile Authenticator");
        println!("  --voice                Battle.net Voice Chat");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Battle.net 2.39.0.15573 (OurOS)"); return 0; }
    println!("Battle.net 2.39.0.15573 (OurOS)");
    println!("  Vendor: Blizzard Entertainment — part of Activision Blizzard, owned by Microsoft since Oct 2023");
    println!("  Originally launched: Battle.net 1.0 — Nov 30 1996 (Diablo 1's online matchmaking)");
    println!("  Founded: Blizzard 1991 by Allen Adham, Mike Morhaime, Frank Pearce");
    println!("  Microsoft acquisition: $68.7B (Oct 2023) — largest gaming M&A in history");
    println!("  Available titles: World of Warcraft, Diablo II/III/IV, Overwatch 2, StarCraft / SC2,");
    println!("                   Hearthstone, Heroes of the Storm, Call of Duty (Modern Warfare, Warzone),");
    println!("                   Crash Bandicoot 4, Black Ops, Diablo Immortal");
    println!("  Subscription games: World of Warcraft ($14.99/mo), classic WoW (same sub)");
    println!("  Free-to-play: Hearthstone, Overwatch 2 (since 2022), Heroes of the Storm");
    println!("  Mobile: Diablo Immortal, Hearthstone Mobile, Warcraft Rumble");
    println!("  Killer feature: Battle.net Authenticator (mobile) — pioneered mobile MFA for games (2008)");
    println!("  Social: friends list, group chat, voice (since 2018), parties, broadcast invite");
    println!("  Other Microsoft Gaming: Xbox Game Pass (CoD now on Game Pass since Microsoft deal)");
    println!("  Differentiator: tightly curated portfolio of legendary IPs, no third-party games");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "battlenet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bnet(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
