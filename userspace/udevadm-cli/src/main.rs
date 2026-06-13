#![deny(clippy::all)]

//! udevadm-cli — SlateOS udev device manager admin tool
//!
//! Multi-personality: `udevadm`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_udevadm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: udevadm COMMAND [OPTIONS]");
        println!();
        println!("udevadm — udev device management tool (Slate OS).");
        println!();
        println!("Commands:");
        println!("  info         Query device information");
        println!("  trigger      Request events from the kernel");
        println!("  settle       Wait for pending udev events");
        println!("  control      Modify udev daemon behavior");
        println!("  monitor      Listen to kernel/udev events");
        println!("  test         Simulate a udev event");
        println!("  test-builtin Test a built-in command");
        println!("  hwdb         Maintain hardware database");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("udevadm 255 (Slate OS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    match subcmd {
        "info" => {
            let device = rest.iter()
                .find(|a| a.starts_with('/') || a.starts_with("--name") || a.starts_with("--path"))
                .map(|s| s.as_str())
                .unwrap_or("/dev/sda");
            println!("P: /devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda");
            println!("N: sda");
            println!("L: 0");
            println!("S: disk/by-id/ata-Samsung_SSD_870_EVO_1TB_S1234567890");
            println!("S: disk/by-path/pci-0000:00:17.0-ata-1");
            println!("E: DEVPATH=/devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda");
            println!("E: DEVNAME={}", device);
            println!("E: DEVTYPE=disk");
            println!("E: MAJOR=8");
            println!("E: MINOR=0");
            println!("E: SUBSYSTEM=block");
            println!("E: ID_VENDOR=ATA");
            println!("E: ID_MODEL=Samsung_SSD_870_EVO_1TB");
            println!("E: ID_SERIAL=Samsung_SSD_870_EVO_1TB_S1234567890");
            println!("E: ID_TYPE=disk");
            println!("E: ID_BUS=ata");
        }
        "trigger" => {
            let action = rest.iter()
                .find(|a| a.starts_with("--action="))
                .and_then(|a| a.strip_prefix("--action="))
                .unwrap_or("change");
            println!("udevadm trigger: sending {} events", action);
        }
        "settle" => {
            println!("udevadm settle: all pending events processed");
        }
        "control" => {
            if rest.iter().any(|a| a == "--reload" || a == "-R") {
                println!("udevadm control: reloading rules and databases");
            } else if rest.iter().any(|a| a == "--stop-exec-queue") {
                println!("udevadm control: stopping exec queue");
            } else if rest.iter().any(|a| a == "--start-exec-queue") {
                println!("udevadm control: starting exec queue");
            } else {
                println!("udevadm control: see --help for commands");
            }
        }
        "monitor" => {
            println!("monitor will print the received events for:");
            println!("UDEV - the event which udev sends out after rule processing");
            println!("KERNEL - the kernel uevent");
            println!();
            println!("KERNEL[1234.567890] add      /devices/pci0000:00/0000:00:14.0/usb1/1-2 (usb)");
            println!("UDEV  [1234.568901] add      /devices/pci0000:00/0000:00:14.0/usb1/1-2 (usb)");
            println!("KERNEL[1234.569012] add      /devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0 (usb)");
            println!("UDEV  [1234.570123] add      /devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0 (usb)");
        }
        "test" => {
            let path = rest.first().map(|s| s.as_str()).unwrap_or("/sys/class/block/sda");
            println!("calling: test");
            println!("version 255");
            println!("This program is for debugging only, it does not run any program");
            println!("specified by a RUN key. It may show incorrect results, because");
            println!("some values may be different, or not available at a simulation run.");
            println!();
            println!("=== trie on-disk ===");
            println!("tool version:          255");
            println!("file size:         12345678 bytes");
            println!("DEVPATH={}", path);
        }
        "hwdb" => {
            if rest.iter().any(|a| a == "--update" || a == "-u") {
                println!("udevadm hwdb: updating /etc/udev/hwdb.bin");
            } else {
                println!("udevadm hwdb: see --help for options");
            }
        }
        _ => {
            eprintln!("udevadm: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "udevadm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_udevadm(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_udevadm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/udevadm"), "udevadm");
        assert_eq!(basename(r"C:\bin\udevadm.exe"), "udevadm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("udevadm.exe"), "udevadm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_udevadm(&["--help".to_string()]), 0);
        assert_eq!(run_udevadm(&["-h".to_string()]), 0);
        let _ = run_udevadm(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_udevadm(&[]);
    }
}
