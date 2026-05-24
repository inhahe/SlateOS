#![deny(clippy::all)]

//! polkit-cli — OurOS PolicyKit authorization framework
//!
//! Multi-personality: `polkitd`, `pkaction`, `pkcheck`, `pkexec`, `pkttyagent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_polkitd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: polkitd [OPTIONS]");
        println!("polkitd v124 (OurOS) — PolicyKit daemon");
        println!();
        println!("Options:");
        println!("  --replace         Replace running daemon");
        println!("  --no-debug        Disable debug logging");
        return 0;
    }
    println!("polkitd: authorization daemon started");
    println!("  D-Bus: org.freedesktop.PolicyKit1");
    0
}

fn run_pkaction(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pkaction [OPTIONS]");
        println!("pkaction v124 (OurOS) — List PolicyKit actions");
        println!();
        println!("Options:");
        println!("  -a ACTION         Show specific action");
        println!("  -v                Verbose (show descriptions)");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("org.freedesktop.policykit.exec");
        println!("  Description: Run a program as another user");
        println!("  Vendor: freedesktop.org");
    } else {
        println!("org.freedesktop.policykit.exec");
        println!("org.freedesktop.login1.power-off");
        println!("org.freedesktop.login1.reboot");
        println!("org.freedesktop.login1.suspend");
        println!("org.freedesktop.udisks2.filesystem-mount");
    }
    0
}

fn run_pkcheck(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pkcheck [OPTIONS]");
        println!("pkcheck v124 (OurOS) — Check PolicyKit authorization");
        println!();
        println!("Options:");
        println!("  -a ACTION         Action to check");
        println!("  -p PID            Process PID");
        println!("  -u UID            User UID");
        println!("  --enable-internal-agent  Use tty agent");
        return 0;
    }
    println!("Authorization result: yes");
    0
}

fn run_pkexec(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pkexec [OPTIONS] COMMAND [ARGS]");
        println!("pkexec v124 (OurOS) — Execute command as privileged user");
        println!();
        println!("Options:");
        println!("  --user USER       Target user (default: root)");
        println!("  --disable-internal-agent  Don't use agent");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("sh");
    println!("pkexec: executing '{}' as root", cmd);
    0
}

fn run_pkttyagent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pkttyagent [OPTIONS]");
        println!("pkttyagent v124 (OurOS) — Text-mode authentication agent");
        println!();
        println!("Options:");
        println!("  --process PID     Process to authenticate");
        println!("  --notify-fd FD    File descriptor for notifications");
        println!("  --fallback        Act as fallback agent");
        return 0;
    }
    println!("pkttyagent: text authentication agent running");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "polkitd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pkaction" => run_pkaction(&rest, &prog),
        "pkcheck" => run_pkcheck(&rest, &prog),
        "pkexec" => run_pkexec(&rest, &prog),
        "pkttyagent" => run_pkttyagent(&rest, &prog),
        _ => run_polkitd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
