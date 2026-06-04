#![deny(clippy::all)]

//! signal-cli — OurOS Signal end-to-end encrypted messenger
//!
//! Single personality: `signal`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sig(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: signal [OPTIONS]");
        println!("Signal (OurOS) — End-to-end encrypted messenger");
        println!();
        println!("Options:");
        println!("  --chat CONTACT         Open chat");
        println!("  --call CONTACT         Voice/video call (PFS, sealed sender)");
        println!("  --story                Signal Stories (24h ephemeral)");
        println!("  --pin SET              Set Signal PIN (account recovery)");
        println!("  --note                 Note to Self");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Signal Desktop 7.36.0 (OurOS)"); return 0; }
    println!("Signal Desktop 7.36.0 (OurOS)");
    println!("  Vendor: Signal Foundation + Signal Messenger LLC (501(c)(3) nonprofit)");
    println!("  Funded: Brian Acton (WhatsApp co-founder) $50M loan Feb 2018");
    println!("  Founders: Moxie Marlinspike (Matthew Rosenfeld), Brian Acton");
    println!("  Lineage: TextSecure + RedPhone → Signal (2014)");
    println!("  Protocol: Signal Protocol — open spec, double ratchet, X3DH key agreement,");
    println!("            forward secrecy, post-compromise security, sealed sender, PQXDH (PQ)");
    println!("  Adoption: Signal Protocol licensed by WhatsApp, Google Messages, Skype, Meta");
    println!("  Features: messages, voice/video calls, group calls (40 ppl), stories,");
    println!("            disappearing messages, view-once media, link previews (opt-in)");
    println!("  Privacy: phone number historically required → usernames added 2024");
    println!("  Data minimization: only timestamp of registration + last connect on servers");
    println!("  Funding: donations only — no ads, no telemetry, no data monetization");
    println!("  Used by: journalists, activists, security professionals, EU Commission staff");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "signal".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sig(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sig};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/signal"), "signal");
        assert_eq!(basename(r"C:\bin\signal.exe"), "signal.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("signal.exe"), "signal");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sig(&["--help".to_string()], "signal"), 0);
        assert_eq!(run_sig(&["-h".to_string()], "signal"), 0);
        let _ = run_sig(&["--version".to_string()], "signal");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sig(&[], "signal");
    }
}
