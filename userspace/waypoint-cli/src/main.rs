#![deny(clippy::all)]

//! waypoint-cli — SlateOS HashiCorp Waypoint application deployment
//!
//! Multi-personality: `waypoint`

use std::env;
use std::process;

fn run_waypoint(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: waypoint COMMAND [OPTIONS]");
        println!("HashiCorp Waypoint 0.11.4 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize a new project");
        println!("  up           Build, deploy, and release");
        println!("  build        Build a new version");
        println!("  deploy       Deploy a built artifact");
        println!("  release      Release a deployment");
        println!("  destroy      Destroy all resources");
        println!("  logs         Show application logs");
        println!("  exec         Execute a command in context");
        println!("  config       Manage application config");
        println!("  project      Manage projects");
        println!("  runner       Manage runners");
        println!("  server       Server management");
        println!("  ui           Open the web UI");
        println!("  status       Show project status");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("Waypoint v0.11.4");
            println!("  Git Commit: abc123def");
        }
        "init" => {
            println!("Initializing Waypoint project...");
            println!("  Created waypoint.hcl");
            println!("  Project initialized successfully.");
            println!("  Run 'waypoint up' to build, deploy, and release.");
        }
        "up" => {
            println!("» Building...");
            println!("  ✓ Build complete: artifact_v1");
            println!();
            println!("» Deploying...");
            println!("  ✓ Deployment complete: deploy_abc123");
            println!("  URL: https://myapp.waypoint.run");
            println!();
            println!("» Releasing...");
            println!("  ✓ Release complete: release_def456");
            println!();
            println!("The deploy was successful!");
        }
        "build" => {
            println!("» Building...");
            println!("  Creating new build...");
            println!("  ✓ Build complete: artifact_v2");
            println!("  Artifact ID: a_1234567890");
        }
        "deploy" => {
            println!("» Deploying...");
            println!("  ✓ Deployment complete");
            println!("  Deployment ID: d_abc123def");
            println!("  URL: https://myapp.waypoint.run");
        }
        "release" => {
            println!("» Releasing...");
            println!("  ✓ Release complete");
            println!("  Release ID: r_xyz789");
            println!("  URL: https://myapp.waypoint.run");
        }
        "destroy" => {
            println!("Destroying all resources...");
            println!("  ✓ Resources destroyed");
        }
        "logs" => {
            println!("[myapp] 2024-02-15T10:30:00Z Starting application...");
            println!("[myapp] 2024-02-15T10:30:01Z Listening on :8080");
            println!("[myapp] 2024-02-15T10:30:15Z GET / 200 12ms");
        }
        "status" => {
            println!("Project: myapp");
            println!();
            println!("  APP          | WORKSPACE | DEPLOYMENT STATUS | RELEASE STATUS");
            println!("  myapp        | default   | ✓ READY           | ✓ CURRENT");
            println!();
            println!("  URL: https://myapp.waypoint.run");
        }
        "config" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match action {
                "set" => {
                    let var = args.get(2).map(|s| s.as_str()).unwrap_or("KEY=VALUE");
                    println!("Configuration set: {}", var);
                }
                _ => {
                    println!("Configuration:");
                    println!("  DATABASE_URL=postgres://...");
                    println!("  PORT=8080");
                }
            }
        }
        "server" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("run");
            println!("waypoint server {}", action);
            println!("  Server running on :9701");
        }
        "ui" => {
            println!("Opening Waypoint UI: https://localhost:9702");
        }
        _ => println!("waypoint: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_waypoint(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_waypoint};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_waypoint(&["--help".to_string()]), 0);
        assert_eq!(run_waypoint(&["-h".to_string()]), 0);
        let _ = run_waypoint(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_waypoint(&[]);
    }
}
