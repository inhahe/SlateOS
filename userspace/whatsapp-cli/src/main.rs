#![deny(clippy::all)]

//! whatsapp-cli — SlateOS Meta WhatsApp messaging
//!
//! Single personality: `whatsapp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wa(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: whatsapp [OPTIONS]");
        println!("WhatsApp Messenger (SlateOS) — Meta's end-to-end encrypted messenger");
        println!();
        println!("Options:");
        println!("  --chat CONTACT         Open chat");
        println!("  --call CONTACT         Voice/video call");
        println!("  --status               WhatsApp Status (24h ephemeral)");
        println!("  --communities          Communities (group of groups)");
        println!("  --business             WhatsApp Business / Business Platform (API)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("WhatsApp Desktop 2.2448.7.0 (SlateOS)"); return 0; }
    println!("WhatsApp Desktop 2.2448.7.0 (SlateOS)");
    println!("  Owner: Meta Platforms (acquired Feb 2014 from Koum/Acton for $19B — largest of era)");
    println!("  Founded: 2009 by Jan Koum and Brian Acton (both ex-Yahoo!)");
    println!("  Crypto: Signal Protocol (Open Whisper Systems) end-to-end by default since 2016");
    println!("  Users: 2.5B+ MAU — most-used messaging app worldwide");
    println!("  Dominant in: Latin America, India, Africa, Europe, Middle East");
    println!("  Features: messages, voice notes, photo/video, location, contacts, payments,");
    println!("            voice/video calls (up to 32), Status, Communities, Channels");
    println!("  Web/Desktop: linked devices model, multi-device since 2021");
    println!("  Business: free Business app + paid Business Platform (Cloud API)");
    println!("  Payments: WhatsApp Pay in India (UPI), Brazil, Singapore");
    println!("  Privacy: 2021 ToS update controversy drove millions to Signal/Telegram");
    println!("  Acton left Meta 2017 (#DeleteFacebook), funded Signal Foundation");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "whatsapp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wa(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wa};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/whatsapp"), "whatsapp");
        assert_eq!(basename(r"C:\bin\whatsapp.exe"), "whatsapp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("whatsapp.exe"), "whatsapp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wa(&["--help".to_string()], "whatsapp"), 0);
        assert_eq!(run_wa(&["-h".to_string()], "whatsapp"), 0);
        let _ = run_wa(&["--version".to_string()], "whatsapp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wa(&[], "whatsapp");
    }
}
