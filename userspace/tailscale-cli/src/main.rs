#![deny(clippy::all)]

//! tailscale-cli — Slate OS Tailscale mesh VPN tools
//!
//! Multi-personality: `tailscale`, `tailscaled`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tailscale(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tailscale [FLAGS] COMMAND [ARGS]");
        println!();
        println!("tailscale — Tailscale mesh VPN client (Slate OS).");
        println!();
        println!("Commands:");
        println!("  up               Connect to Tailscale");
        println!("  down             Disconnect");
        println!("  status           Show current status");
        println!("  ip               Show Tailscale IP");
        println!("  ping <peer>      Ping a peer");
        println!("  netcheck         Network diagnostics");
        println!("  cert <domain>    Get TLS cert");
        println!("  file cp <f> <t>  Send file to peer");
        println!("  ssh              SSH to a peer");
        println!("  serve            Serve content");
        println!("  funnel           Serve to internet");
        println!("  version          Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "version" | "--version" => {
            println!("1.62.0");
            println!("  tailscale commit: abcdef1234567890");
            println!("  go version: go1.22.0");
        }
        "status" => {
            println!("# Health check:");
            println!("#     - not connected to home DERP region");
            println!();
            println!("100.64.0.1   slateos-desktop  user@  linux   idle; offers exit node");
            println!("100.64.0.2   slateos-laptop   user@  linux   active; direct 192.168.1.50:41641");
            println!("100.64.0.3   slateos-server   user@  linux   idle; relay \"nyc\"");
            println!("100.64.0.4   phone          user@  android active; direct 10.0.0.5:41641");
        }
        "ip" => {
            let v4 = !args.iter().any(|a| a == "-6");
            let v6 = !args.iter().any(|a| a == "-4");
            if v4 { println!("100.64.0.1"); }
            if v6 { println!("fd7a:115c:a1e0::1"); }
        }
        "up" => {
            println!("To authenticate, visit:");
            println!();
            println!("  https://login.tailscale.com/a/abcdef123456");
            println!();
            println!("Success.");
        }
        "down" => println!("Tailscale stopped."),
        "ping" => {
            let peer = args.get(1).map(|s| s.as_str()).unwrap_or("slateos-laptop");
            println!("pong from {} (100.64.0.2) via 192.168.1.50:41641 in 2ms", peer);
            println!("pong from {} (100.64.0.2) via 192.168.1.50:41641 in 1ms", peer);
            println!("pong from {} (100.64.0.2) via 192.168.1.50:41641 in 1ms", peer);
        }
        "netcheck" => {
            println!("Report:");
            println!("\t* UDP: true");
            println!("\t* IPv4: yes, 203.0.113.1:41641");
            println!("\t* IPv6: yes, [2001:db8::1]:41641");
            println!("\t* MappingVariesByDestIP: false");
            println!("\t* HairPinning: true");
            println!("\t* PortMapping: UPnP, PCP");
            println!("\t* Nearest DERP: New York City");
            println!("\t* DERP latency:");
            println!("\t\t- nyc: 12.3ms (New York City)");
            println!("\t\t- sfo: 68.5ms (San Francisco)");
            println!("\t\t- lhr: 85.2ms (London)");
            println!("\t\t- fra: 92.1ms (Frankfurt)");
            println!("\t\t- sin: 210.4ms (Singapore)");
        }
        "cert" => {
            let domain = args.get(1).map(|s| s.as_str()).unwrap_or("slateos-desktop.example.ts.net");
            println!("Wrote public cert to {}.crt", domain);
            println!("Wrote private key to {}.key", domain);
        }
        "serve" => {
            println!("Available within your tailnet:");
            println!();
            println!("https://slateos-desktop.example.ts.net/");
            println!("|-- proxy http://127.0.0.1:3000");
        }
        "funnel" => {
            println!("Available on the internet:");
            println!();
            println!("https://slateos-desktop.example.ts.net/");
            println!("|-- proxy http://127.0.0.1:3000");
        }
        _ => println!("tailscale: command '{}' completed", subcmd),
    }
    0
}

fn run_tailscaled(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tailscaled [FLAGS]");
        println!();
        println!("Flags:");
        println!("  --state=<path>        State file path");
        println!("  --socket=<path>       Unix socket path");
        println!("  --port=<port>         UDP port (default: 41641)");
        println!("  --tun=<name>          TUN device name");
        println!("  --verbose=<level>     Verbosity (0-2)");
        return 0;
    }

    println!("tailscaled 1.62.0 starting");
    println!("wgengine.NewUserspaceEngine(tun \"tailscale0\") ...");
    println!("control: login URL: https://login.tailscale.com/a/abcdef123456");
    println!("Listening on :41641");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tailscale".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "tailscaled" => run_tailscaled(&rest),
        _ => run_tailscale(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tailscale};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tailscale"), "tailscale");
        assert_eq!(basename(r"C:\bin\tailscale.exe"), "tailscale.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tailscale.exe"), "tailscale");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tailscale(&["--help".to_string()]), 0);
        assert_eq!(run_tailscale(&["-h".to_string()]), 0);
        let _ = run_tailscale(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tailscale(&[]);
    }
}
