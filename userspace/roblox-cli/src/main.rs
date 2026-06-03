#![deny(clippy::all)]

//! roblox-cli — OurOS Roblox + Roblox Studio (UGC mega-platform)
//!
//! Single personality: `roblox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: roblox [OPTIONS]");
        println!("Roblox Player + Studio 0.650 (OurOS) — UGC game platform (kids/teens dominant)");
        println!();
        println!("Options:");
        println!("  --play                 Roblox Player (consume experiences)");
        println!("  --studio               Roblox Studio (create experiences)");
        println!("  --robux                Robux (virtual currency, $0.0125 each retail)");
        println!("  --premium              Roblox Premium ($4.99/$9.99/$19.99 monthly)");
        println!("  --luau                 Luau (Roblox's safer Lua 5.1 fork)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Roblox 0.650.0.6500664 (OurOS)"); return 0; }
    println!("Roblox 0.650.0.6500664 (OurOS)");
    println!("  Vendor: Roblox Corporation (San Mateo, CA — NYSE: RBLX, $35B market cap 2024)");
    println!("  Founders: David Baszucki (CEO), Erik Cassel (RIP 2013)");
    println!("  Launch: 2006 (originally 'DynaBlocks' beta 2004)");
    println!("  Users: 88M+ daily active (Q3 2024), peak 100M+ users on platform during events");
    println!("  Hours: 16B hours played per quarter (more than Netflix watch time some quarters)");
    println!("  Demographic: heavily skewed 9-16 years old (60%+ of users), shifting older 2020-2024");
    println!("  Platforms: Windows, macOS, iOS, Android, Xbox One/Series, Meta Quest VR, PS4/5 (2024+)");
    println!("  Roblox Studio: free creation tool — anyone makes a game (called 'experience')");
    println!("                Built in Qt (C++), runs on the Roblox engine, uses Luau scripting");
    println!("  Language: Luau — Lua 5.1 fork with type checking, perf optimizations, sandboxed");
    println!("  Killer feature: UGC (User-Generated Content). Roblox doesn't make games;");
    println!("                 millions of devs do. Top experiences make $1M+ DEVEX payouts");
    println!("  Famous experiences:");
    println!("    - Adopt Me! — 30B+ visits, made by indie team via Roblox");
    println!("    - Brookhaven RP — ~50B visits, 1-person dev shop initially");
    println!("    - Doors (Lsplash), Murder Mystery 2, Jailbreak, Tower of Hell");
    println!("    - Garten of Banban, Pet Simulator X, Blox Fruits");
    println!("  Currency: Robux — earned by devs from in-experience purchases, converted to USD via DevEx");
    println!("           ($1 USD = ~0.0035 USD payout per Robux to creators, simplified)");
    println!("  Premium: $4.99/$9.99/$19.99 monthly — adds Robux allowance + trading + economy access");
    println!("  Controversy: 2022+ media coverage of dev exploitation, kids gambling Robux, moderation");
    println!("  Differentiator: largest UGC creator economy in entertainment (~10M+ active creators)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "roblox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/roblox"), "roblox");
        assert_eq!(basename(r"C:\bin\roblox.exe"), "roblox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("roblox.exe"), "roblox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_rx(&["--help".to_string()], "roblox"), 0);
        assert_eq!(run_rx(&["-h".to_string()], "roblox"), 0);
        assert_eq!(run_rx(&["--version".to_string()], "roblox"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_rx(&[], "roblox"), 0);
    }
}
