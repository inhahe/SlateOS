#![deny(clippy::all)]

//! wl-protocols-cli — Slate OS Wayland protocol info tool
//!
//! Single personality: `wl-protocols`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_protocols(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wl-protocols COMMAND [OPTIONS]");
        println!("wl-protocols v1.36 (Slate OS) — Wayland protocol information");
        println!();
        println!("Commands:");
        println!("  list              List installed protocols");
        println!("  info PROTOCOL     Show protocol details");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!("Stable protocols:");
            println!("  xdg-shell, presentation-time, viewporter");
            println!("Staging protocols:");
            println!("  xdg-activation-v1, content-type-v1, fractional-scale-v1");
            println!("  cursor-shape-v1, ext-idle-notify-v1");
            println!("Unstable protocols:");
            println!("  xdg-decoration-unstable-v1, text-input-unstable-v3");
        }
        "info" => {
            let proto = args.get(1).map(|s| s.as_str()).unwrap_or("xdg-shell");
            println!("Protocol: {}", proto);
            println!("  Status: stable");
            println!("  Version: 5");
        }
        "version" => println!("wayland-protocols v1.36 (Slate OS)"),
        _ => println!("wl-protocols: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-protocols".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_protocols(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wl_protocols};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wl-protocols"), "wl-protocols");
        assert_eq!(basename(r"C:\bin\wl-protocols.exe"), "wl-protocols.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wl-protocols.exe"), "wl-protocols");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wl_protocols(&["--help".to_string()], "wl-protocols"), 0);
        assert_eq!(run_wl_protocols(&["-h".to_string()], "wl-protocols"), 0);
        let _ = run_wl_protocols(&["--version".to_string()], "wl-protocols");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wl_protocols(&[], "wl-protocols");
    }
}
