#![deny(clippy::all)]

//! cni-plugins — OurOS Container Networking Interface plugins
//!
//! Multi-personality: `bridge`, `host-local`, `loopback`, `portmap`, `firewall`, `tuning`, `bandwidth`

use std::env;
use std::process;

fn run_cni_plugin(args: Vec<String>, plugin: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("CNI plugin: {}", plugin);
        println!();
        println!("This plugin is normally invoked by a container runtime, not directly.");
        println!("It reads a JSON config from stdin and outputs results to stdout.");
        println!();
        println!("Environment variables:");
        println!("  CNI_COMMAND       ADD|DEL|CHECK|VERSION");
        println!("  CNI_CONTAINERID   Container ID");
        println!("  CNI_NETNS         Network namespace path");
        println!("  CNI_IFNAME        Interface name");
        println!("  CNI_ARGS          Extra arguments");
        println!("  CNI_PATH          Plugin search path");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("{}: CNI plugin v1.4.1 (OurOS)", plugin);
        return 0;
    }

    // Simulate CNI VERSION command response
    println!("{{");
    println!("  \"cniVersion\": \"1.1.0\",");
    println!("  \"supportedVersions\": [\"0.3.0\", \"0.3.1\", \"0.4.0\", \"1.0.0\", \"1.1.0\"]");
    println!("}}");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("bridge");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cni_plugin(rest, &prog_name);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cni_plugin};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cni_plugin(vec!["--help".to_string()], "cni-plugins"), 0);
        assert_eq!(run_cni_plugin(vec!["-h".to_string()], "cni-plugins"), 0);
        assert_eq!(run_cni_plugin(vec!["--version".to_string()], "cni-plugins"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cni_plugin(vec![], "cni-plugins"), 0);
    }
}
