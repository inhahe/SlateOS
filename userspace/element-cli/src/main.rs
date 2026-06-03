#![deny(clippy::all)]

//! element-cli — OurOS Element Matrix client (open federated chat)
//!
//! Single personality: `element`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_el(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: element [OPTIONS]");
        println!("Element (OurOS) — Matrix protocol client, decentralized E2EE chat");
        println!();
        println!("Options:");
        println!("  --homeserver URL       Connect to Matrix homeserver");
        println!("  --room ID              Open room (#name:server)");
        println!("  --call                 Element Call (group video, MSC3401)");
        println!("  --spaces               Spaces (group of rooms)");
        println!("  --bridge SVC           Bridge to IRC/XMPP/Slack/Discord/Telegram/etc.");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Element Desktop 1.11.86 (OurOS)"); return 0; }
    println!("Element Desktop 1.11.86 (OurOS)");
    println!("  Vendor: Element Software / New Vector Ltd (UK, founded 2017)");
    println!("  Founders: Matthew Hodgson, Amandine Le Pape");
    println!("  Protocol: Matrix (Matrix.org Foundation) — open, federated, decentralized");
    println!("  Rebrand: Riot.im → Element (Jul 2020)");
    println!("  Crypto: Olm/Megolm (double ratchet variant) end-to-end by default");
    println!("  Federation: any homeserver federates with any other (like email)");
    println!("  Homeservers: Synapse (reference, Python), Dendrite (Go), Conduit (Rust)");
    println!("  Bridging: native IRC, XMPP, Slack, Discord, Telegram, WhatsApp, Signal bridges");
    println!("  Adopted by: French government (Tchap), German military (BwMessenger),");
    println!("              Mozilla, KDE, Wikimedia, US Navy, NATO");
    println!("  Element Call: WebRTC + MatrixRTC for group video");
    println!("  Plans: free hosted on matrix.org, Element One ($5/mo), Element Server Suite");
    println!("  Strengths: self-hostable, federated, bridgeable, open spec, sovereign");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "element".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_el(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_el};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/element"), "element");
        assert_eq!(basename(r"C:\bin\element.exe"), "element.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("element.exe"), "element");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_el(&["--help".to_string()], "element"), 0);
        assert_eq!(run_el(&["-h".to_string()], "element"), 0);
        assert_eq!(run_el(&["--version".to_string()], "element"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_el(&[], "element"), 0);
    }
}
