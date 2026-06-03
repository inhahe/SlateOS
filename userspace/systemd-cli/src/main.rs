#![deny(clippy::all)]

//! systemd-cli — OurOS systemd-like service manager tools
//!
//! Multi-personality: `systemctl`, `journalctl`, `hostnamectl`, `timedatectl`,
//! `loginctl`, `localectl`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_systemctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: systemctl [OPTIONS] COMMAND [UNIT...]");
        println!();
        println!("systemctl — service manager control (OurOS).");
        println!();
        println!("Commands:");
        println!("  start UNIT         Start a unit");
        println!("  stop UNIT          Stop a unit");
        println!("  restart UNIT       Restart a unit");
        println!("  reload UNIT        Reload config");
        println!("  status [UNIT]      Show unit status");
        println!("  enable UNIT        Enable unit");
        println!("  disable UNIT       Disable unit");
        println!("  is-active UNIT     Check if active");
        println!("  is-enabled UNIT    Check if enabled");
        println!("  list-units         List units");
        println!("  daemon-reload      Reload daemon config");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list-units");
    let unit = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match subcmd {
        "start" => println!("Started {}", unit),
        "stop" => println!("Stopped {}", unit),
        "restart" => println!("Restarted {}", unit),
        "reload" => println!("Reloaded {}", unit),
        "enable" => {
            println!("Created symlink /etc/systemd/system/multi-user.target.wants/{} -> /lib/systemd/system/{}", unit, unit);
        }
        "disable" => {
            println!("Removed /etc/systemd/system/multi-user.target.wants/{}", unit);
        }
        "status" => {
            if unit.is_empty() {
                println!("● ouros");
                println!("    State: running");
                println!("    Units: 142 loaded (incl. loaded: 142; masked: 0)");
                println!("     Jobs: 0 queued");
                println!("   Memory: 1.2G");
                println!("      CPU: 4.567s");
            } else {
                println!("● {} - {}", unit, unit);
                println!("     Loaded: loaded (/lib/systemd/system/{}; enabled)", unit);
                println!("     Active: active (running) since Thu 2025-01-01 00:00:00 UTC");
                println!("   Main PID: 1234 ({})", unit.split('.').next().unwrap_or(unit));
                println!("      Tasks: 4 (limit: 4096)");
                println!("     Memory: 24.5M");
                println!("        CPU: 123ms");
            }
        }
        "is-active" => println!("active"),
        "is-enabled" => println!("enabled"),
        "daemon-reload" => { /* silent success */ }
        "list-units" | _ => {
            println!("UNIT                        LOAD   ACTIVE SUB     DESCRIPTION");
            println!("init.service                loaded active running System and Service Manager");
            println!("dbus.service                loaded active running D-Bus System Message Bus");
            println!("networking.service          loaded active running Network Configuration");
            println!("sshd.service                loaded active running OpenSSH server daemon");
            println!("cron.service                loaded active running Regular background tasks");
            println!("rsyslog.service             loaded active running System Logging Service");
            println!("udev.service                loaded active running udev Kernel Device Manager");
            println!();
            println!("LOAD   = Reflects whether the unit definition was properly loaded.");
            println!("ACTIVE = The high-level unit activation state, i.e. generalization of SUB.");
            println!("SUB    = The low-level unit activation state, values depend on unit type.");
            println!();
            println!("7 loaded units listed.");
        }
    }
    0
}

fn run_journalctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: journalctl [OPTIONS]");
        println!();
        println!("journalctl — query the systemd journal (OurOS).");
        println!();
        println!("Options:");
        println!("  -f, --follow     Follow new entries");
        println!("  -n N, --lines=N  Show last N entries");
        println!("  -u UNIT          Show entries for UNIT");
        println!("  -b [ID]          Show entries from boot ID");
        println!("  -p PRIORITY      Filter by priority");
        println!("  --since TIME     Show entries since TIME");
        println!("  --until TIME     Show entries until TIME");
        println!("  -k, --dmesg      Show kernel messages");
        println!("  --disk-usage     Show journal disk usage");
        println!("  --vacuum-size=S  Reduce to size S");
        println!("  -o FORMAT        Output format (short, json, verbose)");
        return 0;
    }

    let disk_usage = args.iter().any(|a| a == "--disk-usage");
    if disk_usage {
        println!("Archived and active journals take up 256.0M in the file system.");
        return 0;
    }

    println!("-- Journal begins at Thu 2025-01-01 00:00:00 UTC, ends at Thu 2025-01-01 12:00:00 UTC. --");
    println!("Jan 01 00:00:00 ouros kernel: OurOS version 1.0.0 booting");
    println!("Jan 01 00:00:01 ouros systemd[1]: Starting system initialization...");
    println!("Jan 01 00:00:02 ouros systemd[1]: Started udev Kernel Device Manager.");
    println!("Jan 01 00:00:03 ouros systemd[1]: Started D-Bus System Message Bus.");
    println!("Jan 01 00:00:04 ouros systemd[1]: Started Network Configuration.");
    println!("Jan 01 00:00:05 ouros sshd[456]: Server listening on 0.0.0.0 port 22.");
    println!("Jan 01 00:00:06 ouros systemd[1]: Reached target Multi-User System.");
    0
}

fn run_hostnamectl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: hostnamectl [OPTIONS] COMMAND");
        println!("Commands: status, hostname [NAME], icon-name [NAME], chassis [TYPE]");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "hostname" if args.len() > 1 => println!("Hostname set to '{}'", args[1]),
        _ => {
            println!("   Static hostname: ouros");
            println!("         Icon name: computer-desktop");
            println!("           Chassis: desktop");
            println!("        Machine ID: 12345678901234567890123456789012");
            println!("           Boot ID: abcdefabcdefabcdefabcdefabcdefab");
            println!("  Operating System: OurOS 1.0");
            println!("            Kernel: OurOS 1.0.0");
            println!("      Architecture: x86-64");
        }
    }
    0
}

fn run_timedatectl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: timedatectl [OPTIONS] COMMAND");
        println!("Commands: status, set-time TIME, set-timezone TZ, set-ntp BOOL");
        return 0;
    }
    println!("               Local time: Thu 2025-01-01 12:00:00 UTC");
    println!("           Universal time: Thu 2025-01-01 12:00:00 UTC");
    println!("                 RTC time: Thu 2025-01-01 12:00:00");
    println!("                Time zone: UTC (UTC, +0000)");
    println!("System clock synchronized: yes");
    println!("              NTP service: active");
    println!("          RTC in local TZ: no");
    0
}

fn run_loginctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: loginctl [OPTIONS] COMMAND");
        println!("Commands: list-sessions, list-users, show-session, show-user, terminate-session");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list-sessions");
    match subcmd {
        "list-users" => {
            println!("  UID USER");
            println!(" 1000 user");
            println!("    0 root");
        }
        _ => {
            println!("SESSION  UID USER   SEAT  TTY");
            println!("      1 1000 user   seat0 tty1");
        }
    }
    0
}

fn run_localectl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: localectl [OPTIONS] COMMAND");
        println!("Commands: status, set-locale LOCALE, set-keymap MAP, list-locales, list-keymaps");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "list-locales" => {
            println!("C.UTF-8");
            println!("en_US.UTF-8");
            println!("POSIX");
        }
        "list-keymaps" => {
            println!("us");
            println!("uk");
            println!("de");
            println!("fr");
        }
        _ => {
            println!("   System Locale: LANG=en_US.UTF-8");
            println!("       VC Keymap: us");
            println!("      X11 Layout: us");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "systemctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "journalctl" => run_journalctl(&rest),
        "hostnamectl" => run_hostnamectl(&rest),
        "timedatectl" => run_timedatectl(&rest),
        "loginctl" => run_loginctl(&rest),
        "localectl" => run_localectl(&rest),
        _ => run_systemctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_systemctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/systemd"), "systemd");
        assert_eq!(basename(r"C:\bin\systemd.exe"), "systemd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("systemd.exe"), "systemd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_systemctl(&["--help".to_string()]), 0);
        assert_eq!(run_systemctl(&["-h".to_string()]), 0);
        assert_eq!(run_systemctl(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_systemctl(&[]), 0);
    }
}
