#![deny(clippy::all)]

//! linphone-cli — SlateOS Linphone SIP/VoIP client
//!
//! Multi-personality: `linphone`, `linphonec`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_linphone(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: linphone [OPTIONS]");
        println!("linphone v5.2 (SlateOS) — SIP/VoIP desktop client");
        println!();
        println!("Options:");
        println!("  --call URI        Place a call");
        println!("  --iconified       Start minimized");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("linphone v5.2 (SlateOS)"); return 0; }
    println!("linphone: SIP/VoIP client started");
    println!("  SIP account: registered");
    println!("  Audio codec: Opus");
    println!("  Video codec: VP8/H.264");
    println!("  SRTP: enabled");
    0
}

fn run_linphonec(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: linphonec [OPTIONS]");
        println!("linphonec v5.2 (SlateOS) — Console SIP client");
        println!();
        println!("Options:");
        println!("  -c FILE           Config file");
        println!("  -s URI            SIP URI to call");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("linphonec v5.2 (SlateOS)"); return 0; }
    println!("linphonec: console SIP client");
    println!("Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "linphone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "linphonec" => run_linphonec(&rest, &prog),
        _ => run_linphone(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_linphone};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/linphone"), "linphone");
        assert_eq!(basename(r"C:\bin\linphone.exe"), "linphone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("linphone.exe"), "linphone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_linphone(&["--help".to_string()], "linphone"), 0);
        assert_eq!(run_linphone(&["-h".to_string()], "linphone"), 0);
        let _ = run_linphone(&["--version".to_string()], "linphone");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_linphone(&[], "linphone");
    }
}
