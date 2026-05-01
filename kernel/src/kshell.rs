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
        "disk" | "blkinfo" => cmd_disk(),
        "blkread" => cmd_blkread(args),
        "ls" | "dir" => cmd_ls(args),
        "cat" | "type" => cmd_cat(args),
        "write" => cmd_write(args),
        "rm" | "del" => cmd_rm(args),
        "mkdir" => cmd_mkdir(args),
        "rmdir" => cmd_rmdir(args),
        "run" | "exec" => cmd_run(args),
        "mkelf" => cmd_mkelf(),
        "net" | "ifconfig" => cmd_net(),
        "dhcp" => cmd_dhcp(),
        "ping" => cmd_ping(args),
        "dns" | "nslookup" => cmd_dns(args),
        "wget" | "http" => cmd_wget(args),
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
    crate::console_println!("  disk      Show block device info");
    crate::console_println!("  blkread N Hex-dump sector N from disk");
    crate::console_println!("  ls [path] List files in directory");
    crate::console_println!("  cat FILE  Print file contents");
    crate::console_println!("  write F T Write text T to file F");
    crate::console_println!("  rm FILE   Delete a file");
    crate::console_println!("  mkdir DIR Create a directory");
    crate::console_println!("  rmdir DIR Remove an empty directory");
    crate::console_println!("  run FILE  Load and execute an ELF binary");
    crate::console_println!("  mkelf     Create test ELF binaries (EXIT.ELF + HELLO.ELF)");
    crate::console_println!("  net       Show network interface info");
    crate::console_println!("  dhcp      Obtain an IP address via DHCP");
    crate::console_println!("  ping IP   Send ICMP echo requests (ping)");
    crate::console_println!("  dns NAME  Resolve a domain name to IP");
    crate::console_println!("  wget URL  Fetch a URL via HTTP GET");
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

// Sector formatting uses small arithmetic on known-bounded values.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_disk() {
    let devices = crate::blkdev::list_devices_full();
    if devices.is_empty() {
        crate::console_println!("No block devices registered.");
        return;
    }
    crate::console_println!("Block devices:");
    for dev in &devices {
        let kib = dev.sector_count.saturating_mul(u64::from(dev.sector_size)) / 1024;
        let mib = kib / 1024;
        crate::console_println!(
            "  {} — {} sectors ({} KiB / {} MiB){}",
            dev.name,
            dev.sector_count,
            kib,
            mib,
            if dev.read_only { " [read-only]" } else { "" }
        );
    }
}

// Hex-dump formatting uses offsets bounded by SECTOR_SIZE (512).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_blkread(args: &str) {
    // Parse: "blkread <sector>" or "blkread <device> <sector>"
    let (dev_name, sector) = parse_blkread_args(args);
    let Some(sector) = sector else {
        crate::console_println!("Usage: blkread [device] <sector>");
        crate::console_println!("  e.g., blkread 0  or  blkread vda 0");
        return;
    };

    let result = crate::blkdev::with_device(&dev_name, |dev| {
        let mut buf = [0u8; crate::blkdev::SECTOR_SIZE];
        match dev.read_sector(sector, &mut buf) {
            Ok(()) => {
                crate::console_println!("Sector {} on {}:", sector, dev_name);
                // Print 32 rows of 16 bytes each (512 bytes total).
                for row in 0..32 {
                    let offset = row * 16;
                    crate::console_print!("  {:04x}:", offset);
                    for col in 0..16 {
                        if let Some(&byte) = buf.get(offset + col) {
                            crate::console_print!(" {:02x}", byte);
                        }
                    }
                    // ASCII column.
                    crate::console_print!("  |");
                    for col in 0..16 {
                        if let Some(&byte) = buf.get(offset + col) {
                            let ch = if byte >= 0x20 && byte < 0x7F {
                                byte as char
                            } else {
                                '.'
                            };
                            crate::console_print!("{}", ch);
                        }
                    }
                    crate::console_println!("|");
                }
            }
            Err(e) => {
                crate::console_println!("Error reading sector {}: {:?}", sector, e);
            }
        }
    });
    if result.is_none() {
        crate::console_println!("No block device '{}' found.", dev_name);
    }
}

/// Parse blkread args: either "<sector>" or "<device> <sector>".
/// Returns (device_name, Some(sector)) or (_, None) on parse error.
fn parse_blkread_args(args: &str) -> (alloc::string::String, Option<u64>) {
    let mut parts = args.split_whitespace();
    let first = match parts.next() {
        Some(s) => s,
        None => return (alloc::string::String::from("vda"), None),
    };

    if let Some(second) = parts.next() {
        // Two args: device name + sector
        match second.parse::<u64>() {
            Ok(s) => (alloc::string::String::from(first), Some(s)),
            Err(_) => (alloc::string::String::from("vda"), None),
        }
    } else {
        // One arg: try as sector number (default device "vda")
        match first.parse::<u64>() {
            Ok(s) => (alloc::string::String::from("vda"), Some(s)),
            Err(_) => (alloc::string::String::from("vda"), None),
        }
    }
}

fn cmd_ls(args: &str) {
    let path = if args.is_empty() { "/" } else { args };

    match crate::fs::Vfs::readdir(path) {
        Ok(entries) => {
            if entries.is_empty() {
                crate::console_println!("(empty directory)");
                return;
            }
            for entry in &entries {
                let type_indicator = match entry.entry_type {
                    crate::fs::EntryType::Directory => "<DIR>    ",
                    crate::fs::EntryType::File => "         ",
                    crate::fs::EntryType::VolumeLabel => "<VOL>    ",
                };
                crate::console_println!(
                    "  {} {:>8}  {}",
                    type_indicator, entry.size, entry.name
                );
            }
            crate::console_println!("{} entry(ies)", entries.len());
        }
        Err(e) => {
            crate::console_println!("ls: {}: {:?}", path, e);
        }
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: cat <filename>");
        return;
    }

    // Prepend "/" if the path doesn't start with one.
    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    match crate::fs::Vfs::read_file(&path) {
        Ok(data) => {
            // Try to display as UTF-8 text.
            match core::str::from_utf8(&data) {
                Ok(text) => {
                    crate::console_print!("{}", text);
                    // Ensure there's a newline at the end.
                    if !text.ends_with('\n') {
                        crate::console_println!();
                    }
                }
                Err(_) => {
                    crate::console_println!(
                        "(binary file, {} bytes — use blkread for hex dump)",
                        data.len()
                    );
                }
            }
        }
        Err(e) => {
            crate::console_println!("cat: {}: {:?}", path, e);
        }
    }
}

fn cmd_write(args: &str) {
    // Parse: "write FILENAME text to write..."
    let mut parts = args.splitn(2, ' ');
    let filename = match parts.next() {
        Some(f) if !f.is_empty() => f,
        _ => {
            crate::console_println!("Usage: write <filename> <text>");
            return;
        }
    };
    let text = parts.next().unwrap_or("");

    let path = if filename.starts_with('/') {
        alloc::string::String::from(filename)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(filename);
        s
    };

    // Append a newline if the text doesn't have one.
    let mut data = alloc::vec::Vec::from(text.as_bytes());
    if !text.ends_with('\n') {
        data.push(b'\n');
    }

    match crate::fs::Vfs::write_file(&path, &data) {
        Ok(()) => {
            crate::console_println!("Wrote {} bytes to {}", data.len(), path);
        }
        Err(e) => {
            crate::console_println!("write: {}: {:?}", path, e);
        }
    }
}

fn cmd_rm(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: rm <filename>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    match crate::fs::Vfs::remove(&path) {
        Ok(()) => {
            crate::console_println!("Deleted {}", path);
        }
        Err(e) => {
            crate::console_println!("rm: {}: {:?}", path, e);
        }
    }
}

fn cmd_mkdir(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: mkdir <dirname>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    match crate::fs::Vfs::mkdir(&path) {
        Ok(()) => {
            crate::console_println!("Created directory {}", path);
        }
        Err(e) => {
            crate::console_println!("mkdir: {}: {:?}", path, e);
        }
    }
}

fn cmd_rmdir(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: rmdir <dirname>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    match crate::fs::Vfs::rmdir(&path) {
        Ok(()) => {
            crate::console_println!("Removed directory {}", path);
        }
        Err(e) => {
            crate::console_println!("rmdir: {}: {:?}", path, e);
        }
    }
}

fn cmd_run(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: run <elf-file>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    // Read the ELF binary from the filesystem.
    let elf_data = match crate::fs::Vfs::read_file(&path) {
        Ok(data) => data,
        Err(e) => {
            crate::console_println!("run: {}: {:?}", path, e);
            return;
        }
    };

    crate::console_println!("Loading {} ({} bytes)...", path, elf_data.len());

    // Spawn a new process from the ELF data.
    let name = args.rsplit('/').next().unwrap_or(args);
    let options = crate::proc::spawn::SpawnOptions::new(name);

    match crate::proc::spawn::spawn_process(&elf_data, &options) {
        Ok(result) => {
            crate::console_println!(
                "Process '{}' spawned: pid={}, tid={}, entry={:#x}",
                name,
                result.pid,
                result.task_id,
                result.entry_point
            );
        }
        Err(e) => {
            crate::console_println!("run: failed to spawn: {:?}", e);
        }
    }
}

fn cmd_mkelf() {
    // Generate both test ELFs and write them to the filesystem.

    // 1. EXIT.ELF — minimal ELF that just calls SYS_EXIT(0).
    let exit_elf = crate::proc::elf::build_test_elf_public();
    match crate::fs::Vfs::write_file("/EXIT.ELF", &exit_elf) {
        Ok(()) => {
            crate::console_println!(
                "Created /EXIT.ELF ({} bytes) — calls SYS_EXIT(0)",
                exit_elf.len()
            );
        }
        Err(e) => {
            crate::console_println!("mkelf: failed to write EXIT.ELF: {:?}", e);
        }
    }

    // 2. HELLO.ELF — prints "Hello from userspace!" via SYS_CONSOLE_WRITE, then exits.
    let hello_elf = crate::proc::elf::build_hello_elf();
    match crate::fs::Vfs::write_file("/HELLO.ELF", &hello_elf) {
        Ok(()) => {
            crate::console_println!(
                "Created /HELLO.ELF ({} bytes) — prints to console, then exits",
                hello_elf.len()
            );
        }
        Err(e) => {
            crate::console_println!("mkelf: failed to write HELLO.ELF: {:?}", e);
        }
    }
    crate::console_println!("Run them with: run EXIT.ELF / run HELLO.ELF");
}

fn cmd_net() {
    let info = crate::net::interface::info();
    if !info.up {
        crate::console_println!("No network interface.");
        return;
    }

    crate::console_println!("Network interface: virtio-net");
    crate::console_println!("  MAC address:  {}", info.mac);
    crate::console_println!("  IPv4 address: {}", info.ip);
    crate::console_println!("  Subnet mask:  {}", info.subnet_mask);
    crate::console_println!("  Gateway:      {}", info.gateway);
    crate::console_println!("  DNS server:   {}", info.dns);
    crate::console_println!("  DHCP state:   {}", crate::net::dhcp::state_str());

    // Also show RX buffer status from the NIC.
    let rx_info = crate::virtio::net::with_device(|dev| dev.rx_pending());
    if let Some(pending) = rx_info {
        crate::console_println!("  RX buffers:   {} pending", pending);
    }
}

fn cmd_dhcp() {
    crate::console_println!("Running DHCP discovery...");
    match crate::net::dhcp::discover() {
        Ok(ip) => {
            crate::console_println!("DHCP successful: {}", ip);
            // Show full config.
            let info = crate::net::interface::info();
            crate::console_println!("  Subnet mask: {}", info.subnet_mask);
            crate::console_println!("  Gateway:     {}", info.gateway);
            crate::console_println!("  DNS server:  {}", info.dns);
        }
        Err(e) => {
            crate::console_println!("DHCP failed: {:?}", e);
        }
    }
}

fn cmd_ping(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: ping <ip-address>");
        crate::console_println!("  e.g., ping 10.0.2.2");
        return;
    }

    // Parse IP address or resolve hostname.
    let ip = if let Some(ip) = parse_ipv4(args) {
        ip
    } else {
        // Try DNS resolution.
        match crate::net::dns::resolve(args) {
            Ok(ip) => {
                crate::console_println!("PING {} ({})", args, ip);
                ip
            }
            Err(e) => {
                crate::console_println!("Cannot resolve {}: {:?}", args, e);
                return;
            }
        }
    };

    // Send 4 ICMP echo requests.
    let mut sent = 0u32;
    let mut received = 0u32;
    for i in 0..4u32 {
        match crate::net::icmp::ping(ip) {
            Ok(seq) => {
                sent = sent.saturating_add(1);
                if crate::net::icmp::wait_reply(seq, 2000) {
                    received = received.saturating_add(1);
                    crate::console_println!(
                        "Reply from {}: seq={}", ip, seq
                    );
                } else {
                    crate::console_println!("Request timed out: seq={}", seq);
                }
            }
            Err(e) => {
                crate::console_println!("ping: send failed: {:?}", e);
            }
        }

        // Brief delay between pings (if not the last one).
        if i < 3 {
            for _ in 0..500_000 {
                core::hint::spin_loop();
            }
        }
    }

    crate::console_println!(
        "--- {} ping statistics: {} sent, {} received ---",
        ip, sent, received
    );
}

/// Parse a simple URL: "http://host/path" or just "host/path" or "host".
/// Returns (host, port, path).
fn parse_url(url: &str) -> Option<(&str, u16, &str)> {
    let url = url.strip_prefix("http://").unwrap_or(url);

    // Split host and path.
    let (host_port, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };

    // Split host and port.
    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let port_str = &host_port[i + 1..];
            match port_str.parse::<u16>() {
                Ok(p) => (&host_port[..i], p),
                Err(_) => (host_port, 80),
            }
        }
        None => (host_port, 80),
    };

    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

// String formatting uses small bounded values.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_wget(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: wget <url>");
        crate::console_println!("  e.g., wget http://example.com/");
        return;
    }

    let Some((host, port, path)) = parse_url(args) else {
        crate::console_println!("Invalid URL: {}", args);
        return;
    };

    crate::console_println!("Resolving {}...", host);

    // Resolve hostname to IP.
    let ip = if let Some(ip) = parse_ipv4(host) {
        ip
    } else {
        match crate::net::dns::resolve(host) {
            Ok(ip) => ip,
            Err(e) => {
                crate::console_println!("DNS resolution failed: {:?}", e);
                return;
            }
        }
    };

    crate::console_println!("Connecting to {}:{}...", ip, port);

    // Open TCP connection.
    let conn = match crate::net::tcp::connect(ip, port) {
        Ok(c) => c,
        Err(e) => {
            crate::console_println!("Connection failed: {:?}", e);
            return;
        }
    };

    // Build HTTP request.
    let request = alloc::format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );

    crate::console_println!("Sending HTTP request...");

    if let Err(e) = crate::net::tcp::send(conn, request.as_bytes()) {
        crate::console_println!("Send failed: {:?}", e);
        let _ = crate::net::tcp::close(conn);
        return;
    }

    // Read response.
    crate::console_println!("--- Response ---");

    let mut total = 0usize;
    loop {
        match crate::net::tcp::read_blocking(conn, 3000) {
            Ok(data) => {
                if data.is_empty() {
                    // Check if connection closed.
                    if crate::net::tcp::is_remote_closed(conn) {
                        break;
                    }
                    // No data yet — try again briefly.
                    continue;
                }
                total = total.saturating_add(data.len());
                // Print as text.
                match core::str::from_utf8(&data) {
                    Ok(text) => crate::console_print!("{}", text),
                    Err(_) => crate::console_print!("(binary: {} bytes)", data.len()),
                }
            }
            Err(_) => break,
        }
    }

    crate::console_println!("\n--- End ({} bytes received) ---", total);
    let _ = crate::net::tcp::close(conn);
}

fn cmd_dns(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: dns <domain-name>");
        crate::console_println!("  e.g., dns example.com");
        return;
    }

    crate::console_println!("Resolving {}...", args);
    match crate::net::dns::resolve(args) {
        Ok(ip) => {
            crate::console_println!("{} -> {}", args, ip);
        }
        Err(e) => {
            crate::console_println!("DNS resolution failed: {:?}", e);
        }
    }
}

/// Parse an IPv4 address from a dotted-quad string.
fn parse_ipv4(s: &str) -> Option<crate::net::interface::Ipv4Addr> {
    let mut parts = s.split('.');
    let a = parts.next()?.parse::<u8>().ok()?;
    let b = parts.next()?.parse::<u8>().ok()?;
    let c = parts.next()?.parse::<u8>().ok()?;
    let d = parts.next()?.parse::<u8>().ok()?;
    // Reject trailing parts.
    if parts.next().is_some() {
        return None;
    }
    Some(crate::net::interface::Ipv4Addr::new(a, b, c, d))
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
