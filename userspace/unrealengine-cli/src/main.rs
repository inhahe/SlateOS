#![deny(clippy::all)]

//! unrealengine-cli — OurOS Unreal Engine (Epic's AAA game engine)
//!
//! Single personality: `unrealengine`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ue(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unrealengine [OPTIONS]");
        println!("Unreal Engine 5.5 (OurOS) — Epic Games AAA game engine + creation suite");
        println!();
        println!("Options:");
        println!("  --launcher             Epic Games Launcher (engine version + marketplace)");
        println!("  --new                  New project (Game / Film & TV / Architecture / Auto)");
        println!("  --nanite               Nanite virtualized geometry (billions of polygons)");
        println!("  --lumen                Lumen real-time global illumination");
        println!("  --metahuman            MetaHuman Creator (photoreal digital humans)");
        println!("  --blueprint            Visual Scripting (Blueprint) editor");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Unreal Engine 5.5.1 (OurOS)"); return 0; }
    println!("Unreal Engine 5.5.1 (OurOS)");
    println!("  Vendor: Epic Games Inc. (Cary, NC) — owned by Tim Sweeney + Tencent 40%");
    println!("  History: Unreal Engine 1.0 (1998, ships with Unreal game)");
    println!("           UE2 (2002), UE3 (2006 — Gears of War era), UE4 (2014 — open license)");
    println!("           UE5 launched Apr 2022 — Lumen + Nanite + Chaos physics");
    println!("  Royalty: FREE to download/use; 5% royalty after first $1M lifetime revenue per game");
    println!("           (Epic Games Store: 0% — incentive to publish there)");
    println!("  Languages: C++ (engine), Blueprints (visual scripting, ~same power as C++),");
    println!("            Verse (new functional language, used in Unreal Editor for Fortnite)");
    println!("  Killer features (UE5):");
    println!("    - Nanite: virtualized micropolygon geometry. Import 8-billion-poly ZBrush sculpt directly");
    println!("    - Lumen: fully dynamic real-time GI (no baking — change time of day live)");
    println!("    - World Partition: streams open-world worlds (Final Fantasy VII Rebirth, Black Myth Wukong)");
    println!("    - MetaHuman: 1-hour digital double creation, fully rigged + animated");
    println!("    - Chaos Physics: fracture, vehicle, destruction sim");
    println!("    - Niagara FX: GPU particles, ~100K-particle effects real-time");
    println!("  Marketplace: Quixel Megascans (FREE for UE users — Epic acquired Quixel 2019)");
    println!("  Famous UE5 titles: Fortnite, Hellblade II, Black Myth: Wukong, Stalker 2, Lords of the Fallen");
    println!("  Non-game uses: virtual production (The Mandalorian Volume LED stage),");
    println!("                automotive design (BMW, Audi), architecture (Twinmotion path), VR/AR");
    println!("  Differentiator: highest-end real-time graphics in industry + free + free Megascans");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "unrealengine".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ue(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ue};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/unrealengine"), "unrealengine");
        assert_eq!(basename(r"C:\bin\unrealengine.exe"), "unrealengine.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("unrealengine.exe"), "unrealengine");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ue(&["--help".to_string()], "unrealengine"), 0);
        assert_eq!(run_ue(&["-h".to_string()], "unrealengine"), 0);
        let _ = run_ue(&["--version".to_string()], "unrealengine");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ue(&[], "unrealengine");
    }
}
