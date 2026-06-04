#![deny(clippy::all)]

//! distcc — OurOS distributed compiler
//!
//! Multi-personality: `distcc`, `distccd`, `distccmon-text`, `pump`

use std::env;
use std::process;

fn run_distcc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: distcc [COMPILER] [COMPILER-OPTIONS] [-o OUTPUT] SOURCE");
        println!();
        println!("Options:");
        println!("  --host-list       Print host list");
        println!("  --show-hosts      Show configured hosts");
        println!("  --show-principal  Show Kerberos principal");
        println!("  -j                Suggest parallel jobs");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("distcc 3.4 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--show-hosts" || a == "--host-list") {
        println!("localhost/4");
        println!("192.168.1.10/8,lzo");
        println!("192.168.1.11/8,lzo");
        return 0;
    }
    if args.iter().any(|a| a == "-j") {
        println!("12");
        return 0;
    }

    let compiler = args.first().map(|s| s.as_str()).unwrap_or("cc");
    println!("distcc: distributing compilation of source file via {}", compiler);
    println!("(distributed compilation simulated)");
    0
}

fn run_distccd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: distccd [OPTIONS]");
        println!();
        println!("Options:");
        println!("  --daemon          Run as daemon");
        println!("  --no-detach       Don't detach from terminal");
        println!("  --log-file FILE   Log to FILE");
        println!("  --log-level LEVEL Set log verbosity");
        println!("  --listen ADDR     Listen address");
        println!("  --port PORT       Listen port (default: 3632)");
        println!("  --allow NETWORK   Allow connections from NETWORK");
        println!("  --nice LEVEL      Priority level for compilations");
        println!("  --jobs N          Maximum concurrent jobs");
        println!("  --enable-tcp-insecure  Allow non-auth TCP");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("distccd 3.4 (OurOS)");
        return 0;
    }

    let port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("3632");
    let jobs = args.iter().position(|a| a == "--jobs")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("4");
    println!("distccd: listening on port {} (max {} jobs)", port, jobs);
    0
}

fn run_distccmon_text(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: distccmon-text [INTERVAL]");
        return 0;
    }
    println!(" 24327  Compile     main.c            192.168.1.10[0]");
    println!(" 24328  Compile     utils.c           192.168.1.11[1]");
    println!(" 24329  Preprocess  config.c          localhost[0]");
    let _ = args;
    0
}

fn run_pump(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pump COMMAND [ARGS...]");
        println!();
        println!("Start distcc's \"pump\" mode for include-server-based distribution.");
        return 0;
    }

    let cmd: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    if cmd.is_empty() {
        eprintln!("pump: no command specified");
        return 1;
    }
    println!("__________Using distcc-pump from /usr/bin/distcc");
    println!("__________Using 3 distcc servers in pump mode");
    println!("__________Running: {}", cmd.join(" "));
    println!("__________Shutting down distcc-pump include server");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("distcc");
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
        "distccd" => run_distccd(rest),
        "distccmon-text" => run_distccmon_text(rest),
        "pump" => run_pump(rest),
        _ => run_distcc(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_distcc};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_distcc(vec!["--help".to_string()]), 0);
        assert_eq!(run_distcc(vec!["-h".to_string()]), 0);
        let _ = run_distcc(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_distcc(vec![]);
    }
}
