#![deny(clippy::all)]

//! flyctl — OurOS Fly.io CLI
//!
//! Single personality: `fly` (or `flyctl`)

use std::env;
use std::process;

fn run_fly(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fly <COMMAND> [OPTIONS]");
        println!();
        println!("The Fly.io command-line interface (OurOS).");
        println!();
        println!("Commands:");
        println!("  apps         Manage apps");
        println!("  machine      Manage Fly Machines");
        println!("  deploy       Deploy an app");
        println!("  status       Show app status");
        println!("  logs         View app logs");
        println!("  secrets      Manage app secrets");
        println!("  volumes      Manage volumes");
        println!("  regions      List regions");
        println!("  scale        Scale app resources");
        println!("  ssh          SSH into a machine");
        println!("  proxy        Proxy to a machine");
        println!("  postgres     Manage Postgres clusters");
        println!("  redis        Manage Upstash Redis");
        println!("  tokens       Manage tokens");
        println!("  auth         Authentication");
        println!("  launch       Create and configure a new app");
        println!("  version      Show version");
        println!();
        println!("Flags:");
        println!("  -a, --app <APP>     App name");
        println!("  --json              Output as JSON");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("flyctl v0.2.20 OurOS (2024-01-15)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "auth" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "login" => {
                    println!("Opening https://fly.io/app/auth/cli/...");
                    println!("Waiting for session...");
                    println!("Successfully logged in as user@example.com");
                }
                "whoami" => {
                    println!("user@example.com");
                }
                "token" => {
                    println!("FlyV1 fm1_abc123...");
                }
                _ => {
                    eprintln!("Usage: fly auth <login|whoami|token|signup>. See --help.");
                    return 1;
                }
            }
            0
        }
        "apps" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("NAME              ORG              STATUS          PLATFORM");
                    println!("my-web-app        personal         deployed        machines");
                    println!("my-api            personal         deployed        machines");
                    println!("my-worker         personal         suspended       machines");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-app");
                    println!("New app created: {}", name);
                    println!("  Hostname = {}.fly.dev", name);
                }
                _ => { println!("Apps operation: {}", sub); }
            }
            0
        }
        "status" => {
            let app = args.windows(2).find(|w| w[0] == "-a" || w[0] == "--app").map(|w| w[1].as_str()).unwrap_or("my-web-app");
            println!("App");
            println!("  Name     = {}", app);
            println!("  Owner    = personal");
            println!("  Hostname = {}.fly.dev", app);
            println!("  Platform = machines");
            println!();
            println!("Machines");
            println!("PROCESS  ID              VERSION  REGION  STATE    CHECKS  LAST UPDATED");
            println!("app      178140abc123    5        iad     started  1/1     2024-01-15T14:00:00Z");
            println!("app      278140def456    5        ord     started  1/1     2024-01-15T14:00:00Z");
            0
        }
        "deploy" => {
            let app = args.windows(2).find(|w| w[0] == "-a" || w[0] == "--app").map(|w| w[1].as_str()).unwrap_or("my-web-app");
            println!("==> Verifying app config");
            println!("--> Verified app config");
            println!("==> Building image");
            println!("--> Building image done");
            println!("==> Pushing image to fly");
            println!("--> Pushing image done");
            println!("image: registry.fly.io/{}:deployment-1705320000", app);
            println!("image size: 45 MB");
            println!("==> Creating release");
            println!("--> release v5 created");
            println!("==> Monitoring deployment");
            println!("--> v5 deployed successfully");
            0
        }
        "logs" => {
            let app = args.windows(2).find(|w| w[0] == "-a" || w[0] == "--app").map(|w| w[1].as_str()).unwrap_or("my-web-app");
            println!("2024-01-15T14:00:00Z app[178140abc123] iad [info]  Listening on 0.0.0.0:8080");
            println!("2024-01-15T14:00:01Z app[178140abc123] iad [info]  GET / 200 12ms");
            println!("2024-01-15T14:00:02Z app[278140def456] ord [info]  GET /api/health 200 2ms");
            println!("(from {})", app);
            0
        }
        "secrets" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("NAME          DIGEST                          CREATED AT");
                    println!("DATABASE_URL  abc123def456                    2024-01-10T00:00:00Z");
                    println!("SECRET_KEY    789012ghi345                    2024-01-10T00:00:00Z");
                }
                "set" => {
                    println!("Secrets are staged for the first deployment.");
                    println!("Release v6 created.");
                }
                _ => { println!("Secrets operation: {}", sub); }
            }
            0
        }
        "scale" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "show" => {
                    println!("VM Resources for my-web-app:");
                    println!("  Groups:");
                    println!("    app:");
                    println!("      Count: 2");
                    println!("      CPUs:  1 (shared)");
                    println!("      Memory: 256 MB");
                }
                "count" => {
                    let count = args.get(2).map(|s| s.as_str()).unwrap_or("2");
                    println!("App scaled to {} machines.", count);
                }
                "vm" => {
                    let size = args.get(2).map(|s| s.as_str()).unwrap_or("shared-cpu-1x");
                    println!("Scaled VM size to {}.", size);
                }
                _ => { println!("Scale operation: {}", sub); }
            }
            0
        }
        "regions" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("CODE  NAME                           GATEWAY");
                println!("ams   Amsterdam, Netherlands         ✓");
                println!("cdg   Paris, France                  ✓");
                println!("dfw   Dallas, Texas (US)");
                println!("iad   Ashburn, Virginia (US)         ✓");
                println!("lax   Los Angeles, California (US)");
                println!("nrt   Tokyo, Japan                   ✓");
                println!("ord   Chicago, Illinois (US)         ✓");
                println!("sin   Singapore                      ✓");
                println!("syd   Sydney, Australia              ✓");
            }
            0
        }
        "launch" => {
            println!("Creating app in /app...");
            println!("Scanning source code");
            println!("Detected a Dockerfile app");
            println!("? Choose an app name (leave blank to generate one): my-app");
            println!("? Choose a region for deployment: iad (Ashburn, Virginia (US))");
            println!("Created app 'my-app' in organization 'personal'");
            println!("Wrote config file fly.toml");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: fly <command>. See --help.");
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
    let code = run_fly(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_fly};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fly(vec!["--help".to_string()]), 0);
        assert_eq!(run_fly(vec!["-h".to_string()]), 0);
        assert_eq!(run_fly(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fly(vec![]), 0);
    }
}
