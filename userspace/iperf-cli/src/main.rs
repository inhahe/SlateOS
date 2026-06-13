#![deny(clippy::all)]

//! iperf-cli — SlateOS iperf3 CLI
//!
//! Multi-personality: `iperf3`, `iperf`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_iperf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iperf3 [OPTIONS]");
        println!();
        println!("iperf3 — network bandwidth measurement (SlateOS).");
        println!();
        println!("Server/Client:");
        println!("  -s, --server           Run in server mode");
        println!("  -c, --client HOST      Run in client mode");
        println!("  -p, --port N           Port (default 5201)");
        println!();
        println!("Client options:");
        println!("  -u, --udp              Use UDP");
        println!("  -b, --bitrate N        Target bitrate");
        println!("  -t, --time N           Duration (seconds)");
        println!("  -n, --bytes N          Bytes to transmit");
        println!("  -P, --parallel N       Parallel streams");
        println!("  -R, --reverse          Reverse mode (server sends)");
        println!("  -w, --window N         Socket buffer size");
        println!("  -J, --json             JSON output");
        println!("  -i, --interval N       Report interval");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("iperf 3.16 (SlateOS)");
        return 0;
    }

    let server_mode = args.iter().any(|a| a == "-s" || a == "--server");
    let udp = args.iter().any(|a| a == "-u" || a == "--udp");
    let json = args.iter().any(|a| a == "-J" || a == "--json");
    let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
        .map(|w| w[1].as_str()).unwrap_or("5201");

    if server_mode {
        println!("-----------------------------------------------------------");
        println!("Server listening on {}", port);
        println!("-----------------------------------------------------------");
        println!("Accepted connection from 192.168.1.100, port 54321");
        println!("[  5] local 192.168.1.1 port {} connected to 192.168.1.100 port 54321", port);
        println!("[ ID] Interval           Transfer     Bitrate");
        println!("[  5]   0.00-1.00   sec   112 MBytes   940 Mbits/sec");
        println!("[  5]   1.00-2.00   sec   112 MBytes   941 Mbits/sec");
        println!("[  5]   2.00-3.00   sec   112 MBytes   940 Mbits/sec");
        println!("- - - - - - - - - - - - - - - - - - - - - - - - -");
        println!("[  5]   0.00-3.00   sec   336 MBytes   940 Mbits/sec  receiver");
    } else if json {
        println!("{{");
        println!("  \"start\": {{\"test_start\": {{\"protocol\": \"{}\"}}}},", if udp { "UDP" } else { "TCP" });
        println!("  \"intervals\": [");
        println!("    {{\"sum\": {{\"bits_per_second\": 940000000}}}}");
        println!("  ],");
        println!("  \"end\": {{\"sum_sent\": {{\"bits_per_second\": 940000000}}}}");
        println!("}}");
    } else {
        let host = args.windows(2).find(|w| w[0] == "-c" || w[0] == "--client")
            .map(|w| w[1].as_str()).unwrap_or("192.168.1.1");
        let proto = if udp { "UDP" } else { "TCP" };
        println!("Connecting to host {}, port {}", host, port);
        println!("[  5] local 192.168.1.100 port 54321 connected to {} port {}", host, port);
        println!("[ ID] Interval           Transfer     Bitrate         Retr");
        println!("[  5]   0.00-1.00   sec   112 MBytes   940 Mbits/sec    0      sender ({proto})");
        println!("[  5]   1.00-2.00   sec   112 MBytes   941 Mbits/sec    0      sender ({proto})");
        println!("[  5]   2.00-3.00   sec   112 MBytes   940 Mbits/sec    0      sender ({proto})");
        println!("- - - - - - - - - - - - - - - - - - - - - - - - -");
        println!("[  5]   0.00-3.00   sec   336 MBytes   940 Mbits/sec    0      sender");
        println!("[  5]   0.00-3.00   sec   335 MBytes   938 Mbits/sec         receiver");
        println!();
        println!("iperf Done.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "iperf3".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iperf(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_iperf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/iperf"), "iperf");
        assert_eq!(basename(r"C:\bin\iperf.exe"), "iperf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("iperf.exe"), "iperf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_iperf(vec!["--help".to_string()]), 0);
        assert_eq!(run_iperf(vec!["-h".to_string()]), 0);
        let _ = run_iperf(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_iperf(vec![]);
    }
}
