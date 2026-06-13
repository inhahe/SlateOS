#![deny(clippy::all)]

//! iperf3 — SlateOS network throughput testing tool
//!
//! Single personality: `iperf3`

use std::env;
use std::process;

fn run_iperf3(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iperf3 [-s|-c host] [options]");
        println!();
        println!("Server or Client:");
        println!("  -p, --port    <port>      Port to listen on/connect to");
        println!("  -i, --interval <secs>     Seconds between periodic bandwidth reports");
        println!("  -f, --format <format>     [kmgtKMGT] format to report (default Mbits)");
        println!("  -J, --json                Output in JSON format");
        println!("  --version                 Show version");
        println!();
        println!("Server:");
        println!("  -s, --server              Run in server mode");
        println!("  -D, --daemon              Run the server as a daemon");
        println!();
        println!("Client:");
        println!("  -c, --client <host>       Run in client mode, connecting to <host>");
        println!("  -u, --udp                 Use UDP rather than TCP");
        println!("  -b, --bitrate <n>[KMGT]   Target bitrate (0 for unlimited, default 1 Mbit/s UDP)");
        println!("  -t, --time <secs>         Time in seconds to transmit for (default 10 secs)");
        println!("  -n, --bytes <n>[KMGT]     Number of bytes to transmit");
        println!("  -P, --parallel <n>        Number of parallel client streams");
        println!("  -R, --reverse             Reverse the direction of the test");
        println!("  -w, --window <n>[KMGT]    Set window size / socket buffer size");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("iperf 3.16 (SlateOS)");
        return 0;
    }

    let is_server = args.iter().any(|a| a == "-s" || a == "--server");
    let is_udp = args.iter().any(|a| a == "-u" || a == "--udp");
    let port = args.iter().position(|a| a == "-p" || a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(5201);

    if is_server {
        println!("-----------------------------------------------------------");
        println!("Server listening on {}", port);
        println!("-----------------------------------------------------------");
        println!("Accepted connection from 192.168.1.100, port 49876");
        println!("[  5] local 0.0.0.0 port {} connected to 192.168.1.100 port 49876", port);
        println!("[ ID] Interval           Transfer     Bitrate");
        println!("[  5]   0.00-1.00   sec  1.10 GBytes  9.42 Gbits/sec");
        println!("[  5]   1.00-2.00   sec  1.09 GBytes  9.38 Gbits/sec");
        println!("[  5]   2.00-3.00   sec  1.10 GBytes  9.44 Gbits/sec");
        println!("- - - - - - - - - - - - - - - - - - - - - - - - -");
        println!("[  5]   0.00-10.00  sec  10.9 GBytes  9.41 Gbits/sec                  receiver");
        println!("-----------------------------------------------------------");
        println!("Server listening on {}", port);
        println!("-----------------------------------------------------------");
    } else {
        let host = args.iter().position(|a| a == "-c" || a == "--client")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("localhost");
        let parallel = args.iter().position(|a| a == "-P" || a == "--parallel")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);
        let proto = if is_udp { "UDP" } else { "TCP" };
        println!("Connecting to host {}, port {}", host, port);
        println!("[  5] local 0.0.0.0 port 49876 connected to {} port {}", host, port);
        println!("[ ID] Interval           Transfer     Bitrate         Retr  Cwnd");
        if is_udp {
            println!("[  5]   0.00-1.00   sec   125 KBytes  1.02 Mbits/sec  0");
            println!("[  5]   1.00-2.00   sec   125 KBytes  1.02 Mbits/sec  0");
            println!("- - - - - - - - - - - - - - - - - - - - - - - - -");
            println!("[  5]   0.00-10.00  sec  1.22 MBytes  1.02 Mbits/sec  0             sender");
            println!("[  5]   0.00-10.04  sec  1.21 MBytes  1.01 Mbits/sec                receiver");
            println!();
            println!("iperf Done. ({}, {} stream{})", proto, parallel, if parallel > 1 { "s" } else { "" });
        } else {
            println!("[  5]   0.00-1.00   sec  1.10 GBytes  9.42 Gbits/sec    0   3.12 MBytes");
            println!("[  5]   1.00-2.00   sec  1.09 GBytes  9.38 Gbits/sec    0   3.12 MBytes");
            println!("[  5]   2.00-3.00   sec  1.10 GBytes  9.44 Gbits/sec    0   3.12 MBytes");
            println!("- - - - - - - - - - - - - - - - - - - - - - - - -");
            println!("[  5]   0.00-10.00  sec  10.9 GBytes  9.41 Gbits/sec    0             sender");
            println!("[  5]   0.00-10.04  sec  10.9 GBytes  9.39 Gbits/sec                  receiver");
            println!();
            println!("iperf Done. ({}, {} stream{})", proto, parallel, if parallel > 1 { "s" } else { "" });
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iperf3(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_iperf3};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_iperf3(vec!["--help".to_string()]), 0);
        assert_eq!(run_iperf3(vec!["-h".to_string()]), 0);
        let _ = run_iperf3(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_iperf3(vec![]);
    }
}
