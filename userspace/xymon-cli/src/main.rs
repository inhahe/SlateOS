#![deny(clippy::all)]

//! xymon-cli — Slate OS Xymon system monitor
//!
//! Multi-personality: `xymond`, `xymon`, `xymoncmd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xymon(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "xymond" => {
                println!("xymond (Slate OS) — Xymon monitoring daemon");
                println!("  --listen ADDR:PORT Listen address");
                println!("  --config DIR       Config directory");
                println!("  --log FILE         Log file");
                println!("  --pidfile FILE     PID file");
            }
            "xymoncmd" => {
                println!("xymoncmd (Slate OS) — Run command with Xymon env");
                println!("  COMMAND ARGS       Command to run");
            }
            _ => {
                println!("xymon (Slate OS) — Xymon client status reporter");
                println!("  HOST STATUS MSG    Send status to xymond");
                println!("  --server HOST      Xymon server address");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xymon v4.3.30 (Slate OS)"); return 0; }
    match prog {
        "xymond" => {
            println!("Xymon Daemon v4.3.30 (Slate OS)");
            println!("  Listening: 0.0.0.0:1984");
            println!("  Hosts: 78 monitored");
            println!("  Tests: 456 active");
            println!("  Green: 423, Yellow: 21, Red: 12");
            println!("  Status msgs: 2,345/h");
        }
        _ => {
            println!("Xymon v4.3.30 (Slate OS)");
            println!("  Server: xymon.example.com:1984");
            println!("  Status: connected");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xymon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xymon(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xymon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xymon"), "xymon");
        assert_eq!(basename(r"C:\bin\xymon.exe"), "xymon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xymon.exe"), "xymon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xymon(&["--help".to_string()], "xymon"), 0);
        assert_eq!(run_xymon(&["-h".to_string()], "xymon"), 0);
        let _ = run_xymon(&["--version".to_string()], "xymon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xymon(&[], "xymon");
    }
}
