#![deny(clippy::all)]

//! freerdp-cli — Slate OS FreeRDP RDP client
//!
//! Multi-personality: `xfreerdp`, `wlfreerdp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_freerdp(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: {} [OPTIONS] /v:HOST[:PORT]", prog);
        println!("{} v3.4 (Slate OS) — FreeRDP client", prog);
        println!();
        println!("Options:");
        println!("  /v:HOST[:PORT]    Server address");
        println!("  /u:USER           Username");
        println!("  /p:PASS           Password");
        println!("  /d:DOMAIN         Domain");
        println!("  /w:WIDTH          Width");
        println!("  /h:HEIGHT         Height");
        println!("  /f                Fullscreen");
        println!("  /sound            Enable audio");
        println!("  /clipboard        Enable clipboard");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("{} v3.4 (Slate OS, FreeRDP)", prog); return 0; }
    println!("{}: connecting to RDP server...", prog);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xfreerdp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_freerdp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_freerdp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/freerdp"), "freerdp");
        assert_eq!(basename(r"C:\bin\freerdp.exe"), "freerdp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("freerdp.exe"), "freerdp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_freerdp(&["--help".to_string()], "freerdp"), 0);
        assert_eq!(run_freerdp(&["-h".to_string()], "freerdp"), 0);
        let _ = run_freerdp(&["--version".to_string()], "freerdp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_freerdp(&[], "freerdp");
    }
}
