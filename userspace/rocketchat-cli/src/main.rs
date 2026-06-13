#![deny(clippy::all)]

//! rocketchat-cli — SlateOS Rocket.Chat self-hosted team chat
//!
//! Single personality: `rocketchat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rocketchat [OPTIONS]");
        println!("Rocket.Chat (Slate OS) — Open-source self-hostable team chat");
        println!();
        println!("Options:");
        println!("  --server URL           Connect to Rocket.Chat workspace");
        println!("  --channel NAME         Open channel");
        println!("  --omnichannel          Omnichannel (live chat, support, WhatsApp, FB)");
        println!("  --federation           Matrix federation (since 5.0)");
        println!("  --plan PLAN            community/enterprise/government");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rocket.Chat Desktop 4.4.0 (Slate OS)"); return 0; }
    println!("Rocket.Chat Desktop 4.4.0 (Slate OS)");
    println!("  Vendor: Rocket.Chat Technologies Corp (founded 2015, Porto Alegre, Brazil)");
    println!("  License: MIT (Community), Enterprise license for advanced features");
    println!("  Stack: Meteor.js (Node.js), MongoDB, React (UI), Apollo GraphQL");
    println!("  Federation: Matrix protocol (RC 5.0+), Rocket.Chat App federation");
    println!("  Features: channels, DMs, threads, voice/video calls (Jitsi), screen share,");
    println!("            file sharing, integrations (Slack-compatible incoming/outgoing webhooks)");
    println!("  Omnichannel: live chat widget, SMS (Twilio), WhatsApp, Facebook Messenger,");
    println!("               Telegram, email — unified agent inbox");
    println!("  Hosting: self-hosted (Docker/K8s/snap), or Rocket.Chat Cloud (SaaS)");
    println!("  Plans: Community (free, self-host), Enterprise ($4/user/mo), Government FedRAMP");
    println!("  Adopted by: US Navy/Air Force, Deutsche Bahn, Credit Suisse, ANSSI");
    println!("  Strengths: data sovereignty, on-prem deployment, open source, customizable");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rocketchat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rocketchat"), "rocketchat");
        assert_eq!(basename(r"C:\bin\rocketchat.exe"), "rocketchat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rocketchat.exe"), "rocketchat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rc(&["--help".to_string()], "rocketchat"), 0);
        assert_eq!(run_rc(&["-h".to_string()], "rocketchat"), 0);
        let _ = run_rc(&["--version".to_string()], "rocketchat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rc(&[], "rocketchat");
    }
}
