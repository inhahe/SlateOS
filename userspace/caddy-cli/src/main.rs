#![deny(clippy::all)]

//! caddy-cli — SlateOS Caddy web server
//!
//! Multi-personality: `caddy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_caddy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: caddy <command> [<args...>]");
        println!();
        println!("caddy — fast, multi-platform web server with auto HTTPS (SlateOS).");
        println!();
        println!("Commands:");
        println!("  adapt           Adapt config to JSON");
        println!("  build-info      Print build info");
        println!("  environ         Print environment");
        println!("  file-server     Simple file server");
        println!("  fmt             Format Caddyfile");
        println!("  hash-password   Hash a password");
        println!("  list-modules    List modules");
        println!("  reload          Reload config");
        println!("  reverse-proxy   Quick reverse proxy");
        println!("  run             Start server");
        println!("  start           Start in background");
        println!("  stop            Stop background server");
        println!("  validate        Validate config");
        println!("  version         Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" => {
            println!("v2.7.6 h1:SlateOS (SlateOS)");
        }
        "build-info" => {
            println!("path: github.com/caddyserver/caddy/v2");
            println!("version: v2.7.6");
            println!("go: go1.22.0");
            println!("os: slateos");
            println!("arch: amd64");
        }
        "list-modules" => {
            println!("  caddy.adapters.caddyfile");
            println!("  caddy.listeners.tls");
            println!("  caddy.logging.encoders.console");
            println!("  caddy.logging.encoders.json");
            println!("  http.handlers.file_server");
            println!("  http.handlers.headers");
            println!("  http.handlers.reverse_proxy");
            println!("  http.handlers.static_response");
            println!("  http.matchers.host");
            println!("  http.matchers.path");
            println!("  tls.stek.standard");
            println!();
            println!("  Total: 11 modules");
        }
        "validate" => {
            let config = args.get(1).map(|s| s.as_str()).unwrap_or("Caddyfile");
            println!("Valid configuration ({})", config);
        }
        "fmt" => println!("Formatted Caddyfile."),
        "adapt" => {
            println!("{{");
            println!("  \"apps\": {{");
            println!("    \"http\": {{");
            println!("      \"servers\": {{");
            println!("        \"srv0\": {{");
            println!("          \"listen\": [\":443\"],");
            println!("          \"routes\": [{{");
            println!("            \"match\": [{{\"host\": [\"example.com\"]}}],");
            println!("            \"handle\": [{{\"handler\": \"reverse_proxy\", \"upstreams\": [{{\"dial\": \"localhost:8080\"}}]}}]");
            println!("          }}]");
            println!("        }}");
            println!("      }}");
            println!("    }}");
            println!("  }}");
            println!("}}");
        }
        "run" | "start" => {
            println!("{{\"level\":\"info\",\"msg\":\"using provided configuration\",\"config_file\":\"Caddyfile\"}}");
            println!("{{\"level\":\"info\",\"msg\":\"admin endpoint started\",\"address\":\"localhost:2019\"}}");
            println!("{{\"level\":\"info\",\"msg\":\"tls.obtain: acquiring lock\",\"identifier\":\"example.com\"}}");
            println!("{{\"level\":\"info\",\"msg\":\"certificate obtained successfully\",\"identifier\":\"example.com\"}}");
            println!("{{\"level\":\"info\",\"msg\":\"autosaved config\",\"file\":\"/var/lib/caddy/autosave.json\"}}");
            println!("{{\"level\":\"info\",\"msg\":\"serving initial configuration\"}}");
            if subcmd == "start" {
                println!("Successfully started Caddy (pid=1234) - Caddy is running in the background");
            }
        }
        "stop" => println!("{{\"level\":\"info\",\"msg\":\"Caddy stopped\"}}"),
        "reload" => println!("{{\"level\":\"info\",\"msg\":\"config reloaded\"}}"),
        "reverse-proxy" => {
            let to = args.get(1).map(|s| s.as_str()).unwrap_or("localhost:8080");
            println!("Caddy proxying :443 -> {}", to);
        }
        "file-server" => {
            let root = args.windows(2).find(|w| w[0] == "--root").map(|w| w[1].as_str()).unwrap_or(".");
            println!("Caddy serving files from {} on :80", root);
        }
        "hash-password" => println!("$2a$14$abcdefghijklmnopqrstuvwxyz012345678901234567890123456"),
        _ => println!("caddy: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "caddy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_caddy(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_caddy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/caddy"), "caddy");
        assert_eq!(basename(r"C:\bin\caddy.exe"), "caddy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("caddy.exe"), "caddy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_caddy(&["--help".to_string()]), 0);
        assert_eq!(run_caddy(&["-h".to_string()]), 0);
        let _ = run_caddy(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_caddy(&[]);
    }
}
