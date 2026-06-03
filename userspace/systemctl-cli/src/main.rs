#![deny(clippy::all)]

//! systemctl-cli — OurOS systemctl CLI
//!
//! Single personality: `systemctl`

use std::env;
use std::process;

fn run_systemctl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: systemctl [OPTIONS] COMMAND [UNIT...]");
        println!();
        println!("systemctl — service manager control (OurOS).");
        println!();
        println!("Unit commands:");
        println!("  start UNIT            Start unit");
        println!("  stop UNIT             Stop unit");
        println!("  restart UNIT          Restart unit");
        println!("  reload UNIT           Reload configuration");
        println!("  status [UNIT]         Show unit status");
        println!("  enable UNIT           Enable unit at boot");
        println!("  disable UNIT          Disable unit at boot");
        println!("  is-active UNIT        Check if active");
        println!("  is-enabled UNIT       Check if enabled");
        println!("  mask UNIT             Mask unit");
        println!("  unmask UNIT           Unmask unit");
        println!();
        println!("System commands:");
        println!("  list-units            List loaded units");
        println!("  list-unit-files       List installed unit files");
        println!("  daemon-reload         Reload systemd config");
        println!("  reboot                Reboot system");
        println!("  poweroff              Power off system");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("systemd 255 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list-units");
    let unit = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "status" => {
            let svc = if unit.is_empty() { "system" } else { unit };
            if svc == "system" {
                println!("  State: running");
                println!("   Jobs: 0 queued");
                println!(" Failed: 0 units");
                println!("  Since: Mon 2024-01-15 08:00:00 UTC; 4h ago");
            } else {
                println!("● {}", svc);
                println!("     Loaded: loaded (/etc/systemd/system/{}; enabled)", svc);
                println!("     Active: active (running) since Mon 2024-01-15 08:00:00 UTC; 4h ago");
                println!("   Main PID: 1234 ({})", svc.split('.').next().unwrap_or(svc));
                println!("      Tasks: 4 (limit: 4096)");
                println!("     Memory: 24.0M");
                println!("        CPU: 1.234s");
                println!("     CGroup: /system.slice/{}", svc);
                println!();
                println!("Jan 15 08:00:00 ouros systemd[1]: Started {}.", svc);
            }
        }
        "start" => { println!("Started {}", unit); }
        "stop" => { println!("Stopped {}", unit); }
        "restart" => { println!("Restarted {}", unit); }
        "reload" => { println!("Reloaded {}", unit); }
        "enable" => {
            println!("Created symlink /etc/systemd/system/multi-user.target.wants/{} -> /usr/lib/systemd/system/{}", unit, unit);
        }
        "disable" => {
            println!("Removed /etc/systemd/system/multi-user.target.wants/{}", unit);
        }
        "is-active" => { println!("active"); }
        "is-enabled" => { println!("enabled"); }
        "mask" => { println!("Created symlink /etc/systemd/system/{} -> /dev/null", unit); }
        "unmask" => { println!("Removed /etc/systemd/system/{}", unit); }
        "daemon-reload" => { /* silent success */ }
        "list-units" => {
            println!("  UNIT                          LOAD   ACTIVE SUB     DESCRIPTION");
            println!("  init.service                  loaded active running Service Manager");
            println!("  networking.service            loaded active running Network Service");
            println!("  sshd.service                  loaded active running OpenSSH Server");
            println!("  nginx.service                 loaded active running Nginx HTTP Server");
            println!("  postgresql.service             loaded active running PostgreSQL Database");
            println!("  cron.service                  loaded active running Periodic Task Scheduler");
            println!();
            println!("LOAD   = Reflects whether the unit definition was properly loaded.");
            println!("ACTIVE = The high-level unit activation state, i.e. generalization of SUB.");
            println!("SUB    = The low-level unit activation state, values depend on unit type.");
            println!();
            println!("6 loaded units listed.");
        }
        "list-unit-files" => {
            println!("UNIT FILE                          STATE     PRESET");
            println!("init.service                       enabled   enabled");
            println!("networking.service                 enabled   enabled");
            println!("sshd.service                       enabled   enabled");
            println!("nginx.service                      enabled   disabled");
            println!("postgresql.service                 disabled  disabled");
            println!();
            println!("5 unit files listed.");
        }
        "reboot" => { println!("System is rebooting..."); }
        "poweroff" => { println!("System is powering off..."); }
        _ => {
            eprintln!("systemctl: unknown command '{}'. See systemctl --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_systemctl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_systemctl};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_systemctl(vec!["--help".to_string()]), 0);
        assert_eq!(run_systemctl(vec!["-h".to_string()]), 0);
        assert_eq!(run_systemctl(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_systemctl(vec![]), 0);
    }
}
