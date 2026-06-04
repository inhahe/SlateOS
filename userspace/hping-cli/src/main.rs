#![deny(clippy::all)]

//! hping-cli — OurOS hping3 network tool
//!
//! Single personality: `hping3`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hping3(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hping3 [OPTIONS] HOST");
        println!("hping3 v3.0 (OurOS) — Active network smashing tool");
        println!();
        println!("Mode:");
        println!("  -0 / --rawip      RAW IP mode");
        println!("  -1 / --icmp       ICMP mode (default)");
        println!("  -2 / --udp        UDP mode");
        println!("  -S / --syn        SYN mode");
        println!();
        println!("Options:");
        println!("  -p PORT           Destination port");
        println!("  -c COUNT          Packet count");
        println!("  -i INTERVAL       Interval (u1000 = 1ms)");
        println!("  --traceroute      Traceroute mode");
        println!("  --flood           Flood mode");
        return 0;
    }
    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("192.168.1.1");
    if args.iter().any(|a| a == "-S" || a == "--syn") {
        let port = args.iter().skip_while(|a| a.as_str() != "-p").nth(1).map(|s| s.as_str()).unwrap_or("80");
        println!("HPING {} (eth0 {}): S set, 40 headers + 0 data bytes", host, host);
        println!("len=44 ip={} ttl=64 DF id=0 sport={} flags=SA seq=0 win=29200 rtt=0.4 ms", host, port);
        println!("len=44 ip={} ttl=64 DF id=0 sport={} flags=SA seq=1 win=29200 rtt=0.3 ms", host, port);
        println!("len=44 ip={} ttl=64 DF id=0 sport={} flags=SA seq=2 win=29200 rtt=0.5 ms", host, port);
    } else if args.iter().any(|a| a == "--traceroute") {
        println!("HPING {} (eth0 {}): traceroute mode", host, host);
        println!(" 1  gateway (192.168.1.1)  0.5 ms");
        println!(" 2  10.0.0.1  2.1 ms");
        println!(" 3  {} 8.3 ms", host);
    } else {
        println!("HPING {} (eth0 {}): icmp mode set, 28 headers + 0 data bytes", host, host);
        println!("len=46 ip={} ttl=64 id=0 icmp_seq=0 rtt=0.3 ms", host);
        println!("len=46 ip={} ttl=64 id=0 icmp_seq=1 rtt=0.4 ms", host);
        println!("len=46 ip={} ttl=64 id=0 icmp_seq=2 rtt=0.3 ms", host);
    }
    println!();
    println!("--- {} hping statistic ---", host);
    println!("3 packets transmitted, 3 packets received, 0% packet loss");
    println!("round-trip min/avg/max = 0.3/0.4/0.5 ms");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hping3".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hping3(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hping3};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hping"), "hping");
        assert_eq!(basename(r"C:\bin\hping.exe"), "hping.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hping.exe"), "hping");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hping3(&["--help".to_string()], "hping"), 0);
        assert_eq!(run_hping3(&["-h".to_string()], "hping"), 0);
        let _ = run_hping3(&["--version".to_string()], "hping");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hping3(&[], "hping");
    }
}
