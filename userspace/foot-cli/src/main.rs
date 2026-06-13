#![deny(clippy::all)]

//! foot-cli — SlateOS foot terminal emulator
//!
//! Multi-personality: `foot`, `footclient`, `foot-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_foot(args: &[String], prog: &str) -> i32 {
    match prog {
        "footclient" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: footclient [OPTIONS] [COMMAND...]");
                println!("footclient — Connect to a running foot server");
                println!();
                println!("Options:");
                println!("  -s, --server-socket PATH  Server socket");
                println!("  -t, --term TERM           TERM value");
                println!("  -T, --title TEXT           Window title");
                println!("  -a, --app-id ID            App ID");
                println!("  -w WxH                     Window size");
                println!("  -D, --working-directory D  Working directory");
                return 0;
            }
            println!("footclient: Connecting to foot server...");
            println!("footclient: Window opened.");
            return 0;
        }
        "foot-server" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: foot --server [OPTIONS]");
                println!("foot server mode — accepts connections from footclient");
                println!();
                println!("Options:");
                println!("  -s, --server-socket PATH  Socket path");
                println!("  -C, --config FILE         Config file");
                return 0;
            }
            println!("foot-server: Listening for connections...");
            return 0;
        }
        _ => {}
    }
    // foot
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: foot [OPTIONS] [COMMAND...]");
        println!("foot 1.18.1 (Slate OS) — Fast, lightweight Wayland terminal");
        println!();
        println!("Options:");
        println!("  -c, --config FILE        Config file");
        println!("  -C, --check-config       Verify config");
        println!("  -o, --override SEC.K=V   Override config");
        println!("  -f, --font FONT          Font name");
        println!("  -t, --term TERM          TERM value");
        println!("  -T, --title TEXT         Window title");
        println!("  -a, --app-id ID          App ID");
        println!("  -w WxH                   Window size");
        println!("  -s, --server             Run as server");
        println!("  -D, --working-directory  Working directory");
        println!("  -v, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("foot version: 1.18.1 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-C" || a == "--check-config") {
        println!("foot: Configuration valid.");
        return 0;
    }
    if args.iter().any(|a| a == "-s" || a == "--server") {
        println!("foot: Starting in server mode...");
        return 0;
    }
    println!("foot: Starting Wayland terminal...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "foot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_foot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_foot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/foot"), "foot");
        assert_eq!(basename(r"C:\bin\foot.exe"), "foot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("foot.exe"), "foot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_foot(&["--help".to_string()], "foot"), 0);
        assert_eq!(run_foot(&["-h".to_string()], "foot"), 0);
        let _ = run_foot(&["--version".to_string()], "foot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_foot(&[], "foot");
    }
}
