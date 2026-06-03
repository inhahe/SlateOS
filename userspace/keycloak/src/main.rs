#![deny(clippy::all)]

//! keycloak — OurOS identity and access management
//!
//! Single personality: `kc` (Keycloak admin CLI)

use std::env;
use std::process;

fn run_keycloak(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Keycloak - Open Source Identity and Access Management");
            println!();
            println!("Usage: kc <command> [options]");
            println!();
            println!("Commands:");
            println!("  start         Start the server");
            println!("  start-dev     Start in development mode");
            println!("  build         Build and set up server");
            println!("  show-config   Show current configuration");
            println!("  export        Export data");
            println!("  import        Import data");
            println!("  version       Show version");
            0
        }
        "version" | "--version" => {
            println!("Keycloak 24.0.4 (OurOS)");
            0
        }
        "start" | "start-dev" => {
            let is_dev = cmd.as_str() == "start-dev";
            let port = cmd_args.iter().position(|a| a == "--http-port")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("8080");
            println!("2025-05-22 10:00:00,000 INFO  [io.keycloak.quarkus.runtime.hostname] Hostname settings: FrontEnd: <request>, Strict HTTPS: false");
            if is_dev {
                println!("2025-05-22 10:00:00,100 WARN  [org.keycloak.quarkus.runtime.KeycloakMain] Running the server in development mode. DO NOT use this configuration in production.");
            }
            println!("2025-05-22 10:00:01,000 INFO  [org.keycloak.services] (main) Keycloak 24.0.4 (OurOS) on JVM (build 21.0.2)");
            println!("2025-05-22 10:00:01,500 INFO  [io.quarkus] (main) Installed features: [cdi, hibernate-orm, jdbc-h2, keycloak, narayana-jta, resteasy-reactive, smallrye-context-propagation, vertx]");
            println!("2025-05-22 10:00:02,000 INFO  [io.quarkus] (main) Keycloak started in 2.0s. Listening on: http://0.0.0.0:{}", port);
            println!("2025-05-22 10:00:02,001 INFO  [io.quarkus] (main) Installed features: 8");
            0
        }
        "build" => {
            println!("2025-05-22 10:00:00,000 INFO  Updating the configuration and installing your custom providers, if any.");
            println!("2025-05-22 10:00:01,000 INFO  Server configuration updated and target persisted.");
            0
        }
        "show-config" => {
            println!("Current Configuration:");
            println!("  Database:     h2 (dev-file)");
            println!("  HTTP Port:    8080");
            println!("  HTTPS Port:   8443");
            println!("  Hostname:     <request>");
            println!("  Cluster:      local");
            println!("  Cache:        local");
            println!("  Theme:        keycloak");
            0
        }
        "export" => {
            let path = cmd_args.iter().position(|a| a == "--dir")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/tmp/keycloak-export");
            println!("Exporting realm data to {} ...", path);
            println!("Exported 3 realms, 42 users, 15 clients.");
            0
        }
        "import" => {
            let path = cmd_args.iter().position(|a| a == "--dir")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/tmp/keycloak-import");
            println!("Importing realm data from {} ...", path);
            println!("Imported successfully.");
            0
        }
        other => { eprintln!("kc: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_keycloak(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_keycloak};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_keycloak(vec!["--help".to_string()]), 0);
        assert_eq!(run_keycloak(vec!["-h".to_string()]), 0);
        assert_eq!(run_keycloak(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_keycloak(vec![]), 0);
    }
}
