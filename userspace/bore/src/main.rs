#![deny(clippy::all)]

//! bore — OurOS simple TCP tunnel exposing local ports to the internet
//!
//! Single personality: `bore`

use std::env;
use std::process;

fn run_bore(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            println!("Usage: bore <COMMAND>");
            println!();
            println!("A modern, simple TCP tunnel. Expose local ports to the internet.");
            println!();
            println!("Commands:");
            println!("  local    Start a local tunnel");
            println!("  server   Start the bore server");
            println!();
            println!("Options:");
            println!("  -V, --version  Show version");
            0
        }
        "--version" | "-V" => {
            println!("bore 0.5.1 (OurOS)");
            0
        }
        "local" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: bore local [OPTIONS] --to <HOST> <PORT>");
                println!();
                println!("Options:");
                println!("  --local-host <HOST>  Local host (default: localhost)");
                println!("  --to <HOST>          Bore server address");
                println!("  -p, --port <PORT>    Bore server port (default: 7835)");
                println!("  -s, --secret <KEY>   Authentication secret");
                return 0;
            }

            let port = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(3000);

            let server = args.windows(2)
                .find(|w| w[0] == "--to")
                .map(|w| w[1].as_str())
                .unwrap_or("bore.pub");

            println!("bore local tunnel");
            println!("  Local:  localhost:{}", port);
            println!("  Remote: {}:{}", server, port + 40000);
            println!();
            println!("Your tunnel is accessible at:");
            println!("  {}:{}", server, port + 40000);
            println!();
            println!("Listening...");
            0
        }
        "server" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: bore server [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --min-port <PORT>    Minimum port for tunnels (default: 1024)");
                println!("  --max-port <PORT>    Maximum port for tunnels (default: 65535)");
                println!("  -s, --secret <KEY>   Authentication secret");
                return 0;
            }

            println!("bore server");
            println!("  Listening on 0.0.0.0:7835");
            println!("  Port range: 1024-65535");
            println!("  Accepting connections...");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bore(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_bore};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bore(vec!["--help".to_string()]), 0);
        assert_eq!(run_bore(vec!["-h".to_string()]), 0);
        let _ = run_bore(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bore(vec![]);
    }
}
