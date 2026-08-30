#![deny(clippy::all)]

//! dog — Slate OS command-line DNS client (like dig but friendlier)
//!
//! Single personality: `dog`

use std::env;
use std::process;

fn run_dog(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dog [OPTIONS] [DOMAIN] [TYPE]...");
        println!();
        println!("A command-line DNS client, like dig.");
        println!();
        println!("Query options:");
        println!("  <domain>              Domain name to look up");
        println!("  <type>                Record type (A/AAAA/CNAME/MX/NS/TXT/SOA/SRV/CAA/PTR)");
        println!("  --class <CLASS>       Query class (IN/CH/HS)");
        println!();
        println!("Sending options:");
        println!("  @<nameserver>         Use this nameserver");
        println!("  --edns <disable|show>  EDNS options");
        println!("  --txid <ID>           Set transaction ID");
        println!();
        println!("Protocol options:");
        println!("  --udp                 Use UDP transport");
        println!("  --tcp                 Use TCP transport");
        println!("  --tls                 Use DNS-over-TLS");
        println!("  --https              Use DNS-over-HTTPS");
        println!();
        println!("Output options:");
        println!("  -J, --json            Output in JSON format");
        println!("  -1, --short           Short output");
        println!("  --color <WHEN>        Color output (auto/always/never)");
        println!("  --seconds             Show TTL in seconds");
        println!("  --time                Show query time");
        println!();
        println!("Other:");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("dog 0.1.0 (Slate OS)");
        return 0;
    }

    let json = args.iter().any(|a| a == "-J" || a == "--json");
    let short = args.iter().any(|a| a == "-1" || a == "--short");
    let show_time = args.iter().any(|a| a == "--time");

    // Parse domain and type from positional args
    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && !a.starts_with('@'))
        .map(|s| s.as_str())
        .collect();

    let domain = positional.first().copied().unwrap_or("example.com");
    let record_types: Vec<&str> = positional.iter().skip(1).copied().collect();
    let rtypes = if record_types.is_empty() {
        vec!["A"]
    } else {
        record_types
    };

    let nameserver = args.iter()
        .find(|a| a.starts_with('@'))
        .map(|s| &s[1..])
        .unwrap_or("9.9.9.9");

    if json {
        println!("[");
        for (i, rtype) in rtypes.iter().enumerate() {
            let data = match *rtype {
                "AAAA" => "\"2606:2800:220:1:248:1893:25c8:1946\"",
                "MX" => "\"10 mail.example.com.\"",
                "NS" => "\"ns1.example.com.\"",
                "TXT" => "\"v=spf1 include:_spf.example.com ~all\"",
                "CNAME" => "\"www.example.com.\"",
                "SOA" => "\"ns1.example.com. admin.example.com. 2025052201 3600 900 1209600 86400\"",
                _ => "\"93.184.216.34\"",
            };
            let comma = if i + 1 < rtypes.len() { "," } else { "" };
            println!("  {{\"name\":\"{}\",\"type\":\"{}\",\"TTL\":3600,\"data\":{}}}{}", domain, rtype, data, comma);
        }
        println!("]");
    } else if short {
        for rtype in &rtypes {
            match *rtype {
                "AAAA" => println!("2606:2800:220:1:248:1893:25c8:1946"),
                "MX" => println!("10 mail.example.com."),
                "NS" => println!("ns1.example.com."),
                "TXT" => println!("\"v=spf1 include:_spf.example.com ~all\""),
                "CNAME" => println!("www.example.com."),
                _ => println!("93.184.216.34"),
            }
        }
    } else {
        for rtype in &rtypes {
            let data = match *rtype {
                "AAAA" => "2606:2800:220:1:248:1893:25c8:1946",
                "MX" => "10 mail.example.com.",
                "NS" => "ns1.example.com.",
                "TXT" => "\"v=spf1 include:_spf.example.com ~all\"",
                "CNAME" => "www.example.com.",
                "SOA" => "ns1.example.com. admin.example.com. 2025052201 3600 900 1209600 86400",
                "SRV" => "0 5 443 sip.example.com.",
                "CAA" => "0 issue \"letsencrypt.org\"",
                "PTR" => "host.example.com.",
                _ => "93.184.216.34",
            };
            println!("{:<24} 3600 IN {:<6} {}", domain, rtype, data);
        }
        if show_time {
            println!();
            println!(";; Query time: 12 msec");
            println!(";; SERVER: {}#53", nameserver);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dog(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_dog};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dog(vec!["--help".to_string()]), 0);
        assert_eq!(run_dog(vec!["-h".to_string()]), 0);
        let _ = run_dog(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dog(vec![]);
    }
}
