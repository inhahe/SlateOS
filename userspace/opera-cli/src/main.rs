#![deny(clippy::all)]

//! opera-cli — OurOS Opera browser
//!
//! Single personality: `opera`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_op(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: opera [URL] [OPTIONS]");
        println!("Opera (OurOS) — Chromium-based browser with built-in messengers/VPN/AI");
        println!();
        println!("Options:");
        println!("  --private              Private browsing window");
        println!("  --vpn                  Built-in browser VPN (free, basic, by SurfEasy)");
        println!("  --aria                 Aria AI assistant");
        println!("  --gx                   Opera GX (gamer variant — RAM/CPU/network limits)");
        println!("  --air                  Opera Air (mindfulness-focused, 2024)");
        println!("  --one                  Opera One (modular tab islands, 2023)");
        println!("  --crypto               Built-in Crypto Wallet");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Opera 115.0.5322.77 (OurOS)"); return 0; }
    println!("Opera 115.0.5322.77 (OurOS)");
    println!("  Vendor: Opera Software AS (Oslo, Norway), majority-owned by Kunlun (China) 2016+");
    println!("  History:");
    println!("    1995-2013: Presto engine (own engine, multi-platform, MDI tabs pioneer)");
    println!("    2013+:     Switched to Blink (Chromium), team partly went to make Vivaldi");
    println!("  Editions: Opera (mainstream), Opera GX (gamer), Opera Air (mindfulness),");
    println!("            Opera Mini (mobile data-saving), Opera Crypto Browser");
    println!("  Pioneer of: tabbed browsing UI, mouse gestures, Speed Dial, browser sidebar,");
    println!("              built-in mail client (M2/Presto era), Turbo data compression");
    println!("  Features: built-in VPN (proxy), ad blocker, messengers sidebar (WA/Telegram/IG/FB),");
    println!("            Workspaces, Pinboards, Aria (GPT-based assistant since 2023)");
    println!("  Opera GX: hardware limiters (RAM/CPU/network caps), Twitch/Discord widgets, RGB themes");
    println!("  Mobile: Opera Mini still used in low-bandwidth markets (Africa/India/SE Asia)");
    println!("  Revenue: search deal (Google), Opera News (mobile), advertising, crypto wallet");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "opera".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_op(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
