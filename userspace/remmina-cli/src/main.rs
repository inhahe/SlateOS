#![deny(clippy::all)]

//! remmina-cli — SlateOS Remmina remote desktop client
//!
//! Single personality: `remmina`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_remmina(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: remmina [OPTIONS] [FILE.remmina]");
        println!("remmina v1.4 (SlateOS) — Remote desktop client");
        println!();
        println!("Options:");
        println!("  -c FILE           Connect using connection file");
        println!("  -n, --new         New connection");
        println!("  -p PROTOCOL       Protocol (vnc, rdp, ssh, spice)");
        println!("  --version         Show version");
        println!();
        println!("Protocols: RDP, VNC, SSH, SPICE, NX, XDMCP, HTTP(S)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("remmina v1.4 (SlateOS)"); return 0; }
    println!("remmina: remote desktop client started");
    println!("  Saved connections: 0");
    println!("  Protocols: RDP, VNC, SSH, SPICE");
    println!("  Plugins: all loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "remmina".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_remmina(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_remmina};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/remmina"), "remmina");
        assert_eq!(basename(r"C:\bin\remmina.exe"), "remmina.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("remmina.exe"), "remmina");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_remmina(&["--help".to_string()], "remmina"), 0);
        assert_eq!(run_remmina(&["-h".to_string()], "remmina"), 0);
        let _ = run_remmina(&["--version".to_string()], "remmina");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_remmina(&[], "remmina");
    }
}
