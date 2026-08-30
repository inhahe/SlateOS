#![deny(clippy::all)]

//! brave-cli — Slate OS Brave Browser (privacy + crypto)
//!
//! Single personality: `brave`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_br(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: brave [URL] [OPTIONS]");
        println!("Brave Browser (Slate OS) — Privacy-first Chromium browser with crypto/BAT rewards");
        println!();
        println!("Options:");
        println!("  --tor                  Private window with Tor routing");
        println!("  --shields              Brave Shields (ad/tracker blocking)");
        println!("  --leo                  Leo AI assistant");
        println!("  --rewards              Brave Rewards (BAT tokens for viewing ads)");
        println!("  --wallet               Brave Wallet (built-in crypto wallet)");
        println!("  --search               Brave Search (independent index, no Google/Bing)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Brave 1.73.91 (Slate OS)"); return 0; }
    println!("Brave 1.73.91 Chromium: 131.0.6778.86 (Slate OS)");
    println!("  Vendor: Brave Software, Inc. (San Francisco, founded 2015)");
    println!("  Founders: Brendan Eich (creator of JavaScript, ex-Mozilla CEO), Brian Bondy");
    println!("  Engine: Blink (Chromium fork), with Google services stripped");
    println!("  Privacy by default: ad blocking, tracker blocking, fingerprint randomization,");
    println!("                      HTTPS Everywhere, no third-party cookies, no telemetry");
    println!("  Brave Shields: per-site control of blocking aggressiveness");
    println!("  BAT: Basic Attention Token (ERC-20) — earn by viewing privacy-respecting ads,");
    println!("       tip creators, swap to USD via Uphold/Gemini");
    println!("  Tor: built-in 'Private Window with Tor' (not full Tor Browser security model)");
    println!("  Brave Search: own index built from Cliqz/Tailcat (~30B pages), AI summaries");
    println!("  Talk: built-in Jitsi-based video calls");
    println!("  Leo: in-browser AI (Claude/Llama/Mistral options), Premium $14.99/mo");
    println!("  Wallet: native Eth/Sol/BTC wallet, no extension needed");
    println!("  Controversies: 2020 affiliate-URL injection (resolved), Eich's 2014 Prop 8 donation");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "brave".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_br(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_br};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/brave"), "brave");
        assert_eq!(basename(r"C:\bin\brave.exe"), "brave.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("brave.exe"), "brave");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_br(&["--help".to_string()], "brave"), 0);
        assert_eq!(run_br(&["-h".to_string()], "brave"), 0);
        let _ = run_br(&["--version".to_string()], "brave");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_br(&[], "brave");
    }
}
