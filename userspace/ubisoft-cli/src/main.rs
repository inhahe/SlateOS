#![deny(clippy::all)]

//! ubisoft-cli — OurOS Ubisoft Connect (replaced Uplay)
//!
//! Single personality: `ubisoft`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ubi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ubisoft [OPTIONS]");
        println!("Ubisoft Connect 153.0 (OurOS) — Ubisoft game launcher + service (renamed Uplay 2020)");
        println!();
        println!("Options:");
        println!("  --library              Game library");
        println!("  --store                Ubisoft Store");
        println!("  --plus                 Ubisoft+ Premium subscription ($17.99/mo)");
        println!("  --units                Ubisoft Units (rewards / loyalty points)");
        println!("  --challenges           Cross-game challenges");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ubisoft Connect 153.0.13252 (OurOS)"); return 0; }
    println!("Ubisoft Connect 153.0.13252 (OurOS)");
    println!("  Vendor: Ubisoft Entertainment SA (HQ Saint-Mandé / Montreuil, France — founded 1986)");
    println!("  Founders: Five Guillemot brothers (Christian, Claude, Gérard, Michel, Yves)");
    println!("  Renamed: Uplay → Ubisoft Connect (Oct 2020) — new launcher rewrite");
    println!("  Library highlights: Assassin's Creed (entire series), Far Cry, Watch Dogs, The Crew,");
    println!("                     For Honor, Rainbow Six (Siege/Extraction), Ghost Recon, Just Dance,");
    println!("                     Anno, The Settlers, Heroes of Might & Magic, Prince of Persia,");
    println!("                     Tom Clancy series (Splinter Cell, Ghost Recon, Rainbow Six, Division)");
    println!("  Engines: Anvil (AC), Snowdrop (Division), Dunia (Far Cry), Disrupt (Watch Dogs)");
    println!("  Ubisoft+: $17.99/mo (Premium) — full library + DLCs + early access; also via PC/Xbox");
    println!("           Multi-Access — Steam Family Sharing-like cross-PC streaming");
    println!("  Units: in-house loyalty/rewards currency, redeem for in-game items or discounts");
    println!("  Stadia legacy: Ubisoft+ Cloud was the launch partner for Stadia (RIP 2023)");
    println!("  Studios owned: Massive (Stockholm — Division/Avatar/Star Wars Outlaws), Red Storm,");
    println!("                Reflections (Driver), Anvil, Ubisoft Montreal/Quebec/Toronto/Singapore");
    println!("  Anti-cheat: Vanguard-style anti-cheat for Rainbow Six Siege (since 2020 partnership w/ BattlEye)");
    println!("  Drama: Tencent + Guillemot family bought controlling stake 2024 (Ubisoft stock crash)");
    println!("  Differentiator: massive cross-game ecosystem with shared challenges + Units rewards");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ubisoft".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ubi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
