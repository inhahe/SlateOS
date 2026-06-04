#![deny(clippy::all)]

//! webex-cli — OurOS Cisco Webex collaboration suite
//!
//! Single personality: `webex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: webex [OPTIONS]");
        println!("Cisco Webex (OurOS) — Enterprise meetings, messaging, calling");
        println!();
        println!("Options:");
        println!("  --meeting              Webex Meetings (video conference)");
        println!("  --app                  Webex App (messaging + meetings + calling)");
        println!("  --calling              Webex Calling (cloud PBX)");
        println!("  --devices              Webex Devices (Board/Desk/Room kits)");
        println!("  --plan PLAN            free/starter/business/enterprise");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cisco Webex App 44.11.0 (OurOS)"); return 0; }
    println!("Cisco Webex App 44.11.0 (OurOS)");
    println!("  Vendor: Cisco Systems (acquired Webex from founder Subrah Iyar 2007 for $3.2B)");
    println!("  Brand rebrand: Cisco Spark → Webex Teams → Webex App (2020 unified)");
    println!("  Meetings: up to 1,000 participants standard, 100K via Webex Events");
    println!("  Features: HD video, virtual backgrounds, noise cancel, transcription,");
    println!("            AI Assistant, real-time translation (100+ languages)");
    println!("  Calling: Webex Calling cloud PBX, replaces on-prem Cisco Unified CM");
    println!("  Hardware: Webex Board, Desk Pro, Room Kit, DX cameras, headsets");
    println!("  Contact Center: Webex Contact Center (cloud), CCE (on-prem)");
    println!("  Plans: Free (40min/100p), Starter ($14.50/host/mo), Business ($25),");
    println!("         Enterprise (custom, unlimited)");
    println!("  Markets: large enterprise + US government (FedRAMP, DoD IL5)");
    println!("  Strengths: enterprise security/compliance, integrated calling, hardware");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "webex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/webex"), "webex");
        assert_eq!(basename(r"C:\bin\webex.exe"), "webex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("webex.exe"), "webex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wx(&["--help".to_string()], "webex"), 0);
        assert_eq!(run_wx(&["-h".to_string()], "webex"), 0);
        let _ = run_wx(&["--version".to_string()], "webex");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wx(&[], "webex");
    }
}
