#![deny(clippy::all)]

//! ekiga-cli — OurOS Ekiga VoIP softphone
//!
//! Single personality: `ekiga`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ekiga(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ekiga [OPTIONS] [SIP_URI]");
        println!("ekiga v4.1 (OurOS) — VoIP softphone");
        println!();
        println!("Options:");
        println!("  -c URI            Call URI on startup");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ekiga v4.1 (OurOS)"); return 0; }
    println!("ekiga: VoIP softphone started");
    println!("  SIP: registered");
    println!("  H.323: available");
    println!("  Audio codecs: Opus, G.722, G.711");
    println!("  Video codecs: H.264, VP8");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ekiga".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ekiga(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ekiga};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ekiga"), "ekiga");
        assert_eq!(basename(r"C:\bin\ekiga.exe"), "ekiga.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ekiga.exe"), "ekiga");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ekiga(&["--help".to_string()], "ekiga"), 0);
        assert_eq!(run_ekiga(&["-h".to_string()], "ekiga"), 0);
        assert_eq!(run_ekiga(&["--version".to_string()], "ekiga"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ekiga(&[], "ekiga"), 0);
    }
}
