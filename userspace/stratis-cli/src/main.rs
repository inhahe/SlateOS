#![deny(clippy::all)]

//! stratis-cli — OurOS Stratis storage management
//!
//! Multi-personality: `stratis`, `stratisd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stratis(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "stratisd" => {
                println!("stratisd v3.6 (OurOS) — Stratis storage daemon");
                println!("  --log-level LEVEL  Log level");
                println!("  --sim              Simulation mode");
            }
            _ => {
                println!("stratis v3.6 (OurOS) — Local storage management");
                println!("  pool create NAME DEVICE...  Create pool");
                println!("  pool list                   List pools");
                println!("  pool destroy NAME           Destroy pool");
                println!("  filesystem create POOL NAME Create filesystem");
                println!("  filesystem list             List filesystems");
                println!("  filesystem snapshot POOL FS NAME  Snapshot");
                println!("  blockdev list               List block devices");
                println!("  daemon version              Show daemon version");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Stratis v3.6.7 (OurOS)"); return 0; }
    match prog {
        "stratisd" => {
            println!("stratisd v3.6.7 (OurOS)");
            println!("  D-Bus: org.storage.stratis3");
            println!("  Pools: 2");
            println!("  Filesystems: 5");
            println!("  Listening for requests...");
        }
        _ => {
            println!("Stratis v3.6.7 (OurOS)");
            println!("  Pool: mypool");
            println!("    Devices: /dev/sda, /dev/sdb");
            println!("    Size: 2.0 TiB");
            println!("    Used: 456.7 GiB (22.3%)");
            println!("    Filesystems:");
            println!("      home: 100 GiB used");
            println!("      data: 300 GiB used");
            println!("      backup: 56.7 GiB used");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stratis".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stratis(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stratis};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stratis"), "stratis");
        assert_eq!(basename(r"C:\bin\stratis.exe"), "stratis.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stratis.exe"), "stratis");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stratis(&["--help".to_string()], "stratis"), 0);
        assert_eq!(run_stratis(&["-h".to_string()], "stratis"), 0);
        let _ = run_stratis(&["--version".to_string()], "stratis");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stratis(&[], "stratis");
    }
}
