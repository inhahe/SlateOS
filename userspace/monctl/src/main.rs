//! OurOS Monitor/Display Control Utility
//!
//! Control display power state, brightness, and DPMS settings.
//! Similar to `xset dpms` or Windows `nircmd monitor off`.
//!
//! Design spec: "utility to shut off the monitor like 'nircmd monitor off' does"
//!
//! # Usage
//!
//! ```text
//! monctl off                Turn off monitor (DPMS standby)
//! monctl on                 Turn on monitor (wake from standby)
//! monctl standby            DPMS standby mode
//! monctl suspend            DPMS suspend mode
//! monctl brightness <n>     Set brightness (0-100)
//! monctl brightness         Show current brightness
//! monctl status             Show display status
//! monctl list               List connected displays
//! monctl resolution         Show current resolution
//! monctl dpms <on|off>      Enable or disable DPMS
//! monctl dpms timers <s> <s> <s>  Set standby/suspend/off timers
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

const SYS_DRM_IOCTL: u64 = 850;

// DRM IOCTL sub-commands for display control.
const DRM_DPMS_SET: u64 = 1;
const DRM_BRIGHTNESS_SET: u64 = 2;
#[allow(dead_code)] // Available for direct brightness query via ioctl.
const DRM_BRIGHTNESS_GET: u64 = 3;
const DRM_DPMS_ENABLE: u64 = 4;
const DRM_DPMS_TIMERS: u64 = 5;

// DPMS states.
const DPMS_ON: u64 = 0;
const DPMS_STANDBY: u64 = 1;
const DPMS_SUSPEND: u64 = 2;
const DPMS_OFF: u64 = 3;

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn drm_ioctl(cmd: u64, arg1: u64, arg2: u64) -> i64 {
    unsafe { syscall3(SYS_DRM_IOCTL, cmd, arg1, arg2) }
}

// ============================================================================
// /sys readers
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

struct DisplayInfo {
    name: String,
    connected: bool,
    resolution: String,
    refresh_hz: u32,
    brightness: u32,
    dpms_state: String,
}

fn read_displays() -> Vec<DisplayInfo> {
    let mut displays = Vec::new();

    // Try /sys/class/drm/ for DRM connectors.
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                // DRM connectors look like card0-HDMI-A-1, card0-eDP-1, etc.
                if !name.starts_with("card") || !name.contains('-') {
                    continue;
                }

                let base = format!("/sys/class/drm/{name}");

                let status = read_file(&format!("{base}/status"))
                    .unwrap_or_default();
                let connected = status == "connected";

                let modes = read_file(&format!("{base}/modes"))
                    .unwrap_or_default();
                let resolution = modes.lines()
                    .next()
                    .unwrap_or("unknown")
                    .to_string();

                let refresh_hz = resolution.split('@')
                    .nth(1)
                    .and_then(|s| s.trim_end_matches("Hz").parse().ok())
                    .unwrap_or(60);

                let dpms = read_file(&format!("{base}/dpms"))
                    .unwrap_or_else(|| "On".to_string());

                // Brightness from backlight subsystem.
                let brightness = read_backlight_percent();

                displays.push(DisplayInfo {
                    name: name.to_string(),
                    connected,
                    resolution,
                    refresh_hz,
                    brightness,
                    dpms_state: dpms,
                });
            }
        }
    }

    // Fallback: check /proc/drm/displays.
    if displays.is_empty() {
        if let Some(content) = read_file("/proc/drm/displays") {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    displays.push(DisplayInfo {
                        name: parts[0].to_string(),
                        connected: parts[1] == "connected",
                        resolution: parts.get(2).unwrap_or(&"?").to_string(),
                        refresh_hz: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(60),
                        brightness: read_backlight_percent(),
                        dpms_state: "On".to_string(),
                    });
                }
            }
        }
    }

    displays
}

fn read_backlight_percent() -> u32 {
    // Try /sys/class/backlight/*/brightness and max_brightness.
    if let Ok(entries) = fs::read_dir("/sys/class/backlight") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let base = format!("/sys/class/backlight/{name}");
                let current = read_file(&format!("{base}/brightness"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                let max = read_file(&format!("{base}/max_brightness"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(100);

                if max > 0 {
                    return (current * 100) / max;
                }
            }
        }
    }

    100 // Default: assume full brightness.
}

fn set_backlight_percent(percent: u32) -> Result<(), String> {
    let percent = percent.min(100);

    // Try sysfs first.
    if let Ok(entries) = fs::read_dir("/sys/class/backlight") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let base = format!("/sys/class/backlight/{name}");
                let max = read_file(&format!("{base}/max_brightness"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(100);

                let value = (max * percent) / 100;
                if fs::write(format!("{base}/brightness"), value.to_string()).is_ok() {
                    return Ok(());
                }
            }
        }
    }

    // Fall back to DRM ioctl.
    let ret = drm_ioctl(DRM_BRIGHTNESS_SET, percent as u64, 0);
    if ret < 0 {
        Err(format!("failed to set brightness: error {ret}"))
    } else {
        Ok(())
    }
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_off() {
    println!("Turning monitor off...");
    let ret = drm_ioctl(DRM_DPMS_SET, DPMS_OFF, 0);
    if ret < 0 {
        // Try writing to sysfs as fallback.
        let wrote = write_dpms_sysfs("Off");
        if !wrote {
            eprintln!("Failed to turn off monitor (error {ret})");
            eprintln!("Try running with appropriate permissions.");
            process::exit(1);
        }
    }
}

fn cmd_on() {
    println!("Turning monitor on...");
    let ret = drm_ioctl(DRM_DPMS_SET, DPMS_ON, 0);
    if ret < 0 {
        let wrote = write_dpms_sysfs("On");
        if !wrote {
            eprintln!("Failed to wake monitor (error {ret})");
            process::exit(1);
        }
    }
}

fn cmd_standby() {
    println!("Setting monitor to standby...");
    let ret = drm_ioctl(DRM_DPMS_SET, DPMS_STANDBY, 0);
    if ret < 0 {
        let wrote = write_dpms_sysfs("Standby");
        if !wrote {
            eprintln!("Failed (error {ret})");
            process::exit(1);
        }
    }
}

fn cmd_suspend() {
    println!("Setting monitor to suspend...");
    let ret = drm_ioctl(DRM_DPMS_SET, DPMS_SUSPEND, 0);
    if ret < 0 {
        let wrote = write_dpms_sysfs("Suspend");
        if !wrote {
            eprintln!("Failed (error {ret})");
            process::exit(1);
        }
    }
}

fn write_dpms_sysfs(state: &str) -> bool {
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("card") && name.contains('-') {
                    let path = format!("/sys/class/drm/{name}/dpms");
                    if fs::write(&path, state).is_ok() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn cmd_brightness(args: &[String]) {
    if args.is_empty() {
        // Show current brightness.
        let pct = read_backlight_percent();
        println!("Brightness: {pct}%");
    } else {
        let pct: u32 = match args[0].parse() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("error: brightness must be a number (0-100)");
                process::exit(1);
            }
        };

        match set_backlight_percent(pct) {
            Ok(()) => println!("Brightness set to {pct}%"),
            Err(e) => {
                eprintln!("{e}");
                process::exit(1);
            }
        }
    }
}

fn cmd_status() {
    let displays = read_displays();

    if displays.is_empty() {
        println!("No displays detected.");
        println!("(Is /sys/class/drm or /proc/drm available?)");
        return;
    }

    for disp in &displays {
        let status = if disp.connected {
            "\x1b[32mconnected\x1b[0m"
        } else {
            "\x1b[31mdisconnected\x1b[0m"
        };

        println!("{}: {}", disp.name, status);
        if disp.connected {
            println!("  Resolution:  {}", disp.resolution);
            println!("  Refresh:     {} Hz", disp.refresh_hz);
            println!("  Brightness:  {}%", disp.brightness);
            println!("  DPMS:        {}", disp.dpms_state);
        }
    }
}

fn cmd_list() {
    let displays = read_displays();

    if displays.is_empty() {
        println!("No displays found.");
        return;
    }

    println!("{:<25} {:<12} {:<16} {:<8}",
        "Display", "Status", "Resolution", "DPMS");
    println!("{:<25} {:<12} {:<16} {:<8}",
        "-------", "------", "----------", "----");

    for disp in &displays {
        let status = if disp.connected { "connected" } else { "disconnected" };
        println!("{:<25} {:<12} {:<16} {:<8}",
            disp.name, status, disp.resolution, disp.dpms_state);
    }
}

fn cmd_resolution() {
    let displays = read_displays();
    for disp in &displays {
        if disp.connected {
            println!("{}: {} @ {}Hz", disp.name, disp.resolution, disp.refresh_hz);
        }
    }
}

fn cmd_dpms(args: &[String]) {
    if args.is_empty() {
        // Show DPMS status.
        let displays = read_displays();
        for disp in &displays {
            if disp.connected {
                println!("{}: DPMS {}", disp.name, disp.dpms_state);
            }
        }
        return;
    }

    match args[0].as_str() {
        "on" | "enable" => {
            let ret = drm_ioctl(DRM_DPMS_ENABLE, 1, 0);
            if ret < 0 {
                eprintln!("Failed to enable DPMS (error {ret})");
            } else {
                println!("DPMS enabled");
            }
        }
        "off" | "disable" => {
            let ret = drm_ioctl(DRM_DPMS_ENABLE, 0, 0);
            if ret < 0 {
                eprintln!("Failed to disable DPMS (error {ret})");
            } else {
                println!("DPMS disabled");
            }
        }
        "timers" => {
            if args.len() < 4 {
                eprintln!("usage: monctl dpms timers <standby_sec> <suspend_sec> <off_sec>");
                process::exit(1);
            }
            let standby: u64 = args[1].parse().unwrap_or(600);
            let suspend: u64 = args[2].parse().unwrap_or(900);
            let off: u64 = args[3].parse().unwrap_or(1200);
            // Pack timers: standby in bits 0-15, suspend 16-31, off 32-47.
            let packed = (standby & 0xFFFF) | ((suspend & 0xFFFF) << 16) | ((off & 0xFFFF) << 32);
            let ret = drm_ioctl(DRM_DPMS_TIMERS, packed, 0);
            if ret < 0 {
                eprintln!("Failed to set DPMS timers (error {ret})");
            } else {
                println!("DPMS timers: standby={standby}s, suspend={suspend}s, off={off}s");
            }
        }
        other => {
            eprintln!("unknown DPMS command: {other}");
            eprintln!("Options: on, off, timers");
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("OurOS Monitor Control v0.1.0");
    println!();
    println!("Control display power state, brightness, and DPMS.");
    println!();
    println!("USAGE:");
    println!("  monctl <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("  off               Turn off monitor");
    println!("  on                Turn on monitor");
    println!("  standby           DPMS standby mode");
    println!("  suspend           DPMS suspend mode");
    println!("  brightness [n]    Get or set brightness (0-100)");
    println!("  status            Show display status");
    println!("  list              List connected displays");
    println!("  resolution        Show current resolution");
    println!("  dpms [on|off]     Enable/disable DPMS");
    println!("  dpms timers S S S Set standby/suspend/off timers");
    println!();
    println!("EXAMPLES:");
    println!("  monctl off                   # turn off screen");
    println!("  monctl brightness 50         # set 50% brightness");
    println!("  monctl dpms timers 300 600 900");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    match args[1].as_str() {
        "off" | "poweroff" => cmd_off(),
        "on" | "poweron" | "wake" => cmd_on(),
        "standby" => cmd_standby(),
        "suspend" => cmd_suspend(),
        "brightness" | "bright" | "bl" => cmd_brightness(&args[2..]),
        "status" | "show" => cmd_status(),
        "list" | "ls" => cmd_list(),
        "resolution" | "res" => cmd_resolution(),
        "dpms" => cmd_dpms(&args[2..]),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'monctl help' for usage.");
            process::exit(1);
        }
    }
}
