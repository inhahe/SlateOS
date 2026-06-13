#![deny(clippy::all)]

//! heroku-cli — Slate OS Heroku CLI
//!
//! Single personality: `heroku`

use std::env;
use std::process;

fn run_heroku(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: heroku <COMMAND> [OPTIONS]");
        println!();
        println!("Heroku command-line interface (Slate OS).");
        println!();
        println!("Commands:");
        println!("  apps         Manage apps");
        println!("  addons       Manage add-ons");
        println!("  config       Manage config vars");
        println!("  ps           Manage dynos");
        println!("  releases     Manage releases");
        println!("  logs         Display logs");
        println!("  run          Run one-off processes");
        println!("  domains      Manage domains");
        println!("  certs        Manage SSL certificates");
        println!("  pg           Manage Postgres databases");
        println!("  redis        Manage Redis instances");
        println!("  pipelines    Manage pipelines");
        println!("  git          Git operations");
        println!("  container    Container operations");
        println!("  auth         Authentication");
        println!("  create       Create a new app");
        println!("  destroy      Destroy an app");
        println!("  info         Show app info");
        println!("  open         Open app in browser");
        println!("  scale        Scale dynos");
        println!("  maintenance  Manage maintenance mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("heroku/8.11.0 slateos-x64 node-v20.11.0");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let app = args.windows(2).find(|w| w[0] == "-a" || w[0] == "--app").map(|w| w[1].as_str()).unwrap_or("my-app");

    match cmd {
        "auth:login" | "login" => {
            println!("heroku: Press any key to open up the browser to login or q to exit:");
            println!("Opening browser to https://cli-auth.heroku.com/auth/cli/browser/...");
            println!("Logging in... done");
            println!("Logged in as user@example.com");
            0
        }
        "auth:whoami" | "whoami" => {
            println!("user@example.com");
            0
        }
        "apps" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "" => {
                    println!("=== user@example.com Apps");
                    println!("my-app (us)");
                    println!("my-api (eu)");
                    println!("staging-app (us)");
                }
                "info" => {
                    println!("=== {}", app);
                    println!("Dynos:         web: 1");
                    println!("Git URL:       https://git.heroku.com/{}.git", app);
                    println!("Owner:         user@example.com");
                    println!("Region:        us");
                    println!("Stack:         heroku-22");
                    println!("Web URL:       https://{}.herokuapp.com/", app);
                }
                _ => { println!("Apps operation: {}", sub); }
            }
            0
        }
        "create" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("new-app");
            println!("Creating ⬢ {}... done", name);
            println!("https://{}.herokuapp.com/ | https://git.heroku.com/{}.git", name, name);
            0
        }
        "info" => {
            println!("=== {}", app);
            println!("Dynos:         web: 1");
            println!("Git URL:       https://git.heroku.com/{}.git", app);
            println!("Owner:         user@example.com");
            println!("Region:        us");
            println!("Stack:         heroku-22");
            println!("Web URL:       https://{}.herokuapp.com/", app);
            0
        }
        "ps" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "" => {
                    println!("=== web (Standard-1X): npm start (1)");
                    println!("web.1: up 2024/01/15 14:00:00 +0000 (~ 2h ago)");
                    println!();
                    println!("=== worker (Standard-1X): node worker.js (1)");
                    println!("worker.1: up 2024/01/15 14:00:00 +0000 (~ 2h ago)");
                }
                _ => { println!("Dyno operation: {}", sub); }
            }
            0
        }
        "scale" => {
            let dyno_spec = args.get(1).map(|s| s.as_str()).unwrap_or("web=1");
            println!("Scaling dynos... done, now running {}.", dyno_spec);
            0
        }
        "logs" => {
            let tail = args.iter().any(|a| a == "--tail" || a == "-t");
            println!("2024-01-15T14:00:00.000000+00:00 heroku[web.1]: State changed from starting to up");
            println!("2024-01-15T14:00:01.000000+00:00 app[web.1]: Listening on port 3000");
            println!("2024-01-15T14:00:02.000000+00:00 heroku[router]: at=info method=GET path=\"/\" status=200 bytes=1234");
            println!("2024-01-15T14:00:03.000000+00:00 app[web.1]: GET / 200 12ms");
            if tail {
                println!("(streaming logs, press Ctrl+C to stop)");
            }
            0
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "" => {
                    println!("=== {} Config Vars", app);
                    println!("DATABASE_URL:  postgres://user:pass@host:5432/db");
                    println!("REDIS_URL:     redis://user:pass@host:6379");
                    println!("SECRET_KEY:    abc123def456");
                    println!("NODE_ENV:      production");
                }
                "set" => {
                    let kv = args.get(2).map(|s| s.as_str()).unwrap_or("KEY=value");
                    println!("Setting {}... done", kv);
                    println!("Restarting ⬢ {}... done", app);
                }
                "unset" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("KEY");
                    println!("Unsetting {}... done", key);
                    println!("Restarting ⬢ {}... done", app);
                }
                _ => { println!("Config operation: {}", sub); }
            }
            0
        }
        "addons" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "" => {
                    println!("=== Resources for {}", app);
                    println!("Plan                         Name                     Price    State");
                    println!("heroku-postgresql:essential-0 postgresql-cubed-12345   $5/mo    created");
                    println!("heroku-redis:mini             redis-cubic-67890        $3/mo    created");
                }
                "create" => {
                    let addon = args.get(2).map(|s| s.as_str()).unwrap_or("heroku-postgresql:essential-0");
                    println!("Creating {}... done", addon);
                    println!("Created postgresql-cubed-99999 as DATABASE_URL");
                }
                _ => { println!("Addons operation: {}", sub); }
            }
            0
        }
        "releases" => {
            println!("=== {} Releases - Current: v10", app);
            println!("v10  Deploy abc1234  user@example.com  2024/01/15 14:00:00 +0000");
            println!("v9   Deploy def5678  user@example.com  2024/01/14 14:00:00 +0000");
            println!("v8   Set NODE_ENV config vars  user@example.com  2024/01/13 14:00:00 +0000");
            0
        }
        "run" => {
            let command_str = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            println!("Running {} on ⬢ {}... up, run.1234 (Standard-1X)", command_str, app);
            0
        }
        "pg" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("=== DATABASE_URL");
                    println!("Plan:                  Essential 0");
                    println!("Status:                Available");
                    println!("Connections:           3/20");
                    println!("PG Version:            16.1");
                    println!("Created:               2024-01-01 00:00");
                    println!("Data Size:             15.2 MB");
                    println!("Tables:                12");
                    println!("Rows:                  5678");
                }
                "psql" => {
                    println!("--> Connecting to postgresql-cubed-12345");
                    println!("psql (16.1)");
                }
                _ => { println!("Postgres operation: {}", sub); }
            }
            0
        }
        "maintenance" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "on" => { println!("Enabling maintenance mode for ⬢ {}... done", app); }
                "off" => { println!("Disabling maintenance mode for ⬢ {}... done", app); }
                _ => { println!("Maintenance mode is off for {}.", app); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: heroku <command>. See --help.");
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
    let code = run_heroku(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_heroku};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_heroku(vec!["--help".to_string()]), 0);
        assert_eq!(run_heroku(vec!["-h".to_string()]), 0);
        let _ = run_heroku(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_heroku(vec![]);
    }
}
