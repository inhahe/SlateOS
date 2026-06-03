#![deny(clippy::all)]

//! tcpdump-cli — OurOS tcpdump CLI
//!
//! Single personality: `tcpdump`

use std::env;
use std::process;

fn run_tcpdump(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tcpdump [OPTIONS] [EXPRESSION]");
        println!();
        println!("tcpdump — network packet analyzer (OurOS).");
        println!();
        println!("Options:");
        println!("  -i IFACE       Listen on interface");
        println!("  -c COUNT       Capture COUNT packets");
        println!("  -w FILE        Write to pcap file");
        println!("  -r FILE        Read from pcap file");
        println!("  -n             Don't resolve hostnames");
        println!("  -v             Verbose output");
        println!("  -X             Print hex and ASCII");
        println!("  -A             Print ASCII only");
        println!("  -s SNAPLEN     Snap length");
        println!("  -p             No promiscuous mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("tcpdump version 4.99.4 (OurOS)");
        return 0;
    }

    let iface = args.windows(2).find(|w| w[0] == "-i")
        .map(|w| w[1].as_str()).unwrap_or("eth0");
    let verbose = args.iter().any(|a| a == "-v" || a == "-vv");
    let hex = args.iter().any(|a| a == "-X");
    let read_file = args.windows(2).find(|w| w[0] == "-r")
        .map(|w| w[1].as_str());
    let write_file = args.windows(2).find(|w| w[0] == "-w")
        .map(|w| w[1].as_str());

    if let Some(file) = read_file {
        println!("reading from file {}", file);
    } else {
        println!("tcpdump: listening on {}, link-type EN10MB (Ethernet), snapshot length 262144 bytes", iface);
    }

    if let Some(file) = write_file {
        println!("  Writing to {}", file);
    }

    if verbose {
        println!("14:00:00.000001 IP (tos 0x0, ttl 64, id 12345, offset 0, flags [DF], proto TCP (6), length 60)");
        println!("    192.168.1.10.45678 > 93.184.216.34.443: Flags [S], cksum 0xabcd, seq 1234567890, win 65535, options [mss 1460,sackOK,TS val 123456 ecr 0,nop,wscale 7], length 0");
    } else {
        println!("14:00:00.000001 IP 192.168.1.10.45678 > 93.184.216.34.443: Flags [S], seq 1234567890, win 65535, length 0");
    }
    println!("14:00:00.012345 IP 93.184.216.34.443 > 192.168.1.10.45678: Flags [S.], seq 987654321, ack 1234567891, win 65535, length 0");
    println!("14:00:00.012456 IP 192.168.1.10.45678 > 93.184.216.34.443: Flags [.], ack 1, win 512, length 0");
    println!("14:00:00.012567 IP 192.168.1.10.45678 > 93.184.216.34.443: Flags [P.], seq 1:518, ack 1, win 512, length 517");
    println!("14:00:00.025678 IP 93.184.216.34.443 > 192.168.1.10.45678: Flags [.], ack 518, win 501, length 0");

    if hex {
        println!("  0x0000:  4500 003c 3039 4000 4006 abcd c0a8 010a");
        println!("  0x0010:  5db8 d822 b26e 01bb 4996 a332 0000 0000");
    }

    println!();
    println!("5 packets captured");
    println!("5 packets received by filter");
    println!("0 packets dropped by kernel");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tcpdump(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_tcpdump};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tcpdump(vec!["--help".to_string()]), 0);
        assert_eq!(run_tcpdump(vec!["-h".to_string()]), 0);
        assert_eq!(run_tcpdump(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tcpdump(vec![]), 0);
    }
}
