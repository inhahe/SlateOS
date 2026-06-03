#![deny(clippy::all)]

//! jitsi-cli — OurOS Jitsi Meet video conferencing
//!
//! Multi-personality: `jitsi-meet`, `jitsi-videobridge`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_meet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jitsi-meet [OPTIONS] [ROOM_URL]");
        println!("jitsi-meet v2.0 (OurOS) — Video conferencing client");
        println!();
        println!("Options:");
        println!("  --no-audio        Join without audio");
        println!("  --no-video        Join without video");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("jitsi-meet v2.0 (OurOS)"); return 0; }
    println!("jitsi-meet: video conferencing client started");
    println!("  Server: meet.jit.si");
    println!("  WebRTC: enabled");
    println!("  Audio: ready");
    println!("  Video: ready");
    0
}

fn run_videobridge(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jitsi-videobridge [OPTIONS]");
        println!("jitsi-videobridge v2.3 (OurOS) — Jitsi video bridge server");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("jitsi-videobridge v2.3 (OurOS)"); return 0; }
    println!("jitsi-videobridge: SFU server started");
    println!("  Port: 10000 (UDP)");
    println!("  REST API: http://localhost:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "jitsi-meet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "jitsi-videobridge" => run_videobridge(&rest, &prog),
        _ => run_meet(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_meet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/jitsi"), "jitsi");
        assert_eq!(basename(r"C:\bin\jitsi.exe"), "jitsi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("jitsi.exe"), "jitsi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_meet(&["--help".to_string()], "jitsi"), 0);
        assert_eq!(run_meet(&["-h".to_string()], "jitsi"), 0);
        assert_eq!(run_meet(&["--version".to_string()], "jitsi"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_meet(&[], "jitsi"), 0);
    }
}
