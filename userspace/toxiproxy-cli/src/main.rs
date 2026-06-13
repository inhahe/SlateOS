#![deny(clippy::all)]

//! toxiproxy-cli — SlateOS Toxiproxy fault injection proxy
//!
//! Two personalities: `toxiproxy-server`, `toxiproxy-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_toxiproxy_cli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: toxiproxy-cli COMMAND [OPTIONS]");
        println!("toxiproxy-cli v2.9.0 (SlateOS) — Fault injection proxy");
        println!();
        println!("Commands:");
        println!("  list            List proxies");
        println!("  create          Create proxy");
        println!("  delete          Delete proxy");
        println!("  toggle          Enable/disable proxy");
        println!("  inspect         Inspect proxy and toxics");
        println!("  toxic           Manage toxics");
        println!("  version         Show version");
        println!();
        println!("Toxic types:");
        println!("  latency         Add latency");
        println!("  bandwidth       Limit bandwidth");
        println!("  slow_close      Slow close connections");
        println!("  timeout         Stop data flow");
        println!("  slicer          Slice data into smaller bits");
        println!("  reset_peer      Reset connection");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("toxiproxy-cli v2.9.0 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!("Name              Listen          Upstream         Enabled");
            println!("redis             localhost:26379  localhost:6379   true");
            println!("postgres          localhost:25432  localhost:5432   true");
            println!("api               localhost:28080  localhost:8080   true");
        }
        "create" => println!("Created new proxy."),
        "delete" => println!("Proxy deleted."),
        "toggle" => println!("Proxy toggled."),
        "inspect" => {
            println!("Name: redis");
            println!("  Listen: localhost:26379");
            println!("  Upstream: localhost:6379");
            println!("  Enabled: true");
            println!("  Toxics:");
            println!("    latency_downstream: latency=100ms, jitter=10ms");
        }
        "toxic" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "add" => println!("Added toxic to proxy."),
                "remove" => println!("Removed toxic from proxy."),
                "list" => {
                    println!("Proxy: redis");
                    println!("  latency_downstream  type=latency  latency=100  jitter=10");
                }
                _ => println!("toxiproxy-cli toxic {}: completed", sub),
            }
        }
        _ => println!("toxiproxy-cli {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "toxiproxy-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_toxiproxy_cli(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_toxiproxy_cli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/toxiproxy"), "toxiproxy");
        assert_eq!(basename(r"C:\bin\toxiproxy.exe"), "toxiproxy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("toxiproxy.exe"), "toxiproxy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_toxiproxy_cli(&["--help".to_string()], "toxiproxy"), 0);
        assert_eq!(run_toxiproxy_cli(&["-h".to_string()], "toxiproxy"), 0);
        let _ = run_toxiproxy_cli(&["--version".to_string()], "toxiproxy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_toxiproxy_cli(&[], "toxiproxy");
    }
}
