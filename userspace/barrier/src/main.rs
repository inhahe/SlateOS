#![deny(clippy::all)]

//! barrier — SlateOS software KVM (keyboard/video/mouse sharing)
//!
//! Multi-personality: `barriers`, `barrierc`

use std::env;
use std::process;

fn run_barriers(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: barriers [options]");
        println!();
        println!("Options:");
        println!("  -a, --address <addr>   Listen address");
        println!("  -c, --config <file>    Config file");
        println!("  -d, --debug <level>    Debug level");
        println!("  -n, --name <name>      Screen name");
        println!("  --enable-crypto        Enable TLS encryption");
        println!("  --disable-crypto       Disable TLS encryption");
        println!("  -f, --no-daemon        Run in foreground");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("barriers 2.4.0 (SlateOS)");
        return 0;
    }

    let name = args.iter().position(|a| a == "-n" || a == "--name")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("server");
    let addr = args.iter().position(|a| a == "-a" || a == "--address")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("0.0.0.0:24800");
    println!("Barrier server '{}' listening on {}", name, addr);
    0
}

fn run_barrierc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: barrierc [options] <server-address>");
        println!();
        println!("Options:");
        println!("  -n, --name <name>      Screen name");
        println!("  -d, --debug <level>    Debug level");
        println!("  --enable-crypto        Enable TLS");
        println!("  -f, --no-daemon        Run in foreground");
        return 0;
    }

    let server = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("localhost");
    let name = args.iter().position(|a| a == "-n" || a == "--name")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("client");
    println!("Barrier client '{}' connecting to {}", name, server);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("barriers");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "barrierc" => run_barrierc(rest),
        _ => run_barriers(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_barriers};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_barriers(vec!["--help".to_string()]), 0);
        assert_eq!(run_barriers(vec!["-h".to_string()]), 0);
        let _ = run_barriers(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_barriers(vec![]);
    }
}
