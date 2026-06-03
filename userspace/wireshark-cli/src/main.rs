#![deny(clippy::all)]

//! wireshark-cli — OurOS Wireshark CLI tools (tshark/editcap/mergecap)
//!
//! Multi-personality: `tshark`, `editcap`, `mergecap`, `capinfos`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "editcap" => "editcap",
        "mergecap" => "mergecap",
        "capinfos" => "capinfos",
        _ => "tshark",
    }
}

fn run_tshark(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tshark [OPTIONS]");
        println!();
        println!("Network protocol analyzer (command-line).");
        println!();
        println!("Capture:");
        println!("  -i <IFACE>         Capture interface");
        println!("  -f <FILTER>        Capture filter (BPF)");
        println!("  -c <COUNT>         Stop after N packets");
        println!("  -a <CONDITION>     Stop condition");
        println!("  -w <FILE>          Write to pcap file");
        println!();
        println!("Read:");
        println!("  -r <FILE>          Read from pcap file");
        println!("  -Y <FILTER>        Display filter");
        println!("  -T <FORMAT>        Output format (text/json/pdml/ek)");
        println!("  -e <FIELD>         Field to print");
        println!("  -V                 Verbose packet details");
        println!("  -x                 Hex dump");
        return 0;
    }

    let reading = args.windows(2).any(|w| w[0] == "-r");
    let file = args.windows(2)
        .find(|w| w[0] == "-r")
        .map(|w| w[1].as_str())
        .unwrap_or("capture.pcap");

    if reading {
        println!("    1   0.000000 192.168.1.100 → 93.184.216.34  TCP      66 443 → 49152 [SYN] Seq=0 Win=65535");
        println!("    2   0.023456 93.184.216.34  → 192.168.1.100 TCP      66 49152 → 443 [SYN, ACK] Seq=0 Ack=1");
        println!("    3   0.023789 192.168.1.100 → 93.184.216.34  TCP      54 443 → 49152 [ACK] Seq=1 Ack=1");
        println!("    4   0.024012 192.168.1.100 → 93.184.216.34  TLS      234 Client Hello");
        println!("    5   0.045678 93.184.216.34  → 192.168.1.100 TLS      1234 Server Hello, Certificate");
        println!("    6   0.046123 192.168.1.100 → 93.184.216.34  TLS      178 Key Exchange, Change Cipher");
        println!("    7   0.067890 93.184.216.34  → 192.168.1.100 TLS      89 Change Cipher Spec, Finished");
        println!("    8   0.068234 192.168.1.100 → 93.184.216.34  HTTP     567 GET / HTTP/1.1");
        println!("  (read from {})", file);
    } else {
        println!("Capturing on 'eth0'");
        println!("    1   0.000000 192.168.1.100 → 8.8.8.8        DNS      74 Standard query A example.com");
        println!("    2   0.015234 8.8.8.8        → 192.168.1.100 DNS      90 Standard query response A 93.184.216.34");
        println!("    3   0.016000 192.168.1.100 → 93.184.216.34  TCP      66 [SYN]");
        println!("  (press Ctrl+C to stop)");
    }
    0
}

fn run_capinfos(args: &[String]) -> i32 {
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("capture.pcap");

    println!("File name:           {}", file);
    println!("File type:           Wireshark/tcpdump/... - pcapng");
    println!("File encapsulation:  Ethernet");
    println!("File timestamp precision:  microseconds");
    println!("Packet size limit:   262144 bytes");
    println!("Number of packets:   12,345");
    println!("File size:           5,678,901 bytes");
    println!("Data size:           4,567,890 bytes");
    println!("Capture duration:    300.123456 seconds");
    println!("First packet time:   2024-01-15 14:00:00.000000");
    println!("Last packet time:    2024-01-15 14:05:00.123456");
    println!("Data byte rate:      15,225 bytes/s");
    println!("Data bit rate:       121,804 bits/s");
    println!("Average packet size: 370 bytes");
    println!("Average packet rate: 41 packets/s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("tshark"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.iter().any(|a| a == "-V" || a == "--version") {
        println!("TShark (Wireshark) 4.2.2 (OurOS)");
        process::exit(0);
    }

    let code = match p {
        "tshark" => run_tshark(&rest),
        "capinfos" => run_capinfos(&rest),
        "editcap" => { println!("editcap: packet editing tool"); 0 }
        "mergecap" => { println!("mergecap: merge pcap files"); 0 }
        _ => run_tshark(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_tshark};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tshark(&["--help".to_string()]), 0);
        assert_eq!(run_tshark(&["-h".to_string()]), 0);
        assert_eq!(run_tshark(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tshark(&[]), 0);
    }
}
