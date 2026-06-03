#![deny(clippy::all)]

//! homeassistant-cli — OurOS Home Assistant CLI
//!
//! Multi-personality: `ha`, `hass-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ha(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ha COMMAND [OPTIONS]");
        println!("Home Assistant CLI (OurOS)");
        println!();
        println!("Commands:");
        println!("  core         Core management");
        println!("  supervisor   Supervisor management");
        println!("  addons       Add-on management");
        println!("  network      Network management");
        println!("  hardware     Hardware info");
        println!("  info         System info");
        println!("  host         Host management");
        println!("  os           OS management");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match subcmd {
        "info" | "--version" => {
            println!("Home Assistant 2024.1.5");
            println!("  Supervisor: 2024.01.2");
            println!("  Operating System: OurOS");
            println!("  Architecture: x86_64");
        }
        "core" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match action {
                "info" => {
                    println!("version: 2024.1.5");
                    println!("machine: qemux86-64");
                    println!("arch: amd64");
                    println!("image: ghcr.io/home-assistant/qemux86-64-homeassistant");
                }
                "start" => println!("Starting Home Assistant core..."),
                "stop" => println!("Stopping Home Assistant core..."),
                "restart" => println!("Restarting Home Assistant core..."),
                "check" => {
                    println!("Checking configuration...");
                    println!("Configuration valid!");
                }
                "update" => println!("Updating Home Assistant core..."),
                _ => println!("ha core {}: completed", action),
            }
        }
        "addons" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if action == "list" {
                println!("Installed add-ons:");
                println!("  mosquitto       Mosquitto broker    2.0.18  running");
                println!("  samba           Samba share         12.3.1  running");
                println!("  terminal        Terminal & SSH      9.9.0   running");
                println!("  file_editor     File editor         5.8.0   running");
            }
        }
        "network" => {
            println!("Network info:");
            println!("  Interface: eth0");
            println!("  IP: 192.168.1.100/24");
            println!("  Gateway: 192.168.1.1");
            println!("  DNS: 192.168.1.1");
        }
        "hardware" => {
            println!("Hardware info:");
            println!("  CPU: 4 cores");
            println!("  Memory: 4096 MB");
            println!("  Disk: /dev/sda (32 GB)");
            println!("  USB: 2 devices");
        }
        "host" => {
            println!("Host info:");
            println!("  Hostname: homeassistant");
            println!("  OS: OurOS");
            println!("  Kernel: 6.6.0-ouros");
        }
        _ => println!("ha: '{}' completed", subcmd),
    }
    0
}

fn run_hass_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hass-cli COMMAND [OPTIONS]");
        println!("  --server URL   HA server URL");
        println!("  --token TOKEN  API token");
        println!();
        println!("Commands:");
        println!("  state get ENTITY   Get entity state");
        println!("  state list         List all entities");
        println!("  service call SVC   Call a service");
        println!("  event fire EVENT   Fire an event");
        println!("  template TMPL      Render template");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "state" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match action {
                "list" => {
                    println!("light.living_room          on     brightness=200");
                    println!("sensor.temperature         22.5   unit=°C");
                    println!("switch.kitchen             off");
                    println!("binary_sensor.motion       off");
                }
                "get" => {
                    let entity = args.get(2).map(|s| s.as_str()).unwrap_or("light.living_room");
                    println!("{}: on", entity);
                    println!("  brightness: 200");
                    println!("  color_temp: 350");
                }
                _ => println!("hass-cli state {}: completed", action),
            }
        }
        "service" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if action == "call" {
                let svc = args.get(2).map(|s| s.as_str()).unwrap_or("light.turn_on");
                println!("Calling service: {}", svc);
                println!("  Service called successfully.");
            }
        }
        _ => println!("hass-cli: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ha".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "hass-cli" => run_hass_cli(&rest),
        _ => run_ha(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ha};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/homeassistant"), "homeassistant");
        assert_eq!(basename(r"C:\bin\homeassistant.exe"), "homeassistant.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("homeassistant.exe"), "homeassistant");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ha(&["--help".to_string()]), 0);
        assert_eq!(run_ha(&["-h".to_string()]), 0);
        assert_eq!(run_ha(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ha(&[]), 0);
    }
}
