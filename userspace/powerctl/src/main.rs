//! Slate OS Power Management Utility
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

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::process;

// NOTE: Slate OS exposes NO userspace power-management syscall (there is no
// SYS_SHUTDOWN / SYS_REBOOT / suspend syscall in kernel/src/syscall/number.rs).
// System power state changes go through the service manager over IPC (the
// orderly_* path below).  The "direct" fallbacks therefore use the only
// non-IPC mechanism available — ACPI control files exposed via procfs/sysfs,
// if present — and report a clear error when nothing works.  See the
// power-management DESIGN GAP note in todo.txt.

// ============================================================================
// The service manager, over the service bus
// ============================================================================

/// The service manager's name on the service bus.
///
/// Nothing registers it yet (known-issues.md, "`org.slateos.ServiceManager`
/// has two clients and no provider"), so every request below ends at "no such
/// service" and the direct fallbacks run.
const SERVICE_MANAGER: &str = "org.slateos.ServiceManager";

/// How long to wait for the service manager's answer: 25 seconds, D-Bus's
/// default method-call timeout. It answers once it has *accepted* a request,
/// not when the machine has finished stopping.
const SERVICE_MANAGER_TIMEOUT_NS: u64 = libservicebus::secs_to_ns(25);

/// What the service manager said to a request.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    /// It accepted.
    Accepted,
    /// It refused: its error's name, and the explanation it sent, if any.
    Refused(String, Vec<u8>),
}

/// Ask the service manager to run `method` with `args`.
///
/// A service-bus method call with `libservicebus::fields` arguments: a return
/// means accepted, an error means refused, and an error's first field, if it
/// sent one, is its explanation. `Err` means the question could not be asked
/// at all -- no service manager, or no answer in time.
///
/// This replaced a hand-rolled channel client that called syscall 200 as
/// "open a channel to a service". 200 is `SYS_CHANNEL_CREATE`: it took the
/// name's *address* as its flags and returned a fresh channel connected to
/// nothing, so no request ever reached any service (lane F's
/// `requests/f-b-logind-refuses-every-caller-because-libservicebus-never-asks-who-it-is.md`,
/// point 3). Connecting by name is `SYS_SERVICE_CONNECT`, which
/// `libservicebus` wraps.
fn ask_service_manager(method: &str, args: &[&[u8]]) -> Result<Answer, String> {
    let mut conn = libservicebus::Connection::connect(SERVICE_MANAGER)
        .map_err(|e| format!("cannot reach {SERVICE_MANAGER}: {e}"))?;
    match conn.call_fields(method, args, SERVICE_MANAGER_TIMEOUT_NS) {
        Ok(libservicebus::Outcome::Done(_)) => Ok(Answer::Accepted),
        // A refusal that sent no explanation has an empty one; that is an
        // absence, not a failure to discard.
        Ok(libservicebus::Outcome::Refused { error, fields }) => Ok(Answer::Refused(
            error,
            fields.into_iter().next().unwrap_or_default(),
        )),
        Err(e) => Err(format!("{SERVICE_MANAGER} did not answer {method}: {e}")),
    }
}

/// Say on stderr that the service manager refused `what`, in its own words.
fn report_refusal(what: &str, error: &str, explanation: &[u8]) {
    // Nothing useful can be done if stderr itself is gone; the exit status
    // still says the request was refused.
    let _ = write_refusal(&mut std::io::stderr().lock(), what, error, explanation);
}

/// The refusal message, written to `out`.
///
/// The explanation goes out as the bytes it came in: it is another program's
/// text, and decoding it lossily would print something it did not say.
fn write_refusal(
    out: &mut impl std::io::Write,
    what: &str,
    error: &str,
    explanation: &[u8],
) -> std::io::Result<()> {
    write!(out, "error: the service manager refused {what}: {error}")?;
    if !explanation.is_empty() {
        out.write_all(b": ")?;
        out.write_all(explanation)?;
    }
    out.write_all(b"\n")
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
/// Seconds since boot, or `None` if that cannot be determined.
///
/// The `None` matters. Both callers used to spell this
/// `…parse::<f64>().ok()).unwrap_or(0.0) as u64`, which turns "I cannot read
/// the clock" into "the machine booted this instant" -- and both callers were
/// measuring a *deadline* against it. In [`write_schedule`] that silently
/// produced a target of `minutes * 60`, i.e. a moment already long past on any
/// machine that had been up a while; in `cmd_status` it made the remaining
/// time read as the entire target. For a value guarding a shutdown, "I do not
/// know" and a specific number must not be the same value.
fn read_uptime() -> Option<u64> {
    let raw = read_file("/proc/uptime")?;
    let secs = raw.split_whitespace().next()?.parse::<f64>().ok()?;
    if secs.is_finite() && secs >= 0.0 {
        Some(secs as u64)
    } else {
        None
    }
}

/// What a schedule file says about *now*.
///
/// Split out from `cmd_status` so the three cases can be tested without a
/// `/proc`. The old code computed `target.saturating_sub(uptime)` and printed
/// only when the result was non-zero, which collapsed two of these into
/// silence: an unreadable clock and an overdue deadline both printed nothing,
/// or worse, an unreadable clock printed the full target as the time
/// remaining.
#[derive(Debug, PartialEq, Eq)]
enum ScheduleState {
    /// The deadline is this many seconds away.
    Pending(u64),
    /// The deadline passed this many seconds ago and nothing acted on it.
    Overdue(u64),
    /// `/proc/uptime` is unreadable, so the distance is not computable.
    UnknownClock,
}

/// Classify a stored deadline against the current uptime.
fn schedule_state(target: u64, now: Option<u64>) -> ScheduleState {
    let Some(now) = now else {
        return ScheduleState::UnknownClock;
    };
    if target > now {
        ScheduleState::Pending(target.saturating_sub(now))
    } else {
        ScheduleState::Overdue(now.saturating_sub(target))
    }
}

fn write_schedule(minutes: u64, action: &str) -> Result<(), String> {
    // A deadline measured from an unknown origin is not a deadline. Refusing
    // here is what stops `powerctl schedule` reporting success on a target
    // that means "already due".
    let Some(uptime_secs) = read_uptime() else {
        return Err(
            "cannot read /proc/uptime, so there is no clock to schedule against".to_string(),
        );
    };

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

/// What came of asking the service manager for a power change.
///
/// Three outcomes where there used to be a `bool`. The manager's answer was a
/// string then, accepted unless it began `ERR`, and a refusal came back as
/// `false` -- the same as nobody answering -- so it fell through to the
/// direct fallback, which forced the very change the system had declined. On
/// the bus a refusal is its own kind of message, and it stops here.
#[derive(Debug, PartialEq, Eq)]
enum Orderly {
    /// It accepted, and drives the rest of the sequence.
    Accepted,
    /// It refused, and that has been said on stderr. Not overridden: the
    /// direct fallback would do exactly what the system just declined to do.
    Refused,
    /// There was nobody to ask, which has been said; the direct fallback is
    /// the only way left.
    Unavailable,
}

/// Ask the service manager for an orderly power change, `method` on the bus
/// (`what` names it in messages).
///
/// The sequence is: stop all services in dependency order, sync filesystems,
/// then trigger the final power state change. The service manager handles
/// the ordering; this sends the request and waits for it to be accepted.
fn orderly(method: &str, what: &str) -> Orderly {
    match ask_service_manager(method, &[]) {
        Ok(Answer::Accepted) => Orderly::Accepted,
        Ok(Answer::Refused(error, why)) => {
            report_refusal(what, &error, &why);
            Orderly::Refused
        }
        Err(e) => {
            eprintln!("powerctl: {e}");
            Orderly::Unavailable
        }
    }
}

// ============================================================================
// Direct-syscall fallbacks
// ============================================================================

/// Flush every filesystem to stable storage before the machine stops.
///
/// # What this replaces, and why it mattered
///
/// It wrote `"1"` to `/proc/sys/vm/sync`, and if that failed, to
/// `/sys/kernel/sync`. **Neither path exists** -- neither is a real Linux
/// interface and this kernel serves neither -- so both writes failed, the
/// function returned *silently*, and [`direct_shutdown`], [`direct_reboot`]
/// and [`direct_hibernate`] went on to stop the machine with dirty buffers
/// unflushed.
///
/// The `if …is_ok() { return; }` looked like a check and was one, but of the
/// wrong thing: it asked whether a write to a nonexistent file had succeeded,
/// not whether a sync had happened. A test of the wrong proposition reads
/// exactly like a test of the right one.
///
/// `libcall::sync` issues the kernel's `SYS_FS_SYNC` -- the same flush
/// `fsync(2)` performs, but for every mounted filesystem rather than one
/// descriptor, which is a valid superset of POSIX's `sync(2)` guarantee. It
/// has been available the whole time.
///
/// Through `libcall` rather than `posix` directly, and the difference is not
/// cosmetic. **For its first two hours this function called
/// `posix::unistd::sync()` as a Rust path, and that flushed exactly as much as
/// the two invented writes it replaced: nothing.** The commit that made the
/// change said it had closed a data-loss path. It had not; `libcall` did,
/// later the same morning.
///
/// The reason is worth stating precisely, because the general rule
/// (`design-decisions.md` 768: the `posix` rlib is a second libc with its
/// syscalls stubbed to `-ENOSYS`) is *milder* than what happens here.
/// `posix::unistd::sync` is `pub extern "C" fn sync()` whose entire body is
/// one block gated `#[cfg(target_os = "none")]`. A SlateOS program is built
/// for `target_os = "linux"`, so the rlib copy is **an empty function**. Not a
/// stub that returns `-ENOSYS`, which a caller could at least test for -- a
/// `void` function with no statements in it. There is no error, no return
/// value, and nothing to check.
///
/// Found by lane A, from the source and the `cfg` rather than by running it,
/// while looking at *why* the gate count had moved instead of reporting that
/// it had. The same reading applies to every `pub extern "C"` entry point in
/// `posix` whose body is gated that way; the gate
/// `scripts/check-one-libc-per-process.py` now refuses the Rust path to all of
/// them, which is what makes this unrepeatable rather than merely known.
///
/// # There is nothing to check
///
/// `sync(2)` returns `void`: POSIX defines it as scheduling the writes, with
/// no failure to report. So this function cannot tell its callers whether the
/// flush reached the platter, and neither could either of the two things it
/// replaced -- the difference is that this one actually asks the kernel.
///
/// That property is why both earlier versions survived review. A call that
/// cannot report failure looks identical whether it works or not, so the only
/// way to know is to read what is on the other side of it. Twice, nobody
/// did.
///
/// Not called from [`direct_suspend`], deliberately: suspend keeps RAM powered
/// and the buffers with it, so there is nothing to flush and a needless full
/// sync would only delay the suspend.
fn try_sync_filesystems() {
    libcall::sync();
}

/// Power off the machine directly when the service manager is unreachable.
///
/// Slate OS has no power-off syscall, so the only non-IPC mechanism is the ACPI
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
    // Slate OS has no suspend syscall; the only direct path is an ACPI sleep
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

    // Slate OS has no hibernate syscall; the only direct path is an ACPI sleep
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

/// Ask for `method`; on acceptance say so and return `true`, on refusal exit
/// non-zero, and on no answer return `false` so the caller falls back.
fn ask_or_fall_back(method: &str, what: &str) -> bool {
    match orderly(method, what) {
        Orderly::Accepted => {
            // The service manager drives the rest of the sequence; it stops
            // services and syncs filesystems before the final transition.
            println!("Service manager acknowledged {what}.");
            true
        }
        Orderly::Refused => process::exit(1),
        Orderly::Unavailable => false,
    }
}

fn cmd_shutdown() {
    println!("Initiating system shutdown...");
    if ask_or_fall_back("PowerOff", "shutdown") {
        return;
    }
    eprintln!("Service manager unavailable -- falling back to direct shutdown.");
    eprintln!("Warning: services may not be stopped cleanly.");
    direct_shutdown();
}

fn cmd_reboot() {
    println!("Initiating system reboot...");
    if ask_or_fall_back("Reboot", "reboot") {
        return;
    }
    eprintln!("Service manager unavailable -- falling back to direct reboot.");
    eprintln!("Warning: services may not be stopped cleanly.");
    direct_reboot();
}

fn cmd_suspend() {
    println!("Suspending system (ACPI S3)...");
    if ask_or_fall_back("Suspend", "suspend") {
        return;
    }
    eprintln!("Service manager unavailable -- falling back to direct suspend.");
    direct_suspend();
    println!("Resumed from suspend.");
}

fn cmd_hibernate() {
    println!("Hibernating system (ACPI S4)...");
    if ask_or_fall_back("Hibernate", "hibernate") {
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
    if let Some(secs) = read_uptime() {
        println!("  Uptime:        {}", format_duration(secs));
    }

    // Scheduled operation.
    if let Some((target, action)) = read_schedule() {
        match schedule_state(target, read_uptime()) {
            ScheduleState::Pending(remaining) => {
                println!(
                    "  Scheduled:     {action} in {}",
                    format_duration(remaining)
                );
            }
            ScheduleState::Overdue(late) => {
                println!(
                    "  Scheduled:     {action} was due {} ago and did not run",
                    format_duration(late)
                );
            }
            ScheduleState::UnknownClock => {
                println!(
                    "  Scheduled:     {action}, time remaining unknown (/proc/uptime unreadable)"
                );
            }
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
            eprintln!(
                "error: unknown action {} (expected shutdown or reboot)",
                quoteaf_os(other)
            );
            process::exit(1);
        }
    };

    match write_schedule(minutes, action) {
        Ok(()) => {
            // Ask the service manager to arm its timer *before* saying
            // anything, because whether it accepted is the whole difference
            // between a schedule and a file. This used to print "Scheduled
            // shutdown in 5 minutes." and "Run 'powerctl cancel' to abort."
            // first, and demote a total failure to a trailing "note:" -- so
            // the one case where the machine will certainly not shut down
            // read as the success case with a footnote.
            let minutes_arg = minutes.to_string();
            let plural = if minutes == 1 { "" } else { "s" };
            match ask_service_manager(
                "SchedulePower",
                &[minutes_arg.as_bytes(), action.as_bytes()],
            ) {
                Ok(Answer::Accepted) => {
                    println!("Scheduled {action} in {minutes} minute{plural}.");
                    println!("Run 'powerctl cancel' to abort.");
                }
                Ok(Answer::Refused(error, why)) => {
                    report_refusal("the schedule", &error, &why);
                    // The record was written before asking; a refused
                    // schedule must not linger in `powerctl status` as one
                    // that is merely waiting.
                    if let Err(e) = cancel_schedule() {
                        eprintln!("error: {SCHEDULE_FILE} could not be removed: {e}");
                    }
                    eprintln!("Nothing is scheduled; the machine will not {action} on its own.");
                    process::exit(1);
                }
                Err(e) => {
                    eprintln!("error: no service manager to run the schedule ({e}).");
                    eprintln!(
                        "The request is recorded in {SCHEDULE_FILE} and 'powerctl status' \
                         will show it, but nothing exists to fire it: the machine will NOT \
                         {action} on its own. Run 'powerctl {action}' when you want it."
                    );
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_cancel() {
    // The service manager first: if it armed a timer and will not disarm it,
    // the machine will still act, and "cancelled" would be false. No service
    // manager at all is not a failure here -- then there is no timer, and the
    // record below is all there is to cancel.
    if let Ok(Answer::Refused(error, why)) = ask_service_manager("CancelScheduledPower", &[]) {
        report_refusal("the cancellation", &error, &why);
        eprintln!("The scheduled operation is still pending.");
        process::exit(1);
    }

    match cancel_schedule() {
        Ok(()) => println!("Scheduled operation cancelled."),
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

/// `powerctl reload`: restart SlateOS without restarting the computer -- the
/// start menu's "Restart OS, keep the computer on" (`design.txt`; lane C's
/// `requests/c-ab-a-restart-that-keeps-the-computer-on.md`).
///
/// It needs a kernel call that loads the installed kernel and starts it
/// without going back through the firmware (Linux's `kexec`), and this kernel
/// has none yet -- asked of lane A in the same request. So it refuses, and
/// refuses *first*: asking the service manager to stop everything and only
/// then finding nothing to restart into would leave the machine running with
/// its services stopped. When the call exists this takes `cmd_reboot`'s
/// shape: the orderly stop (a `Reload` request on the service bus), then that
/// call where `direct_reboot` asks the firmware.
fn cmd_reload() {
    eprintln!(
        "powerctl: this kernel cannot restart without the firmware, so SlateOS \
         cannot be restarted with the computer kept on"
    );
    eprintln!("Nothing was stopped. Run 'powerctl reboot' to restart the computer.");
    process::exit(1);
}

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
    println!("  reload              Restart SlateOS, keeping the computer on (not yet:");
    println!("                      this kernel cannot restart without the firmware)");
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
        "reload" => cmd_reload(),
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

    /// The deadline is in the future.
    #[test]
    fn a_future_deadline_is_pending_with_the_distance_to_it() {
        assert_eq!(schedule_state(600, Some(300)), ScheduleState::Pending(300));
        assert_eq!(schedule_state(1, Some(0)), ScheduleState::Pending(1));
    }

    /// A deadline that has passed is reported, not silently dropped.
    ///
    /// The old code printed the schedule only when `target - uptime` was
    /// non-zero, so the moment a deadline passed the line vanished from
    /// `powerctl status` -- while the file was still there and `powerctl
    /// cancel` still had something to cancel. Since nothing in the system
    /// currently fires these deadlines, *every* schedule ends up here.
    #[test]
    fn a_passed_deadline_is_overdue_not_silence() {
        assert_eq!(schedule_state(300, Some(600)), ScheduleState::Overdue(300));
        // Exactly due counts as overdue: the instant is not in the future.
        assert_eq!(schedule_state(300, Some(300)), ScheduleState::Overdue(0));
    }

    /// An unreadable clock is its own answer, not zero.
    ///
    /// This is the case the whole change exists for. With `unwrap_or(0.0)`,
    /// `schedule_state(600, None)` would have been `Pending(600)` -- a
    /// confident ten minutes derived from no clock at all.
    #[test]
    fn an_unreadable_clock_is_not_a_reading_of_zero() {
        assert_eq!(schedule_state(600, None), ScheduleState::UnknownClock);
        assert_ne!(schedule_state(600, None), schedule_state(600, Some(0)));
    }

    /// No service manager is "unavailable", which falls back -- never
    /// "refused", which would stop a shutdown nobody declined.
    ///
    /// On a development host the bus answers "not supported" to everything,
    /// which is the same outcome as no service manager registered on SlateOS:
    /// the question could not be asked.
    #[test]
    fn no_service_manager_is_unavailable_not_refused() {
        let err = ask_service_manager("PowerOff", &[]).unwrap_err();
        assert!(err.contains(SERVICE_MANAGER), "{err}");
        assert_eq!(orderly("PowerOff", "shutdown"), Orderly::Unavailable);
    }

    /// A refusal is reported in the service manager's own bytes.
    #[test]
    fn a_refusal_is_reported_as_it_was_sent() {
        let mut out = Vec::new();
        write_refusal(
            &mut out,
            "shutdown",
            "org.slateos.Error.Inhibited",
            b"held by \xffupdate",
        )
        .unwrap();
        assert_eq!(
            out,
            b"error: the service manager refused shutdown: org.slateos.Error.Inhibited: \
              held by \xffupdate\n"
        );

        let mut out = Vec::new();
        write_refusal(&mut out, "reboot", "org.slateos.Error.Denied", b"").unwrap();
        assert_eq!(
            out,
            b"error: the service manager refused reboot: org.slateos.Error.Denied\n"
        );
    }

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
