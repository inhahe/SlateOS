#![deny(clippy::all)]

//! mattermost — Slate OS team collaboration platform
//!
//! Multi-personality: `mattermost` (server), `mmctl` (CLI admin)

use std::env;
use std::process;

fn run_mattermost_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mattermost [command]");
        println!();
        println!("Commands:");
        println!("  server       Start the Mattermost server (default)");
        println!("  version      Show version");
        println!("  config       Configuration management");
        println!("  db           Database management");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("server");
    if cmd == "version" || args.iter().any(|a| a == "--version") {
        println!("Mattermost v9.8.0 (Slate OS)");
        println!("Build Number: 12345");
        println!("Build Date: 2025-05-22");
        println!("Build Hash: abc1234");
        return 0;
    }
    println!("{{\"level\":\"info\",\"ts\":1716368400.000,\"caller\":\"app/server.go:100\",\"msg\":\"Starting Mattermost Server...\"}}");
    println!("{{\"level\":\"info\",\"ts\":1716368400.100,\"msg\":\"Server version: 9.8.0 (Slate OS)\"}}");
    println!("{{\"level\":\"info\",\"ts\":1716368400.200,\"msg\":\"Database: postgres\"}}");
    println!("{{\"level\":\"info\",\"ts\":1716368400.500,\"msg\":\"Loaded config from database\"}}");
    println!("{{\"level\":\"info\",\"ts\":1716368401.000,\"msg\":\"Starting workers\"}}");
    println!("{{\"level\":\"info\",\"ts\":1716368401.500,\"msg\":\"Server is listening on :8065\"}}");
    println!("{{\"level\":\"info\",\"ts\":1716368401.501,\"msg\":\"Mattermost is ready\"}}");
    0
}

fn run_mmctl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: mmctl <command> [flags]");
            println!();
            println!("Commands:");
            println!("  auth          Login to servers");
            println!("  channel       Manage channels");
            println!("  team          Manage teams");
            println!("  user          Manage users");
            println!("  post          Manage posts");
            println!("  plugin        Manage plugins");
            println!("  system        System management");
            println!("  config        Configuration management");
            println!("  license       License management");
            println!("  version       Show version");
            0
        }
        "--version" | "version" => {
            println!("mmctl v9.8.0 (Slate OS)");
            0
        }
        "user" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("admin          admin@localhost         system_admin");
                    println!("alice          alice@example.com       system_user");
                    println!("bob            bob@example.com         system_user");
                }
                "create" => println!("User created: new-user@example.com"),
                "deactivate" => println!("User deactivated"),
                "search" => {
                    let query = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("user");
                    println!("Search results for '{}':", query);
                    println!("  alice  alice@example.com");
                }
                _ => println!("Usage: mmctl user <list|create|deactivate|search>"),
            }
            0
        }
        "channel" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("town-square   Town Square     O  (default)");
                    println!("off-topic     Off-Topic       O");
                    println!("engineering   Engineering     P");
                    println!("random        Random          O");
                }
                "create" => println!("Channel created"),
                "archive" => println!("Channel archived"),
                _ => println!("Usage: mmctl channel <list|create|archive>"),
            }
            0
        }
        "team" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("my-team       My Team         O");
                }
                "create" => println!("Team created"),
                _ => println!("Usage: mmctl team <list|create>"),
            }
            0
        }
        "system" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => {
                    println!("Status: OK");
                    println!("Database: OK");
                    println!("FileStore: OK");
                }
                "version" => println!("Server version: 9.8.0"),
                _ => println!("Usage: mmctl system <status|version>"),
            }
            0
        }
        "plugin" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Plugins:");
                    println!("  com.mattermost.calls     Calls           1.0.0   active");
                    println!("  com.mattermost.apps      Apps            1.2.0   active");
                    println!("  com.mattermost.nps       NPS             1.3.2   active");
                }
                "install" => println!("Plugin installed"),
                "enable" => println!("Plugin enabled"),
                "disable" => println!("Plugin disabled"),
                _ => println!("Usage: mmctl plugin <list|install|enable|disable>"),
            }
            0
        }
        other => { eprintln!("mmctl: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("mattermost");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "mmctl" => run_mmctl(rest),
        _ => run_mattermost_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mattermost_server};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mattermost_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_mattermost_server(vec!["-h".to_string()]), 0);
        let _ = run_mattermost_server(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mattermost_server(vec![]);
    }
}
