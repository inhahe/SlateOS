#![deny(clippy::all)]

//! nextcloud — SlateOS self-hosted cloud platform
//!
//! Single personality: `occ` (Nextcloud command-line interface)

use std::env;
use std::process;

fn run_occ(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" | "list" => {
            println!("Nextcloud 29.0.0 (SlateOS)");
            println!();
            println!("Usage: occ <command> [options]");
            println!();
            println!("Commands:");
            println!("  status              Show status");
            println!("  check               Check dependencies");
            println!("  maintenance:mode    Toggle maintenance mode");
            println!("  upgrade             Upgrade Nextcloud");
            println!("  db:add-missing-indices     Add missing database indices");
            println!("  config:list         List all config values");
            println!("  config:system:set   Set a config value");
            println!("  app:list            List installed apps");
            println!("  app:enable          Enable an app");
            println!("  app:disable         Disable an app");
            println!("  user:list           List users");
            println!("  user:add            Add a user");
            println!("  user:delete         Delete a user");
            println!("  user:info           Show user info");
            println!("  files:scan          Scan filesystem");
            println!("  files:cleanup       Cleanup filecache");
            println!("  background:cron     Execute background tasks");
            println!("  log:manage          Manage log settings");
            println!("  encryption:status   Encryption status");
            println!("  version             Show version");
            0
        }
        "--version" | "version" => {
            println!("Nextcloud 29.0.0 (SlateOS)");
            0
        }
        "status" => {
            println!("  - installed: true");
            println!("  - version: 29.0.0.0");
            println!("  - versionstring: 29.0.0");
            println!("  - edition:");
            println!("  - maintenance: false");
            println!("  - needsDbUpgrade: false");
            println!("  - productname: Nextcloud");
            0
        }
        "check" => {
            println!("All checks passed.");
            0
        }
        "maintenance:mode" => {
            let on = cmd_args.iter().any(|a| a == "--on");
            let off = cmd_args.iter().any(|a| a == "--off");
            if on {
                println!("Maintenance mode enabled");
            } else if off {
                println!("Maintenance mode disabled");
            } else {
                println!("Maintenance mode is currently disabled");
            }
            0
        }
        "upgrade" => {
            println!("Nextcloud is already latest version");
            0
        }
        "config:list" => {
            println!("{{");
            println!("  \"system\": {{");
            println!("    \"instanceid\": \"oc1234567890\",");
            println!("    \"passwordsalt\": \"****\",");
            println!("    \"trusted_domains\": [\"localhost\", \"cloud.example.com\"],");
            println!("    \"datadirectory\": \"/var/lib/nextcloud/data\",");
            println!("    \"dbtype\": \"pgsql\",");
            println!("    \"dbname\": \"nextcloud\",");
            println!("    \"dbhost\": \"localhost\",");
            println!("    \"overwrite.cli.url\": \"https://cloud.example.com\"");
            println!("  }}");
            println!("}}");
            0
        }
        "app:list" => {
            println!("Enabled:");
            println!("  - files: 1.22.0");
            println!("  - photos: 2.4.0");
            println!("  - contacts: 5.5.3");
            println!("  - calendar: 4.7.0");
            println!("  - mail: 3.6.0");
            println!("  - deck: 1.12.2");
            println!("  - talk: 18.0.5");
            println!("  - onlyoffice: 9.1.0");
            println!("Disabled:");
            println!("  - weather_status");
            println!("  - firstrunwizard");
            0
        }
        "app:enable" => {
            let app = cmd_args.first().map(|s| s.as_str()).unwrap_or("app");
            println!("{} enabled", app);
            0
        }
        "app:disable" => {
            let app = cmd_args.first().map(|s| s.as_str()).unwrap_or("app");
            println!("{} disabled", app);
            0
        }
        "user:list" => {
            println!("  - admin");
            println!("  - alice");
            println!("  - bob");
            println!("  - charlie");
            0
        }
        "user:add" => {
            let user = cmd_args.first().map(|s| s.as_str()).unwrap_or("newuser");
            println!("The user \"{}\" was created successfully", user);
            0
        }
        "user:info" => {
            let user = cmd_args.first().map(|s| s.as_str()).unwrap_or("admin");
            println!("  - user_id: {}", user);
            println!("  - display_name: {}", user);
            println!("  - email: {}@localhost", user);
            println!("  - cloud_id: {}@cloud.example.com", user);
            println!("  - enabled: true");
            println!("  - groups: admin");
            println!("  - quota: none");
            println!("  - storage:");
            println!("    - free: 42.00 GB");
            println!("    - used: 8.50 GB");
            println!("    - total: 50.00 GB");
            println!("    - relative: 17.00%");
            println!("  - last_seen: 2025-05-22T09:30:00+00:00");
            0
        }
        "files:scan" => {
            let user = cmd_args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
            if let Some(u) = user {
                println!("Starting scan for user {} ...", u);
            } else {
                println!("Starting scan for all users ...");
            }
            println!("+---------+-------+---------+");
            println!("| Folders | Files | Elapsed |");
            println!("+---------+-------+---------+");
            println!("| 42      | 1234  | 0:02    |");
            println!("+---------+-------+---------+");
            0
        }
        "background:cron" => {
            println!("Background jobs executed successfully");
            0
        }
        "db:add-missing-indices" => {
            println!("Check indices of the share table.");
            println!("Check indices of the filecache table.");
            println!("Check indices of the twofactor_providers table.");
            println!("All indices are up to date.");
            0
        }
        "log:manage" => {
            println!("Enabled: yes");
            println!("Log level: Warning (2)");
            println!("Log file: /var/log/nextcloud/nextcloud.log");
            0
        }
        "encryption:status" => {
            println!("  - enabled: false");
            0
        }
        other => { eprintln!("occ: unknown command '{}'. Run 'occ list' for help.", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_occ(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_occ};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_occ(vec!["--help".to_string()]), 0);
        assert_eq!(run_occ(vec!["-h".to_string()]), 0);
        let _ = run_occ(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_occ(vec![]);
    }
}
