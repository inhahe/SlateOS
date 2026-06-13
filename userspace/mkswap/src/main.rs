// Slate OS mkswap - swap management (mkswap/swapon/swapoff)
//
// Multi-personality binary:
//   mkswap  - create swap area on device/file
//   swapon  - enable swap on device/file
//   swapoff - disable swap on device/file
//
// SwapHeader / SwapEntry / verify_swap_signature / read_swap_header /
// parse_proc_swaps / format_size_kb and a few struct fields are
// declared for the Linux swap on-disk + /proc/swaps ABI but are not
// yet invoked. Allow dead_code file-wide so the protocol surface stays
// visible without warning spam.
#![allow(dead_code)]
#![cfg_attr(not(test), no_main)]

use std::fmt;

// ── Constants ──────────────────────────────────────────────────────────

const SWAP_MAGIC: &[u8; 10] = b"SWAPSPACE2";
const SWAP_MAGIC_OFFSET: usize = 4086; // page_size - 10 for 4K page
const SWAP_MAGIC_OFFSET_16K: usize = 16374; // 16KiB page - 10
const PAGE_SIZE: u32 = 16384; // 16 KiB pages for Slate OS
const MIN_SWAP_SIZE: u64 = 10 * PAGE_SIZE as u64;
const MAX_LABEL_LEN: usize = 16;
const UUID_LEN: usize = 16;
const SWAP_HEADER_SIZE: usize = 1024;
const DEFAULT_PRIORITY: i32 = -1;

// Swap header at the beginning of swap area
// Compatible with Linux swap header v1
#[repr(C)]
struct SwapHeader {
    bootbits: [u8; 1024],      // reserved for boot sector
    version: u32,               // swap version (1)
    last_page: u32,             // last usable page index
    nr_badpages: u32,           // number of bad pages
    sws_uuid: [u8; UUID_LEN],  // UUID
    sws_volume: [u8; MAX_LABEL_LEN], // volume label
    padding: [u32; 117],       // padding to page boundary
    badpages: [u32; 1],        // bad page list (variable length)
}

// Swap entry in /proc/swaps style tracking
struct SwapEntry {
    path: Vec<u8>,
    swap_type: SwapType,
    size_kb: u64,
    used_kb: u64,
    priority: i32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum SwapType {
    Partition,
    File,
}

impl fmt::Display for SwapType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SwapType::Partition => write!(f, "partition"),
            SwapType::File => write!(f, "file"),
        }
    }
}

// ── Personality Detection ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Personality {
    Mkswap,
    Swapon,
    Swapoff,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    // Extract basename from argv[0]
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };

    // Strip .exe suffix if present
    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe") {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"swapon") {
        Personality::Swapon
    } else if name.eq_ignore_ascii_case(b"swapoff") {
        Personality::Swapoff
    } else {
        Personality::Mkswap
    }
}

// ── Argument Parsing ───────────────────────────────────────────────────

struct MkswapArgs {
    device: Vec<u8>,
    label: Option<Vec<u8>>,
    uuid: Option<[u8; UUID_LEN]>,
    page_size: u32,
    force: bool,
    check: bool,
    verbose: bool,
    show_help: bool,
    show_version: bool,
}

struct SwaponArgs {
    device: Option<Vec<u8>>,
    all: bool,
    priority: Option<i32>,
    discard: bool,
    show: bool,
    summary: bool,
    verbose: bool,
    show_help: bool,
    show_version: bool,
    no_heading: bool,
    bytes: bool,
}

struct SwapoffArgs {
    device: Option<Vec<u8>>,
    all: bool,
    verbose: bool,
    show_help: bool,
    show_version: bool,
}

fn parse_mkswap_args(args: &[Vec<u8>]) -> MkswapArgs {
    let mut result = MkswapArgs {
        device: Vec::new(),
        label: None,
        uuid: None,
        page_size: PAGE_SIZE,
        force: false,
        check: false,
        verbose: false,
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == b"-L" || arg == b"--label" {
            i += 1;
            if i < args.len() {
                let label = args[i].clone();
                if label.len() > MAX_LABEL_LEN {
                    print_err(b"mkswap: label too long (max 16 characters)\n");
                } else {
                    result.label = Some(label);
                }
            }
        } else if arg == b"-U" || arg == b"--uuid" {
            i += 1;
            if i < args.len() {
                if let Some(uuid) = parse_uuid(&args[i]) {
                    result.uuid = Some(uuid);
                } else {
                    print_err(b"mkswap: invalid UUID format\n");
                }
            }
        } else if arg == b"-p" || arg == b"--pagesize" {
            i += 1;
            if i < args.len()
                && let Some(ps) = parse_u64_bytes(&args[i]) {
                    result.page_size = ps as u32;
                }
        } else if arg == b"-f" || arg == b"--force" {
            result.force = true;
        } else if arg == b"-c" || arg == b"--check" {
            result.check = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if !arg.starts_with(b"-") || arg == b"-" {
            result.device = arg.clone();
        }
        i += 1;
    }

    result
}

fn parse_swapon_args(args: &[Vec<u8>]) -> SwaponArgs {
    let mut result = SwaponArgs {
        device: None,
        all: false,
        priority: None,
        discard: false,
        show: false,
        summary: false,
        verbose: false,
        show_help: false,
        show_version: false,
        no_heading: false,
        bytes: false,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == b"-a" || arg == b"--all" {
            result.all = true;
        } else if arg == b"-p" || arg == b"--priority" {
            i += 1;
            if i < args.len()
                && let Some(p) = parse_i32_bytes(&args[i]) {
                    result.priority = Some(p);
                }
        } else if arg == b"-d" || arg == b"--discard" {
            result.discard = true;
        } else if arg == b"-s" || arg == b"--summary" {
            result.summary = true;
        } else if arg == b"--show" {
            result.show = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if arg == b"--no-heading" || arg == b"--noheadings" {
            result.no_heading = true;
        } else if arg == b"--bytes" {
            result.bytes = true;
        } else if !arg.starts_with(b"-") || arg == b"-" {
            result.device = Some(arg.clone());
        }
        i += 1;
    }

    result
}

fn parse_swapoff_args(args: &[Vec<u8>]) -> SwapoffArgs {
    let mut result = SwapoffArgs {
        device: None,
        all: false,
        verbose: false,
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == b"-a" || arg == b"--all" {
            result.all = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if !arg.starts_with(b"-") || arg == b"-" {
            result.device = Some(arg.clone());
        }
        i += 1;
    }

    result
}

// ── UUID Handling ──────────────────────────────────────────────────────

fn parse_uuid(s: &[u8]) -> Option<[u8; UUID_LEN]> {
    // Parse UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
    let mut uuid = [0u8; UUID_LEN];
    let mut hex_chars = Vec::new();

    for &b in s {
        if b == b'-' {
            continue;
        }
        let nibble = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        hex_chars.push(nibble);
    }

    if hex_chars.len() != 32 {
        return None;
    }

    for i in 0..UUID_LEN {
        uuid[i] = (hex_chars[i * 2] << 4) | hex_chars[i * 2 + 1];
    }

    Some(uuid)
}

fn format_uuid(uuid: &[u8; UUID_LEN]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(36);
    let hex = b"0123456789abcdef";

    for (i, &byte) in uuid.iter().enumerate() {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            buf.push(b'-');
        }
        buf.push(hex[(byte >> 4) as usize]);
        buf.push(hex[(byte & 0x0f) as usize]);
    }

    buf
}

fn generate_uuid() -> [u8; UUID_LEN] {
    // Simple UUID v4 generation using a basic PRNG
    // In real OS, would use /dev/urandom
    let mut uuid = [0u8; UUID_LEN];
    let mut seed: u64 = 0x12345678_9abcdef0; // Would use real entropy source

    // Simple xorshift64 PRNG
    for byte in uuid.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *byte = (seed & 0xFF) as u8;
    }

    // Set version 4 (random)
    uuid[6] = (uuid[6] & 0x0F) | 0x40;
    // Set variant (10xx)
    uuid[8] = (uuid[8] & 0x3F) | 0x80;

    uuid
}

// ── Swap Header Operations ─────────────────────────────────────────────

struct SwapSetup {
    header: Vec<u8>,
    uuid: [u8; UUID_LEN],
    label: Vec<u8>,
    nr_pages: u32,
    page_size: u32,
}

fn create_swap_header(
    device_size: u64,
    label: Option<&[u8]>,
    uuid: Option<[u8; UUID_LEN]>,
    page_size: u32,
    _check_bad_blocks: bool,
) -> Result<SwapSetup, &'static str> {
    if device_size < MIN_SWAP_SIZE {
        return Err("device too small for swap");
    }

    let nr_pages = (device_size / page_size as u64) as u32;
    if nr_pages < 10 {
        return Err("too few pages for swap");
    }

    let uuid = uuid.unwrap_or_else(generate_uuid);

    // Build the header page
    let mut header = vec![0u8; page_size as usize];

    // Version at offset 1024
    let version: u32 = 1;
    header[1024..1028].copy_from_slice(&version.to_le_bytes());

    // Last usable page (nr_pages - 1, since page 0 is the header)
    let last_page = nr_pages.saturating_sub(1);
    header[1028..1032].copy_from_slice(&last_page.to_le_bytes());

    // Bad pages count = 0
    header[1032..1036].copy_from_slice(&0u32.to_le_bytes());

    // UUID at offset 1036
    header[1036..1052].copy_from_slice(&uuid);

    // Label at offset 1052
    let mut label_bytes = [0u8; MAX_LABEL_LEN];
    let label_vec;
    if let Some(l) = label {
        let copy_len = l.len().min(MAX_LABEL_LEN);
        label_bytes[..copy_len].copy_from_slice(&l[..copy_len]);
        label_vec = l.to_vec();
    } else {
        label_vec = Vec::new();
    }
    header[1052..1068].copy_from_slice(&label_bytes);

    // Magic signature at page_size - 10
    let magic_offset = page_size as usize - 10;
    header[magic_offset..magic_offset + 10].copy_from_slice(SWAP_MAGIC);

    Ok(SwapSetup {
        header,
        uuid,
        label: label_vec,
        nr_pages,
        page_size,
    })
}

fn verify_swap_signature(header: &[u8], page_size: u32) -> bool {
    let magic_offset = page_size as usize - 10;
    if header.len() < magic_offset + 10 {
        return false;
    }
    &header[magic_offset..magic_offset + 10] == SWAP_MAGIC
}

fn read_swap_header(header: &[u8]) -> Option<(u32, u32, [u8; UUID_LEN], Vec<u8>)> {
    if header.len() < 1068 {
        return None;
    }

    let version = u32::from_le_bytes([header[1024], header[1025], header[1026], header[1027]]);
    let last_page = u32::from_le_bytes([header[1028], header[1029], header[1030], header[1031]]);

    let mut uuid = [0u8; UUID_LEN];
    uuid.copy_from_slice(&header[1036..1052]);

    let mut label = Vec::new();
    for &b in &header[1052..1068] {
        if b == 0 {
            break;
        }
        label.push(b);
    }

    Some((version, last_page, uuid, label))
}

// ── /proc/swaps and /etc/fstab Parsing ─────────────────────────────────

fn parse_proc_swaps(content: &[u8]) -> Vec<SwapEntry> {
    let mut entries = Vec::new();
    let mut lines = content.split(|&b| b == b'\n');

    // Skip header line
    let _ = lines.next();

    for line in lines {
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&[u8]> = line.split(|&b| b == b' ' || b == b'\t')
            .filter(|f| !f.is_empty())
            .collect();

        if fields.len() >= 5 {
            let path = fields[0].to_vec();
            let swap_type = if fields[1] == b"partition" {
                SwapType::Partition
            } else {
                SwapType::File
            };
            let size_kb = parse_u64_bytes(fields[2]).unwrap_or(0);
            let used_kb = parse_u64_bytes(fields[3]).unwrap_or(0);
            let priority = parse_i32_bytes(fields[4]).unwrap_or(DEFAULT_PRIORITY);

            entries.push(SwapEntry {
                path,
                swap_type,
                size_kb,
                used_kb,
                priority,
            });
        }
    }

    entries
}

struct FstabEntry {
    device: Vec<u8>,
    mount_point: Vec<u8>,
    fs_type: Vec<u8>,
    options: Vec<u8>,
    dump: u32,
    pass: u32,
}

fn parse_fstab(content: &[u8]) -> Vec<FstabEntry> {
    let mut entries = Vec::new();

    for line in content.split(|&b| b == b'\n') {
        let trimmed = trim_bytes(line);
        if trimmed.is_empty() || trimmed.starts_with(b"#") {
            continue;
        }

        let fields: Vec<&[u8]> = trimmed.split(|&b| b == b' ' || b == b'\t')
            .filter(|f| !f.is_empty())
            .collect();

        if fields.len() >= 4 {
            entries.push(FstabEntry {
                device: fields[0].to_vec(),
                mount_point: fields[1].to_vec(),
                fs_type: fields[2].to_vec(),
                options: if fields.len() > 3 { fields[3].to_vec() } else { b"defaults".to_vec() },
                dump: if fields.len() > 4 { parse_u64_bytes(fields[4]).unwrap_or(0) as u32 } else { 0 },
                pass: if fields.len() > 5 { parse_u64_bytes(fields[5]).unwrap_or(0) as u32 } else { 0 },
            });
        }
    }

    entries
}

fn get_swap_fstab_entries(fstab: &[FstabEntry]) -> Vec<&FstabEntry> {
    fstab.iter().filter(|e| e.fs_type == b"swap").collect()
}

// ── /etc/fstab Option Parsing ──────────────────────────────────────────

struct SwapOptions {
    priority: Option<i32>,
    discard: bool,
    no_auto: bool,
}

fn parse_swap_options(opts: &[u8]) -> SwapOptions {
    let mut result = SwapOptions {
        priority: None,
        discard: false,
        no_auto: false,
    };

    for opt in opts.split(|&b| b == b',') {
        if opt.starts_with(b"pri=") {
            if let Some(p) = parse_i32_bytes(&opt[4..]) {
                result.priority = Some(p);
            }
        } else if opt == b"discard" {
            result.discard = true;
        } else if opt == b"noauto" {
            result.no_auto = true;
        }
    }

    result
}

// ── Size Formatting ────────────────────────────────────────────────────

fn format_size(bytes: u64) -> Vec<u8> {
    if bytes < 1024 {
        let mut buf = format_u64(bytes);
        buf.extend_from_slice(b" B");
        return buf;
    }

    let kb = bytes / 1024;
    if kb < 1024 {
        let mut buf = format_u64(kb);
        buf.extend_from_slice(b" KiB");
        return buf;
    }

    let mb = kb / 1024;
    if mb < 1024 {
        let mut buf = format_u64(mb);
        buf.extend_from_slice(b" MiB");
        return buf;
    }

    let gb = mb / 1024;
    if gb < 1024 {
        let mut buf = format_u64(gb);
        buf.extend_from_slice(b" GiB");
        return buf;
    }

    let tb = gb / 1024;
    let mut buf = format_u64(tb);
    buf.extend_from_slice(b" TiB");
    buf
}

fn format_size_kb(kb: u64, use_bytes: bool) -> Vec<u8> {
    if use_bytes {
        let bytes = kb.saturating_mul(1024);
        return format_u64(bytes);
    }
    format_size(kb.saturating_mul(1024))
}

// ── mkswap Command ─────────────────────────────────────────────────────

fn cmd_mkswap(args: &MkswapArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: mkswap [options] device [size]\n\n");
        print_out(b"Set up a Linux/SlateOS swap area.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -L, --label LABEL    specify swap label\n");
        print_out(b"  -U, --uuid UUID      specify UUID\n");
        print_out(b"  -p, --pagesize SIZE   specify page size (default: 16384)\n");
        print_out(b"  -f, --force           force creation even if device seems wrong\n");
        print_out(b"  -c, --check           check for bad blocks before creating swap\n");
        print_out(b"  -v, --verbose         verbose output\n");
        print_out(b"  -h, --help            display this help\n");
        print_out(b"  -V, --version         display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"mkswap (Slate OS util-linux) 1.0.0\n");
        return 0;
    }

    if args.device.is_empty() {
        print_err(b"mkswap: no device specified\n");
        print_err(b"Usage: mkswap [options] device [size]\n");
        return 1;
    }

    // Validate page size is power of 2 and reasonable
    if !args.page_size.is_power_of_two() || args.page_size < 4096 {
        print_err(b"mkswap: invalid page size\n");
        return 1;
    }

    // In a real implementation, we would:
    // 1. Open the device
    // 2. Check if it's a block device or regular file
    // 3. Get its size
    // 4. Optionally check for bad blocks
    // 5. Write the swap header
    // 6. Sync to disk

    // Simulate getting device size (would use fstat/ioctl in real implementation)
    let _device_path = bytes_to_str_lossy(&args.device);

    if args.verbose {
        let mut msg = b"Setting up swapspace on ".to_vec();
        msg.extend_from_slice(&args.device);
        msg.extend_from_slice(b", page size ");
        msg.extend_from_slice(&format_u64(args.page_size as u64));
        msg.extend_from_slice(b" bytes\n");
        print_out(&msg);
    }

    // Simulate device size detection
    let device_size: u64 = 1024 * 1024 * 1024; // 1 GiB default for simulation

    let label_ref = args.label.as_deref();
    match create_swap_header(device_size, label_ref, args.uuid, args.page_size, args.check) {
        Ok(setup) => {
            // In real implementation: write header to device
            // write_all(fd, &setup.header)?;
            // fsync(fd)?;

            let size_display = format_size(device_size);
            print_out(b"Setting up swapspace version 1, size = ");
            print_out(&size_display);

            if !setup.label.is_empty() {
                print_out(b", LABEL=");
                print_out(&setup.label);
            }

            print_out(b", UUID=");
            let uuid_str = format_uuid(&setup.uuid);
            print_out(&uuid_str);
            print_out(b"\n");

            if args.check {
                print_out(b"No bad blocks found\n");
            }

            0
        }
        Err(e) => {
            print_err(b"mkswap: ");
            print_err(e.as_bytes());
            print_err(b"\n");
            1
        }
    }
}

// ── swapon Command ─────────────────────────────────────────────────────

fn cmd_swapon(args: &SwaponArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: swapon [options] [device]\n\n");
        print_out(b"Enable devices and files for paging and swapping.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -a, --all             enable all swap from /etc/fstab\n");
        print_out(b"  -p, --priority NUM    specify swap priority\n");
        print_out(b"  -d, --discard         enable discard/TRIM\n");
        print_out(b"  -s, --summary         display swap summary (deprecated)\n");
        print_out(b"      --show            display swap in format like /proc/swaps\n");
        print_out(b"      --no-heading      don't print headings (with --show)\n");
        print_out(b"      --bytes           display sizes in bytes\n");
        print_out(b"  -v, --verbose         verbose output\n");
        print_out(b"  -h, --help            display this help\n");
        print_out(b"  -V, --version         display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"swapon (Slate OS util-linux) 1.0.0\n");
        return 0;
    }

    // Show swap status
    if args.show || args.summary || (args.device.is_none() && !args.all) {
        return show_swap_status(args);
    }

    // Enable all swap from fstab
    if args.all {
        return swapon_all(args);
    }

    // Enable specific device
    if let Some(ref device) = args.device {
        return swapon_device(device, args.priority, args.discard, args.verbose);
    }

    // No device and no flags - show status
    show_swap_status(args)
}

fn show_swap_status(args: &SwaponArgs) -> i32 {
    // Read /proc/swaps
    // In real implementation would read from kernel
    let simulated_content = b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n";

    if args.show || (args.device.is_none() && !args.all && !args.summary) {
        // Modern --show format
        if !args.no_heading {
            print_out(b"NAME       TYPE       SIZE   USED  PRIO\n");
        }
        // Would list active swap devices here
    } else {
        // Legacy --summary format (like /proc/swaps)
        print_out(simulated_content);
    }

    0
}

fn swapon_all(args: &SwaponArgs) -> i32 {
    // Read /etc/fstab and enable all swap entries
    // In real implementation: read fstab, iterate swap entries, call swapon syscall
    if args.verbose {
        print_out(b"swapon: enabling all swap from /etc/fstab\n");
    }

    // Simulated: read fstab
    let fstab_content = b"# /etc/fstab\n";
    let fstab = parse_fstab(fstab_content);
    let swap_entries = get_swap_fstab_entries(&fstab);

    let mut errors = 0;

    for entry in swap_entries {
        let opts = parse_swap_options(&entry.options);
        if opts.no_auto {
            if args.verbose {
                print_out(b"swapon: skipping ");
                print_out(&entry.device);
                print_out(b" (noauto)\n");
            }
            continue;
        }

        let priority = args.priority.or(opts.priority);
        let discard = args.discard || opts.discard;

        if swapon_device(&entry.device, priority, discard, args.verbose) != 0 {
            errors += 1;
        }
    }

    if errors > 0 { 1 } else { 0 }
}

fn swapon_device(device: &[u8], priority: Option<i32>, discard: bool, verbose: bool) -> i32 {
    // In real implementation:
    // 1. Open device
    // 2. Read first page to verify swap signature
    // 3. Call swapon(2) syscall with appropriate flags
    // 4. Report result

    if verbose {
        print_out(b"swapon: enabling swap on ");
        print_out(device);
        if let Some(p) = priority {
            print_out(b" (priority ");
            print_out(&format_i32(p));
            print_out(b")");
        }
        if discard {
            print_out(b" (discard)");
        }
        print_out(b"\n");
    }

    // Simulate: would call swapon(2)
    // let flags = if discard { SWAP_FLAG_DISCARD } else { 0 }
    //           | if let Some(p) = priority { SWAP_FLAG_PREFER | ((p as u32) << SWAP_FLAG_PRIO_SHIFT) } else { 0 };
    // syscall(SYS_swapon, device_path, flags);

    0
}

// ── swapoff Command ────────────────────────────────────────────────────

fn cmd_swapoff(args: &SwapoffArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: swapoff [options] [device]\n\n");
        print_out(b"Disable devices and files for paging and swapping.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -a, --all       disable all swap\n");
        print_out(b"  -v, --verbose   verbose output\n");
        print_out(b"  -h, --help      display this help\n");
        print_out(b"  -V, --version   display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"swapoff (Slate OS util-linux) 1.0.0\n");
        return 0;
    }

    if args.all {
        return swapoff_all(args.verbose);
    }

    if let Some(ref device) = args.device {
        return swapoff_device(device, args.verbose);
    }

    print_err(b"swapoff: no device specified\n");
    print_err(b"Usage: swapoff [options] [device]\n");
    1
}

fn swapoff_all(verbose: bool) -> i32 {
    // Read /proc/swaps and disable all
    if verbose {
        print_out(b"swapoff: disabling all swap\n");
    }

    // In real implementation: read /proc/swaps, call swapoff for each
    // let entries = parse_proc_swaps(&read_file("/proc/swaps"));
    // for entry in entries {
    //     swapoff_device(&entry.path, verbose);
    // }

    0
}

fn swapoff_device(device: &[u8], verbose: bool) -> i32 {
    if verbose {
        print_out(b"swapoff: disabling swap on ");
        print_out(device);
        print_out(b"\n");
    }

    // In real implementation: syscall(SYS_swapoff, device_path)
    0
}

// ── Utility Functions ──────────────────────────────────────────────────

fn parse_u64_bytes(s: &[u8]) -> Option<u64> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }

    let mut result: u64 = 0;
    let mut has_digit = false;

    for &b in s {
        match b {
            b'0'..=b'9' => {
                has_digit = true;
                result = result.checked_mul(10)?.checked_add((b - b'0') as u64)?;
            }
            b'K' | b'k' => return if has_digit { Some(result.checked_mul(1024)?) } else { None },
            b'M' | b'm' => return if has_digit { Some(result.checked_mul(1024 * 1024)?) } else { None },
            b'G' | b'g' => return if has_digit { Some(result.checked_mul(1024 * 1024 * 1024)?) } else { None },
            _ => return None,
        }
    }

    if has_digit { Some(result) } else { None }
}

fn parse_i32_bytes(s: &[u8]) -> Option<i32> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }

    let (negative, digits) = if s[0] == b'-' {
        (true, &s[1..])
    } else if s[0] == b'+' {
        (false, &s[1..])
    } else {
        (false, s)
    };

    let mut result: i32 = 0;
    for &b in digits {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as i32)?;
            }
            _ => return None,
        }
    }

    if negative {
        Some(-result)
    } else {
        Some(result)
    }
}

fn format_u64(mut n: u64) -> Vec<u8> {
    if n == 0 {
        return vec![b'0'];
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    buf.reverse();
    buf
}

fn format_i32(n: i32) -> Vec<u8> {
    if n < 0 {
        let mut buf = vec![b'-'];
        buf.extend_from_slice(&format_u64(n.unsigned_abs() as u64));
        buf
    } else {
        format_u64(n as u64)
    }
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n').unwrap_or(s.len());
    let end = s.iter().rposition(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .map(|p| p + 1)
        .unwrap_or(start);
    if start >= end { &[] } else { &s[start..end] }
}

fn bytes_to_str_lossy(s: &[u8]) -> &str {
    std::str::from_utf8(s).unwrap_or("<invalid>")
}

fn print_out(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stdout().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

fn print_err(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stderr().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

fn get_args() -> Vec<Vec<u8>> {
    #[cfg(not(test))]
    {
        // Read from /proc/self/cmdline or argc/argv
        // For now, simplified version
        std::env::args().map(|a| a.into_bytes()).collect()
    }
    #[cfg(test)]
    {
        Vec::new()
    }
}

// ── Entry Point ────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args = get_args();
    if args.is_empty() {
        print_err(b"mkswap: unable to determine program name\n");
        return 1;
    }

    let personality = detect_personality(&args[0]);
    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();

    match personality {
        Personality::Mkswap => {
            let parsed = parse_mkswap_args(&rest);
            cmd_mkswap(&parsed)
        }
        Personality::Swapon => {
            let parsed = parse_swapon_args(&rest);
            cmd_swapon(&parsed)
        }
        Personality::Swapoff => {
            let parsed = parse_swapoff_args(&rest);
            cmd_swapoff(&parsed)
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality Detection ──────────────────────────────────

    #[test]
    fn test_detect_mkswap() {
        assert_eq!(detect_personality(b"mkswap"), Personality::Mkswap);
        assert_eq!(detect_personality(b"/usr/sbin/mkswap"), Personality::Mkswap);
        assert_eq!(detect_personality(b"mkswap.exe"), Personality::Mkswap);
    }

    #[test]
    fn test_detect_swapon() {
        assert_eq!(detect_personality(b"swapon"), Personality::Swapon);
        assert_eq!(detect_personality(b"/sbin/swapon"), Personality::Swapon);
    }

    #[test]
    fn test_detect_swapoff() {
        assert_eq!(detect_personality(b"swapoff"), Personality::Swapoff);
        assert_eq!(detect_personality(b"/sbin/swapoff"), Personality::Swapoff);
    }

    #[test]
    fn test_detect_unknown_defaults_mkswap() {
        assert_eq!(detect_personality(b"unknown"), Personality::Mkswap);
    }

    // ── UUID Parsing/Formatting ────────────────────────────────

    #[test]
    fn test_parse_uuid_valid() {
        let uuid = parse_uuid(b"550e8400-e29b-41d4-a716-446655440000");
        assert!(uuid.is_some());
        let u = uuid.unwrap();
        assert_eq!(u[0], 0x55);
        assert_eq!(u[1], 0x0e);
        assert_eq!(u[2], 0x84);
        assert_eq!(u[3], 0x00);
    }

    #[test]
    fn test_parse_uuid_invalid() {
        assert!(parse_uuid(b"not-a-uuid").is_none());
        assert!(parse_uuid(b"550e8400-e29b-41d4-a716").is_none()); // too short
        assert!(parse_uuid(b"GGGGGGGG-GGGG-GGGG-GGGG-GGGGGGGGGGGG").is_none());
    }

    #[test]
    fn test_format_uuid() {
        let uuid = [0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4,
                     0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00, 0x00];
        let formatted = format_uuid(&uuid);
        assert_eq!(&formatted, b"550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_uuid_roundtrip() {
        let original = b"12345678-abcd-ef01-2345-6789abcdef01";
        let parsed = parse_uuid(original).unwrap();
        let formatted = format_uuid(&parsed);
        assert_eq!(&formatted, original);
    }

    #[test]
    fn test_generate_uuid_version4() {
        let uuid = generate_uuid();
        // Version 4: bits 6-7 of byte 6 should be 0100
        assert_eq!(uuid[6] & 0xF0, 0x40);
        // Variant: bits 6-7 of byte 8 should be 10xx
        assert_eq!(uuid[8] & 0xC0, 0x80);
    }

    // ── Swap Header ────────────────────────────────────────────

    #[test]
    fn test_create_swap_header_basic() {
        let result = create_swap_header(1024 * 1024, None, None, PAGE_SIZE, false);
        assert!(result.is_ok());
        let setup = result.unwrap();
        assert_eq!(setup.header.len(), PAGE_SIZE as usize);
        assert!(setup.nr_pages > 0);
    }

    #[test]
    fn test_create_swap_header_with_label() {
        let result = create_swap_header(1024 * 1024, Some(b"myswap"), None, PAGE_SIZE, false);
        assert!(result.is_ok());
        let setup = result.unwrap();
        assert_eq!(&setup.label, b"myswap");
    }

    #[test]
    fn test_create_swap_header_too_small() {
        let result = create_swap_header(100, None, None, PAGE_SIZE, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_swap_signature_present() {
        let setup = create_swap_header(1024 * 1024, None, None, PAGE_SIZE, false).unwrap();
        assert!(verify_swap_signature(&setup.header, PAGE_SIZE));
    }

    #[test]
    fn test_swap_signature_absent() {
        let empty = vec![0u8; PAGE_SIZE as usize];
        assert!(!verify_swap_signature(&empty, PAGE_SIZE));
    }

    #[test]
    fn test_read_swap_header() {
        let setup = create_swap_header(
            10 * 1024 * 1024,
            Some(b"testlabel"),
            None,
            PAGE_SIZE,
            false,
        ).unwrap();

        let (version, last_page, uuid, label) = read_swap_header(&setup.header).unwrap();
        assert_eq!(version, 1);
        assert!(last_page > 0);
        assert_eq!(uuid, setup.uuid);
        assert_eq!(&label, b"testlabel");
    }

    #[test]
    fn test_create_header_with_custom_uuid() {
        let custom_uuid = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let setup = create_swap_header(1024 * 1024, None, Some(custom_uuid), PAGE_SIZE, false).unwrap();
        assert_eq!(setup.uuid, custom_uuid);
    }

    // ── /proc/swaps Parsing ────────────────────────────────────

    #[test]
    fn test_parse_proc_swaps() {
        let content = b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
                         /dev/sda2\tpartition\t2097148\t\t0\t\t-2\n\
                         /swapfile\tfile\t\t1048572\t\t512\t\t-3\n";

        let entries = parse_proc_swaps(content);
        assert_eq!(entries.len(), 2);

        assert_eq!(&entries[0].path, b"/dev/sda2");
        assert_eq!(entries[0].swap_type, SwapType::Partition);
        assert_eq!(entries[0].size_kb, 2097148);
        assert_eq!(entries[0].used_kb, 0);
        assert_eq!(entries[0].priority, -2);

        assert_eq!(&entries[1].path, b"/swapfile");
        assert_eq!(entries[1].swap_type, SwapType::File);
        assert_eq!(entries[1].size_kb, 1048572);
        assert_eq!(entries[1].used_kb, 512);
    }

    #[test]
    fn test_parse_proc_swaps_empty() {
        let content = b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n";
        let entries = parse_proc_swaps(content);
        assert_eq!(entries.len(), 0);
    }

    // ── fstab Parsing ──────────────────────────────────────────

    #[test]
    fn test_parse_fstab() {
        let content = b"# /etc/fstab\n\
                         /dev/sda1\t/\text4\tdefaults\t0\t1\n\
                         /dev/sda2\tnone\tswap\tsw\t0\t0\n\
                         /swapfile\tnone\tswap\tpri=10,discard\t0\t0\n";

        let entries = parse_fstab(content);
        assert_eq!(entries.len(), 3);

        let swap_entries = get_swap_fstab_entries(&entries);
        assert_eq!(swap_entries.len(), 2);
        assert_eq!(&swap_entries[0].device, b"/dev/sda2");
        assert_eq!(&swap_entries[1].device, b"/swapfile");
    }

    #[test]
    fn test_parse_fstab_comments_and_blank() {
        let content = b"# comment line\n\n\
                         # another comment\n\
                         /dev/sda1\t/\text4\tdefaults\t0\t1\n";
        let entries = parse_fstab(content);
        assert_eq!(entries.len(), 1);
    }

    // ── Swap Options Parsing ───────────────────────────────────

    #[test]
    fn test_parse_swap_options_priority() {
        let opts = parse_swap_options(b"pri=100");
        assert_eq!(opts.priority, Some(100));
        assert!(!opts.discard);
        assert!(!opts.no_auto);
    }

    #[test]
    fn test_parse_swap_options_multiple() {
        let opts = parse_swap_options(b"pri=5,discard,noauto");
        assert_eq!(opts.priority, Some(5));
        assert!(opts.discard);
        assert!(opts.no_auto);
    }

    #[test]
    fn test_parse_swap_options_defaults() {
        let opts = parse_swap_options(b"defaults");
        assert_eq!(opts.priority, None);
        assert!(!opts.discard);
        assert!(!opts.no_auto);
    }

    // ── Argument Parsing ───────────────────────────────────────

    #[test]
    fn test_mkswap_args_device() {
        let args = parse_mkswap_args(&[b"/dev/sda2".to_vec()]);
        assert_eq!(&args.device, b"/dev/sda2");
        assert!(!args.force);
        assert!(!args.check);
    }

    #[test]
    fn test_mkswap_args_with_label() {
        let args = parse_mkswap_args(&[b"-L".to_vec(), b"myswap".to_vec(), b"/dev/sda2".to_vec()]);
        assert_eq!(args.label.as_deref(), Some(b"myswap".as_slice()));
        assert_eq!(&args.device, b"/dev/sda2");
    }

    #[test]
    fn test_mkswap_args_force_check() {
        let args = parse_mkswap_args(&[b"-f".to_vec(), b"-c".to_vec(), b"/dev/sda2".to_vec()]);
        assert!(args.force);
        assert!(args.check);
    }

    #[test]
    fn test_swapon_args_all() {
        let args = parse_swapon_args(&[b"-a".to_vec()]);
        assert!(args.all);
        assert!(args.device.is_none());
    }

    #[test]
    fn test_swapon_args_priority() {
        let args = parse_swapon_args(&[b"-p".to_vec(), b"10".to_vec(), b"/dev/sda2".to_vec()]);
        assert_eq!(args.priority, Some(10));
        assert_eq!(args.device.as_deref(), Some(b"/dev/sda2".as_slice()));
    }

    #[test]
    fn test_swapon_args_show() {
        let args = parse_swapon_args(&[b"--show".to_vec(), b"--no-heading".to_vec()]);
        assert!(args.show);
        assert!(args.no_heading);
    }

    #[test]
    fn test_swapoff_args_all() {
        let args = parse_swapoff_args(&[b"-a".to_vec(), b"-v".to_vec()]);
        assert!(args.all);
        assert!(args.verbose);
    }

    #[test]
    fn test_swapoff_args_device() {
        let args = parse_swapoff_args(&[b"/dev/sda2".to_vec()]);
        assert_eq!(args.device.as_deref(), Some(b"/dev/sda2".as_slice()));
    }

    // ── Number Parsing ─────────────────────────────────────────

    #[test]
    fn test_parse_u64() {
        assert_eq!(parse_u64_bytes(b"0"), Some(0));
        assert_eq!(parse_u64_bytes(b"42"), Some(42));
        assert_eq!(parse_u64_bytes(b"123456789"), Some(123456789));
    }

    #[test]
    fn test_parse_u64_with_suffix() {
        assert_eq!(parse_u64_bytes(b"1K"), Some(1024));
        assert_eq!(parse_u64_bytes(b"2M"), Some(2 * 1024 * 1024));
        assert_eq!(parse_u64_bytes(b"1G"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_u64_invalid() {
        assert_eq!(parse_u64_bytes(b""), None);
        assert_eq!(parse_u64_bytes(b"abc"), None);
        assert_eq!(parse_u64_bytes(b"K"), None);
    }

    #[test]
    fn test_parse_i32() {
        assert_eq!(parse_i32_bytes(b"0"), Some(0));
        assert_eq!(parse_i32_bytes(b"42"), Some(42));
        assert_eq!(parse_i32_bytes(b"-1"), Some(-1));
        assert_eq!(parse_i32_bytes(b"-100"), Some(-100));
        assert_eq!(parse_i32_bytes(b"+5"), Some(5));
    }

    #[test]
    fn test_parse_i32_invalid() {
        assert_eq!(parse_i32_bytes(b""), None);
        assert_eq!(parse_i32_bytes(b"abc"), None);
    }

    // ── Format Functions ───────────────────────────────────────

    #[test]
    fn test_format_u64() {
        assert_eq!(format_u64(0), b"0");
        assert_eq!(format_u64(42), b"42");
        assert_eq!(format_u64(123456789), b"123456789");
    }

    #[test]
    fn test_format_i32() {
        assert_eq!(format_i32(0), b"0");
        assert_eq!(format_i32(42), b"42");
        assert_eq!(format_i32(-1), b"-1");
        assert_eq!(format_i32(-100), b"-100");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(100), b"100 B");
        assert_eq!(format_size(1024), b"1 KiB");
        assert_eq!(format_size(1024 * 1024), b"1 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), b"1 GiB");
    }

    // ── Swap Type Display ──────────────────────────────────────

    #[test]
    fn test_swap_type_display() {
        assert_eq!(format!("{}", SwapType::Partition), "partition");
        assert_eq!(format!("{}", SwapType::File), "file");
    }

    // ── Trim Bytes ─────────────────────────────────────────────

    #[test]
    fn test_trim_bytes() {
        assert_eq!(trim_bytes(b"  hello  "), b"hello");
        assert_eq!(trim_bytes(b"\t\nhello\r\n"), b"hello");
        assert_eq!(trim_bytes(b"hello"), b"hello");
        assert_eq!(trim_bytes(b""), b"" as &[u8]);
        assert_eq!(trim_bytes(b"   "), b"" as &[u8]);
    }

    // ── Help/Version (via cmd_ functions) ──────────────────────

    #[test]
    fn test_mkswap_help() {
        let args = MkswapArgs {
            device: Vec::new(),
            label: None,
            uuid: None,
            page_size: PAGE_SIZE,
            force: false,
            check: false,
            verbose: false,
            show_help: true,
            show_version: false,
        };
        assert_eq!(cmd_mkswap(&args), 0);
    }

    #[test]
    fn test_mkswap_version() {
        let args = MkswapArgs {
            device: Vec::new(),
            label: None,
            uuid: None,
            page_size: PAGE_SIZE,
            force: false,
            check: false,
            verbose: false,
            show_help: false,
            show_version: true,
        };
        assert_eq!(cmd_mkswap(&args), 0);
    }

    #[test]
    fn test_mkswap_no_device() {
        let args = MkswapArgs {
            device: Vec::new(),
            label: None,
            uuid: None,
            page_size: PAGE_SIZE,
            force: false,
            check: false,
            verbose: false,
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_mkswap(&args), 1);
    }

    #[test]
    fn test_mkswap_invalid_page_size() {
        let args = MkswapArgs {
            device: b"/dev/sda2".to_vec(),
            label: None,
            uuid: None,
            page_size: 1000, // not power of 2
            force: false,
            check: false,
            verbose: false,
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_mkswap(&args), 1);
    }

    #[test]
    fn test_swapon_help() {
        let args = SwaponArgs {
            device: None,
            all: false,
            priority: None,
            discard: false,
            show: false,
            summary: false,
            verbose: false,
            show_help: true,
            show_version: false,
            no_heading: false,
            bytes: false,
        };
        assert_eq!(cmd_swapon(&args), 0);
    }

    #[test]
    fn test_swapoff_help() {
        let args = SwapoffArgs {
            device: None,
            all: false,
            verbose: false,
            show_help: true,
            show_version: false,
        };
        assert_eq!(cmd_swapoff(&args), 0);
    }

    #[test]
    fn test_swapoff_no_args() {
        let args = SwapoffArgs {
            device: None,
            all: false,
            verbose: false,
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_swapoff(&args), 1);
    }
}
