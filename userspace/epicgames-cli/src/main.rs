#![deny(clippy::all)]

//! epicgames-cli — OurOS Epic Games Launcher / Store
//!
//! Single personality: `epicgames`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: epicgames [OPTIONS]");
        println!("Epic Games Launcher 16.5 (OurOS) — Epic Games Store + Unreal/Fortnite hub");
        println!();
        println!("Options:");
        println!("  --library              Game library");
        println!("  --store                Epic Games Store");
        println!("  --free-game            This week's free game (always a free PC game on offer)");
        println!("  --fortnite             Launch Fortnite");
        println!("  --unreal-engine        Unreal Engine (free download + Marketplace)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Epic Games Launcher 16.5.4 (OurOS)"); return 0; }
    println!("Epic Games Launcher 16.5.4 (OurOS)");
    println!("  Vendor: Epic Games Inc. (HQ Cary, NC — founded 1991 as 'Potomac Computer Systems')");
    println!("  Founder/CEO: Tim Sweeney (Pennsylvania State Univ. dropout, started Epic at age 20)");
    println!("  Major shareholder: Tencent (40% non-voting, $330M acquisition 2012)");
    println!("  Valuation: $31.5B (2022 funding round)");
    println!("  Revenue: ~$5B/yr Fortnite alone, ~$1B+ Unreal Engine licensing");
    println!("  Store launch: Dec 2018 — directly challenging Steam");
    println!("  Store deal: developers keep 88% (Steam takes 30%); Epic Online Services free");
    println!("  Killer feature #1: Free Game Every Week (since launch) — gave away ~$2k value");
    println!("  Killer feature #2: Exclusives (Borderlands 3, Metro Exodus, Alan Wake 2, Fortnite)");
    println!("  Fortnite: launched 2017 (Battle Royale 2017 hit) — $9B+ revenue");
    println!("  Unreal Engine: free download, royalty only on commercial games > $1M/yr revenue");
    println!("  UE Marketplace: assets / blueprints / Megascans (free since 2019 acquisition of Quixel)");
    println!("  Legal: Epic v. Apple lawsuit 2020 (App Store fees) — partial win, third-party stores now allowed");
    println!("  Other Epic products: Houseparty (RIP 2021), SuperAwesome, Bandcamp (sold 2024 to Songtradr)");
    println!("  Differentiator: aggressive freebies + exclusives + revenue-share — disrupting Steam");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "epicgames".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eg(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/epicgames"), "epicgames");
        assert_eq!(basename(r"C:\bin\epicgames.exe"), "epicgames.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("epicgames.exe"), "epicgames");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_eg(&["--help".to_string()], "epicgames"), 0);
        assert_eq!(run_eg(&["-h".to_string()], "epicgames"), 0);
        assert_eq!(run_eg(&["--version".to_string()], "epicgames"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_eg(&[], "epicgames"), 0);
    }
}
