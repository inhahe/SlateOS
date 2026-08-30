#![deny(clippy::all)]

//! skype-cli — Slate OS Microsoft Skype consumer VoIP
//!
//! Single personality: `skype`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: skype [OPTIONS]");
        println!("Skype (Slate OS) — Microsoft Skype consumer VoIP / video calls");
        println!();
        println!("Options:");
        println!("  --call CONTACT         Place audio/video call");
        println!("  --chat                 Skype chat");
        println!("  --credit               Skype Credit (call landlines/mobiles)");
        println!("  --skype-number         Get Skype Number (incoming PSTN)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Skype 8.130.0.207 (Slate OS)"); return 0; }
    println!("Skype 8.130.0.207 (Slate OS)");
    println!("  Owner: Microsoft (acquired May 2011 from eBay+silver lake/canada pension for $8.5B)");
    println!("  Founders: Niklas Zennstrom, Janus Friis (Estonian devs Ahti Heinla et al)");
    println!("  Founded: 2003 in Tallinn/Luxembourg — pioneered consumer P2P VoIP");
    println!("  Original P2P architecture replaced with cloud (Microsoft Azure) in 2014-17");
    println!("  Acquisition path: eBay 2005 ($2.6B) → private 2009 → Microsoft 2011");
    println!("  Features: audio/video calls (up to 100), screen sharing, chat, file transfer,");
    println!("            live captions, background blur, Together Mode, Meet Now");
    println!("  Subs: Skype to Phone (call PSTN), Skype Number (incoming PSTN), $0-15/mo");
    println!("  Decline: Microsoft shifted enterprise to Teams (2017), consumer focus on Teams free");
    println!("  Skype for Business: discontinued Jul 2021 (online), Jul 2025 on-prem (Teams replaces)");
    println!("  Mobile: iOS, Android, web (web.skype.com)");
    println!("  Reputation: declining mindshare but still has 40M+ MAU mostly in Europe/Asia");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "skype".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/skype"), "skype");
        assert_eq!(basename(r"C:\bin\skype.exe"), "skype.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("skype.exe"), "skype");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sk(&["--help".to_string()], "skype"), 0);
        assert_eq!(run_sk(&["-h".to_string()], "skype"), 0);
        let _ = run_sk(&["--version".to_string()], "skype");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sk(&[], "skype");
    }
}
