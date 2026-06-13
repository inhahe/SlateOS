#![deny(clippy::all)]

//! rockstar-cli — SlateOS Rockstar Games Launcher
//!
//! Single personality: `rockstar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rockstar [OPTIONS]");
        println!("Rockstar Games Launcher 1.0.106 (Slate OS) — Rockstar's own launcher");
        println!();
        println!("Options:");
        println!("  --library              Game library (GTA series, RDR series, Max Payne 3, etc.)");
        println!("  --social-club          Rockstar Social Club (account / friends / crews)");
        println!("  --launch-gta-v         Launch Grand Theft Auto V");
        println!("  --launch-gta-online    Launch GTA Online");
        println!("  --launch-rdr2          Launch Red Dead Redemption 2");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rockstar Games Launcher 1.0.106.842 (Slate OS)"); return 0; }
    println!("Rockstar Games Launcher 1.0.106.842 (Slate OS)");
    println!("  Vendor: Rockstar Games (HQ New York, NY) — subsidiary of Take-Two Interactive");
    println!("  Founded: 1998 (Rockstar New York), merged BMG Interactive teams");
    println!("  Founders: Sam + Dan Houser (Sam left 2020, Dan founded 'Absurd Ventures' 2023),");
    println!("           Terry Donovan, Jamie King, Gary Foreman");
    println!("  Owner: Take-Two Interactive (NASDAQ: TTWO, $26B market cap)");
    println!("  Launched: Rockstar Games Launcher (RGL) Sep 2019");
    println!("  Library on RGL: GTA III, Vice City, San Andreas, IV, V, GTA Online,");
    println!("                  Red Dead Redemption (2010 PC port via RGL), RDR2,");
    println!("                  Max Payne 1+2+3, Bully, L.A. Noire, Manhunt, Midnight Club: Los Angeles");
    println!("  GTA V: still printing money — sold 200M+ copies (Q3 2024), GTA Online keeps growing");
    println!("  RDR2: 65M+ copies sold, widely cited as best-looking AAA of last decade");
    println!("  GTA VI: announced Dec 2023 trailer — set in Vice City, due fall 2025");
    println!("         (Take-Two has confirmed: 'most ambitious entertainment project ever made')");
    println!("  Engine: RAGE — Rockstar Advanced Game Engine, used since GTA IV 2008");
    println!("  Social Club: cross-game progression, friend lists, crews (GTA Online), cloud saves");
    println!("  Launcher reception: heavily criticized — required for purchased games even on Steam");
    println!("  Differentiator: gatekeeps the most lucrative single IP in gaming history (GTA)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rockstar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rockstar"), "rockstar");
        assert_eq!(basename(r"C:\bin\rockstar.exe"), "rockstar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rockstar.exe"), "rockstar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rs(&["--help".to_string()], "rockstar"), 0);
        assert_eq!(run_rs(&["-h".to_string()], "rockstar"), 0);
        let _ = run_rs(&["--version".to_string()], "rockstar");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rs(&[], "rockstar");
    }
}
