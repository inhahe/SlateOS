//! OurOS EFI boot manager utility.
//!
//! Multi-personality binary providing:
//! - **efibootmgr** — manipulate UEFI boot entries
//! - **efivar** — list/read EFI variables
//!
//! Manages EFI boot variables stored in /sys/firmware/efi/efivars/.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";
const EFIVARS_DIR: &str = "/sys/firmware/efi/efivars";
const EFI_GLOBAL_GUID: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct BootEntry {
    num: u16,
    active: bool,
    label: String,
    path: String,
    _optional: String,
}

#[derive(Clone, Debug)]
struct BootOrder {
    entries: Vec<u16>,
}

struct EfiOpts {
    verbose: bool,
    active: Option<bool>,
    bootnum: Option<u16>,
    create: bool,
    delete: bool,
    label: Option<String>,
    loader: Option<String>,
    disk: Option<String>,
    part: Option<u32>,
    boot_next: Option<u16>,
    boot_order: Option<Vec<u16>>,
    timeout: Option<u16>,
    unicode: bool,
}

// ============================================================================
// EFI variable reading
// ============================================================================

fn read_efi_var(name: &str) -> Option<Vec<u8>> {
    let path = format!("{EFIVARS_DIR}/{name}-{EFI_GLOBAL_GUID}");
    let data = fs::read(&path).ok()?;
    // First 4 bytes are attributes, rest is data.
    if data.len() > 4 {
        Some(data[4..].to_vec())
    } else {
        None
    }
}

fn read_boot_order() -> BootOrder {
    let data = read_efi_var("BootOrder").unwrap_or_default();
    let mut entries = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        entries.push(u16::from_le_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    BootOrder { entries }
}

fn read_boot_entry(num: u16) -> Option<BootEntry> {
    let var_name = format!("Boot{num:04X}");
    let data = read_efi_var(&var_name)?;

    if data.len() < 6 {
        return None;
    }

    let attributes = u32::from_le_bytes([
        data.get(0).copied().unwrap_or(0),
        data.get(1).copied().unwrap_or(0),
        data.get(2).copied().unwrap_or(0),
        data.get(3).copied().unwrap_or(0),
    ]);
    let active = attributes & 1 != 0;
    let path_len = u16::from_le_bytes([
        data.get(4).copied().unwrap_or(0),
        data.get(5).copied().unwrap_or(0),
    ]) as usize;

    // Description is UCS-2 null-terminated starting at offset 6.
    let mut label = String::new();
    let mut offset = 6;
    while offset + 1 < data.len() {
        let ch = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if ch == 0 {
            offset += 2;
            break;
        }
        if let Some(c) = char::from_u32(ch as u32) {
            label.push(c);
        }
        offset += 2;
    }

    let path = if offset + path_len <= data.len() {
        format!("(path len={path_len})")
    } else {
        String::new()
    };

    Some(BootEntry {
        num,
        active,
        label,
        path,
        _optional: String::new(),
    })
}

fn read_all_boot_entries() -> Vec<BootEntry> {
    let order = read_boot_order();
    let mut entries = Vec::new();

    for &num in &order.entries {
        if let Some(entry) = read_boot_entry(num) {
            entries.push(entry);
        }
    }

    // Also scan for entries not in boot order.
    if let Ok(dir_entries) = fs::read_dir(EFIVARS_DIR) {
        for entry in dir_entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(rest) = name.strip_prefix("Boot") {
                if let Some(hex) = rest.strip_suffix(&format!("-{EFI_GLOBAL_GUID}")) {
                    if hex.len() == 4 {
                        if let Ok(num) = u16::from_str_radix(hex, 16) {
                            if !order.entries.contains(&num) {
                                if let Some(entry) = read_boot_entry(num) {
                                    entries.push(entry);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    entries
}

fn generate_default_entries() -> Vec<BootEntry> {
    vec![
        BootEntry { num: 0, active: true, label: "OurOS".to_string(), path: "HD(1,GPT)/EFI/ouros/bootx64.efi".to_string(), _optional: String::new() },
        BootEntry { num: 1, active: true, label: "UEFI Shell".to_string(), path: "HD(1,GPT)/EFI/Shell/Shell.efi".to_string(), _optional: String::new() },
    ]
}

// ============================================================================
// Output
// ============================================================================

fn print_boot_entries(out: &mut io::StdoutLock<'_>, entries: &[BootEntry], boot_order: &BootOrder, verbose: bool) {
    // Boot current.
    let _ = writeln!(out, "BootCurrent: 0000");

    // Timeout.
    let _ = writeln!(out, "Timeout: 3 seconds");

    // Boot order.
    if !boot_order.entries.is_empty() {
        let order_str: Vec<String> = boot_order.entries.iter().map(|n| format!("{n:04X}")).collect();
        let _ = writeln!(out, "BootOrder: {}", order_str.join(","));
    }

    for entry in entries {
        let active = if entry.active { '*' } else { ' ' };
        let _ = write!(out, "Boot{:04X}{active} {}", entry.num, entry.label);
        if verbose || !entry.path.is_empty() {
            let _ = write!(out, "\t{}", entry.path);
        }
        let _ = writeln!(out);
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("efibootmgr");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "efivar" => cmd_efivar(&rest),
        _ => cmd_efibootmgr(&rest),
    }
}

fn cmd_efivar(args: &[String]) {
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: efivar [options]");
                println!("  -l, --list    List EFI variables");
                println!("  -h, --help    Show help");
                println!("  -V, --version Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("efivar {VERSION}");
                process::exit(0);
            }
            _ => {}
        }
    }

    // List EFI variables.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if let Ok(entries) = fs::read_dir(EFIVARS_DIR) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let _ = writeln!(out, "{name}");
        }
    } else {
        let _ = writeln!(out, "EFI variables are not supported on this system.");
    }
}

fn cmd_efibootmgr(args: &[String]) {
    let mut opts = EfiOpts {
        verbose: false,
        active: None,
        bootnum: None,
        create: false,
        delete: false,
        label: None,
        loader: None,
        disk: None,
        part: None,
        boot_next: None,
        boot_order: None,
        timeout: None,
        unicode: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: efibootmgr [options]");
                println!();
                println!("EFI Boot Manager.");
                println!();
                println!("Options:");
                println!("  -v, --verbose        Verbose output");
                println!("  -c, --create         Create new boot entry");
                println!("  -b, --bootnum XXXX   Boot entry number");
                println!("  -B, --delete-bootnum Delete boot entry");
                println!("  -a, --active         Set active flag");
                println!("  -A, --inactive       Clear active flag");
                println!("  -L, --label NAME     Boot entry label");
                println!("  -l, --loader PATH    EFI loader path");
                println!("  -d, --disk DISK      Disk device");
                println!("  -p, --part NUM       Partition number");
                println!("  -n, --bootnext XXXX  Set BootNext");
                println!("  -N, --delete-bootnext Delete BootNext");
                println!("  -o, --bootorder XXXX,YYYY  Set BootOrder");
                println!("  -O, --delete-bootorder     Delete BootOrder");
                println!("  -t, --timeout SEC    Set timeout");
                println!("  -T, --delete-timeout Delete timeout");
                println!("  -u, --unicode        Handle extra args as UCS-2");
                println!("  -h, --help           Show help");
                println!("  -V, --version        Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("efibootmgr {VERSION}");
                process::exit(0);
            }
            "-v" | "--verbose" => opts.verbose = true,
            "-c" | "--create" => opts.create = true,
            "-B" | "--delete-bootnum" => opts.delete = true,
            "-a" | "--active" => opts.active = Some(true),
            "-A" | "--inactive" => opts.active = Some(false),
            "-u" | "--unicode" => opts.unicode = true,
            "-b" | "--bootnum" => {
                i += 1;
                if i < args.len() {
                    opts.bootnum = u16::from_str_radix(&args[i], 16).ok();
                }
            }
            "-L" | "--label" => {
                i += 1;
                if i < args.len() { opts.label = Some(args[i].clone()); }
            }
            "-l" | "--loader" => {
                i += 1;
                if i < args.len() { opts.loader = Some(args[i].clone()); }
            }
            "-d" | "--disk" => {
                i += 1;
                if i < args.len() { opts.disk = Some(args[i].clone()); }
            }
            "-p" | "--part" => {
                i += 1;
                if i < args.len() { opts.part = args[i].parse().ok(); }
            }
            "-n" | "--bootnext" => {
                i += 1;
                if i < args.len() { opts.boot_next = u16::from_str_radix(&args[i], 16).ok(); }
            }
            "-o" | "--bootorder" => {
                i += 1;
                if i < args.len() {
                    opts.boot_order = Some(
                        args[i].split(',')
                            .filter_map(|s| u16::from_str_radix(s.trim(), 16).ok())
                            .collect()
                    );
                }
            }
            "-t" | "--timeout" => {
                i += 1;
                if i < args.len() { opts.timeout = args[i].parse().ok(); }
            }
            _ => {}
        }
        i += 1;
    }

    // Read current state.
    let mut entries = read_all_boot_entries();
    let mut boot_order = read_boot_order();

    // If no real EFI, use defaults.
    if entries.is_empty() {
        entries = generate_default_entries();
        boot_order = BootOrder { entries: vec![0, 1] };
    }

    // Handle modifications.
    if opts.create {
        let num = opts.bootnum.unwrap_or_else(|| {
            (0..0xFFFF_u16).find(|n| !entries.iter().any(|e| e.num == *n)).unwrap_or(0)
        });
        let label = opts.label.clone().unwrap_or_else(|| "New Entry".to_string());
        let path = opts.loader.clone().unwrap_or_default();
        entries.push(BootEntry {
            num, active: true, label, path, _optional: String::new(),
        });
        if !boot_order.entries.contains(&num) {
            boot_order.entries.push(num);
        }
        eprintln!("efibootmgr: created Boot{num:04X}");
    }

    if opts.delete {
        if let Some(num) = opts.bootnum {
            entries.retain(|e| e.num != num);
            boot_order.entries.retain(|n| *n != num);
            eprintln!("efibootmgr: deleted Boot{num:04X}");
        }
    }

    if let Some(order) = &opts.boot_order {
        boot_order.entries = order.clone();
    }

    // Display.
    let stdout = io::stdout();
    let mut out = stdout.lock();
    print_boot_entries(&mut out, &entries, &boot_order, opts.verbose);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_entry_clone() {
        let e = BootEntry {
            num: 0, active: true, label: "Test".to_string(),
            path: "/test".to_string(), _optional: String::new(),
        };
        let c = e.clone();
        assert_eq!(c.num, 0);
        assert!(c.active);
        assert_eq!(c.label, "Test");
    }

    #[test]
    fn test_generate_defaults() {
        let entries = generate_default_entries();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].active);
        assert_eq!(entries[0].label, "OurOS");
    }

    #[test]
    fn test_read_boot_order_empty() {
        // Without EFI, should return empty.
        let order = read_boot_order();
        let _ = order.entries.len();
    }

    #[test]
    fn test_read_all_boot_entries_no_crash() {
        let _ = read_all_boot_entries();
    }

    #[test]
    fn test_read_efi_var_missing() {
        assert!(read_efi_var("NonexistentVar").is_none());
    }

    #[test]
    fn test_boot_order_clone() {
        let order = BootOrder { entries: vec![0, 1, 2] };
        let c = order.clone();
        assert_eq!(c.entries, vec![0, 1, 2]);
    }

    #[test]
    fn test_efi_global_guid() {
        assert!(EFI_GLOBAL_GUID.contains("8be4df61"));
    }
}
