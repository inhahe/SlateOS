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
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{self, ExitCode};

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
        // The kernel names its power supplies in ASCII; a name that is not
        // text is not one of them.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
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
        // Whole seconds of a finite, non-negative reading; `as` saturates a
        // value past `u64::MAX`, which no uptime reaches.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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

    // Ensure the parent directory exists. If it cannot be made, the write
    // below fails and says why, which is the error worth reporting.
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
    cancel_schedule_at(Path::new(SCHEDULE_FILE))
}

/// Remove the schedule record at `path`.
///
/// Only a record that is not there is "nothing to cancel". Every failure used
/// to be reported that way, so `powerctl cancel` without permission to remove
/// the file said nothing was scheduled -- while the record stayed, and
/// `powerctl status` went on showing it.
fn cancel_schedule_at(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("no scheduled operation to cancel".to_string())
        }
        Err(e) => Err(format!(
            "cannot remove {}: {}",
            path.display(),
            errmsg::strerror(&e)
        )),
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
            warn(&format!("powerctl: {e}\n"));
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

    warn(
        "powerctl: cannot power off directly — the service manager is \
         unreachable and Slate OS exposes no power-off syscall or ACPI control \
         file.  System NOT powered off.\n",
    );
    process::exit(1);
}

/// Reboot the machine directly when the service manager is unreachable.
fn direct_reboot() -> ! {
    try_sync_filesystems();

    // Best-effort: a procfs ACPI knob, if present, reboots.
    let _ = fs::write("/proc/acpi/power", "reboot");

    warn(
        "powerctl: cannot reboot directly — the service manager is \
         unreachable and Slate OS exposes no reboot syscall or ACPI control \
         file.  System NOT rebooted.\n",
    );
    process::exit(1);
}

/// Enter ACPI S3 suspend directly when the service manager is unreachable.
fn direct_suspend() {
    // Slate OS has no suspend syscall; the only direct path is an ACPI sleep
    // control file, if procfs/sysfs exposes one.
    if fs::write("/proc/acpi/sleep", "S3").is_err() && fs::write("/sys/power/state", "mem").is_err()
    {
        warn(
            "powerctl: cannot suspend — no ACPI sleep control file (is ACPI S3 \
             supported and exposed by the kernel?).\n",
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
        warn(
            "powerctl: cannot hibernate — no ACPI sleep control file (is a swap \
             partition configured and hibernate supported?).\n",
        );
        process::exit(1);
    }
}

// ============================================================================
// Output
// ============================================================================

/// Write `text` to standard output.
///
/// Not `println!`, which panics when the write fails -- a closed or full
/// standard output, `powerctl status | head -1` -- and a panic in an action
/// would end it before the action: `powerctl shutdown >/dev/full` died saying
/// "Initiating system shutdown..." could not be printed, and did not shut down.
/// Whether a failure matters is the caller's to say: see [`note`] and
/// [`report`].
fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// An action's progress note, on standard output. Lost if standard output
/// is: the note is about the action, and not being able to say it is no
/// reason not to do it.
fn note(text: &str) {
    // Deliberately ignored -- see above. The action's outcome is in the exit
    // status either way.
    let _ = write_stdout(text);
}

/// A report -- `status`, the usage -- on standard output, where it is the
/// whole point: a failure to write it is the command failing.
fn report(text: &str) -> ExitCode {
    match write_stdout(text) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            warn(&format!(
                "powerctl: write error: {}\n",
                errmsg::strerror(&e)
            ));
            ExitCode::FAILURE
        }
    }
}

/// A diagnostic, on standard error. Not `eprintln!`, which panics if that
/// cannot be written either; a diagnostic nobody can see is still no reason
/// to stop.
fn warn(text: &str) {
    // Nothing else is left to report the failure on.
    let _ = std::io::stderr().lock().write_all(text.as_bytes());
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
            note(&format!("Service manager acknowledged {what}.\n"));
            true
        }
        Orderly::Refused => process::exit(1),
        Orderly::Unavailable => false,
    }
}

fn cmd_shutdown() {
    note("Initiating system shutdown...\n");
    if ask_or_fall_back("PowerOff", "shutdown") {
        return;
    }
    warn("Service manager unavailable -- falling back to direct shutdown.\n");
    warn("Warning: services may not be stopped cleanly.\n");
    direct_shutdown();
}

fn cmd_reboot() {
    note("Initiating system reboot...\n");
    if ask_or_fall_back("Reboot", "reboot") {
        return;
    }
    warn("Service manager unavailable -- falling back to direct reboot.\n");
    warn("Warning: services may not be stopped cleanly.\n");
    direct_reboot();
}

fn cmd_suspend() {
    note("Suspending system (ACPI S3)...\n");
    if ask_or_fall_back("Suspend", "suspend") {
        return;
    }
    warn("Service manager unavailable -- falling back to direct suspend.\n");
    direct_suspend();
    note("Resumed from suspend.\n");
}

fn cmd_hibernate() {
    note("Hibernating system (ACPI S4)...\n");
    if ask_or_fall_back("Hibernate", "hibernate") {
        return;
    }
    warn("Service manager unavailable -- falling back to direct hibernate.\n");
    direct_hibernate();
    note("Resumed from hibernate.\n");
}

fn cmd_status() -> ExitCode {
    let ps = read_power_status();
    // ACPI state from /proc/acpi/state or /sys/power/state.
    let acpi_state = read_file("/sys/power/state")
        .or_else(|| read_file("/proc/acpi/state"))
        .unwrap_or_else(|| "unknown".to_string());
    let uptime = read_uptime();
    report(&render_status(&ps, &acpi_state, uptime, read_schedule()))
}

/// What `powerctl status` says, from what it read.
fn render_status(
    ps: &PowerStatus,
    acpi_state: &str,
    uptime: Option<u64>,
    schedule: Option<(u64, String)>,
) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    // `write!` into a `String` cannot fail; the results are ignored for that
    // reason throughout.
    let _ = writeln!(s, "Power Status");
    let _ = writeln!(s, "============");
    let _ = writeln!(s);

    // AC adapter.
    let _ = match ps.ac_online {
        Some(true) => writeln!(s, "  Power source:  \x1b[32mAC (plugged in)\x1b[0m"),
        Some(false) => writeln!(s, "  Power source:  \x1b[33mBattery\x1b[0m"),
        None => writeln!(s, "  Power source:  unknown (no AC adapter detected)"),
    };
    let _ = writeln!(s, "  ACPI states:   {acpi_state}");
    if let Some(secs) = uptime {
        let _ = writeln!(s, "  Uptime:        {}", format_duration(secs));
    }

    // Scheduled operation.
    if let Some((target, action)) = schedule {
        let _ = match schedule_state(target, uptime) {
            ScheduleState::Pending(remaining) => writeln!(
                s,
                "  Scheduled:     {action} in {}",
                format_duration(remaining)
            ),
            ScheduleState::Overdue(late) => writeln!(
                s,
                "  Scheduled:     {action} was due {} ago and did not run",
                format_duration(late)
            ),
            ScheduleState::UnknownClock => writeln!(
                s,
                "  Scheduled:     {action}, time remaining unknown (/proc/uptime unreadable)"
            ),
        };
    }

    // Batteries.
    if ps.batteries.is_empty() {
        let _ = writeln!(s);
        let _ = writeln!(s, "  No batteries detected (desktop or VM).");
    }
    for bat in &ps.batteries {
        let _ = writeln!(s);
        let _ = writeln!(s, "  Battery: {}", bat.name);
        let _ = writeln!(s, "    Status:      {}", bat.status);
        if let Some(pct) = bat.capacity_pct {
            let _ = writeln!(s, "    Capacity:    {pct}% {}", capacity_bar(pct));
        } else if let (Some(now), Some(full)) = (bat.energy_now_uj, bat.energy_full_uj) {
            // Computed from energy readings when the capacity sysfs node is
            // absent. checked_mul/checked_div avoid both overflow on large
            // energy values and division by zero; a percentage too large for
            // `u32` is not a reading.
            if let Some(pct) = now
                .checked_mul(100)
                .and_then(|scaled| scaled.checked_div(full))
                .and_then(|p| u32::try_from(p).ok())
            {
                let _ = writeln!(
                    s,
                    "    Capacity:    {pct}% {} (computed)",
                    capacity_bar(pct.min(100))
                );
            }
        }
        if let Some(uv) = bat.voltage_now_uv {
            // Microvolts to volts, for display: the precision a float loses
            // above 2^53 µV is far below the two decimals printed.
            #[allow(clippy::cast_precision_loss)]
            let volts = uv as f64 / 1_000_000.0;
            let _ = writeln!(s, "    Voltage:     {volts:.2} V");
        }
        let _ = writeln!(s, "    Technology:  {}", bat.technology);
    }
    s
}

fn cmd_schedule(minutes: u64, action: &'static str) {
    if let Err(e) = write_schedule(minutes, action) {
        warn(&format!("error: {e}\n"));
        process::exit(1);
    }
    // Ask the service manager to arm its timer *before* saying anything,
    // because whether it accepted is the whole difference between a schedule
    // and a file. This used to print "Scheduled shutdown in 5 minutes." and
    // "Run 'powerctl cancel' to abort." first, and demote a total failure to
    // a trailing "note:" -- so the one case where the machine will certainly
    // not shut down read as the success case with a footnote.
    let minutes_arg = minutes.to_string();
    let plural = if minutes == 1 { "" } else { "s" };
    match ask_service_manager(
        "SchedulePower",
        &[minutes_arg.as_bytes(), action.as_bytes()],
    ) {
        Ok(Answer::Accepted) => {
            note(&format!(
                "Scheduled {action} in {minutes} minute{plural}.\nRun 'powerctl cancel' to abort.\n"
            ));
        }
        Ok(Answer::Refused(error, why)) => {
            report_refusal("the schedule", &error, &why);
            // The record was written before asking; a refused schedule must
            // not linger in `powerctl status` as one that is merely waiting.
            if let Err(e) = cancel_schedule() {
                warn(&format!(
                    "error: {SCHEDULE_FILE} could not be removed: {e}\n"
                ));
            }
            warn(&format!(
                "Nothing is scheduled; the machine will not {action} on its own.\n"
            ));
            process::exit(1);
        }
        Err(e) => {
            warn(&format!(
                "error: no service manager to run the schedule ({e}).\n\
                 The request is recorded in {SCHEDULE_FILE} and 'powerctl status' will show \
                 it, but nothing exists to fire it: the machine will NOT {action} on its own. \
                 Run 'powerctl {action}' when you want it.\n"
            ));
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
        warn("The scheduled operation is still pending.\n");
        process::exit(1);
    }
    match cancel_schedule() {
        Ok(()) => note("Scheduled operation cancelled.\n"),
        Err(e) => {
            warn(&format!("error: {e}\n"));
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
    // 20 cells wide, each 5%; a capacity over 100 fills it and no more.
    let filled = usize::try_from(pct / 5).unwrap_or(usize::MAX).min(20);
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
    warn(
        "powerctl: this kernel cannot restart without the firmware, so SlateOS \
         cannot be restarted with the computer kept on\n\
         Nothing was stopped. Run 'powerctl reboot' to restart the computer.\n",
    );
    process::exit(1);
}

const USAGE: &str = "\
Slate OS Power Control v0.1.0

Manage system power state: shutdown, reboot, suspend, hibernate.

USAGE:
  powerctl <command> [args]

COMMANDS:
  shutdown            Orderly shutdown and power off
  halt                Alias for shutdown
  reboot              Orderly reboot
  reload              Restart SlateOS, keeping the computer on (not yet:
                      this kernel cannot restart without the firmware)
  suspend             ACPI S3 suspend to RAM
  hibernate           ACPI S4 suspend to disk
  status              Show power source, battery, ACPI info
  schedule <m> <cmd>  Schedule shutdown/reboot in <m> minutes
  cancel              Cancel a scheduled operation

EXAMPLES:
  powerctl shutdown
  powerctl schedule 30 shutdown
  powerctl cancel
  powerctl status
";

/// A command line, as `powerctl` reads it.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// No command, or help asked for anywhere on the line.
    Usage,
    Shutdown,
    Reboot,
    Reload,
    Suspend,
    Hibernate,
    Status,
    Cancel,
    Schedule {
        minutes: u64,
        action: &'static str,
    },
}

/// Why a command line was refused: what to say on standard error. Nothing
/// has been done when one is returned.
#[derive(Debug, PartialEq, Eq)]
struct Refusal(String);

/// Read the arguments after the program's name.
///
/// They are `OsString`s because a SlateOS argument may be any bytes but NUL,
/// and `env::args()` panicked on one that was not UTF-8 -- before a word of
/// this ran. Every word `powerctl` understands is ASCII, so each is compared
/// as bytes, and anything else is quoted when it is named back.
///
/// **An argument the command does not take refuses the command.** They were
/// ignored: `powerctl reboot --help` rebooted the machine, as did `powerctl
/// shutdown --dry-run` -- for a command whose effect is turning the computer
/// off, a word it did not expect has to stop it, not be stepped over. So
/// `--help` or `-h` anywhere asks for the usage and does nothing else, and
/// any other extra word is an error.
fn parse(args: &[OsString]) -> Result<Command, Refusal> {
    let Some((first, rest)) = args.split_first() else {
        return Ok(Command::Usage);
    };
    if args
        .iter()
        .any(|a| matches!(a.as_encoded_bytes(), b"--help" | b"-h"))
    {
        return Ok(Command::Usage);
    }
    let command = match first.as_encoded_bytes() {
        b"shutdown" | b"halt" | b"poweroff" => Command::Shutdown,
        b"reboot" | b"restart" => Command::Reboot,
        b"suspend" | b"sleep" => Command::Suspend,
        b"hibernate" | b"hib" => Command::Hibernate,
        b"status" | b"info" => Command::Status,
        b"cancel" | b"abort" => Command::Cancel,
        b"reload" => Command::Reload,
        b"help" => Command::Usage,
        b"schedule" | b"sched" => return parse_schedule(rest),
        _ => {
            return Err(Refusal(format!(
                "unknown command: {}\nRun 'powerctl help' for usage.\n",
                quoteaf_os(first)
            )));
        }
    };
    match rest.first() {
        None => Ok(command),
        Some(extra) => Err(unexpected(extra)),
    }
}

/// `schedule <minutes> <shutdown|reboot>`'s two operands, and no more.
fn parse_schedule(rest: &[OsString]) -> Result<Command, Refusal> {
    let [minutes, action, more @ ..] = rest else {
        return Err(Refusal(
            "usage: powerctl schedule <minutes> <shutdown|reboot>\n".to_string(),
        ));
    };
    if let Some(extra) = more.first() {
        return Err(unexpected(extra));
    }
    let Some(minutes) = minutes
        .to_str()
        .and_then(|m| m.parse::<u64>().ok())
        .filter(|&m| m > 0)
    else {
        return Err(Refusal(
            "error: minutes must be a positive integer\n".to_string(),
        ));
    };
    let action = match action.as_encoded_bytes() {
        b"shutdown" | b"halt" | b"poweroff" => "shutdown",
        b"reboot" | b"restart" => "reboot",
        _ => {
            return Err(Refusal(format!(
                "error: unknown action {} (expected shutdown or reboot)\n",
                quoteaf_os(action)
            )));
        }
    };
    Ok(Command::Schedule { minutes, action })
}

/// The refusal for a word the command does not take.
fn unexpected(extra: &OsStr) -> Refusal {
    Refusal(format!(
        "error: unexpected argument {} -- nothing was done\nRun 'powerctl help' for usage.\n",
        quoteaf_os(extra)
    ))
}

fn main() -> ExitCode {
    stdfdguard::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match parse(&args) {
        Err(Refusal(why)) => {
            warn(&why);
            ExitCode::FAILURE
        }
        Ok(Command::Usage) => report(USAGE),
        Ok(Command::Status) => cmd_status(),
        Ok(command) => {
            match command {
                Command::Shutdown => cmd_shutdown(),
                Command::Reboot => cmd_reboot(),
                Command::Reload => cmd_reload(),
                Command::Suspend => cmd_suspend(),
                Command::Hibernate => cmd_hibernate(),
                Command::Cancel => cmd_cancel(),
                Command::Schedule { minutes, action } => cmd_schedule(minutes, action),
                // Answered above.
                Command::Usage | Command::Status => {}
            }
            ExitCode::SUCCESS
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

    fn words(w: &[&str]) -> Vec<OsString> {
        w.iter().map(OsString::from).collect()
    }

    /// An argument that is not UTF-8 -- legal on SlateOS, where an argument
    /// may hold any byte but NUL.
    fn not_utf8() -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"re\xffboot".to_vec())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[u16::from(b'r'), 0xD800, u16::from(b't')])
        }
    }

    #[test]
    fn every_command_and_alias_is_read() {
        for (w, want) in [
            ("shutdown", Command::Shutdown),
            ("halt", Command::Shutdown),
            ("poweroff", Command::Shutdown),
            ("reboot", Command::Reboot),
            ("restart", Command::Reboot),
            ("suspend", Command::Suspend),
            ("sleep", Command::Suspend),
            ("hibernate", Command::Hibernate),
            ("hib", Command::Hibernate),
            ("status", Command::Status),
            ("info", Command::Status),
            ("cancel", Command::Cancel),
            ("abort", Command::Cancel),
            ("reload", Command::Reload),
            ("help", Command::Usage),
        ] {
            assert_eq!(parse(&words(&[w])), Ok(want), "{w}");
        }
        assert_eq!(parse(&[]), Ok(Command::Usage));
        assert_eq!(
            parse(&words(&["sched", "5", "restart"])),
            Ok(Command::Schedule {
                minutes: 5,
                action: "reboot"
            })
        );
    }

    /// A word the command does not take stops it. `powerctl reboot --help`
    /// rebooted the machine; now it shows the usage and does nothing else.
    #[test]
    fn an_extra_word_never_reaches_the_action() {
        for line in [
            &["reboot", "--help"][..],
            &["shutdown", "-h"],
            &["--help", "shutdown"],
            &["schedule", "5", "shutdown", "--help"],
        ] {
            assert_eq!(parse(&words(line)), Ok(Command::Usage), "{line:?}");
        }
        for line in [
            &["shutdown", "--dry-run"][..],
            &["reboot", "now"],
            &["status", "-v"],
            &["schedule", "5", "shutdown", "now"],
        ] {
            let Err(Refusal(why)) = parse(&words(line)) else {
                panic!("{line:?} was accepted");
            };
            assert!(why.starts_with("error: unexpected argument "), "{why}");
            assert!(why.contains("nothing was done"), "{why}");
        }
    }

    /// An argument that is not text is refused and named, not a crash.
    #[test]
    fn an_argument_that_is_not_text_is_refused_not_a_panic() {
        let Err(Refusal(why)) = parse(&[not_utf8()]) else {
            panic!("accepted");
        };
        assert!(why.starts_with("unknown command: "), "{why}");
        let Err(Refusal(why)) = parse(&[OsString::from("reboot"), not_utf8()]) else {
            panic!("accepted");
        };
        assert!(why.starts_with("error: unexpected argument "), "{why}");
        let Err(Refusal(why)) = parse(&[OsString::from("schedule"), not_utf8(), "reboot".into()])
        else {
            panic!("accepted");
        };
        assert_eq!(why, "error: minutes must be a positive integer\n");
        let Err(Refusal(why)) = parse(&[OsString::from("schedule"), "5".into(), not_utf8()]) else {
            panic!("accepted");
        };
        assert!(why.starts_with("error: unknown action "), "{why}");
    }

    #[test]
    fn schedule_wants_two_operands_and_a_positive_count() {
        let refused = |line: &[&str]| match parse(&words(line)) {
            Err(Refusal(why)) => why,
            Ok(c) => panic!("{line:?} gave {c:?}"),
        };
        assert_eq!(
            refused(&["schedule"]),
            "usage: powerctl schedule <minutes> <shutdown|reboot>\n"
        );
        assert_eq!(
            refused(&["schedule", "5"]),
            "usage: powerctl schedule <minutes> <shutdown|reboot>\n"
        );
        assert_eq!(
            refused(&["schedule", "0", "reboot"]),
            "error: minutes must be a positive integer\n"
        );
        assert_eq!(
            refused(&["schedule", "-5", "reboot"]),
            "error: minutes must be a positive integer\n"
        );
        assert_eq!(
            refused(&["schedule", "5", "suspend"]),
            "error: unknown action 'suspend' (expected shutdown or reboot)\n"
        );
        assert_eq!(
            refused(&["schedule", "5", "a b"]),
            "error: unknown action 'a b' (expected shutdown or reboot)\n"
        );
    }

    #[test]
    fn status_reports_what_it_read() {
        let ps = PowerStatus {
            ac_online: Some(false),
            batteries: vec![BatteryInfo {
                name: "BAT0".into(),
                status: "Discharging".into(),
                capacity_pct: None,
                energy_now_uj: Some(25_000_000),
                energy_full_uj: Some(50_000_000),
                voltage_now_uv: Some(11_900_000),
                technology: "Li-ion".into(),
            }],
        };
        let s = render_status(&ps, "mem disk", Some(3700), Some((4000, "reboot".into())));
        assert!(s.contains("Power source:  \x1b[33mBattery"), "{s}");
        assert!(s.contains("ACPI states:   mem disk\n"), "{s}");
        assert!(s.contains("Uptime:        1h 1m\n"), "{s}");
        assert!(s.contains("Scheduled:     reboot in 5m 0s\n"), "{s}");
        assert!(s.contains("Capacity:    50% "), "{s}");
        assert!(s.contains("(computed)"), "{s}");
        assert!(s.contains("Voltage:     11.90 V\n"), "{s}");
        // Readings too large to scale are left out, not wrapped.
        let huge = PowerStatus {
            ac_online: None,
            batteries: vec![BatteryInfo {
                name: "BAT1".into(),
                status: "Unknown".into(),
                capacity_pct: None,
                energy_now_uj: Some(u64::MAX),
                energy_full_uj: Some(0),
                voltage_now_uv: None,
                technology: "Unknown".into(),
            }],
        };
        let s = render_status(&huge, "unknown", None, Some((10, "shutdown".into())));
        assert!(!s.contains("Capacity"), "{s}");
        assert!(s.contains("time remaining unknown"), "{s}");
        assert!(s.contains("unknown (no AC adapter detected)"), "{s}");
    }

    /// Only a record that is not there is "nothing to cancel".
    #[test]
    fn cancel_says_nothing_is_scheduled_only_when_nothing_is() {
        let dir = std::env::temp_dir().join(format!("powerctl-cancel-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let record = dir.join("scheduled");
        fs::write(&record, "100 reboot\n").unwrap();
        assert_eq!(cancel_schedule_at(&record), Ok(()));
        assert!(!record.exists());
        assert_eq!(
            cancel_schedule_at(&record),
            Err("no scheduled operation to cancel".to_string())
        );
        // A directory where the record should be cannot be removed as a file,
        // and is not "nothing scheduled".
        fs::create_dir_all(&record).unwrap();
        let err = cancel_schedule_at(&record).unwrap_err();
        assert!(err.starts_with("cannot remove "), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn capacity_over_a_hundred_fills_the_bar_and_no_more() {
        let bar = capacity_bar(250);
        assert_eq!(bar.chars().filter(|&c| c == '#').count(), 20);
        assert_eq!(bar.chars().filter(|&c| c == '-').count(), 0);
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
