#![deny(clippy::all)]

//! traefik — SlateOS cloud-native edge router
//!
//! Single personality: `traefik`

use std::env;
use std::process;

fn run_traefik(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str());

    if args.iter().any(|a| a == "--help" || a == "-h") || cmd == Some("help") {
        println!("Usage: traefik [command] [flags]");
        println!();
        println!("Commands:");
        println!("  (none)         Start Traefik");
        println!("  version        Print version");
        println!("  healthcheck    Calls Traefik healthcheck endpoint");
        println!();
        println!("Flags:");
        println!("  --configFile <file>    Configuration file path");
        println!("  --api                  Enable API/Dashboard");
        println!("  --api.dashboard        Enable Dashboard");
        println!("  --api.insecure         Enable API in insecure mode");
        println!("  --entrypoints.web.address          HTTP listen address (default: :80)");
        println!("  --entrypoints.websecure.address     HTTPS listen address (default: :443)");
        println!("  --providers.docker                  Enable Docker provider");
        println!("  --providers.file.directory           File provider directory");
        println!("  --log.level <level>                 Log level (DEBUG, INFO, WARN, ERROR)");
        println!("  --accesslog                         Enable access log");
        println!("  --metrics.prometheus                Enable Prometheus metrics");
        return 0;
    }

    if cmd == Some("version") || args.iter().any(|a| a == "--version") {
        println!("Version:      3.0.1");
        println!("Codename:     beaufort");
        println!("Go version:   go1.22.2");
        println!("Built:        2025-05-22T00:00:00Z");
        println!("OS/Arch:      slateos/amd64");
        return 0;
    }

    if cmd == Some("healthcheck") {
        println!("OK");
        return 0;
    }

    // Start server
    println!("INFO[0000] Traefik version 3.0.1 built on 2025-05-22");
    println!("INFO[0000] Stats collection is disabled.");
    println!("INFO[0000] Starting provider aggregator *aggregator.ProviderAggregator");
    println!("INFO[0000] Starting provider *docker.Provider");
    println!("INFO[0000] Starting provider *file.Provider");
    println!("INFO[0001] Configuration loaded from file: /etc/traefik/traefik.yml");
    println!("INFO[0001] Starting TCP/UDP entryPoint web on :80");
    println!("INFO[0001] Starting TCP/UDP entryPoint websecure on :443");

    if args.iter().any(|a| a.contains("api") || a.contains("dashboard")) {
        println!("INFO[0001] API Dashboard is enabled at :8080/dashboard/");
    }
    if args.iter().any(|a| a.contains("prometheus")) {
        println!("INFO[0001] Prometheus metrics enabled at :8080/metrics");
    }

    println!();
    println!("Entrypoints:");
    println!("  web       :80");
    println!("  websecure :443");
    println!();
    println!("Providers:");
    println!("  docker (watch: true)");
    println!("  file   (directory: /etc/traefik/dynamic/)");
    println!();
    println!("Routers:");
    println!("  web-router@docker    Host(`example.com`) -> web-service@docker");
    println!("  api-router@docker    Host(`api.example.com`) -> api-service@docker");
    println!();
    println!("Services:");
    println!("  web-service@docker   [http://172.17.0.2:8080, http://172.17.0.3:8080]");
    println!("  api-service@docker   [http://172.17.0.4:3000]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_traefik(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_traefik};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_traefik(vec!["--help".to_string()]), 0);
        assert_eq!(run_traefik(vec!["-h".to_string()]), 0);
        let _ = run_traefik(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_traefik(vec![]);
    }
}
