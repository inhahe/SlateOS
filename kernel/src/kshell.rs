//! Kernel debug shell.
//!
//! A simple command-line interface that runs in the kernel's idle context,
//! reading keyboard input and executing built-in diagnostic commands.
//! This provides interactive debugging capability without requiring a
//! filesystem, userspace programs, or a POSIX layer.
//!
//! ## Commands
//!
//! - `help`     — list available commands
//! - `meminfo`  — show physical memory usage
//! - `ps`       — list running tasks (scheduler state)
//! - `clear`    — clear the screen
//! - `uptime`   — show tick count / uptime
//! - `echo ...` — echo text back to console
//! - `reboot`   — triple-fault reboot
//!
//! ## Design
//!
//! The shell runs as a loop in `kmain()` after boot completes.  It blocks
//! on keyboard input using [`crate::keyboard::read_char`] (which HLTs
//! between interrupts).  This keeps the idle loop power-efficient while
//! still processing input promptly when keys arrive.

use alloc::string::String;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum line length.  Longer lines are silently truncated.
const MAX_LINE: usize = 256;

/// The shell prompt string.
const PROMPT: &str = "kernel> ";

// ---------------------------------------------------------------------------
// Shell entry point
// ---------------------------------------------------------------------------

/// Run the kernel debug shell.
///
/// This function never returns.  It prints a prompt, reads a line,
/// executes the command, and repeats.
pub fn run() -> ! {
    crate::console_println!("");
    crate::console_println!("Kernel debug shell. Type 'help' for commands.");
    crate::console_println!("");

    let mut line_buf = String::with_capacity(MAX_LINE);

    loop {
        // Print prompt.
        crate::console_print!("{}", PROMPT);

        // Read a line (blocking on keyboard).
        line_buf.clear();
        read_line(&mut line_buf);

        // Parse and execute.
        let trimmed = line_buf.trim();
        if trimmed.is_empty() {
            continue;
        }

        execute(trimmed);
    }
}

// ---------------------------------------------------------------------------
// Line input
// ---------------------------------------------------------------------------

/// Read a line from the keyboard, echoing characters and handling
/// backspace.  Returns when Enter is pressed.
fn read_line(buf: &mut String) {
    loop {
        let ch = crate::keyboard::read_char();

        match ch {
            b'\n' => {
                // Enter — finish the line.
                // The keyboard handler already echoed the newline to
                // the console, but we need to make sure it appears.
                // (The echo in keyboard.rs handles normal chars; newline
                // needs explicit handling here.)
                crate::console::putchar(b'\n');
                return;
            }
            b'\x08' => {
                // Backspace — remove last character if any.
                if buf.pop().is_some() {
                    // Move cursor back, overwrite with space, move back again.
                    // We use raw putchar calls for this.
                    crate::console::putchar(b'\x08');
                    crate::console::putchar(b' ');
                    crate::console::putchar(b'\x08');
                }
            }
            0x1B => {
                // ESC — ignore (could clear line in the future).
            }
            0x7F => {
                // DEL — treat like backspace.
                if buf.pop().is_some() {
                    crate::console::putchar(b'\x08');
                    crate::console::putchar(b' ');
                    crate::console::putchar(b'\x08');
                }
            }
            ch if ch >= 0x20 && ch < 0x7F => {
                // Printable ASCII — add to buffer if room.
                if buf.len() < MAX_LINE {
                    buf.push(ch as char);
                    // Character is already echoed by the keyboard driver.
                }
            }
            _ => {
                // Non-printable, non-handled — ignore.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

/// Parse a command line and execute the matching command.
fn execute(line: &str) {
    // Split into command and arguments.
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    match cmd {
        "help" | "?" => cmd_help(),
        "meminfo" | "mem" => cmd_meminfo(),
        "ps" | "tasks" => cmd_ps(),
        "clear" | "cls" => cmd_clear(),
        "uptime" => cmd_uptime(),
        "echo" => cmd_echo(args),
        "time" | "date" => cmd_time(),
        "reboot" => cmd_reboot(),
        "irq" => cmd_irq(),
        "pci" => cmd_pci(),
        "version" | "ver" => cmd_version(),
        _ => {
            crate::console_println!("Unknown command: '{}'. Type 'help' for a list.", cmd);
        }
    }
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_help() {
    crate::console_println!("Available commands:");
    crate::console_println!("  help      Show this help message");
    crate::console_println!("  meminfo   Show physical memory usage");
    crate::console_println!("  ps        List scheduler tasks");
    crate::console_println!("  clear     Clear the screen");
    crate::console_println!("  uptime    Show system uptime (tick count)");
    crate::console_println!("  echo ...  Echo text to console");
    crate::console_println!("  time      Show current date and time (RTC)");
    crate::console_println!("  irq       Show IRQ interrupt counts");
    crate::console_println!("  pci       List PCI devices");
    crate::console_println!("  version   Show kernel version");
    crate::console_println!("  reboot    Reboot the system");
}

// Division-by-constant conversions are safe (1024 never overflows).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_meminfo() {
    match crate::mm::frame::stats() {
        Some(stats) => {
            crate::console_println!("Physical memory:");
            // Each frame is 16 KiB.
            let free_kib = stats.free_frames.saturating_mul(16);
            let total_kib = stats.total_frames.saturating_mul(16);
            let used = stats.total_frames.saturating_sub(stats.free_frames);
            let used_kib = used.saturating_mul(16);
            crate::console_println!(
                "  Total: {} frames ({} KiB / {} MiB)",
                stats.total_frames,
                total_kib,
                total_kib / 1024
            );
            crate::console_println!(
                "  Used:  {} frames ({} KiB / {} MiB)",
                used,
                used_kib,
                used_kib / 1024
            );
            crate::console_println!(
                "  Free:  {} frames ({} KiB / {} MiB)",
                stats.free_frames,
                free_kib,
                free_kib / 1024
            );
        }
        None => {
            crate::console_println!("Error: frame allocator not initialized");
        }
    }
}

fn cmd_ps() {
    let task_list = crate::sched::task_list();
    if task_list.is_empty() {
        crate::console_println!("No tasks.");
        return;
    }

    crate::console_println!("{:<6} {:<10} {:<10}", "TID", "STATE", "PRIORITY");
    crate::console_println!("------------------------------");
    for info in &task_list {
        crate::console_println!(
            "{:<6} {:<10} {:<10}",
            info.id,
            info.state,
            info.priority
        );
    }
    crate::console_println!("{} task(s) total", task_list.len());
}

fn cmd_clear() {
    crate::console::clear();
}

fn cmd_uptime() {
    let ticks = crate::apic::tick_count();
    // Timer runs at 100 Hz, so ticks / 100 = seconds.
    let seconds = ticks / 100;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    crate::console_println!(
        "Uptime: {} ticks ({:02}:{:02}:{:02})",
        ticks,
        hours,
        minutes % 60,
        seconds % 60
    );
}

fn cmd_echo(args: &str) {
    crate::console_println!("{}", args);
}

fn cmd_time() {
    let dt = crate::rtc::read_datetime();
    crate::console_println!("{}", dt);
}

// PCI device class/subclass descriptions and bar formatting use simple
// fixed-width arithmetic on small known values.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_pci() {
    let devices = crate::pci::scan_bus0();
    if devices.is_empty() {
        crate::console_println!("No PCI devices found.");
        return;
    }

    crate::console_println!("{:<10} {:<12} {:<8} {:<6}", "BDF", "VENDOR:DEV", "CLASS", "IRQ");
    crate::console_println!("------------------------------------------");
    for dev in &devices {
        crate::console_println!(
            "{:02x}:{:02x}.{}    {:04x}:{:04x}     {:02x}:{:02x}   {}",
            dev.address.bus,
            dev.address.device,
            dev.address.function,
            dev.vendor_id,
            dev.device_id,
            dev.class,
            dev.subclass,
            dev.irq_line
        );
    }
    crate::console_println!("{} device(s)", devices.len());
}

fn cmd_irq() {
    crate::console_println!("IRQ interrupt counts:");
    let mut any = false;
    for i in 0..24u32 {
        let count = crate::ioapic::irq_consume(i);
        if count > 0 {
            crate::console_println!("  IRQ {:2}: {} interrupts", i, count);
            any = true;
        }
    }
    // Also show the total pending (peek without consume) for reference.
    if !any {
        crate::console_println!("  (no IRQ activity recorded)");
    }
}

fn cmd_reboot() {
    crate::console_println!("Rebooting...");

    // Triple-fault reboot: load a null IDT and trigger an interrupt.
    // The CPU will triple-fault, and the chipset will reset.
    //
    // SAFETY: We're intentionally crashing the system to reboot.
    unsafe {
        // Load a zero-length IDT.
        let null_idt: [u8; 10] = [0; 10];
        core::arch::asm!(
            "lidt [{}]",
            in(reg) null_idt.as_ptr(),
            options(noreturn)
        );
    }
}

fn cmd_version() {
    crate::console_println!("Kernel v0.1.0 (x86_64, microkernel)");
    crate::console_println!("Built with Rust, AI-developed");
}
