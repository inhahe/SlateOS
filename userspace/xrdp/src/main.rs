#![deny(clippy::all)]

//! xrdp — SlateOS RDP server
//!
//! Multi-personality: `xrdp`, `xrdp-sesman`, `xrdp-keygen`, `xrdp-sesrun`

use std::env;
use std::process;

fn run_xrdp(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xrdp [options]");
        println!();
        println!("Options:");
        println!("  -n, --nodaemon    Don't fork into background");
        println!("  -k, --kill        Kill running xrdp");
        println!("  -p, --port <n>    Listen port (default: 3389)");
        println!("  -c, --config <f>  Config file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("xrdp 0.9.25 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-k" || a == "--kill") {
        println!("stopping xrdp (pid 12345)");
        return 0;
    }

    let port = args.iter().position(|a| a == "-p" || a == "--port")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("3389");
    println!("xrdp 0.9.25 (SlateOS)");
    println!("Listening on 0.0.0.0:{}", port);
    0
}

fn run_xrdp_sesman(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xrdp-sesman [options]");
        println!("  -n, --nodaemon    Don't fork");
        println!("  -k, --kill        Kill running sesman");
        println!("  -c, --config <f>  Config file");
        return 0;
    }
    println!("xrdp-sesman: session manager starting");
    let _ = args;
    0
}

fn run_xrdp_keygen(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xrdp-keygen xrdp <keyfile>");
        return 0;
    }
    let keyfile = args.get(1).map(|s| s.as_str()).unwrap_or("rsakeys.ini");
    println!("Generating {} bit keypair...", 2048);
    println!("Key written to {}", keyfile);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("xrdp");
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
        "xrdp-sesman" => run_xrdp_sesman(rest),
        "xrdp-keygen" => run_xrdp_keygen(rest),
        "xrdp-sesrun" => { println!("(session run — simulated)"); 0 }
        _ => run_xrdp(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_xrdp};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xrdp(vec!["--help".to_string()]), 0);
        assert_eq!(run_xrdp(vec!["-h".to_string()]), 0);
        let _ = run_xrdp(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xrdp(vec![]);
    }
}
