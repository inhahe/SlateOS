//! SlateOS Power Management Utility
//!
//! Controls system power state: shutdown, reboot, suspend, hibernate.
//! Communicates with the service manager via IPC for orderly shutdown
//! sequencing (stop services, sync filesystems), then falls back to
//! direct syscalls if IPC is unavailable.
//!
//! # Usage
//!
//! ```text
//! powerctl shutdown              Orderly shutdown and power off
//! powerctl halt                  Alias for shutdown
//! powerctl reboot                Orderly reboot
//! powerctl suspend               ACPI S3 suspend to RAM
//! powerctl hibernate             Save state to swap and power off
//! powerctl status                Show power source and battery info
//! powerctl schedule <min> <cmd>  Schedule shutdown or reboot in N minutes
//! powerctl cancel                Cancel a scheduled operation
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall numbers
// ============================================================================

/// Open a named IPC channel to a well-known service.
const SYS_CHANNEL_OPEN: u64 = 200;
/// Send a message on an open IPC channel.
const SYS_CHANNEL_SEND: u64 = 201;
/// Receive a message from an open IPC channel.
const SYS_CHANNEL_RECV: u64 = 202;
/// Close an IPC channel handle.
const SYS_CHANNEL_CLOSE: u64 = 204;

// NOTE: SlateOS exposes NO userspace power-management syscall (there is no
// SYS_SHUTDOWN / SYS_REBOOT / suspend syscall in kernel/src/syscall/number.rs).
// System power state changes go through the service manager over IPC (the
// orderly_* path below).  The "direct" fallbacks therefore use the only
// non-IPC mechanism available — ACPI control files exposed via procfs/sysfs,
// if present — and report a clear error when nothing works.  See the
// power-management DESIGN GAP note in todo.txt.

// ============================================================================
// Low-level syscall interface
// ============================================================================

/// Issue a three-argument syscall using the x86-64 `syscall` instruction.
///
/// Register mapping follows the SlateOS syscall ABI:
///   rax = syscall number, rdi = arg1, rsi = arg2, rdx = arg3
///   Return value in rax. rcx and r11 are clobbered by the CPU.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
    // rcx and r11 are marked as clobbered per the hardware specification.
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

// ============================================================================
// IPC helpers — talk to the service manager
// ============================================================================

/// The service manager's well-known IPC endpoint name.
const SERVICE_MANAGER_NAME: &[u8] = b"org.slateos.ServiceManager\0";

/// Send a command string to the service manager and return its text response.
///
/// The protocol is line-oriented: we send `"COMMAND\0"` and receive a
/// NUL-terminated response string.
fn send_ipc_command(command: &str) -> Result<String, String> {
    let msg = format!("{command}\0");

    // SAFETY: SYS_CHANNEL_OPEN takes a pointer to a NUL-terminated service
    // name, its length, and flags (0). The pointer is valid for the duration
    // of the syscall because `SERVICE_MANAGER_NAME` is a static byte string.
    let channel = unsafe {
        syscall3(
            SYS_CHANNEL_OPEN,
            SERVICE_MANAGER_NAME.as_ptr() as u64,
            SERVICE_MANAGER_NAME.len() as u64,
            0,
        )
    };

    if channel < 0 {
        return Err(format!(
            "cannot connect to service manager (error {channel})"
        ));
    }

    let ch = channel as u64;

    // SAFETY: SYS_CHANNEL_SEND takes the channel handle, a pointer to the
    // message buffer, and its length. `msg` lives on the stack and outlives
    // the syscall.
    let send_ret = unsafe { syscall3(SYS_CHANNEL_SEND, ch, msg.as_ptr() as u64, msg.len() as u64) };

    if send_ret < 0 {
        // SAFETY: SYS_CHANNEL_CLOSE takes the handle and two unused args.
        let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };
        return Err(format!("send failed (error {send_ret})"));
    }

    // Receive the response into a stack buffer.
    let mut buf = [0u8; 4096];

    // SAFETY: SYS_CHANNEL_RECV takes the channel handle, a pointer to a
    // writable buffer, and the buffer length. `buf` is valid for 4096 bytes
    // and outlives the syscall.
    let recv_ret = unsafe {
        syscall3(
            SYS_CHANNEL_RECV,
            ch,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };

    // Always close the channel, even if recv failed.
    // SAFETY: ch is a valid channel handle obtained from a successful open.
    let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };

    if recv_ret < 0 {
        return Err(format!("recv failed (error {recv_ret})"));
    }

    let len = (recv_ret as usize).min(buf.len());
    let response = String::from_utf8_lossy(&buf[..len]).to_string();
    Ok(response)
}

// ============================================================================
// Filesystem helpers
// ============================================================================

/// Read a sysfs/procfs file, returning its trimmed contents.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

// ============================================================================
// Battery / power-supply status
// ============================================================================

/// Aggregated power-supply information gathered from sysfs.
struct PowerStatus {
    ac_online: Option<bool>,
    batteries: Vec<BatteryInfo>,
}

/// Per-battery information read from /sys/class/power_supply/<name>/.
struct BatteryInfo {
    name: String,
    status: String,
    capacity_pct: Option<u32>,
    energy_now_uj: Option<u64>,
    energy_full_uj: Option<u64>,
    voltage_now_uv: Option<u64>,
    technology: String,
}

/// Scan /sys/class/power_supply/ for AC adapters and batteries.
fn read_power_status() -> PowerStatus {
    let mut status = PowerStatus {
        ac_online: None,
        batteries: Vec::new(),
    };

    let entries = match fs::read_dir("/sys/class/power_supply") {
        Ok(e) => e,
        Err(_) => return status,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        let base = format!("/sys/class/power_supply/{name}");
        let supply_type = read_file(&format!("{base}/type")).unwrap_or_default();

        match supply_type.as_str() {
            "Mains" => {
                // AC adapter — "1" means online.
                let online = read_file(&format!("{base}/online"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(|v| v != 0);
                if online.is_some() {
                    status.ac_online = online;
                }
            }
            "Battery" => {
                let bat_status =
                    read_file(&format!("{base}/status")).unwrap_or_else(|| "Unknown".to_string());
                let capacity =
                    read_file(&format!("{base}/capacity")).and_then(|s| s.parse::<u32>().ok());
                let energy_now =
                    read_file(&format!("{base}/energy_now")).and_then(|s| s.parse::<u64>().ok());
                let energy_full =
                    read_file(&format!("{base}/energy_full")).and_then(|s| s.parse::<u64>().ok());
                let voltage =
                    read_file(&format!("{base}/voltage_now")).and_then(|s| s.parse::<u64>().ok());
                let tech = read_file(&format!("{base}/technology"))
                    .unwrap_or_else(|| "Unknown".to_string());

                status.batteries.push(BatteryInfo {
                    name,
                    status: bat_status,
                    capacity_pct: capacity,
                    energy_now_uj: energy_now,
                    energy_full_uj: energy_full,
                    voltage_now_uv: voltage,
                    technology: tech,
                });
            }
            _ => {}
        }
    }

    status
}

// ============================================================================
// Scheduled-operation persistence
// ============================================================================

/// Path where a pending scheduled operation is stored.
const SCHEDULE_FILE: &str = "/run/powerctl/scheduled";

/// Write a scheduled operation descriptor so a background timer can act on it.
///
/// The file format is one line: `<unix_epoch_seconds> <action>\n`
/// where action is "shutdown" or "reboot".
fn write_schedule(minutes: u64, action: &str) -> Result<(), String> {
    // Read current uptime to compute the target time.
    let uptime_secs = read_file("/proc/uptime")
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .unwrap_or(0.0) as u64;

    let target_secs = uptime_secs.saturating_add(minutes.saturating_mul(60));
    let content = format!("{target_secs} {action}\n");

    // Ensure the parent directory exists.
    let _ = fs::create_dir_all("/run/powerctl");

    fs::write(SCHEDULE_FILE, content).map_err(|e| format!("failed to write schedule file: {e}"))
}

/// Read back the currently-scheduled operation, if any.
fn read_schedule() -> Option<(u64, String)> {
    let content = read_file(SCHEDULE_FILE)?;
    let mut parts = content.split_whitespace();
    let target_secs = parts.next()?.parse::<u64>().ok()?;
    let action = parts.next()?.to_string();
    Some((target_secs, action))
}

/// Cancel a pending scheduled operation.
fn cancel_schedule() -> Result<(), String> {
    if fs::remove_file(SCHEDULE_FILE).is_ok() {
        Ok(())
    } else {
        Err("no scheduled operation to cancel".to_string())
    }
}

// ============================================================================
// Orderly shutdown sequence via IPC
// ============================================================================

/// Ask the service manager to perform an orderly shutdown.
///
/// The sequence is: stop all services in dependency order, sync filesystems,
/// then trigger the final power state change.  The service manager handles
/// the ordering; we just send the top-level command and wait for confirmation.
fn orderly_shutdown(action: &str) -> bool {
    let command = match action {
        "shutdown" | "halt" => "SYSTEM_SHUTDOWN",
        "reboot" => "SYSTEM_REBOOT",
        "suspend" => "SYSTEM_SUSPEND",
        "hibernate" => "SYSTEM_HIBERNATE",
        _ => return false,
    };

    match send_ipc_command(command) {
        Ok(resp) => {
            let trimmed = resp.trim().trim_end_matches('\0');
            if trimmed.starts_with("OK") || trimmed.starts_with("ACK") {
                true
            } else {
                eprintln!("service manager responded: {trimmed}");
                // A response that is not an error still counts as handled --
                // the service manager accepted the command.
                !trimmed.starts_with("ERR")
            }
        }
        Err(_) => false,
    }
}

// ============================================================================
// Direct-syscall fallbacks
// ============================================================================

/// Attempt a filesystem sync via /proc/sys/vm/sync (write "1").
fn try_sync_filesystems() {
    // Try the procfs knob first.
    if fs::write("/proc/sys/vm/sync", "1").is_ok() {
        return;
    }
    // Try the sysfs alternative.
    let _ = fs::write("/sys/kernel/sync", "1");
}

/// Power off the machine directly when the service manager is unreachable.
///
/// SlateOS has no power-off syscall, so the only non-IPC mechanism is the ACPI
/// control file (if procfs exposes it).  If the machine powers off, this
/// process never returns; otherwise we report that no mechanism worked.
fn direct_shutdown() -> ! {
    try_sync_filesystems();

    // Best-effort: a procfs ACPI knob, if the kernel exposes one, powers off.
    let _ = fs::write("/proc/acpi/power", "off");

    eprintln!(
        "powerctl: cannot power off directly — the service manager is \
         unreachable and Slate OS exposes no power-off syscall or ACPI control \
         file.  System NOT powered off."
    );
    process::exit(1);
}

/// Reboot the machine directly when the service manager is unreachable.
fn direct_reboot() -> ! {
    try_sync_filesystems();

    // Best-effort: a procfs ACPI knob, if present, reboots.
    let _ = fs::write("/proc/acpi/power", "reboot");

    eprintln!(
        "powerctl: cannot reboot directly — the service manager is \
         unreachable and Slate OS exposes no reboot syscall or ACPI control \
         file.  System NOT rebooted."
    );
    process::exit(1);
}

/// Enter ACPI S3 suspend directly when the service manager is unreachable.
fn direct_suspend() {
    // SlateOS has no suspend syscall; the only direct path is an ACPI sleep
    // control file, if procfs/sysfs exposes one.
    if fs::write("/proc/acpi/sleep", "S3").is_err() && fs::write("/sys/power/state", "mem").is_err()
    {
        eprintln!(
            "powerctl: cannot suspend — no ACPI sleep control file (is ACPI S3 \
             supported and exposed by the kernel?)."
        );
        process::exit(1);
    }
}

/// Enter ACPI S4 hibernate directly when the service manager is unreachable.
fn direct_hibernate() {
    try_sync_filesystems();

    // SlateOS has no hibernate syscall; the only direct path is an ACPI sleep
    // control file, if procfs/sysfs exposes one.
    if fs::write("/proc/acpi/sleep", "S4").is_err()
        && fs::write("/sys/power/state", "disk").is_err()
    {
        eprintln!(
            "powerctl: cannot hibernate — no ACPI sleep control file (is a swap \
             partition configured and hibernate supported?)."
        );
        process::exit(1);
    }
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_shutdown() {
    println!("Initiating system shutdown...");

    if orderly_shutdown("shutdown") {
        println!("Service manager acknowledged shutdown.");
        // The service manager drives the rest of the sequence; it will call
        // SYS_SHUTDOWN itself after all services are stopped and filesystems
        // synced.  We wait briefly so the user can see our message.
        return;
    }

    eprintln!("Service manager unavailable -- falling back to direct shutdown.");
    eprintln!("Warning: services may not be stopped cleanly.");
    direct_shutdown();
}

fn cmd_reboot() {
    println!("Initiating system reboot...");

    if orderly_shutdown("reboot") {
        println!("Service manager acknowledged reboot.");
        return;
    }

    eprintln!("Service manager unavailable -- falling back to direct reboot.");
    eprintln!("Warning: services may not be stopped cleanly.");
    direct_reboot();
}

fn cmd_suspend() {
    println!("Suspending system (ACPI S3)...");

    if orderly_shutdown("suspend") {
        println!("Service manager acknowledged suspend.");
        return;
    }

    eprintln!("Service manager unavailable -- falling back to direct suspend.");
    direct_suspend();
    println!("Resumed from suspend.");
}

fn cmd_hibernate() {
    println!("Hibernating system (ACPI S4)...");

    if orderly_shutdown("hibernate") {
        println!("Service manager acknowledged hibernate.");
        return;
    }

    eprintln!("Service manager unavailable -- falling back to direct hibernate.");
    direct_hibernate();
    println!("Resumed from hibernate.");
}

fn cmd_status() {
    let ps = read_power_status();

    // ACPI state from /proc/acpi/state or /sys/power/state.
    let acpi_state = read_file("/sys/power/state")
        .or_else(|| read_file("/proc/acpi/state"))
        .unwrap_or_else(|| "unknown".to_string());

    println!("Power Status");
    println!("============");
    println!();

    // AC adapter.
    match ps.ac_online {
        Some(true) => println!("  Power source:  \x1b[32mAC (plugged in)\x1b[0m"),
        Some(false) => println!("  Power source:  \x1b[33mBattery\x1b[0m"),
        None => println!("  Power source:  unknown (no AC adapter detected)"),
    }

    println!("  ACPI states:   {acpi_state}");

    // Uptime.
    if let Some(uptime_str) = read_file("/proc/uptime")
        && let Some(secs_str) = uptime_str.split_whitespace().next()
        && let Ok(secs) = secs_str.parse::<f64>()
    {
        println!("  Uptime:        {}", format_duration(secs as u64));
    }

    // Scheduled operation.
    if let Some((target, action)) = read_schedule() {
        let uptime = read_file("/proc/uptime")
            .and_then(|s| {
                s.split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
            })
            .unwrap_or(0.0) as u64;

        let remaining = target.saturating_sub(uptime);
        if remaining > 0 {
            println!(
                "  Scheduled:     {action} in {}",
                format_duration(remaining)
            );
        }
    }

    // Batteries.
    if ps.batteries.is_empty() {
        println!();
        println!("  No batteries detected (desktop or VM).");
    } else {
        for bat in &ps.batteries {
            println!();
            println!("  Battery: {}", bat.name);
            println!("    Status:      {}", bat.status);

            if let Some(pct) = bat.capacity_pct {
                let bar = capacity_bar(pct);
                println!("    Capacity:    {pct}% {bar}");
            } else if let (Some(now), Some(full)) = (bat.energy_now_uj, bat.energy_full_uj) {
                // Compute percentage from energy readings if the capacity
                // sysfs node is absent.  checked_mul/checked_div avoid both
                // overflow on large energy values and division by zero.
                if let Some(pct) = now
                    .checked_mul(100)
                    .and_then(|scaled| scaled.checked_div(full))
                {
                    let pct = pct as u32;
                    let bar = capacity_bar(pct.min(100));
                    println!("    Capacity:    {pct}% {bar} (computed)");
                }
            }

            if let Some(uv) = bat.voltage_now_uv {
                let volts = uv as f64 / 1_000_000.0;
                println!("    Voltage:     {volts:.2} V");
            }

            println!("    Technology:  {}", bat.technology);
        }
    }
}

fn cmd_schedule(args: &[String]) {
    if args.len() < 2 {
        eprintln!("usage: powerctl schedule <minutes> <shutdown|reboot>");
        process::exit(1);
    }

    let minutes: u64 = match args[0].parse() {
        Ok(m) if m > 0 => m,
        _ => {
            eprintln!("error: minutes must be a positive integer");
            process::exit(1);
        }
    };

    let action = match args[1].as_str() {
        "shutdown" | "halt" | "poweroff" => "shutdown",
        "reboot" | "restart" => "reboot",
        other => {
            eprintln!("error: unknown action '{other}' (expected shutdown or reboot)");
            process::exit(1);
        }
    };

    match write_schedule(minutes, action) {
        Ok(()) => {
            println!(
                "Scheduled {action} in {minutes} minute{}.",
                if minutes == 1 { "" } else { "s" }
            );
            println!("Run 'powerctl cancel' to abort.");

            // Also inform the service manager so it can set up its own timer.
            let ipc_cmd = format!("SCHEDULE_POWER {minutes} {action}");
            if let Err(e) = send_ipc_command(&ipc_cmd) {
                eprintln!(
                    "note: could not notify service manager ({e}); \
                     schedule file written to {SCHEDULE_FILE}"
                );
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_cancel() {
    match cancel_schedule() {
        Ok(()) => {
            println!("Scheduled operation cancelled.");

            // Tell the service manager to cancel its timer too.
            let _ = send_ipc_command("CANCEL_SCHEDULED_POWER");
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a duration in seconds as a human-readable string.
fn format_duration(secs: u64) -> String {
    if secs >= 86400 {
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        format!("{days}d {hours}h")
    } else if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{hours}h {mins}m")
    } else if secs >= 60 {
        let mins = secs / 60;
        let s = secs % 60;
        format!("{mins}m {s}s")
    } else {
        format!("{secs}s")
    }
}

/// Render a coloured bar for battery capacity.
fn capacity_bar(pct: u32) -> String {
    let filled = (pct / 5) as usize; // 20 chars wide, each = 5%.
    let empty = 20_usize.saturating_sub(filled);

    let colour = if pct <= 10 {
        "\x1b[31m" // red
    } else if pct <= 30 {
        "\x1b[33m" // yellow
    } else {
        "\x1b[32m" // green
    };

    let bar_filled: String = core::iter::repeat_n('#', filled).collect();
    let bar_empty: String = core::iter::repeat_n('-', empty).collect();

    format!("[{colour}{bar_filled}\x1b[0m{bar_empty}]")
}

// ============================================================================
// CLI entry point
// ============================================================================

fn print_usage() {
    println!("Slate OS Power Control v0.1.0");
    println!();
    println!("Manage system power state: shutdown, reboot, suspend, hibernate.");
    println!();
    println!("USAGE:");
    println!("  powerctl <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("  shutdown            Orderly shutdown and power off");
    println!("  halt                Alias for shutdown");
    println!("  reboot              Orderly reboot");
    println!("  suspend             ACPI S3 suspend to RAM");
    println!("  hibernate           ACPI S4 suspend to disk");
    println!("  status              Show power source, battery, ACPI info");
    println!("  schedule <m> <cmd>  Schedule shutdown/reboot in <m> minutes");
    println!("  cancel              Cancel a scheduled operation");
    println!();
    println!("EXAMPLES:");
    println!("  powerctl shutdown");
    println!("  powerctl schedule 30 shutdown");
    println!("  powerctl cancel");
    println!("  powerctl status");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    match args[1].as_str() {
        "shutdown" | "halt" | "poweroff" => cmd_shutdown(),
        "reboot" | "restart" => cmd_reboot(),
        "suspend" | "sleep" => cmd_suspend(),
        "hibernate" | "hib" => cmd_hibernate(),
        "status" | "info" => cmd_status(),
        "schedule" | "sched" => cmd_schedule(&args[2..]),
        "cancel" | "abort" => cmd_cancel(),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'powerctl help' for usage.");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn format_duration_days() {
        assert_eq!(format_duration(86400), "1d 0h");
        assert_eq!(format_duration(90000), "1d 1h");
    }

    #[test]
    fn capacity_bar_width_is_constant() {
        // The bar always renders 20 cells (each = 5%), regardless of charge.
        // Count '#' (filled) + '-' (empty), ignoring ANSI colour escapes.
        for pct in [0u32, 5, 50, 95, 100] {
            let bar = capacity_bar(pct);
            let cells = bar.chars().filter(|&c| c == '#' || c == '-').count();
            assert_eq!(cells, 20, "pct={pct} produced {cells} cells");
        }
    }

    #[test]
    fn capacity_bar_fill_scales_with_charge() {
        let full = capacity_bar(100);
        let empty = capacity_bar(0);
        assert_eq!(full.chars().filter(|&c| c == '#').count(), 20);
        assert_eq!(empty.chars().filter(|&c| c == '#').count(), 0);
        assert_eq!(empty.chars().filter(|&c| c == '-').count(), 20);
    }
}
