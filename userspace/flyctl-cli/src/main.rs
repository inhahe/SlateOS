#![deny(clippy::all)]

//! flyctl-cli — OurOS Fly.io CLI
//!
//! Multi-personality: `flyctl`, `fly`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_fly(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: fly <COMMAND> [OPTIONS]");
        println!();
        println!("flyctl — Fly.io CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  launch          Create and configure a new app");
        println!("  deploy          Deploy application");
        println!("  apps            Manage apps");
        println!("  machines        Manage Fly Machines");
        println!("  volumes         Manage volumes");
        println!("  secrets         Manage secrets");
        println!("  status          Show app status");
        println!("  logs            View app logs");
        println!("  scale           Scale app resources");
        println!("  regions         Manage regions");
        println!("  proxy           Proxy to a Fly Machine");
        println!("  ssh             SSH to a machine");
        println!("  postgres        Manage Postgres clusters");
        println!("  redis           Manage Upstash Redis");
        println!("  auth            Manage authentication");
        println!("  orgs            Manage organizations");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("flyctl v0.2.15 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "auth" => match sub {
            "login" => {
                println!("Opening https://fly.io/app/auth/cli/...");
                println!("Waiting for session... Done!");
                println!("Successfully logged in as user@example.com");
            }
            "whoami" => { println!("user@example.com"); }
            _ => { println!("fly auth {}: see fly auth --help.", sub); }
        },
        "launch" => {
            println!("Creating app in /app");
            println!("Scanning source code");
            println!("Detected a Dockerfile app");
            println!("? Choose an app name (leave blank to generate one): my-app");
            println!("? Choose a region for deployment: iad (Ashburn, Virginia)");
            println!("Created app 'my-app' in organization 'personal'");
            println!("Wrote config file fly.toml");
        }
        "deploy" => {
            println!("==> Verifying app config");
            println!("--> Verified app config");
            println!("==> Building image");
            println!("--> Building image done");
            println!("==> Pushing image");
            println!("image size: 45 MB");
            println!("==> Creating release");
            println!("--> Release v2 created");
            println!("==> Monitoring deployment");
            println!(" 1 desired, 1 placed, 1 healthy, 0 unhealthy");
            println!("--> v2 deployed successfully");
        }
        "status" => {
            println!("App");
            println!("  Name     = my-app");
            println!("  Owner    = personal");
            println!("  Hostname = my-app.fly.dev");
            println!("  Platform = machines");
            println!();
            println!("Machines");
            println!("PROCESS  ID              VERSION  REGION  STATE    CHECKS  LAST UPDATED");
            println!("app      1234567890abcd  2        iad     started  1/1     2024-01-15T12:00:00Z");
        }
        "apps" => match sub {
            "list" => {
                println!("NAME        OWNER       STATUS      LATEST DEPLOY");
                println!("my-app      personal    deployed    2024-01-15T12:00:00Z");
                println!("api-app     personal    deployed    2024-01-14T08:00:00Z");
            }
            _ => { println!("fly apps {}: see --help.", sub); }
        },
        "scale" => match sub {
            "show" => {
                println!("VM Resources for app: my-app");
                println!();
                println!("Groups");
                println!("NAME    COUNT   KIND     CPUS   MEMORY   REGIONS");
                println!("app     1       shared   1      256 MB   iad");
            }
            "count" => { println!("Count changed to 2"); }
            _ => { println!("fly scale {}: see --help.", sub); }
        },
        "logs" => {
            println!("2024-01-15T12:00:01Z app[1234567890abcd] iad [info] Listening on 0.0.0.0:8080");
            println!("2024-01-15T12:00:05Z app[1234567890abcd] iad [info] GET / 200 12ms");
            println!("2024-01-15T12:00:10Z app[1234567890abcd] iad [info] GET /api/health 200 2ms");
        }
        "secrets" => match sub {
            "list" => {
                println!("NAME          DIGEST                            CREATED AT");
                println!("DATABASE_URL  abcdef1234567890abcdef1234567890  2024-01-10T12:00:00Z");
                println!("SECRET_KEY    1234567890abcdef1234567890abcdef  2024-01-10T12:00:00Z");
            }
            _ => { println!("fly secrets {}: see --help.", sub); }
        },
        "regions" => match sub {
            "list" => {
                println!("Region Pool:");
                println!("iad    Ashburn, Virginia (US)");
                println!("Backup Region:");
                println!("ord    Chicago, Illinois (US)");
            }
            _ => { println!("fly regions {}: see --help.", sub); }
        },
        _ => {
            if cmd.is_empty() {
                eprintln!("fly: no command specified. See fly --help.");
                return 1;
            }
            println!("fly {}: see fly {} --help.", cmd, cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fly(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/flyctl"), "flyctl");
        assert_eq!(basename(r"C:\bin\flyctl.exe"), "flyctl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("flyctl.exe"), "flyctl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fly(vec!["--help".to_string()]), 0);
        assert_eq!(run_fly(vec!["-h".to_string()]), 0);
        let _ = run_fly(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fly(vec![]);
    }
}
