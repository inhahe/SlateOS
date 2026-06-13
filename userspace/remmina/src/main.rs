#![deny(clippy::all)]

//! remmina — Slate OS remote desktop client
//!
//! Single personality: `remmina`

use std::env;
use std::process;

fn run_remmina(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: remmina [OPTIONS] [FILE]");
        println!();
        println!("Options:");
        println!("  -c, --connect=FILE     Connect to .remmina file");
        println!("  -e, --edit=FILE        Edit connection");
        println!("  -n, --new              New connection");
        println!("  --protocol=PROTO       Protocol (RDP/VNC/SSH/SFTP/SPICE)");
        println!("  --server=HOST          Server address");
        println!("  --username=USER        Username");
        println!("  --password=PASS        Password");
        println!("  --resolution=WxH       Resolution");
        println!("  --colordepth=DEPTH     Color depth (8/16/24/32)");
        println!("  -q, --quit             Quit");
        println!("  -p, --pref=TAB         Open preferences");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("remmina 1.4.35 (Slate OS)");
        println!("Protocols: RDP, VNC, SSH, SFTP, SPICE, HTTP, EXEC, NX");
        return 0;
    }

    let connect = args.iter().find_map(|a| a.strip_prefix("--connect=")
        .or_else(|| a.strip_prefix("-c")));
    let server = args.iter().find_map(|a| a.strip_prefix("--server="));
    let protocol = args.iter().find_map(|a| a.strip_prefix("--protocol="))
        .unwrap_or("RDP");

    if let Some(file) = connect {
        println!("Connecting via {} profile: {}", protocol, file);
    } else if let Some(host) = server {
        println!("Connecting to {} via {} ...", host, protocol);
    } else {
        println!("Remmina Remote Desktop Client 1.4.35 (Slate OS)");
        println!("(GUI launched — simulated)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_remmina(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_remmina};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_remmina(vec!["--help".to_string()]), 0);
        assert_eq!(run_remmina(vec!["-h".to_string()]), 0);
        let _ = run_remmina(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_remmina(vec![]);
    }
}
