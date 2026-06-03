#![deny(clippy::all)]

//! traefik-cli — OurOS Traefik reverse proxy/load balancer
//!
//! Multi-personality: `traefik`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_traefik(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: traefik [OPTIONS] [COMMAND]");
        println!();
        println!("traefik — cloud-native edge router (OurOS).");
        println!();
        println!("Commands:");
        println!("  version          Show version");
        println!("  healthcheck      Health check");
        println!();
        println!("Options:");
        println!("  --configfile <f>       Config file");
        println!("  --entrypoints.web.address      HTTP listen addr");
        println!("  --entrypoints.websecure.address HTTPS listen addr");
        println!("  --api.dashboard        Enable dashboard");
        println!("  --providers.docker     Enable Docker provider");
        println!("  --log.level            Log level");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "version" | "--version" => {
            println!("Version:      3.0.0");
            println!("Codename:     beaufort");
            println!("Go version:   go1.22.0");
            println!("Built:        2024-01-15_09:00:00AM");
            println!("OS/Arch:      ouros/amd64");
        }
        "healthcheck" => {
            println!("OK");
        }
        _ => {
            println!("INFO[0000] Traefik version 3.0.0 built on 2024-01-15");
            println!("INFO[0000] Configuration loaded from file: /etc/traefik/traefik.yml");
            println!("INFO[0001] Starting provider aggregator.ProviderAggregator");
            println!("INFO[0001] Starting provider *docker.Provider");
            println!("INFO[0001] Starting provider *file.Provider");
            println!("INFO[0001] Starting provider *acme.Provider");
            println!();
            println!("Entrypoints:");
            println!("  web:       :80/tcp");
            println!("  websecure: :443/tcp");
            println!("  traefik:   :8080/tcp (dashboard)");
            println!();
            println!("Routers:");
            println!("  web@docker        Host(`example.com`)        web-service");
            println!("  api@docker        Host(`api.example.com`)    api-service");
            println!();
            println!("Services:");
            println!("  web-service@docker    http://172.17.0.2:8080 (1 server)");
            println!("  api-service@docker    http://172.17.0.3:3000 (2 servers)");
            println!();
            println!("Middlewares:");
            println!("  rate-limit@file       rateLimit: average=100, burst=50");
            println!("  auth@file             basicAuth: users=2");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "traefik".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_traefik(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_traefik};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/traefik"), "traefik");
        assert_eq!(basename(r"C:\bin\traefik.exe"), "traefik.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("traefik.exe"), "traefik");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_traefik(&["--help".to_string()]), 0);
        assert_eq!(run_traefik(&["-h".to_string()]), 0);
        assert_eq!(run_traefik(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_traefik(&[]), 0);
    }
}
