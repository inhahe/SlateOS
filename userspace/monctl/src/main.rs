//! SlateOS Monitor/Display Control Utility
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
// Display-control interface
// ============================================================================
//
// monctl drives the display through sysfs:
//   * DPMS power state: /sys/class/drm/<connector>/dpms
//   * Backlight:        /sys/class/backlight/<dev>/brightness (+ max_brightness)
//
// The previous version issued a "DRM ioctl" via syscall 850 — which on SlateOS
// is SYS_TCP_SET_NODELAY, NOT a display call. There is no DPMS or backlight
// syscall in the SlateOS ABI (the SYS_DRM_* family, 1000-1060, exposes only the
// compositor's GEM/framebuffer/mode-info primitives). Calling 850 to control a
// monitor was meaningless and potentially harmful, so the raw-syscall path has
// been removed; sysfs is the sole control mechanism. DPMS *policy* (idle-timeout
// enable/disable and timers) belongs to the display server, not the kernel —
// see the monctl DESIGN GAP note in todo.txt.

/// DPMS power states, as written to the sysfs `dpms` node.
const DPMS_STATE_ON: &str = "On";
const DPMS_STATE_STANDBY: &str = "Standby";
const DPMS_STATE_SUSPEND: &str = "Suspend";
const DPMS_STATE_OFF: &str = "Off";

/// Compute a brightness percentage (0-100) from raw current/max values.
fn backlight_pct(current: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    // current * 100 / max, guarded against overflow on large raw values.
    let pct = (u64::from(current) * 100) / u64::from(max);
    (pct.min(100)) as u32
}

/// Convert a desired percentage (0-100) into a raw backlight value for `max`.
fn pct_to_raw(percent: u32, max: u32) -> u32 {
    let percent = percent.min(100);
    ((u64::from(max) * u64::from(percent)) / 100) as u32
}

/// Parse the refresh rate (Hz) from a mode string like "1920x1080@60Hz".
fn parse_refresh_hz(resolution: &str) -> u32 {
    resolution
        .split('@')
        .nth(1)
        .and_then(|s| s.trim_end_matches("Hz").parse().ok())
        .unwrap_or(60)
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

                let refresh_hz = parse_refresh_hz(&resolution);

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
    if displays.is_empty()
        && let Some(content) = read_file("/proc/drm/displays")
    {
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
                    return backlight_pct(current, max);
                }
            }
        }
    }

    100 // Default: assume full brightness.
}

fn set_backlight_percent(percent: u32) -> Result<(), String> {
    let percent = percent.min(100);

    // sysfs is the only backlight-control interface on SlateOS.
    if let Ok(entries) = fs::read_dir("/sys/class/backlight") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let base = format!("/sys/class/backlight/{name}");
                let max = read_file(&format!("{base}/max_brightness"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(100);

                let value = pct_to_raw(percent, max);
                if fs::write(format!("{base}/brightness"), value.to_string()).is_ok() {
                    return Ok(());
                }
            }
        }
    }

    Err(
        "no controllable backlight found (is /sys/class/backlight present and writable?)"
            .to_string(),
    )
}

// ============================================================================
// Commands
// ============================================================================

/// Apply a DPMS power state via sysfs, exiting with an error if no display
/// connector accepts the write.
fn apply_dpms(state: &str, action: &str) {
    if !write_dpms_sysfs(state) {
        eprintln!("Failed to {action}: no DPMS-capable display found");
        eprintln!("(is /sys/class/drm present and writable?)");
        process::exit(1);
    }
}

fn cmd_off() {
    println!("Turning monitor off...");
    apply_dpms(DPMS_STATE_OFF, "turn off monitor");
}

fn cmd_on() {
    println!("Turning monitor on...");
    apply_dpms(DPMS_STATE_ON, "wake monitor");
}

fn cmd_standby() {
    println!("Setting monitor to standby...");
    apply_dpms(DPMS_STATE_STANDBY, "set standby");
}

fn cmd_suspend() {
    println!("Setting monitor to suspend...");
    apply_dpms(DPMS_STATE_SUSPEND, "set suspend");
}

fn write_dpms_sysfs(state: &str) -> bool {
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with("card")
                && name.contains('-')
            {
                let path = format!("/sys/class/drm/{name}/dpms");
                if fs::write(&path, state).is_ok() {
                    return true;
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

    // DPMS *policy* — whether the display server blanks the screen after an
    // idle timeout, and the standby/suspend/off thresholds — is owned by the
    // display server (compositor), not the kernel. SlateOS exposes no syscall or
    // sysfs node for it, so these subcommands report that clearly rather than
    // pretending to succeed. Immediate power-state changes (on/off/standby/
    // suspend) are available as the top-level commands and go through sysfs.
    match args[0].as_str() {
        "on" | "enable" | "off" | "disable" | "timers" => {
            eprintln!(
                "monctl: DPMS idle-timeout policy is managed by the display server, \
                 not the kernel; there is no monctl interface for it yet."
            );
            eprintln!(
                "For an immediate power-state change use: monctl off | on | standby | suspend"
            );
            process::exit(1);
        }
        other => {
            eprintln!("unknown DPMS command: {other}");
            eprintln!("Options: on, off, timers (all managed by the display server)");
            process::exit(1);
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("SlateOS Monitor Control v0.1.0");
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlight_pct_basic() {
        assert_eq!(backlight_pct(0, 100), 0);
        assert_eq!(backlight_pct(50, 100), 50);
        assert_eq!(backlight_pct(100, 100), 100);
        // Non-percent max ranges scale correctly.
        assert_eq!(backlight_pct(120, 240), 50);
    }

    #[test]
    fn backlight_pct_zero_max_is_safe() {
        assert_eq!(backlight_pct(50, 0), 0);
    }

    #[test]
    fn backlight_pct_no_overflow_on_large_values() {
        // current near u32::MAX must not overflow the intermediate product.
        assert_eq!(backlight_pct(u32::MAX, u32::MAX), 100);
    }

    #[test]
    fn pct_to_raw_basic() {
        assert_eq!(pct_to_raw(0, 255), 0);
        assert_eq!(pct_to_raw(100, 255), 255);
        assert_eq!(pct_to_raw(50, 240), 120);
    }

    #[test]
    fn pct_to_raw_clamps_over_100() {
        assert_eq!(pct_to_raw(150, 255), 255);
    }

    #[test]
    fn parse_refresh_hz_variants() {
        assert_eq!(parse_refresh_hz("1920x1080@60Hz"), 60);
        assert_eq!(parse_refresh_hz("3840x2160@144Hz"), 144);
        // No @ part -> default 60.
        assert_eq!(parse_refresh_hz("1280x720"), 60);
        // Malformed refresh -> default.
        assert_eq!(parse_refresh_hz("800x600@bogusHz"), 60);
    }
}
