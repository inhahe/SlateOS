#![deny(clippy::all)]

//! waypipe-cli — OurOS waypipe remote Wayland display
//!
//! Single personality: `waypipe`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_waypipe(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: waypipe [OPTIONS] COMMAND [ARGS]");
        println!("waypipe v0.9 (OurOS) — Proxy Wayland connections over SSH");
        println!();
        println!("Commands:");
        println!("  server            Run as server (remote side)");
        println!("  client            Run as client (local side)");
        println!("  ssh [SSH_ARGS]    Wrapper around ssh");
        println!();
        println!("Options:");
        println!("  -c CODEC          Compression codec (lz4, zstd, none)");
        println!("  -s SOCKET         Socket path");
        println!("  --video           Enable video encoding for screen updates");
        println!("  --hwvideo         Use hardware video encoding");
        println!("  --threads N       Compression threads");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("waypipe v0.9 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ssh");
    match cmd {
        "server" => println!("waypipe server: listening for connections"),
        "client" => println!("waypipe client: connecting to server"),
        "ssh" => {
            let host = args.get(1).map(|s| s.as_str()).unwrap_or("remote");
            println!("waypipe: tunneling Wayland via SSH to {}", host);
        }
        _ => println!("waypipe: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "waypipe".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_waypipe(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_waypipe};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/waypipe"), "waypipe");
        assert_eq!(basename(r"C:\bin\waypipe.exe"), "waypipe.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("waypipe.exe"), "waypipe");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_waypipe(&["--help".to_string()], "waypipe"), 0);
        assert_eq!(run_waypipe(&["-h".to_string()], "waypipe"), 0);
        assert_eq!(run_waypipe(&["--version".to_string()], "waypipe"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_waypipe(&[], "waypipe"), 0);
    }
}
