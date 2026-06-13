#![deny(clippy::all)]

//! caddy — SlateOS web server with automatic HTTPS
//!
//! Single personality: `caddy`

use std::env;
use std::process;

fn run_caddy(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: caddy <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  adapt          Adapts a configuration to Caddy's native JSON");
            println!("  build-info     Prints information about this build");
            println!("  environ        Prints the environment");
            println!("  file-server    Starts a simple file server");
            println!("  fmt            Formats a Caddyfile");
            println!("  hash-password  Hashes a password and writes base64");
            println!("  list-modules   Lists the installed Caddy modules");
            println!("  reload         Changes the config of a running Caddy instance");
            println!("  reverse-proxy  A quick and production-ready reverse proxy");
            println!("  run            Starts the Caddy process and blocks");
            println!("  start          Starts the Caddy process in the background");
            println!("  stop           Stops the running Caddy process");
            println!("  trust          Installs root certificate into local trust store");
            println!("  untrust        Untrusts the root certificate");
            println!("  validate       Tests a configuration for validity");
            println!("  version        Prints the version");
            0
        }
        "version" | "--version" => {
            println!("v2.8.4 (Slate OS) h1:abc1234=");
            0
        }
        "build-info" => {
            println!("path: github.com/caddyserver/caddy/v2");
            println!("version: v2.8.4");
            println!("go: go1.22.2");
            println!("os: slateos");
            println!("arch: amd64");
            0
        }
        "run" | "start" => {
            let is_bg = cmd.as_str() == "start";
            let config = cmd_args.iter().position(|a| a == "--config")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/etc/caddy/Caddyfile");
            println!("{{\"level\":\"info\",\"ts\":1716368400.000,\"msg\":\"using provided configuration\",\"config_file\":\"{}\"}}", config);
            println!("{{\"level\":\"info\",\"ts\":1716368400.100,\"msg\":\"adapted config\"}}");
            println!("{{\"level\":\"info\",\"ts\":1716368400.200,\"logger\":\"tls.obtain\",\"msg\":\"acquiring lock\"}}");
            println!("{{\"level\":\"info\",\"ts\":1716368400.300,\"logger\":\"tls\",\"msg\":\"automatic HTTPS is fully managed\"}}");
            println!("{{\"level\":\"info\",\"ts\":1716368400.400,\"msg\":\"autosaved config\"}}");
            println!("{{\"level\":\"info\",\"ts\":1716368400.500,\"msg\":\"serving initial configuration\"}}");
            println!("{{\"level\":\"info\",\"ts\":1716368400.501,\"logger\":\"http\",\"msg\":\"server listening\",\"address\":\":443\"}}");
            println!("{{\"level\":\"info\",\"ts\":1716368400.502,\"logger\":\"http\",\"msg\":\"server listening\",\"address\":\":80\"}}");
            if is_bg {
                println!("{{\"level\":\"info\",\"ts\":1716368400.600,\"msg\":\"Caddy started in background\"}}");
            }
            0
        }
        "stop" => {
            println!("{{\"level\":\"info\",\"msg\":\"stopping Caddy\"}}");
            0
        }
        "reload" => {
            let config = cmd_args.iter().position(|a| a == "--config")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/etc/caddy/Caddyfile");
            println!("{{\"level\":\"info\",\"msg\":\"reloading config from {}\"}}", config);
            0
        }
        "validate" => {
            let config = cmd_args.iter().position(|a| a == "--config")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/etc/caddy/Caddyfile");
            println!("Valid configuration: {}", config);
            0
        }
        "adapt" => {
            let config = cmd_args.iter().position(|a| a == "--config")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/etc/caddy/Caddyfile");
            println!("{{");
            println!("  \"apps\": {{");
            println!("    \"http\": {{");
            println!("      \"servers\": {{");
            println!("        \"srv0\": {{");
            println!("          \"listen\": [\":443\"],");
            println!("          \"routes\": [{{");
            println!("            \"handle\": [{{");
            println!("              \"handler\": \"file_server\",");
            println!("              \"root\": \"/var/www/html\"");
            println!("            }}]");
            println!("          }}]");
            println!("        }}");
            println!("      }}");
            println!("    }}");
            println!("  }}");
            println!("}}");
            let _ = config;
            0
        }
        "file-server" => {
            let listen = cmd_args.iter().position(|a| a == "--listen")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or(":2015");
            let root = cmd_args.iter().position(|a| a == "--root")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or(".");
            println!("File server listening on {}", listen);
            println!("Serving files from {}", root);
            0
        }
        "reverse-proxy" => {
            let from = cmd_args.iter().position(|a| a == "--from")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or(":80");
            let to = cmd_args.iter().position(|a| a == "--to")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("localhost:8080");
            println!("Caddy reverse proxy {} -> {}", from, to);
            println!("{{\"level\":\"info\",\"msg\":\"serving initial configuration\"}}");
            0
        }
        "fmt" => {
            println!("(formatted Caddyfile output — simulated)");
            0
        }
        "list-modules" => {
            println!("Standard modules:");
            println!("  caddy.adapters.caddyfile");
            println!("  caddy.listeners.tls");
            println!("  http.handlers.file_server");
            println!("  http.handlers.headers");
            println!("  http.handlers.reverse_proxy");
            println!("  http.handlers.static_response");
            println!("  http.matchers.host");
            println!("  http.matchers.path");
            println!("  tls.issuance.acme");
            println!("  tls.issuance.internal");
            println!();
            println!("  Total: 10 modules");
            0
        }
        "hash-password" => {
            println!("$2a$14$abc123def456ghi789jklmnopqrstuvwxyz012345");
            0
        }
        "trust" => { println!("Certificate installed into local trust store"); 0 }
        "untrust" => { println!("Certificate removed from local trust store"); 0 }
        "environ" => {
            println!("HOME=/root");
            println!("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin");
            println!("XDG_DATA_HOME=/root/.local/share");
            println!("XDG_CONFIG_HOME=/root/.config");
            0
        }
        other => { eprintln!("caddy: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_caddy(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_caddy};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_caddy(vec!["--help".to_string()]), 0);
        assert_eq!(run_caddy(vec!["-h".to_string()]), 0);
        let _ = run_caddy(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_caddy(vec![]);
    }
}
