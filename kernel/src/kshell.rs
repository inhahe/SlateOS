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
        "stat" => cmd_stat(args),
        "ln" | "link" => cmd_ln(args),
        "df" => cmd_df(args),
        "cp" | "copy" => cmd_cp(args),
        "mv" | "move" | "ren" => cmd_mv(args),
        "chmod" => cmd_chmod(args),
        "chown" => cmd_chown(args),
        "touch" => cmd_touch(args),
        "append" => cmd_append(args),
        "tree" => cmd_tree(args),
        "du" => cmd_du(args),
        "find" => cmd_find(args),
        "sync" => cmd_sync(),
        "mount" => cmd_mount(args),
        "umount" | "unmount" => cmd_umount(args),
        "wc" => cmd_wc(args),
        "head" => cmd_head(args),
        "tail" => cmd_tail(args),
        "hexdump" | "xxd" => cmd_hexdump(args),
        "lsof" => cmd_lsof(),
        "lsp" => cmd_lsp(args),
        "grep" => cmd_grep(args),
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
    crate::console_println!("  rm [-r] F Delete a file (or directory tree with -r)");
    crate::console_println!("  mkdir DIR Create a directory");
    crate::console_println!("  rmdir DIR Remove an empty directory");
    crate::console_println!("  stat FILE Show detailed file metadata");
    crate::console_println!("  ln S D    Create hard link D pointing to S");
    crate::console_println!("  cp [-r] S D Copy file (or dir tree with -r) S to D");
    crate::console_println!("  mv S D    Move/rename file or directory");
    crate::console_println!("  chmod M F Set permissions (octal, e.g., chmod 755 file)");
    crate::console_println!("  chown U F Set owner (uid:gid, e.g., chown 1000:1000 file)");
    crate::console_println!("  touch F   Create file or update timestamps");
    crate::console_println!("  append F T Append text T to file F");
    crate::console_println!("  tree [D]  Show directory tree recursively");
    crate::console_println!("  du [D]    Show disk usage of directory");
    crate::console_println!("  find [D]P Search for files matching pattern");
    crate::console_println!("  df [path] Show filesystem space usage");
    crate::console_println!("  sync      Flush all filesystems to disk");
    crate::console_println!("  mount     List all mounted filesystems");
    crate::console_println!("  umount P  Unmount filesystem at path P");
    crate::console_println!("  wc FILE   Count lines, words, and bytes");
    crate::console_println!("  head N F  Show first N lines of file");
    crate::console_println!("  tail N F  Show last N lines of file");
    crate::console_println!("  hexdump F Hex dump of file contents");
    crate::console_println!("  lsof      List open file handles");
    crate::console_println!("  lsp [N] D Paginated ls: show N entries at a time");
    crate::console_println!("  grep P F  Search for pattern P in file F");
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

    // Heap allocator stats (always available, lock-free).
    let h = crate::mm::heap::stats();
    crate::console_println!("Kernel heap:");
    crate::console_println!(
        "  Slab:  {} allocs, {} frees (live: {})",
        h.slab_allocs,
        h.slab_frees,
        h.slab_allocs.saturating_sub(h.slab_frees)
    );
    crate::console_println!(
        "  Large: {} allocs, {} frees (live: {})",
        h.large_allocs,
        h.large_frees,
        h.large_allocs.saturating_sub(h.large_frees)
    );
    crate::console_println!(
        "  Refills: {}, Failures: {}",
        h.slab_refills,
        h.alloc_failures
    );

    // Pre-zeroed frame pool.
    let pool_count = crate::mm::frame::zero_pool_count();
    let (pool_hits, pool_misses) = crate::mm::frame::zero_pool_stats();
    let pool_total = pool_hits.saturating_add(pool_misses);
    let hit_pct = if pool_total > 0 {
        pool_hits.saturating_mul(100) / pool_total
    } else {
        0
    };
    crate::console_println!("Zero pool:");
    crate::console_println!(
        "  Cached: {} frames, Hits: {}, Misses: {} ({}% hit rate)",
        pool_count,
        pool_hits,
        pool_misses,
        hit_pct
    );
}

fn cmd_ps() {
    let task_list = crate::sched::task_list();
    if task_list.is_empty() {
        crate::console_println!("No tasks.");
        return;
    }

    crate::console_println!(
        "{:<6} {:<12} {:<10} {:<4} {:<8} {:<8} {:<4}",
        "TID", "NAME", "STATE", "PRI", "TICKS", "SCHED", "CPU"
    );
    crate::console_println!("------------------------------------------------------");
    for info in &task_list {
        let name = core::str::from_utf8(&info.name[..info.name_len])
            .unwrap_or("?");
        crate::console_println!(
            "{:<6} {:<12} {:<10} {:<4} {:<8} {:<8} {:<4}",
            info.id,
            name,
            info.state,
            info.priority,
            info.total_ticks,
            info.schedule_count,
            info.last_cpu,
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
                    crate::fs::EntryType::Symlink => "<LINK>   ",
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
    // Support -r/-R flag for recursive removal.
    let (recursive, args) = if args.starts_with("-r ") || args.starts_with("-R ")
        || args.starts_with("-rf ") || args.starts_with("-Rf ")
    {
        let skip = if args.starts_with("-rf") || args.starts_with("-Rf") { 4 } else { 3 };
        (true, &args[skip..])
    } else {
        (false, args)
    };

    if args.is_empty() {
        crate::console_println!("Usage: rm [-r] <filename>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    if recursive {
        match crate::fs::Vfs::remove_recursive(&path) {
            Ok(count) => {
                crate::console_println!("Removed {} ({} items)", path, count);
            }
            Err(e) => {
                crate::console_println!("rm: {}: {:?}", path, e);
            }
        }
    } else {
        match crate::fs::Vfs::remove(&path) {
            Ok(()) => {
                crate::console_println!("Deleted {}", path);
            }
            Err(e) => {
                crate::console_println!("rm: {}: {:?}", path, e);
            }
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

/// Show detailed file/directory metadata.
#[allow(clippy::cast_possible_truncation)]
fn cmd_stat(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: stat <path>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    match crate::fs::Vfs::metadata(&path) {
        Ok(meta) => {
            let type_str = match meta.entry_type {
                crate::fs::EntryType::File => "regular file",
                crate::fs::EntryType::Directory => "directory",
                crate::fs::EntryType::Symlink => "symbolic link",
                crate::fs::EntryType::VolumeLabel => "volume label",
            };
            crate::console_println!("  File: {}", path);
            crate::console_println!("  Size: {}  Type: {}", meta.size, type_str);
            crate::console_println!("  Links: {}", meta.nlinks);
            if meta.permissions != 0 {
                crate::console_println!("  Perms: {:04o}  Uid: {}  Gid: {}",
                    meta.permissions, meta.uid, meta.gid);
            }
            if meta.attributes != crate::fs::FileAttr::NONE {
                crate::console_println!("  Attrs: {:?}", meta.attributes);
            }

            // Format timestamps (nanoseconds to seconds for readability).
            let ns_to_display = |ns: u64| -> alloc::string::String {
                if ns == 0 {
                    alloc::string::String::from("-")
                } else {
                    // Show as seconds since epoch (or boot, depending on source).
                    let secs = ns / 1_000_000_000;
                    let frac = (ns % 1_000_000_000) / 1_000_000;
                    alloc::format!("{}.{:03}s", secs, frac)
                }
            };
            crate::console_println!("  Created:  {}", ns_to_display(meta.created_ns));
            crate::console_println!("  Modified: {}", ns_to_display(meta.modified_ns));
            crate::console_println!("  Accessed: {}", ns_to_display(meta.accessed_ns));
            crate::console_println!("  Changed:  {}", ns_to_display(meta.changed_ns));
        }
        Err(e) => {
            crate::console_println!("stat: {}: {:?}", path, e);
        }
    }
}

/// Create a hard link.
fn cmd_ln(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: ln <source> <link-name>");
        return;
    }

    let src = parts[0];
    let dst = parts[1];

    let src_path = if src.starts_with('/') {
        alloc::string::String::from(src)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(src);
        s
    };
    let dst_path = if dst.starts_with('/') {
        alloc::string::String::from(dst)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(dst);
        s
    };

    match crate::fs::Vfs::link(&src_path, &dst_path) {
        Ok(()) => {
            crate::console_println!("{} -> {}", dst_path, src_path);
        }
        Err(e) => {
            crate::console_println!("ln: {:?}", e);
        }
    }
}

/// Show filesystem disk usage (like Unix `df`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_df(args: &str) {
    if args.is_empty() {
        // Show all mounts.
        match crate::fs::Vfs::mount_info() {
            Ok(mounts) => {
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>5}  {}",
                    "Filesystem", "Size", "Used", "Avail", "Use%", "Mounted on"
                );
                for (mount_path, info) in &mounts {
                    let total = info.total_bytes();
                    let free = info.free_bytes();
                    let used = info.used_bytes();
                    let pct = info.usage_percent();
                    crate::console_println!(
                        "{:<12} {:>10} {:>10} {:>10} {:>4}%  {}",
                        info.fs_type,
                        format_bytes(total),
                        format_bytes(used),
                        format_bytes(free),
                        pct,
                        mount_path
                    );
                }
            }
            Err(e) => {
                crate::console_println!("df: {:?}", e);
            }
        }
    } else {
        // Show info for specific path.
        let path = if args.starts_with('/') {
            alloc::string::String::from(args)
        } else {
            let mut s = alloc::string::String::from("/");
            s.push_str(args);
            s
        };
        match crate::fs::Vfs::statvfs(&path) {
            Ok(info) => {
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>5}  {}",
                    "Filesystem", "Size", "Used", "Avail", "Use%", "Path"
                );
                let total = info.total_bytes();
                let free = info.free_bytes();
                let used = info.used_bytes();
                let pct = info.usage_percent();
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>4}%  {}",
                    info.fs_type,
                    format_bytes(total),
                    format_bytes(used),
                    format_bytes(free),
                    pct,
                    path
                );
            }
            Err(e) => {
                crate::console_println!("df: {:?}", e);
            }
        }
    }
}

/// Format a byte count as human-readable (K/M/G).
#[allow(clippy::arithmetic_side_effects)]
fn format_bytes(bytes: u64) -> alloc::string::String {
    if bytes < 1024 {
        alloc::format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        alloc::format!("{}K", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        alloc::format!("{}M", bytes / (1024 * 1024))
    } else {
        alloc::format!("{}G", bytes / (1024 * 1024 * 1024))
    }
}

/// Copy a file.
fn cmd_cp(args: &str) {
    // Support -r flag for recursive copy.
    let (recursive, args) = if args.starts_with("-r ") || args.starts_with("-R ") {
        (true, &args[3..])
    } else {
        (false, args)
    };

    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: cp [-r] <source> <dest>");
        return;
    }

    let src = parts[0];
    let dst = parts[1];

    let src_path = if src.starts_with('/') {
        alloc::string::String::from(src)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(src);
        s
    };
    let dst_path = if dst.starts_with('/') {
        alloc::string::String::from(dst)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(dst);
        s
    };

    if recursive {
        match crate::fs::Vfs::copy_recursive(&src_path, &dst_path) {
            Ok(size) => {
                crate::console_println!("'{}' -> '{}' ({} bytes copied)", src_path, dst_path, size);
            }
            Err(e) => {
                crate::console_println!("cp: {:?}", e);
            }
        }
    } else {
        match crate::fs::Vfs::copy(&src_path, &dst_path) {
            Ok(size) => {
                crate::console_println!("'{}' -> '{}' ({} bytes)", src_path, dst_path, size);
            }
            Err(e) => {
                crate::console_println!("cp: {:?}", e);
            }
        }
    }
}

/// Rename/move a file or directory.
fn cmd_mv(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: mv <source> <dest>");
        return;
    }

    let src = parts[0];
    let dst = parts[1];

    let src_path = if src.starts_with('/') {
        alloc::string::String::from(src)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(src);
        s
    };
    let dst_path = if dst.starts_with('/') {
        alloc::string::String::from(dst)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(dst);
        s
    };

    match crate::fs::Vfs::rename(&src_path, &dst_path) {
        Ok(()) => {
            crate::console_println!("'{}' -> '{}'", src_path, dst_path);
        }
        Err(e) => {
            crate::console_println!("mv: {:?}", e);
        }
    }
}

/// Change file permissions.
fn cmd_chmod(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: chmod <mode> <path>");
        crate::console_println!("  mode: octal (e.g., 755, 644)");
        return;
    }

    let mode_str = parts[0];
    let file = parts[1];

    let mode = match u16::from_str_radix(mode_str, 8) {
        Ok(m) => m,
        Err(_) => {
            crate::console_println!("chmod: invalid mode '{}' (use octal, e.g., 755)", mode_str);
            return;
        }
    };

    let path = if file.starts_with('/') {
        alloc::string::String::from(file)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(file);
        s
    };

    match crate::fs::Vfs::set_permissions(&path, mode) {
        Ok(()) => {
            crate::console_println!("{}: mode set to {:04o}", path, mode);
        }
        Err(e) => {
            crate::console_println!("chmod: {:?}", e);
        }
    }
}

/// Change file ownership.
fn cmd_chown(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: chown <uid:gid> <path>");
        crate::console_println!("  e.g., chown 1000:1000 /home/user");
        return;
    }

    let owner_str = parts[0];
    let file = parts[1];

    // Parse uid:gid.
    let (uid, gid) = if let Some(colon) = owner_str.find(':') {
        let uid_s = &owner_str[..colon];
        let gid_s = &owner_str[colon + 1..];
        let uid = match uid_s.parse::<u32>() {
            Ok(u) => u,
            Err(_) => {
                crate::console_println!("chown: invalid uid '{}'", uid_s);
                return;
            }
        };
        let gid = match gid_s.parse::<u32>() {
            Ok(g) => g,
            Err(_) => {
                crate::console_println!("chown: invalid gid '{}'", gid_s);
                return;
            }
        };
        (uid, gid)
    } else {
        // Just uid, set gid to same.
        match owner_str.parse::<u32>() {
            Ok(u) => (u, u),
            Err(_) => {
                crate::console_println!("chown: invalid owner '{}'", owner_str);
                return;
            }
        }
    };

    let path = if file.starts_with('/') {
        alloc::string::String::from(file)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(file);
        s
    };

    match crate::fs::Vfs::set_owner(&path, uid, gid) {
        Ok(()) => {
            crate::console_println!("{}: owner set to {}:{}", path, uid, gid);
        }
        Err(e) => {
            crate::console_println!("chown: {:?}", e);
        }
    }
}

/// Create a file or update timestamps.
fn cmd_touch(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: touch <path>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    // Check if file exists.
    match crate::fs::Vfs::stat(&path) {
        Ok(_) => {
            // File exists — update timestamps to "now".
            let now = crate::hpet::elapsed_ns();
            match crate::fs::Vfs::set_times(&path, now, now) {
                Ok(()) => {
                    crate::console_println!("{}: timestamps updated", path);
                }
                Err(e) => {
                    crate::console_println!("touch: {}: {:?}", path, e);
                }
            }
        }
        Err(_) => {
            // File doesn't exist — create empty file.
            match crate::fs::Vfs::write_file(&path, &[]) {
                Ok(()) => {
                    crate::console_println!("{}: created", path);
                }
                Err(e) => {
                    crate::console_println!("touch: {}: {:?}", path, e);
                }
            }
        }
    }
}

/// Append text to a file.
fn cmd_append(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let filename = match parts.next() {
        Some(f) if !f.is_empty() => f,
        _ => {
            crate::console_println!("Usage: append <filename> <text>");
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

    let mut data = alloc::vec::Vec::from(text.as_bytes());
    if !text.ends_with('\n') {
        data.push(b'\n');
    }
    match crate::fs::Vfs::append(&path, &data) {
        Ok(()) => {
            crate::console_println!("Appended {} bytes to {}", data.len(), path);
        }
        Err(e) => {
            crate::console_println!("append: {}: {:?}", path, e);
        }
    }
}

/// Recursive directory tree listing.
fn cmd_tree(args: &str) {
    let path = if args.is_empty() {
        alloc::string::String::from("/")
    } else if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    crate::console_println!("{}", path);
    let mut dirs: u64 = 0;
    let mut files: u64 = 0;
    tree_recurse(&path, "", &mut dirs, &mut files, 0);
    crate::console_println!("\n{} directories, {} files", dirs, files);
}

/// Internal recursive helper for tree display.
///
/// Limits depth to 8 levels to avoid excessive output.
fn tree_recurse(path: &str, prefix: &str, dirs: &mut u64, files: &mut u64, depth: u32) {
    if depth > 8 {
        return;
    }

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    let count = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i + 1 == count;
        let connector = if is_last { "└── " } else { "├── " };
        let type_marker = match entry.entry_type {
            crate::fs::EntryType::Directory => "/",
            crate::fs::EntryType::Symlink => "@",
            _ => "",
        };

        crate::console_println!("{}{}{}{}", prefix, connector, entry.name, type_marker);

        if entry.entry_type == crate::fs::EntryType::Directory {
            *dirs = dirs.saturating_add(1);
            let child_path = if path == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", path, entry.name)
            };
            let child_prefix = if is_last {
                alloc::format!("{}    ", prefix)
            } else {
                alloc::format!("{}│   ", prefix)
            };
            tree_recurse(&child_path, &child_prefix, dirs, files, depth + 1);
        } else {
            *files = files.saturating_add(1);
        }
    }
}

/// Show disk usage for a path (like Unix `du`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_du(args: &str) {
    let path = if args.is_empty() {
        alloc::string::String::from("/")
    } else if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    let total = du_recurse(&path);
    crate::console_println!("{}\t{}", format_bytes(total), path);
}

/// Recursively calculate total size of a directory tree.
#[allow(clippy::arithmetic_side_effects)]
fn du_recurse(path: &str) -> u64 {
    let mut total: u64 = 0;

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    for entry in &entries {
        let child_path = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        total = total.saturating_add(entry.size);

        if entry.entry_type == crate::fs::EntryType::Directory {
            let subdir_total = du_recurse(&child_path);
            crate::console_println!("{}\t{}", format_bytes(subdir_total), child_path);
            total = total.saturating_add(subdir_total);
        }
    }

    total
}

/// Search for files matching a pattern (basic find).
fn cmd_find(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    let (search_path, pattern) = if parts.len() >= 2 {
        (parts[0], parts[1])
    } else if !args.is_empty() {
        ("/", args)
    } else {
        crate::console_println!("Usage: find [path] <name-pattern>");
        crate::console_println!("  Searches for files/dirs containing the pattern.");
        crate::console_println!("  Example: find /tmp log");
        return;
    };

    let root = if search_path.starts_with('/') {
        alloc::string::String::from(search_path)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(search_path);
        s
    };

    let mut count: u64 = 0;
    find_recurse(&root, pattern, &mut count, 0);
    crate::console_println!("\n{} matches found", count);
}

/// Recursive helper for find — search directory tree for name matches.
///
/// Uses case-insensitive substring matching.  Limits depth to 16.
fn find_recurse(path: &str, pattern: &str, count: &mut u64, depth: u32) {
    if depth > 16 {
        return;
    }

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child_path = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Case-insensitive substring match.
        let name_lower = entry.name.to_ascii_lowercase();
        let pattern_lower = pattern.to_ascii_lowercase();
        if name_lower.contains(&pattern_lower) {
            let type_str = match entry.entry_type {
                crate::fs::EntryType::File => "",
                crate::fs::EntryType::Directory => "/",
                crate::fs::EntryType::Symlink => "@",
                crate::fs::EntryType::VolumeLabel => "*",
            };
            crate::console_println!("{}{}", child_path, type_str);
            *count = count.saturating_add(1);
        }

        if entry.entry_type == crate::fs::EntryType::Directory {
            find_recurse(&child_path, pattern, count, depth + 1);
        }
    }
}

/// Count lines, words, and bytes in a file (like Unix `wc`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_wc(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: wc <file>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("wc: {}: {:?}", path, e);
            return;
        }
    };

    let bytes = data.len();
    let mut lines: usize = 0;
    let mut words: usize = 0;
    let mut in_word = false;

    for &b in &data {
        if b == b'\n' {
            lines += 1;
        }
        let is_ws = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
        if is_ws {
            in_word = false;
        } else if !in_word {
            in_word = true;
            words += 1;
        }
    }

    crate::console_println!("  {} lines  {} words  {} bytes  {}", lines, words, bytes, path);
}

/// Show the first N lines of a file (like Unix `head`).
fn cmd_head(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    let (count, file) = if parts.len() >= 2 {
        match parts[0].parse::<usize>() {
            Ok(n) => (n, parts[1]),
            Err(_) => (10, args), // Default to 10 lines if first arg isn't a number.
        }
    } else {
        crate::console_println!("Usage: head [N] <file>");
        return;
    };

    let path = if file.starts_with('/') {
        alloc::string::String::from(file)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(file);
        s
    };

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("head: {}: {:?}", path, e);
            return;
        }
    };

    let text = core::str::from_utf8(&data).unwrap_or("<binary>");
    let mut printed = 0;
    for line in text.lines() {
        if printed >= count {
            break;
        }
        crate::console_println!("{}", line);
        printed += 1;
    }
}

/// Show the last N lines of a file (like Unix `tail`).
fn cmd_tail(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    let (count, file) = if parts.len() >= 2 {
        match parts[0].parse::<usize>() {
            Ok(n) => (n, parts[1]),
            Err(_) => (10, args),
        }
    } else {
        crate::console_println!("Usage: tail [N] <file>");
        return;
    };

    let path = if file.starts_with('/') {
        alloc::string::String::from(file)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(file);
        s
    };

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("tail: {}: {:?}", path, e);
            return;
        }
    };

    let text = core::str::from_utf8(&data).unwrap_or("<binary>");
    let lines: alloc::vec::Vec<&str> = text.lines().collect();
    let start = if lines.len() > count { lines.len() - count } else { 0 };
    for line in &lines[start..] {
        crate::console_println!("{}", line);
    }
}

/// Hex dump of a file (like `hexdump -C` or `xxd`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_hexdump(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: hexdump <file>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("hexdump: {}: {:?}", path, e);
            return;
        }
    };

    // Limit output to first 512 bytes to avoid flooding the console.
    let limit = data.len().min(512);
    let data = &data[..limit];

    for offset in (0..data.len()).step_by(16) {
        // Offset.
        let mut line = alloc::format!("{:08x}  ", offset);

        // Hex bytes.
        for i in 0..16 {
            if offset + i < data.len() {
                line.push_str(&alloc::format!("{:02x} ", data[offset + i]));
            } else {
                line.push_str("   ");
            }
            if i == 7 {
                line.push(' ');
            }
        }

        line.push_str(" |");

        // ASCII printable characters.
        for i in 0..16 {
            if offset + i < data.len() {
                let b = data[offset + i];
                if (0x20..=0x7e).contains(&b) {
                    line.push(b as char);
                } else {
                    line.push('.');
                }
            }
        }
        line.push('|');

        crate::console_println!("{}", line);
    }

    if data.len() < limit {
        crate::console_println!("{:08x}", data.len());
    } else {
        crate::console_println!("... ({} bytes total, showing first {})", data.len(), limit);
    }
}

/// Search for a pattern in a file (simple substring grep).
fn cmd_grep(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: grep <pattern> <file>");
        return;
    }

    let pattern = parts[0];
    let file_arg = parts[1];

    let path = if file_arg.starts_with('/') {
        alloc::string::String::from(file_arg)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(file_arg);
        s
    };

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("grep: {}: {:?}", path, e);
            return;
        }
    };

    // Try to interpret as UTF-8 text.
    let text = match core::str::from_utf8(&data) {
        Ok(s) => s,
        Err(_) => {
            crate::console_println!("grep: {}: binary file (not UTF-8)", path);
            return;
        }
    };

    // Case-insensitive substring search across lines.
    let pattern_lower = {
        let mut p = alloc::string::String::with_capacity(pattern.len());
        for c in pattern.chars() {
            for lc in c.to_lowercase() {
                p.push(lc);
            }
        }
        p
    };

    let mut match_count = 0usize;
    for (line_num, line) in text.lines().enumerate() {
        // Build lowercase version of line for comparison.
        let line_lower = {
            let mut l = alloc::string::String::with_capacity(line.len());
            for c in line.chars() {
                for lc in c.to_lowercase() {
                    l.push(lc);
                }
            }
            l
        };

        if line_lower.contains(pattern_lower.as_str()) {
            crate::console_println!(
                "{}:{}: {}",
                line_num.saturating_add(1),
                path,
                line,
            );
            match_count = match_count.saturating_add(1);

            // Limit output to prevent flooding.
            if match_count >= 50 {
                crate::console_println!("... (showing first 50 matches)");
                break;
            }
        }
    }

    if match_count == 0 {
        crate::console_println!("grep: no matches for '{}' in {}", pattern, path);
    } else {
        crate::console_println!("{} matches", match_count);
    }
}

/// List open file handles (like `lsof`).
fn cmd_lsof() {
    let handles = crate::fs::handle::list_handles();
    if handles.is_empty() {
        crate::console_println!("No open file handles.");
        return;
    }

    crate::console_println!(
        "{:<7} {:<5} {:<12} {:<12} {}",
        "HANDLE", "FLAGS", "OFFSET", "SIZE", "PATH"
    );

    for h in &handles {
        // Decode flags into a compact string.
        let mut flags = alloc::string::String::new();
        if h.flags & 0x01 != 0 { flags.push('R'); }
        if h.flags & 0x02 != 0 { flags.push('W'); }
        if h.flags & 0x04 != 0 { flags.push('C'); }
        if h.flags & 0x08 != 0 { flags.push('T'); }
        if h.flags & 0x10 != 0 { flags.push('A'); }
        if flags.is_empty() { flags.push('-'); }

        crate::console_println!(
            "{:<7} {:<5} {:<12} {:<12} {}",
            h.id, flags, h.offset, h.size, h.path,
        );
    }

    crate::console_println!("\nTotal: {} open handles", handles.len());
}

/// Paginated directory listing.
///
/// Usage: `lsp [page_size] <path>`
/// Shows entries one page at a time, with "--- more ---" between pages.
/// Default page size is 20 entries if not specified.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_lsp(args: &str) {
    let (page_size, path) = {
        let mut parts = args.splitn(2, ' ');
        let first = parts.next().unwrap_or("");
        let second = parts.next();

        // Try to parse first arg as a number.
        match first.parse::<usize>() {
            Ok(n) if n > 0 => {
                // First arg is page size, second is path (or "/" default).
                (n, second.unwrap_or("/"))
            }
            _ => {
                // First arg is the path (or "/" if empty).
                (20, if first.is_empty() { "/" } else { first })
            }
        }
    };

    let mut offset = 0usize;
    loop {
        match crate::fs::Vfs::readdir_at(path, offset, page_size) {
            Ok((entries, total)) => {
                if offset == 0 {
                    crate::console_println!(
                        "Directory '{}' — {} entries (page size {})",
                        path, total, page_size,
                    );
                    crate::console_println!(
                        "{:<5} {:<8} {:<12} {}",
                        "TYPE", "SIZE", "NAME", "",
                    );
                }

                if entries.is_empty() {
                    if offset == 0 {
                        crate::console_println!("  (empty directory)");
                    }
                    break;
                }

                for entry in &entries {
                    let type_str = match entry.entry_type {
                        crate::fs::vfs::EntryType::File => "FILE",
                        crate::fs::vfs::EntryType::Directory => "DIR",
                        crate::fs::vfs::EntryType::Symlink => "LINK",
                        crate::fs::vfs::EntryType::VolumeLabel => "VOL",
                    };
                    crate::console_println!(
                        "{:<5} {:<8} {}",
                        type_str, entry.size, entry.name,
                    );
                }

                offset += entries.len();

                if offset >= total {
                    crate::console_println!(
                        "--- end ({}/{} entries shown) ---",
                        offset, total,
                    );
                    break;
                }

                crate::console_println!(
                    "--- {}/{} shown, press Enter for next page ---",
                    offset, total,
                );

                // Wait for Enter key to continue.
                let mut dummy = alloc::string::String::new();
                read_line(&mut dummy);
            }
            Err(e) => {
                crate::console_println!("lsp: error: {:?}", e);
                break;
            }
        }
    }
}

/// List mounted filesystems or mount a new one.
fn cmd_mount(args: &str) {
    if args.is_empty() {
        // List all mounts.
        let mounts = crate::fs::Vfs::mounts();
        if mounts.is_empty() {
            crate::console_println!("No filesystems mounted.");
        } else {
            crate::console_println!("{:<12} {}", "Type", "Mount point");
            for (path, fs_type) in &mounts {
                crate::console_println!("{:<12} {}", fs_type, path);
            }
        }
    } else {
        crate::console_println!("mount: mounting from kshell not yet supported.");
        crate::console_println!("Use 'mount' with no args to list mounts.");
    }
}

/// Unmount a filesystem.
fn cmd_umount(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: umount <mount-path>");
        return;
    }

    let path = if args.starts_with('/') {
        alloc::string::String::from(args)
    } else {
        let mut s = alloc::string::String::from("/");
        s.push_str(args);
        s
    };

    match crate::fs::Vfs::unmount(&path) {
        Ok(()) => {
            crate::console_println!("{}: unmounted", path);
        }
        Err(e) => {
            crate::console_println!("umount: {}: {:?}", path, e);
        }
    }
}

/// Flush all filesystems to stable storage.
fn cmd_sync() {
    match crate::fs::Vfs::sync() {
        Ok(()) => {
            crate::console_println!("All filesystems synced.");
        }
        Err(e) => {
            crate::console_println!("sync: {:?}", e);
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
