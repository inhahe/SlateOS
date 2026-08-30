#![deny(clippy::all)]

//! vinagre-cli — Slate OS Vinagre remote desktop viewer
//!
//! Single personality: `vinagre`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vinagre(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vinagre [OPTIONS] [URI]");
        println!("vinagre v3.22 (Slate OS) — GNOME remote desktop viewer");
        println!();
        println!("Options:");
        println!("  --new-window      Open in new window");
        println!("  --fullscreen      Start fullscreen");
        println!("  --version         Show version");
        println!();
        println!("URI: vnc://host:port, rdp://host, spice://host");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vinagre v3.22 (Slate OS)"); return 0; }
    println!("vinagre: remote desktop viewer started");
    println!("  Protocols: VNC, RDP, SPICE, SSH");
    println!("  Avahi: network discovery enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vinagre".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vinagre(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vinagre};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vinagre"), "vinagre");
        assert_eq!(basename(r"C:\bin\vinagre.exe"), "vinagre.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vinagre.exe"), "vinagre");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vinagre(&["--help".to_string()], "vinagre"), 0);
        assert_eq!(run_vinagre(&["-h".to_string()], "vinagre"), 0);
        let _ = run_vinagre(&["--version".to_string()], "vinagre");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vinagre(&[], "vinagre");
    }
}
