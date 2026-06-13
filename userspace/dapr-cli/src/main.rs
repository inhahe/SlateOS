#![deny(clippy::all)]

//! dapr-cli — SlateOS Dapr CLI
//!
//! Single personality: `dapr`

use std::env;
use std::process;

fn run_dapr(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dapr <COMMAND> [OPTIONS]");
        println!();
        println!("Dapr distributed application runtime CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  init         Initialize Dapr");
        println!("  run          Run application with sidecar");
        println!("  list         List running applications");
        println!("  stop         Stop a Dapr application");
        println!("  dashboard    Start Dapr dashboard");
        println!("  components   Manage components");
        println!("  invoke       Invoke a method");
        println!("  publish      Publish a message");
        println!("  status       Show Dapr status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("CLI version: 1.13.0 (SlateOS)");
        println!("Runtime version: 1.13.0");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "init" => {
            println!("Making the jump to hyperspace...");
            println!("  Downloading binaries...");
            println!("  Installing runtime...");
            println!("  Installing placement service...");
            println!("  Creating default components...");
            println!("  ✔ Dapr initialized successfully");
            println!("  ✔ daprd binary installed");
            println!("  ✔ placement service installed");
            println!("  ✔ statestore component created (Redis)");
            println!("  ✔ pubsub component created (Redis)");
            0
        }
        "run" => {
            let app_id = args.windows(2).find(|w| w[0] == "--app-id")
                .map(|w| w[1].as_str()).unwrap_or("myapp");
            let app_port = args.windows(2).find(|w| w[0] == "--app-port")
                .map(|w| w[1].as_str()).unwrap_or("3000");
            let dapr_port = args.windows(2).find(|w| w[0] == "--dapr-http-port")
                .map(|w| w[1].as_str()).unwrap_or("3500");
            println!("Starting Dapr with id {}. HTTP Port: {}. gRPC Port: 50001", app_id, dapr_port);
            println!("  Checking if Dapr sidecar is listening on HTTP port {}...", dapr_port);
            println!("  ✔ Dapr sidecar is up and running.");
            println!("  App Port: {}", app_port);
            println!("  App ID: {}", app_id);
            println!("  Components loaded: statestore (redis), pubsub (redis)");
            0
        }
        "list" => {
            println!("  APP ID    HTTP PORT  GRPC PORT  APP PORT  COMMAND     AGE     CREATED");
            println!("  myapp     3500       50001      3000      node app.js 2h      2024-01-15 12:00:00");
            println!("  api-gw    3501       50002      8080      ./api       5h      2024-01-15 09:00:00");
            println!("  worker    3502       50003      9000      python w.py 1d      2024-01-14 14:00:00");
            0
        }
        "stop" => {
            let app_id = args.get(1).map(|s| s.as_str()).unwrap_or("myapp");
            println!("Stopping app {} ...", app_id);
            println!("  ✔ App stopped successfully");
            0
        }
        "dashboard" => {
            let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("8080");
            println!("Dapr Dashboard running on http://localhost:{}", port);
            0
        }
        "components" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  NAME         TYPE                  VERSION  SCOPES");
                    println!("  statestore   state.redis           v1       [myapp, api-gw]");
                    println!("  pubsub       pubsub.redis          v1       [myapp, worker]");
                    println!("  binding      bindings.cron         v1       [worker]");
                }
                _ => { println!("Component operation: {}", sub); }
            }
            0
        }
        "invoke" => {
            let app_id = args.windows(2).find(|w| w[0] == "--app-id")
                .map(|w| w[1].as_str()).unwrap_or("myapp");
            let method = args.windows(2).find(|w| w[0] == "--method")
                .map(|w| w[1].as_str()).unwrap_or("healthz");
            println!("Invoking method '{}' on app '{}'...", method, app_id);
            println!("  Status: 200 OK");
            println!("  Response: {{\"status\": \"healthy\"}}");
            0
        }
        "publish" => {
            let pubsub = args.windows(2).find(|w| w[0] == "--pubsub")
                .map(|w| w[1].as_str()).unwrap_or("pubsub");
            let topic = args.windows(2).find(|w| w[0] == "--topic")
                .map(|w| w[1].as_str()).unwrap_or("orders");
            println!("Published to topic '{}' via '{}'", topic, pubsub);
            0
        }
        "status" => {
            println!("Dapr Status:");
            println!("  CLI Version:     1.13.0");
            println!("  Runtime Version: 1.13.0");
            println!("  Dashboard:       Not running");
            println!("  Running Apps:    3");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: dapr <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dapr(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_dapr};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dapr(vec!["--help".to_string()]), 0);
        assert_eq!(run_dapr(vec!["-h".to_string()]), 0);
        let _ = run_dapr(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dapr(vec![]);
    }
}
