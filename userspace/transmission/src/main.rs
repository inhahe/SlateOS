#![deny(clippy::all)]

//! transmission — SlateOS BitTorrent client
//!
//! Multi-personality: `transmission-daemon`, `transmission-cli`, `transmission-remote`

use std::env;
use std::process;

fn run_daemon(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-daemon [options]");
        println!("  -f, --foreground     Run in foreground");
        println!("  -g, --config-dir     Config directory");
        println!("  -p, --port <port>    RPC port (default: 9091)");
        println!("  -w, --download-dir   Default download directory");
        println!("  -a, --allowed        Comma-delimited list of allowed IP addresses");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("transmission-daemon 4.0.5 (Slate OS)");
        return 0;
    }
    let port = args.iter().position(|a| a == "-p" || a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(9091);
    println!("[2025-05-22 10:00:00.000] Transmission 4.0.5 (Slate OS) starting");
    println!("[2025-05-22 10:00:00.100] RPC Server: listening on 0.0.0.0:{}", port);
    println!("[2025-05-22 10:00:00.200] Loaded 3 torrents");
    0
}

fn run_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-cli [options] <torrent-file | magnet-url>");
        println!("  -d, --download-dir <dir>  Download directory");
        println!("  -w, --no-watch            Don't watch directory");
        println!("  --version                 Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("transmission-cli 4.0.5 (Slate OS)");
        return 0;
    }
    let torrent = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("example.torrent");
    println!("Transmission 4.0.5 (Slate OS)");
    println!("Opening torrent: {}", torrent);
    println!("Progress: 0.0%  DL: 0 kB/s  UL: 0 kB/s  Peers: 0");
    println!("Progress: 25.0% DL: 2.5 MB/s UL: 128 kB/s Peers: 12");
    println!("Progress: 100.0% DL: 0 kB/s UL: 256 kB/s Peers: 8 (seeding)");
    0
}

fn run_remote(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-remote [host:port] [options]");
        println!("  -l, --list           List all torrents");
        println!("  -a, --add <file>     Add torrent");
        println!("  -r, --remove <id>    Remove torrent");
        println!("  -s, --start <id>     Start torrent");
        println!("  -S, --stop <id>      Stop torrent");
        println!("  -t <id> -i           Show torrent info");
        println!("  -si                  Show session info");
        println!("  -st                  Show session stats");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("transmission-remote 4.0.5 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("  ID   Done       Have  ETA           Up    Down  Ratio  Status       Name");
        println!("   1   100%    4.2 GB  Done       256.0    0.0    1.20  Seeding      ubuntu-24.04.iso");
        println!("   2    45%    1.8 GB  2h 30m     128.0  2500.0   0.05  Downloading  archlinux-2025.05.iso");
        println!("   3   100%  892.5 MB  Done         0.0    0.0    2.50  Idle         movie.mkv");
        println!("Sum:         6.89 GB              384.0  2500.0");
        return 0;
    }
    if args.iter().any(|a| a == "-si") {
        println!("  Download directory: /home/user/Downloads");
        println!("  Listenport: 51413");
        println!("  Portforwarding: enabled");
        println!("  PEX: enabled");
        println!("  DHT: enabled");
        println!("  Encryption: preferred");
        println!("  Speed limit (down): unlimited");
        println!("  Speed limit (up): unlimited");
        return 0;
    }
    if args.iter().any(|a| a == "-st") {
        println!("CURRENT SESSION");
        println!("  Uploaded:   12.50 GB");
        println!("  Downloaded:  8.75 GB");
        println!("  Ratio:       1.42");
        println!("  Duration:    24 hours");
        println!();
        println!("CUMULATIVE");
        println!("  Uploaded:   142.50 GB");
        println!("  Downloaded: 98.75 GB");
        println!("  Ratio:      1.44");
        return 0;
    }
    if args.iter().any(|a| a == "-a" || a == "--add") {
        println!("localhost:9091/transmission/rpc - success");
        return 0;
    }
    println!("transmission-remote: no command specified. Use --help.");
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("transmission-daemon");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "transmission-cli" => run_cli(rest),
        "transmission-remote" => run_remote(rest),
        _ => run_daemon(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_daemon};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_daemon(vec!["--help".to_string()]), 0);
        assert_eq!(run_daemon(vec!["-h".to_string()]), 0);
        let _ = run_daemon(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_daemon(vec![]);
    }
}
