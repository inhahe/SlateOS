#![deny(clippy::all)]

//! ea-cli — OurOS EA app (Electronic Arts, replaced Origin)
//!
//! Single personality: `ea`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ea(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ea [OPTIONS]");
        println!("EA app 13.421 (OurOS) — Electronic Arts game launcher (replaced Origin 2022)");
        println!();
        println!("Options:");
        println!("  --library              Game library");
        println!("  --store                EA Store");
        println!("  --ea-play              EA Play subscription ($4.99/mo or $29.99/yr)");
        println!("  --ea-play-pro          EA Play Pro (premium tier with launch-day access)");
        println!("  --bundled              Xbox Game Pass Ultimate includes EA Play");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("EA app 13.421.0.5859 (OurOS)"); return 0; }
    println!("EA app 13.421.0.5859 (OurOS)");
    println!("  Vendor: Electronic Arts Inc. (HQ Redwood City, CA — founded 1982 by Trip Hawkins)");
    println!("  Stock: EA on NASDAQ — $40B market cap (2024)");
    println!("  Replaced: Origin client (2011-2022) — EA app is full rewrite, lighter + faster");
    println!("  Library highlights: EA Sports FC (formerly FIFA), Madden NFL, NHL, F1, NBA Live,");
    println!("                     Battlefield (1942/3/4/V/2042), Mass Effect, Dragon Age, The Sims,");
    println!("                     Apex Legends, Star Wars Jedi: Survivor, Need for Speed, Dead Space,");
    println!("                     Plants vs Zombies, Titanfall, Anthem (RIP), Crysis (now Crytek again)");
    println!("  EA Play: $4.99/mo or $29.99/yr — instant access to vault + 10-hour trials + 10% off");
    println!("  EA Play Pro: premium tier — launch-day access to EA games before retail release");
    println!("  Bundling: EA Play included with Xbox Game Pass Ultimate ($16.99/mo)");
    println!("  Studios owned: BioWare (Mass Effect, Dragon Age), DICE (Battlefield), Respawn (Apex,");
    println!("                Titanfall, Star Wars Jedi), Maxis (Sims, SimCity), Criterion (Need for Speed),");
    println!("                Codemasters (F1, DiRT), Glu Mobile, Tracktwenty");
    println!("  Anti-cheat: EA Anti-Cheat (kernel-level, since 2022 for Battlefield/Apex)");
    println!("  Multiplayer: EA Origin friends → migrated to EA accounts, cross-progression EA Sports FC");
    println!("  Differentiator: deepest sports portfolio in industry (FC, Madden, NHL, UFC, F1)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ea".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ea(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ea};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ea"), "ea");
        assert_eq!(basename(r"C:\bin\ea.exe"), "ea.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ea.exe"), "ea");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ea(&["--help".to_string()], "ea"), 0);
        assert_eq!(run_ea(&["-h".to_string()], "ea"), 0);
        assert_eq!(run_ea(&["--version".to_string()], "ea"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ea(&[], "ea"), 0);
    }
}
