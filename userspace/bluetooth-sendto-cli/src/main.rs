#![deny(clippy::all)]

//! bluetooth-sendto-cli — SlateOS gnome-bluetooth file sender
//!
//! Single personality: `bluetooth-sendto`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sendto(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bluetooth-sendto [OPTIONS] FILE...");
        println!("bluetooth-sendto v3.34 (Slate OS) — Send files via Bluetooth");
        println!();
        println!("Options:");
        println!("  --device MAC      Target device address");
        println!("  --name NAME       Target device name");
        return 0;
    }
    let device = args.iter().skip_while(|a| a.as_str() != "--device").nth(1)
        .map(|s| s.as_str()).unwrap_or("(select)");
    for f in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Sending {} to {}", f, device);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bluetooth-sendto".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sendto(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sendto};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bluetooth-sendto"), "bluetooth-sendto");
        assert_eq!(basename(r"C:\bin\bluetooth-sendto.exe"), "bluetooth-sendto.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bluetooth-sendto.exe"), "bluetooth-sendto");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sendto(&["--help".to_string()], "bluetooth-sendto"), 0);
        assert_eq!(run_sendto(&["-h".to_string()], "bluetooth-sendto"), 0);
        let _ = run_sendto(&["--version".to_string()], "bluetooth-sendto");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sendto(&[], "bluetooth-sendto");
    }
}
