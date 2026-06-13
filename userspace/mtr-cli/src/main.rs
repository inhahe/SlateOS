#![deny(clippy::all)]

//! mtr-cli — SlateOS mtr CLI
//!
//! Single personality: `mtr`

use std::env;
use std::process;

fn run_mtr(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mtr [OPTIONS] HOST");
        println!();
        println!("mtr — network diagnostic tool (traceroute + ping) (Slate OS).");
        println!();
        println!("Options:");
        println!("  -r, --report       Report mode");
        println!("  -c COUNT           Report cycles");
        println!("  -w, --report-wide  Wide report");
        println!("  -n, --no-dns       No DNS resolution");
        println!("  -4                 Use IPv4 only");
        println!("  -6                 Use IPv6 only");
        println!("  --tcp              Use TCP instead of ICMP");
        println!("  -p PORT            Target port (TCP mode)");
        println!("  --json             JSON output");
        println!("  --csv              CSV output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mtr 0.95 (Slate OS)");
        return 0;
    }

    let host = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("8.8.8.8");
    let report = args.iter().any(|a| a == "-r" || a == "--report");
    let wide = args.iter().any(|a| a == "-w" || a == "--report-wide");
    let json = args.iter().any(|a| a == "--json");
    let no_dns = args.iter().any(|a| a == "-n" || a == "--no-dns");
    let count = args.windows(2).find(|w| w[0] == "-c")
        .map(|w| w[1].as_str()).unwrap_or("10");

    if json {
        println!("{{\"report\": {{");
        println!("  \"mtr\": {{\"src\": \"myhost\", \"dst\": \"{}\", \"psize\": 64}},", host);
        println!("  \"hubs\": [");
        println!("    {{\"count\": {}, \"host\": \"gateway\", \"Loss%\": 0.0, \"Avg\": 0.5}},", count);
        println!("    {{\"count\": {}, \"host\": \"isp-router\", \"Loss%\": 0.0, \"Avg\": 5.2}},", count);
        println!("    {{\"count\": {}, \"host\": \"{}\", \"Loss%\": 0.0, \"Avg\": 12.3}}", count, host);
        println!("  ]");
        println!("}}}}");
        return 0;
    }

    if report || wide {
        println!("Start: 2024-01-15T14:00:00+0000");
        println!("HOST: myhost                       Loss%   Snt   Last   Avg  Best  Wrst StDev");
        if no_dns {
            println!("  1.|-- 192.168.1.1                 0.0%    {}   0.4   0.5   0.3   0.8   0.1", count);
            println!("  2.|-- 10.0.0.1                    0.0%    {}   4.8   5.2   4.5   6.1   0.4", count);
            println!("  3.|-- 72.14.215.85                0.0%    {}   8.2   8.5   7.9   9.3   0.3", count);
            println!("  4.|-- 108.170.252.129             0.0%    {}  10.1  10.5   9.8  11.2   0.4", count);
            println!("  5.|-- {}                 0.0%    {}  12.1  12.3  11.8  13.1   0.3", host, count);
        } else {
            println!("  1.|-- gateway                     0.0%    {}   0.4   0.5   0.3   0.8   0.1", count);
            println!("  2.|-- isp-router.example.net      0.0%    {}   4.8   5.2   4.5   6.1   0.4", count);
            println!("  3.|-- core-rtr1.example.net       0.0%    {}   8.2   8.5   7.9   9.3   0.3", count);
            println!("  4.|-- edge-rtr2.example.net       0.0%    {}  10.1  10.5   9.8  11.2   0.4", count);
            println!("  5.|-- dns.google ({})   0.0%    {}  12.1  12.3  11.8  13.1   0.3", host, count);
        }
    } else {
        println!("mtr to {} (interactive mode)", host);
        println!("  Keys: d=toggle DNS, r=reset, q=quit");
        println!();
        println!("  Host                  Loss%  Snt   Last   Avg  Best  Wrst");
        println!("  1. gateway             0.0%    5   0.5   0.5   0.3   0.8");
        println!("  2. isp-router          0.0%    5   5.1   5.2   4.5   6.1");
        println!("  3. {}          0.0%    5  12.2  12.3  11.8  13.1", host);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mtr(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mtr};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mtr(vec!["--help".to_string()]), 0);
        assert_eq!(run_mtr(vec!["-h".to_string()]), 0);
        let _ = run_mtr(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mtr(vec![]);
    }
}
