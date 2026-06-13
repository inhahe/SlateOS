// SlateOS fstrim - filesystem trim/discard utilities
//
// Multi-personality binary:
//   fstrim     - trim (discard) unused blocks on a mounted filesystem
//   blkdiscard - discard sectors on a block device
//   wipefs     - wipe filesystem signatures from a device

#![cfg_attr(not(test), no_main)]
// Filesystem magic numbers (XFS/NTFS/MD/SWAP), their byte offsets, and
// the `TrimRange` field set are declared up-front because they encode
// the on-disk and ioctl-ABI contract the real implementation must
// speak. They are intentionally kept as documentation for the eventual
// block-device integration; squashing them would erase that contract.
#![allow(dead_code)]

// ── Constants ──────────────────────────────────────────────────────────

// FITRIM ioctl parameters
const FITRIM_MINLEN_DEFAULT: u64 = 0;

// Discard granularity
const SECTOR_SIZE: u64 = 512;
const DEFAULT_STEP_BYTES: u64 = 128 * 1024 * 1024; // 128 MiB step for progress

// Wipefs signature types
const SIG_FILESYSTEM: u8 = 0;
const SIG_RAID: u8 = 1;
const SIG_PARTITION: u8 = 2;
const SIG_CRYPTO: u8 = 3;

// Common magic offsets and sizes
const EXT_MAGIC_OFFSET: u64 = 0x438;
const EXT_MAGIC: u16 = 0xEF53;
const XFS_MAGIC_OFFSET: u64 = 0;
const XFS_MAGIC: u32 = 0x58465342; // "XFSB"
const BTRFS_MAGIC_OFFSET: u64 = 0x10040;
const FAT_MAGIC_OFFSET: u64 = 0x52;
const NTFS_MAGIC_OFFSET: u64 = 3;
const SWAP_MAGIC_OFFSET: u64 = 4086; // pagesize - 10 for old swap
const GPT_MAGIC_OFFSET: u64 = 512;
const MBR_MAGIC_OFFSET: u64 = 510;
const MBR_MAGIC: u16 = 0xAA55;
const LUKS_MAGIC_OFFSET: u64 = 0;
const MD_MAGIC_OFFSET: u64 = 4096;

const MAX_SIGNATURES: usize = 16;
const MAX_PATH: usize = 256;
const MAX_UUID: usize = 48;
const MAX_LABEL: usize = 64;

// ── Output Helpers ─────────────────────────────────────────────────────

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

// ── Data Types ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Fstrim,
    Blkdiscard,
    Wipefs,
}

/// Filesystem TRIM range (maps to struct fstrim_range)
#[derive(Clone, Copy)]
struct TrimRange {
    start: u64,
    len: u64,
    minlen: u64,
}

/// Block discard parameters
#[derive(Clone, Copy)]
struct DiscardParams {
    offset: u64,
    length: u64,       // 0 = entire device
    step: u64,         // step size for progress
    secure: bool,      // secure discard
    zeroout: bool,     // zero instead of discard
}

/// Wipefs signature info
#[derive(Clone, Copy)]
struct Signature {
    sig_type: u8,
    offset: u64,
    magic_len: u16,
    magic: [u8; 16],
    fstype: [u8; 32],
    fstype_len: usize,
    label: [u8; MAX_LABEL],
    label_len: usize,
    uuid: [u8; MAX_UUID],
    uuid_len: usize,
}

impl Signature {
    fn new() -> Self {
        Self {
            sig_type: SIG_FILESYSTEM,
            offset: 0,
            magic_len: 0,
            magic: [0u8; 16],
            fstype: [0u8; 32],
            fstype_len: 0,
            label: [0u8; MAX_LABEL],
            label_len: 0,
            uuid: [0u8; MAX_UUID],
            uuid_len: 0,
        }
    }
}

struct FstrimOpts {
    tool: Tool,
    target: [u8; MAX_PATH],
    target_len: usize,
    // fstrim options
    offset: u64,         // -o
    length: u64,         // -l (0 = entire)
    minimum: u64,        // -m (minimum free extent)
    verbose: bool,       // -v
    dry_run: bool,       // -n (for wipefs)
    all_mounts: bool,    // -a (fstrim all mounted)
    listed_in: [u8; MAX_PATH],  // --listed-in (fstab file)
    listed_in_len: usize,
    // blkdiscard options
    discard_offset: u64, // -o
    discard_length: u64, // -l
    step: u64,           // -p (step size for progress)
    secure: bool,        // -s (secure discard)
    zeroout: bool,       // -z (zero instead of discard)
    force: bool,         // -f
    // wipefs options
    wipe_all: bool,      // -a (wipe all signatures)
    wipe_quiet: bool,    // -q
    no_header: bool,     // -n (no heading for wipefs output)
    backup: bool,        // -b (backup erased data)
    output_parsable: bool, // -p (parsable output)
    types: [u8; 64],     // -t (filter by type)
    types_len: usize,
}

impl FstrimOpts {
    fn new(tool: Tool) -> Self {
        Self {
            tool,
            target: [0u8; MAX_PATH],
            target_len: 0,
            offset: 0,
            length: 0,
            minimum: FITRIM_MINLEN_DEFAULT,
            verbose: false,
            dry_run: false,
            all_mounts: false,
            listed_in: [0u8; MAX_PATH],
            listed_in_len: 0,
            discard_offset: 0,
            discard_length: 0,
            step: DEFAULT_STEP_BYTES,
            secure: false,
            zeroout: false,
            force: false,
            wipe_all: false,
            wipe_quiet: false,
            no_header: false,
            backup: false,
            output_parsable: false,
            types: [0u8; 64],
            types_len: 0,
        }
    }
}

// ── String/Number Helpers ──────────────────────────────────────────────

unsafe fn cstr_to_slice(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() {
        return b"";
    }
    let mut len = 0usize;
    // SAFETY: Walking null-terminated C string from kernel/libc
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
            if len >= 4096 {
                break;
            }
        }
        core::slice::from_raw_parts(ptr, len)
    }
}

fn format_u64(val: u64, buf: &mut [u8]) -> usize {
    if val == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
        }
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut n = val;
    let mut i = 0;
    while n > 0 {
        if let Some(slot) = tmp.get_mut(i) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        i += 1;
    }
    let len = i.min(buf.len());
    for j in 0..len {
        if let (Some(dst), Some(src)) = (buf.get_mut(j), tmp.get(i - 1 - j)) {
            *dst = *src;
        }
    }
    len
}

fn copy_bytes(dst: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let mut p = pos;
    for &c in src {
        if p < dst.len() {
            dst[p] = c;
            p += 1;
        }
    }
    p
}

fn pad_right(buf: &mut [u8], start: usize, width: usize) -> usize {
    let mut pos = start;
    while pos < width && pos < buf.len() {
        buf[pos] = b' ';
        pos += 1;
    }
    pos
}

fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    &haystack[..needle.len()] == needle
}

fn format_hex_u8(val: u8, buf: &mut [u8]) -> usize {
    if buf.len() < 2 {
        return 0;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    buf[0] = HEX[(val >> 4) as usize];
    buf[1] = HEX[(val & 0xF) as usize];
    2
}

fn format_size_human(bytes: u64, buf: &mut [u8]) -> usize {
    let (val, frac, suffix): (u64, u64, &[u8]) = if bytes >= 1024 * 1024 * 1024 * 1024 {
        let gib = bytes / (1024 * 1024 * 1024);
        let frac = (bytes % (1024 * 1024 * 1024)) / (1024 * 1024 * 10);
        (gib, frac, b" TiB")
    } else if bytes >= 1024 * 1024 * 1024 {
        let gib = bytes / (1024 * 1024);
        let frac = (bytes % (1024 * 1024)) / (1024 * 10);
        (gib / 1024, frac, b" GiB")
    } else if bytes >= 1024 * 1024 {
        let mib = bytes / 1024;
        let frac = (bytes % 1024) / 10;
        (mib / 1024, frac, b" MiB")
    } else if bytes >= 1024 {
        (bytes / 1024, (bytes % 1024) / 10, b" KiB")
    } else {
        (bytes, 0, b" B")
    };

    let mut pos = format_u64(val, buf);
    // Show decimal for GiB/TiB
    if (suffix == b" GiB" || suffix == b" TiB") && frac > 0 {
        if pos < buf.len() {
            buf[pos] = b'.';
            pos += 1;
        }
        pos += format_u64(frac.min(99), &mut buf[pos..]);
    }
    pos = copy_bytes(buf, pos, suffix);
    pos
}

/// Parse a size string with optional suffix (K, M, G, T, KiB, MiB, etc.)
fn parse_size(s: &[u8]) -> Result<u64, ()> {
    if s.is_empty() {
        return Err(());
    }

    // Find where digits end
    let mut num_end = 0;
    while num_end < s.len() && s[num_end] >= b'0' && s[num_end] <= b'9' {
        num_end += 1;
    }
    if num_end == 0 {
        return Err(());
    }

    // Parse the numeric part
    let mut val: u64 = 0;
    for &byte in &s[..num_end] {
        val = val.checked_mul(10).ok_or(())?;
        val = val.checked_add((byte - b'0') as u64).ok_or(())?;
    }

    // Parse the suffix
    let suffix = &s[num_end..];
    let multiplier: u64 = if suffix.is_empty() || suffix == b"B" {
        1
    } else if suffix == b"K" || suffix == b"KiB" || suffix == b"k" {
        1024
    } else if suffix == b"M" || suffix == b"MiB" || suffix == b"m" {
        1024 * 1024
    } else if suffix == b"G" || suffix == b"GiB" || suffix == b"g" {
        1024 * 1024 * 1024
    } else if suffix == b"T" || suffix == b"TiB" || suffix == b"t" {
        1024 * 1024 * 1024 * 1024
    } else if suffix == b"KB" || suffix == b"kB" {
        1000
    } else if suffix == b"MB" {
        1_000_000
    } else if suffix == b"GB" {
        1_000_000_000
    } else if suffix == b"TB" {
        1_000_000_000_000
    } else if suffix == b"s" || suffix == b"S" {
        SECTOR_SIZE // sectors
    } else {
        return Err(());
    };

    val.checked_mul(multiplier).ok_or(())
}

// ── Tool Detection ─────────────────────────────────────────────────────

fn detect_tool(argv0: &[u8]) -> Tool {
    let mut start = 0;
    for (i, &b) in argv0.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            start = i + 1;
        }
    }
    let name = &argv0[start..];

    if starts_with(name, b"blkdiscard") {
        Tool::Blkdiscard
    } else if starts_with(name, b"wipefs") {
        Tool::Wipefs
    } else {
        Tool::Fstrim
    }
}

// ── Argument Parsing ───────────────────────────────────────────────────

fn parse_args(argc: i32, argv: *const *const u8) -> Result<FstrimOpts, i32> {
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let argv0 = if !args.is_empty() {
        unsafe { cstr_to_slice(args[0]) }
    } else {
        b"fstrim"
    };

    let tool = detect_tool(argv0);
    let mut opts = FstrimOpts::new(tool);

    let mut i = 1;
    while i < args.len() {
        let arg = unsafe { cstr_to_slice(args[i]) };

        match tool {
            Tool::Fstrim => {
                if arg == b"--help" || arg == b"-h" {
                    show_fstrim_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-v" || arg == b"--verbose" {
                    opts.verbose = true;
                } else if arg == b"-n" || arg == b"--dry-run" {
                    opts.dry_run = true;
                } else if arg == b"-a" || arg == b"--all" {
                    opts.all_mounts = true;
                } else if arg == b"-o" || arg == b"--offset" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"fstrim: -o requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.offset = v,
                        Err(()) => {
                            print_err(b"fstrim: invalid offset: ");
                            print_err(val);
                            print_err(b"\n");
                            return Err(1);
                        }
                    }
                } else if arg == b"-l" || arg == b"--length" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"fstrim: -l requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.length = v,
                        Err(()) => {
                            print_err(b"fstrim: invalid length: ");
                            print_err(val);
                            print_err(b"\n");
                            return Err(1);
                        }
                    }
                } else if arg == b"-m" || arg == b"--minimum" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"fstrim: -m requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.minimum = v,
                        Err(()) => {
                            print_err(b"fstrim: invalid minimum: ");
                            print_err(val);
                            print_err(b"\n");
                            return Err(1);
                        }
                    }
                } else if starts_with(arg, b"--listed-in") {
                    // Handle --listed-in=FILE or --listed-in FILE
                    if arg.len() > 11 && arg[11] == b'=' {
                        let val = &arg[12..];
                        let len = val.len().min(MAX_PATH);
                        opts.listed_in[..len].copy_from_slice(&val[..len]);
                        opts.listed_in_len = len;
                    } else {
                        i += 1;
                        if i >= args.len() {
                            print_err(b"fstrim: --listed-in requires an argument\n");
                            return Err(1);
                        }
                        let val = unsafe { cstr_to_slice(args[i]) };
                        let len = val.len().min(MAX_PATH);
                        opts.listed_in[..len].copy_from_slice(&val[..len]);
                        opts.listed_in_len = len;
                    }
                } else if !arg.is_empty() && arg[0] != b'-' {
                    let len = arg.len().min(MAX_PATH);
                    opts.target[..len].copy_from_slice(&arg[..len]);
                    opts.target_len = len;
                } else {
                    print_err(b"fstrim: unknown option: ");
                    print_err(arg);
                    print_err(b"\n");
                    return Err(1);
                }
            }
            Tool::Blkdiscard => {
                if arg == b"--help" || arg == b"-h" {
                    show_blkdiscard_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-v" || arg == b"--verbose" {
                    opts.verbose = true;
                } else if arg == b"-f" || arg == b"--force" {
                    opts.force = true;
                } else if arg == b"-s" || arg == b"--secure" {
                    opts.secure = true;
                } else if arg == b"-z" || arg == b"--zeroout" {
                    opts.zeroout = true;
                } else if arg == b"-o" || arg == b"--offset" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"blkdiscard: -o requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.discard_offset = v,
                        Err(()) => {
                            print_err(b"blkdiscard: invalid offset\n");
                            return Err(1);
                        }
                    }
                } else if arg == b"-l" || arg == b"--length" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"blkdiscard: -l requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.discard_length = v,
                        Err(()) => {
                            print_err(b"blkdiscard: invalid length\n");
                            return Err(1);
                        }
                    }
                } else if arg == b"-p" || arg == b"--step" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"blkdiscard: -p requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.step = v,
                        Err(()) => {
                            print_err(b"blkdiscard: invalid step size\n");
                            return Err(1);
                        }
                    }
                } else if !arg.is_empty() && arg[0] != b'-' {
                    let len = arg.len().min(MAX_PATH);
                    opts.target[..len].copy_from_slice(&arg[..len]);
                    opts.target_len = len;
                } else {
                    print_err(b"blkdiscard: unknown option: ");
                    print_err(arg);
                    print_err(b"\n");
                    return Err(1);
                }
            }
            Tool::Wipefs => {
                if arg == b"--help" || arg == b"-h" {
                    show_wipefs_help();
                    return Err(0);
                } else if arg == b"--version" || arg == b"-V" {
                    show_version(tool);
                    return Err(0);
                } else if arg == b"-a" || arg == b"--all" {
                    opts.wipe_all = true;
                } else if arg == b"-f" || arg == b"--force" {
                    opts.force = true;
                } else if arg == b"-n" || arg == b"--no-act" || arg == b"--dry-run" {
                    opts.dry_run = true;
                } else if arg == b"-q" || arg == b"--quiet" {
                    opts.wipe_quiet = true;
                } else if arg == b"--no-heading" {
                    opts.no_header = true;
                } else if arg == b"-p" || arg == b"--parsable" {
                    opts.output_parsable = true;
                } else if arg == b"-b" || arg == b"--backup" {
                    opts.backup = true;
                } else if arg == b"-o" || arg == b"--offset" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"wipefs: -o requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    match parse_size(val) {
                        Ok(v) => opts.offset = v,
                        Err(()) => {
                            print_err(b"wipefs: invalid offset\n");
                            return Err(1);
                        }
                    }
                } else if arg == b"-t" || arg == b"--types" {
                    i += 1;
                    if i >= args.len() {
                        print_err(b"wipefs: -t requires an argument\n");
                        return Err(1);
                    }
                    let val = unsafe { cstr_to_slice(args[i]) };
                    let len = val.len().min(63);
                    opts.types[..len].copy_from_slice(&val[..len]);
                    opts.types_len = len;
                } else if !arg.is_empty() && arg[0] != b'-' {
                    let len = arg.len().min(MAX_PATH);
                    opts.target[..len].copy_from_slice(&arg[..len]);
                    opts.target_len = len;
                } else {
                    print_err(b"wipefs: unknown option: ");
                    print_err(arg);
                    print_err(b"\n");
                    return Err(1);
                }
            }
        }
        i += 1;
    }

    // Validate
    if opts.target_len == 0 && !opts.all_mounts {
        let name = tool_name(tool);
        print_err(name);
        if tool == Tool::Fstrim {
            print_err(b": no mountpoint or -a specified\n");
        } else {
            print_err(b": no device specified\n");
        }
        return Err(1);
    }

    Ok(opts)
}

fn tool_name(tool: Tool) -> &'static [u8] {
    match tool {
        Tool::Fstrim => b"fstrim",
        Tool::Blkdiscard => b"blkdiscard",
        Tool::Wipefs => b"wipefs",
    }
}

// ── Help Messages ──────────────────────────────────────────────────────

fn show_version(tool: Tool) {
    print_out(tool_name(tool));
    print_out(b" version 0.1.0 (Slate OS)\n");
}

fn show_fstrim_help() {
    print_out(b"Usage: fstrim [options] <mountpoint>\n\n");
    print_out(b"Discard unused blocks on a mounted filesystem.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -a, --all              Trim all mounted filesystems\n");
    print_out(b"  -o, --offset OFFSET    Start offset in bytes\n");
    print_out(b"  -l, --length LENGTH    Length of region to discard\n");
    print_out(b"  -m, --minimum SIZE     Minimum extent length\n");
    print_out(b"  -v, --verbose          Show statistics after trimming\n");
    print_out(b"  -n, --dry-run          Don't actually trim\n");
    print_out(b"  --listed-in FILE       Trim filesystems listed in fstab\n");
    print_out(b"  -h, --help             Show this help\n");
    print_out(b"  -V, --version          Show version\n");
}

fn show_blkdiscard_help() {
    print_out(b"Usage: blkdiscard [options] <device>\n\n");
    print_out(b"Discard sectors on a block device.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -o, --offset OFFSET    Start offset in bytes\n");
    print_out(b"  -l, --length LENGTH    Length of region to discard\n");
    print_out(b"  -p, --step SIZE        Step size for progress display\n");
    print_out(b"  -s, --secure           Perform secure discard\n");
    print_out(b"  -z, --zeroout          Zero-fill instead of discard\n");
    print_out(b"  -f, --force            Force, allow mounted device\n");
    print_out(b"  -v, --verbose          Show progress\n");
    print_out(b"  -h, --help             Show this help\n");
    print_out(b"  -V, --version          Show version\n");
}

fn show_wipefs_help() {
    print_out(b"Usage: wipefs [options] <device>\n\n");
    print_out(b"Wipe filesystem/RAID/partition-table signatures.\n\n");
    print_out(b"Options:\n");
    print_out(b"  -a, --all              Wipe all signatures\n");
    print_out(b"  -f, --force            Force wipe (even if mounted)\n");
    print_out(b"  -n, --no-act           Dry run, don't actually wipe\n");
    print_out(b"  -q, --quiet            Suppress output\n");
    print_out(b"  -b, --backup           Backup erased data\n");
    print_out(b"  -o, --offset OFFSET    Only wipe signature at offset\n");
    print_out(b"  -t, --types LIST       Only wipe specified types\n");
    print_out(b"  -p, --parsable         Parsable output format\n");
    print_out(b"  --no-heading           Don't show column headers\n");
    print_out(b"  -h, --help             Show this help\n");
    print_out(b"  -V, --version          Show version\n");
}

// ── Simulated Filesystem Operations ────────────────────────────────────

// In a real OS, these would issue FITRIM ioctl, BLKDISCARD ioctl, etc.
// For now, we simulate the logic to demonstrate output formatting.

/// Simulate FITRIM ioctl.
///
/// The `_range` parameter documents intent — a real implementation would
/// pass it down to the kernel as part of the FITRIM ioctl `struct
/// fstrim_range`. The personality-CLI stub computes a representative
/// trimmed-byte count without consulting the range.
fn do_fstrim(mountpoint: &[u8], _range: &TrimRange, verbose: bool, dry_run: bool) -> Result<u64, i32> {
    // In a real implementation:
    // 1. Open the mountpoint
    // 2. Issue FITRIM ioctl with the range
    // 3. Return the number of bytes trimmed

    let mut buf = [0u8; 512];

    if dry_run {
        let mut pos = copy_bytes(&mut buf, 0, b"fstrim: ");
        pos = copy_bytes(&mut buf, pos, mountpoint);
        pos = copy_bytes(&mut buf, pos, b": dry run, not trimming\n");
        print_out(&buf[..pos]);
        return Ok(0);
    }

    // Simulate trimmed bytes (e.g., 256 MiB of unused blocks)
    let trimmed = 256 * 1024 * 1024u64;

    if verbose {
        let mut pos = copy_bytes(&mut buf, 0, mountpoint);
        pos = copy_bytes(&mut buf, pos, b": ");
        pos += format_size_human(trimmed, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" (");
        pos += format_u64(trimmed, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" bytes) trimmed on ");
        pos = copy_bytes(&mut buf, pos, mountpoint);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);
    }

    Ok(trimmed)
}

/// One entry in the discard-capable mount table: mountpoint bytes,
/// mountpoint length, filesystem-type bytes, filesystem-type length.
/// A real implementation would parse /proc/self/mountinfo into something
/// like this; the stub fills it from a hard-coded list.
type DiscardMount = ([u8; MAX_PATH], usize, [u8; 32], usize);

/// Get list of mounted filesystems that support discard
fn get_discard_mounts() -> ([DiscardMount; 16], usize) {
    // Simulated mount list
    let mounts: &[(&[u8], &[u8])] = &[
        (b"/", b"ext4"),
        (b"/home", b"ext4"),
        (b"/boot", b"ext4"),
        (b"/var", b"ext4"),
        (b"/tmp", b"tmpfs"),
    ];

    let mut result = [([0u8; MAX_PATH], 0usize, [0u8; 32], 0usize); 16];
    let mut count = 0;

    for (mountpoint, fstype) in mounts {
        // Skip non-disc-capable filesystems
        if *fstype == b"tmpfs" || *fstype == b"proc" || *fstype == b"sysfs" {
            continue;
        }
        if count < 16 {
            let mlen = mountpoint.len().min(MAX_PATH);
            result[count].0[..mlen].copy_from_slice(&mountpoint[..mlen]);
            result[count].1 = mlen;
            let flen = fstype.len().min(32);
            result[count].2[..flen].copy_from_slice(&fstype[..flen]);
            result[count].3 = flen;
            count += 1;
        }
    }

    (result, count)
}

/// Simulate block discard
fn do_blkdiscard(device: &[u8], params: &DiscardParams, verbose: bool) -> Result<(), i32> {
    let mut buf = [0u8; 512];

    // Get device size (simulated)
    let dev_size: u64 = 500_107_862_016;
    let length = if params.length == 0 {
        dev_size.saturating_sub(params.offset)
    } else {
        params.length
    };

    if verbose {
        let op = if params.zeroout {
            b"Zeroing" as &[u8]
        } else if params.secure {
            b"Secure discarding" as &[u8]
        } else {
            b"Discarding" as &[u8]
        };

        let mut pos = copy_bytes(&mut buf, 0, op);
        pos = copy_bytes(&mut buf, pos, b" ");
        pos = copy_bytes(&mut buf, pos, device);
        pos = copy_bytes(&mut buf, pos, b": offset=");
        pos += format_u64(params.offset, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b", length=");
        pos += format_u64(length, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" (");
        pos += format_size_human(length, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b")\n");
        print_out(&buf[..pos]);
    }

    // In a real implementation:
    // 1. Open the block device
    // 2. Check alignment requirements
    // 3. Issue BLKDISCARD/BLKSECDISCARD/BLKZEROOUT ioctl in steps
    // 4. Show progress if -p specified

    // Simulate progress
    if params.step > 0 && params.step < length {
        let mut done: u64 = 0;
        while done < length {
            let chunk = params.step.min(length - done);
            done += chunk;
            // Progress would be shown here in a real implementation
        }
    }

    if verbose {
        let mut pos = copy_bytes(&mut buf, 0, b"Done: ");
        pos += format_size_human(length, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" discarded\n");
        print_out(&buf[..pos]);
    }

    Ok(())
}

/// Detect filesystem signatures on a device.
///
/// The `_device` parameter documents intent — a real implementation
/// would open the block device, read the relevant superblock offsets,
/// and pattern-match against known magic numbers (see XFS_MAGIC etc.
/// above). The stub returns a fixed plausible set.
fn detect_signatures(_device: &[u8]) -> ([Signature; MAX_SIGNATURES], usize) {
    let mut sigs = [Signature::new(); MAX_SIGNATURES];
    let mut count = 0;

    // In a real implementation, we'd read specific offsets from the device
    // and check for known magic numbers. Here we simulate finding signatures.

    // Simulated: ext4 superblock
    if count < MAX_SIGNATURES {
        sigs[count].sig_type = SIG_FILESYSTEM;
        sigs[count].offset = EXT_MAGIC_OFFSET;
        sigs[count].magic[0] = 0x53;
        sigs[count].magic[1] = 0xEF;
        sigs[count].magic_len = 2;
        set_field(&mut sigs[count].fstype, &mut sigs[count].fstype_len, b"ext4");
        set_field(&mut sigs[count].label, &mut sigs[count].label_len, b"rootfs");
        set_field(&mut sigs[count].uuid, &mut sigs[count].uuid_len,
                  b"12345678-1234-1234-1234-123456789abc");
        count += 1;
    }

    // Simulated: MBR partition table
    if count < MAX_SIGNATURES {
        sigs[count].sig_type = SIG_PARTITION;
        sigs[count].offset = MBR_MAGIC_OFFSET;
        sigs[count].magic[0] = 0x55;
        sigs[count].magic[1] = 0xAA;
        sigs[count].magic_len = 2;
        set_field(&mut sigs[count].fstype, &mut sigs[count].fstype_len, b"dos");
        count += 1;
    }

    (sigs, count)
}

fn set_field(dst: &mut [u8], len: &mut usize, src: &[u8]) {
    let copy_len = src.len().min(dst.len());
    dst[..copy_len].copy_from_slice(&src[..copy_len]);
    *len = copy_len;
}

/// Show detected signatures (wipefs default behavior)
fn show_signatures(device: &[u8], sigs: &[Signature], count: usize, opts: &FstrimOpts) {
    if count == 0 {
        if !opts.wipe_quiet {
            print_out(device);
            print_out(b": no signatures found\n");
        }
        return;
    }

    let mut buf = [0u8; 512];

    if opts.output_parsable {
        // Parsable format: DEVICE OFFSET TYPE LABEL UUID
        for sig in sigs.iter().take(count) {
            let mut pos = copy_bytes(&mut buf, 0, device);
            pos = copy_bytes(&mut buf, pos, b" ");
            pos = copy_bytes(&mut buf, pos, b"0x");
            // Format offset as hex
            pos += format_u64(sig.offset, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" ");
            pos = copy_bytes(&mut buf, pos, &sig.fstype[..sig.fstype_len]);
            pos = copy_bytes(&mut buf, pos, b" ");
            pos = copy_bytes(&mut buf, pos, &sig.label[..sig.label_len]);
            pos = copy_bytes(&mut buf, pos, b" ");
            pos = copy_bytes(&mut buf, pos, &sig.uuid[..sig.uuid_len]);
            pos = copy_bytes(&mut buf, pos, b"\n");
            print_out(&buf[..pos]);
        }
        return;
    }

    // Table format
    if !opts.no_header {
        print_out(b"DEVICE   OFFSET     TYPE   LABEL    UUID\n");
    }

    for sig in sigs.iter().take(count) {
        let mut pos = 0;

        // Device
        pos = copy_bytes(&mut buf, pos, device);
        pos = pad_right(&mut buf, pos, 9);

        // Offset (hex)
        pos = copy_bytes(&mut buf, pos, b"0x");
        let offset_start = pos;
        pos += format_u64(sig.offset, &mut buf[pos..]);
        pos = pad_right(&mut buf, pos, offset_start + 9);

        // Type
        let type_start = pos;
        let type_name = match sig.sig_type {
            SIG_FILESYSTEM => &sig.fstype[..sig.fstype_len],
            SIG_RAID => b"raid" as &[u8],
            SIG_PARTITION => &sig.fstype[..sig.fstype_len],
            SIG_CRYPTO => b"crypto" as &[u8],
            _ => b"unknown" as &[u8],
        };
        pos = copy_bytes(&mut buf, pos, type_name);
        pos = pad_right(&mut buf, pos, type_start + 7);

        // Label
        let label_start = pos;
        if sig.label_len > 0 {
            pos = copy_bytes(&mut buf, pos, &sig.label[..sig.label_len]);
        }
        pos = pad_right(&mut buf, pos, label_start + 9);

        // UUID
        if sig.uuid_len > 0 {
            pos = copy_bytes(&mut buf, pos, &sig.uuid[..sig.uuid_len]);
        }

        buf[pos] = b'\n';
        pos += 1;
        print_out(&buf[..pos]);
    }
}

/// Wipe signatures
fn wipe_signatures(device: &[u8], sigs: &[Signature], count: usize, opts: &FstrimOpts) -> i32 {
    let mut buf = [0u8; 512];

    for sig in sigs.iter().take(count) {
        // If -o specified, only wipe at that offset
        if opts.offset > 0 && sig.offset != opts.offset {
            continue;
        }

        // If -t specified, only wipe matching types
        if opts.types_len > 0 {
            let types = &opts.types[..opts.types_len];
            let fstype = &sig.fstype[..sig.fstype_len];
            if !contains(types, fstype) {
                continue;
            }
        }

        if opts.backup {
            let mut pos = copy_bytes(&mut buf, 0, b"wipefs: backing up ");
            pos += format_u64(sig.magic_len as u64, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" bytes from offset 0x");
            pos += format_u64(sig.offset, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" to ~/wipefs-");
            // Device basename
            let mut dev_start = 0;
            let dev_path = device;
            for (j, &b) in dev_path.iter().enumerate() {
                if b == b'/' {
                    dev_start = j + 1;
                }
            }
            pos = copy_bytes(&mut buf, pos, &dev_path[dev_start..]);
            pos = copy_bytes(&mut buf, pos, b"-0x");
            pos += format_u64(sig.offset, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b".bak\n");
            print_out(&buf[..pos]);
        }

        if !opts.wipe_quiet {
            let mut pos = copy_bytes(&mut buf, 0, device);
            pos = copy_bytes(&mut buf, pos, b": ");
            if opts.dry_run {
                pos = copy_bytes(&mut buf, pos, b"(dry run) ");
            }
            pos = copy_bytes(&mut buf, pos, &sig.fstype[..sig.fstype_len]);
            pos = copy_bytes(&mut buf, pos, b" signature at offset 0x");
            pos += format_u64(sig.offset, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" wiped");
            if sig.magic_len > 0 {
                pos = copy_bytes(&mut buf, pos, b" (");
                pos += format_u64(sig.magic_len as u64, &mut buf[pos..]);
                pos = copy_bytes(&mut buf, pos, b" bytes)");
            }
            pos = copy_bytes(&mut buf, pos, b"\n");
            print_out(&buf[..pos]);
        }

        // In a real implementation:
        // 1. Optionally backup the magic bytes
        // 2. Write zeros over the magic bytes at sig.offset
    }

    0
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() || needle.is_empty() {
        return false;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if haystack[i..i + needle.len()] == *needle {
            return true;
        }
    }
    false
}

// ── Command Implementations ────────────────────────────────────────────

fn cmd_fstrim(opts: &FstrimOpts) -> i32 {
    if opts.all_mounts {
        return cmd_fstrim_all(opts);
    }

    let mountpoint = &opts.target[..opts.target_len];
    let range = TrimRange {
        start: opts.offset,
        len: opts.length,
        minlen: opts.minimum,
    };

    match do_fstrim(mountpoint, &range, opts.verbose, opts.dry_run) {
        Ok(_) => 0,
        Err(code) => {
            print_err(b"fstrim: ");
            print_err(mountpoint);
            print_err(b": trim failed\n");
            code
        }
    }
}

fn cmd_fstrim_all(opts: &FstrimOpts) -> i32 {
    let (mounts, count) = get_discard_mounts();
    let mut errors = 0;

    for mount in mounts.iter().take(count) {
        let mountpoint = &mount.0[..mount.1];
        let range = TrimRange {
            start: opts.offset,
            len: opts.length,
            minlen: opts.minimum,
        };

        match do_fstrim(mountpoint, &range, opts.verbose, opts.dry_run) {
            Ok(_) => {}
            Err(_) => {
                errors += 1;
                if !opts.verbose {
                    let mut buf = [0u8; 256];
                    let mut pos = copy_bytes(&mut buf, 0, b"fstrim: ");
                    pos = copy_bytes(&mut buf, pos, mountpoint);
                    pos = copy_bytes(&mut buf, pos, b": trim failed\n");
                    print_err(&buf[..pos]);
                }
            }
        }
    }

    if errors > 0 { 1 } else { 0 }
}

fn cmd_blkdiscard(opts: &FstrimOpts) -> i32 {
    let device = &opts.target[..opts.target_len];

    let params = DiscardParams {
        offset: opts.discard_offset,
        length: opts.discard_length,
        step: opts.step,
        secure: opts.secure,
        zeroout: opts.zeroout,
    };

    // Safety check: warn if no -f and device might be mounted
    if !opts.force {
        // In a real implementation, check if device is mounted
    }

    match do_blkdiscard(device, &params, opts.verbose) {
        Ok(()) => 0,
        Err(code) => {
            print_err(b"blkdiscard: ");
            print_err(device);
            print_err(b": discard failed\n");
            code
        }
    }
}

fn cmd_wipefs(opts: &FstrimOpts) -> i32 {
    let device = &opts.target[..opts.target_len];

    // Detect signatures
    let (sigs, count) = detect_signatures(device);

    if opts.wipe_all || opts.offset > 0 {
        // Wipe mode
        if count == 0 {
            if !opts.wipe_quiet {
                print_out(device);
                print_out(b": no signatures found\n");
            }
            return 0;
        }
        wipe_signatures(device, &sigs, count, opts)
    } else {
        // Display mode (default)
        show_signatures(device, &sigs, count, opts);
        0
    }
}

// ── Main ───────────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let opts = match parse_args(argc, argv) {
        Ok(o) => o,
        Err(code) => return code,
    };

    match opts.tool {
        Tool::Fstrim => cmd_fstrim(&opts),
        Tool::Blkdiscard => cmd_blkdiscard(&opts),
        Tool::Wipefs => cmd_wipefs(&opts),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_tool() {
        assert_eq!(detect_tool(b"fstrim"), Tool::Fstrim);
        assert_eq!(detect_tool(b"/sbin/fstrim"), Tool::Fstrim);
        assert_eq!(detect_tool(b"blkdiscard"), Tool::Blkdiscard);
        assert_eq!(detect_tool(b"/usr/sbin/blkdiscard"), Tool::Blkdiscard);
        assert_eq!(detect_tool(b"wipefs"), Tool::Wipefs);
    }

    #[test]
    fn test_parse_size_plain() {
        assert_eq!(parse_size(b"1024"), Ok(1024));
        assert_eq!(parse_size(b"0"), Ok(0));
        assert_eq!(parse_size(b"42"), Ok(42));
    }

    #[test]
    fn test_parse_size_suffixes() {
        assert_eq!(parse_size(b"1K"), Ok(1024));
        assert_eq!(parse_size(b"1KiB"), Ok(1024));
        assert_eq!(parse_size(b"1M"), Ok(1024 * 1024));
        assert_eq!(parse_size(b"1G"), Ok(1024 * 1024 * 1024));
        assert_eq!(parse_size(b"1T"), Ok(1024 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_decimal_suffixes() {
        assert_eq!(parse_size(b"1KB"), Ok(1000));
        assert_eq!(parse_size(b"1MB"), Ok(1_000_000));
        assert_eq!(parse_size(b"1GB"), Ok(1_000_000_000));
    }

    #[test]
    fn test_parse_size_sectors() {
        assert_eq!(parse_size(b"1s"), Ok(512));
        assert_eq!(parse_size(b"2S"), Ok(1024));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size(b"").is_err());
        assert!(parse_size(b"abc").is_err());
        assert!(parse_size(b"1X").is_err());
    }

    #[test]
    fn test_format_u64_zero() {
        let mut buf = [0u8; 20];
        let n = format_u64(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn test_format_u64_large() {
        let mut buf = [0u8; 20];
        let n = format_u64(1234567890, &mut buf);
        assert_eq!(&buf[..n], b"1234567890");
    }

    #[test]
    fn test_format_size_human() {
        let mut buf = [0u8; 32];
        let n = format_size_human(1024, &mut buf);
        assert_eq!(&buf[..n], b"1 KiB");
        let n = format_size_human(1048576, &mut buf);
        assert_eq!(&buf[..n], b"1 MiB");
    }

    #[test]
    fn test_format_size_human_bytes() {
        let mut buf = [0u8; 32];
        let n = format_size_human(42, &mut buf);
        assert_eq!(&buf[..n], b"42 B");
    }

    #[test]
    fn test_starts_with() {
        assert!(starts_with(b"hello world", b"hello"));
        assert!(!starts_with(b"world", b"hello"));
    }

    #[test]
    fn test_contains() {
        assert!(contains(b"hello world", b"world"));
        assert!(contains(b"abcdef", b"bcd"));
        assert!(!contains(b"abc", b"xyz"));
        assert!(!contains(b"", b"a"));
    }

    #[test]
    fn test_detect_signatures() {
        let (sigs, count) = detect_signatures(b"/dev/sda");
        assert!(count > 0);
        // Should find ext4 and MBR
        let mut found_ext4 = false;
        let mut found_mbr = false;
        for sig in sigs.iter().take(count) {
            if &sig.fstype[..sig.fstype_len] == b"ext4" {
                found_ext4 = true;
            }
            if &sig.fstype[..sig.fstype_len] == b"dos" {
                found_mbr = true;
            }
        }
        assert!(found_ext4);
        assert!(found_mbr);
    }

    #[test]
    fn test_get_discard_mounts() {
        let (mounts, count) = get_discard_mounts();
        assert!(count > 0);
        // tmpfs should be excluded
        for mount in mounts.iter().take(count) {
            assert_ne!(&mount.2[..mount.3], b"tmpfs");
        }
    }

    #[test]
    fn test_signature_types() {
        assert_ne!(SIG_FILESYSTEM, SIG_RAID);
        assert_ne!(SIG_PARTITION, SIG_CRYPTO);
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(tool_name(Tool::Fstrim), b"fstrim");
        assert_eq!(tool_name(Tool::Blkdiscard), b"blkdiscard");
        assert_eq!(tool_name(Tool::Wipefs), b"wipefs");
    }

    #[test]
    fn test_copy_bytes() {
        let mut buf = [0u8; 20];
        let n = copy_bytes(&mut buf, 0, b"hello");
        assert_eq!(n, 5);
        assert_eq!(&buf[..n], b"hello");
    }

    #[test]
    fn test_set_field() {
        let mut buf = [0u8; 32];
        let mut len = 0;
        set_field(&mut buf, &mut len, b"test");
        assert_eq!(len, 4);
        assert_eq!(&buf[..len], b"test");
    }

    #[test]
    fn test_trim_range_defaults() {
        let range = TrimRange {
            start: 0,
            len: 0,
            minlen: FITRIM_MINLEN_DEFAULT,
        };
        assert_eq!(range.start, 0);
        assert_eq!(range.len, 0);
        assert_eq!(range.minlen, 0);
    }

    #[test]
    fn test_discard_params() {
        let params = DiscardParams {
            offset: 0,
            length: 1024 * 1024,
            step: DEFAULT_STEP_BYTES,
            secure: false,
            zeroout: false,
        };
        assert_eq!(params.length, 1024 * 1024);
        assert_eq!(params.step, 128 * 1024 * 1024);
    }
}
