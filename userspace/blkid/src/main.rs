// Slate OS blkid — block device identification
//
// Multi-personality binary:
//   blkid   — locate/print block device attributes
//   findfs  — find a filesystem by label or UUID
//
// Usage:
//   blkid [OPTIONS] [device...]
//   findfs LABEL=<label> | UUID=<uuid> | PARTUUID=<uuid>

#![cfg_attr(not(test), no_main)]
// BlkidInfo::fs_size is part of the BLKGETSIZE64 ioctl surface and the
// blkid -o size output the real implementation must produce. Dead-code
// lint cannot see across that future boundary.

#[cfg(not(test))]
use std::env;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Blkid,
    Findfs,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "findfs" => Personality::Findfs,
        _ => Personality::Blkid,
    }
}

// ---------------------------------------------------------------------------
// Filesystem detection via magic numbers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct BlkidInfo {
    device: PathBuf,
    fs_type: String,
    label: String,
    uuid: String,
    partuuid: String,
    part_label: String,
    block_size: u64,
    // Filesystem size, read and not printed by the current columns.
    #[allow(dead_code)]
    fs_size: u64,
}

/// Known filesystem magic signatures
struct FsMagic {
    offset: usize,
    magic: &'static [u8],
    fs_type: &'static str,
}

const FS_MAGICS: &[FsMagic] = &[
    FsMagic {
        offset: 0x438,
        magic: &[0x53, 0xEF],
        fs_type: "ext4",
    }, // ext2/3/4
    FsMagic {
        offset: 0,
        magic: b"\xeb\x3c\x90",
        fs_type: "vfat",
    }, // FAT
    FsMagic {
        offset: 0,
        magic: b"\xeb\x58\x90",
        fs_type: "vfat",
    }, // FAT32
    FsMagic {
        offset: 0x10040,
        magic: b"-FVE-FS-",
        fs_type: "bitlocker",
    },
    FsMagic {
        offset: 3,
        magic: b"NTFS    ",
        fs_type: "ntfs",
    },
    FsMagic {
        offset: 0x8001,
        magic: b"CD001",
        fs_type: "iso9660",
    },
    FsMagic {
        offset: 0,
        magic: b"XFSB",
        fs_type: "xfs",
    },
    FsMagic {
        offset: 0x10034,
        magic: b"ReIsEr",
        fs_type: "reiserfs",
    },
    FsMagic {
        offset: 0xFF6,
        magic: b"\x41\xc6\x4e\x92",
        fs_type: "swap",
    },
];

/// Read up to `buf.len()` bytes, stopping only at EOF.
///
/// `Read::read` is allowed to return fewer bytes than asked for even when more
/// are available, and on a block device a short read is ordinary rather than
/// exceptional -- one call tends to stop at a sector or page boundary. A
/// single `read` here therefore under-reports the device, and every caller
/// below decides what it may parse from the length it gets back.
fn read_up_to(file: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0usize;
    while total < buf.len() {
        let Some(rest) = buf.get_mut(total..) else {
            break;
        };
        match file.read(rest) {
            Ok(0) => break,
            Ok(n) => total = total.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

fn detect_filesystem(device_path: &Path) -> Option<BlkidInfo> {
    let mut file = std::fs::File::open(device_path).ok()?;
    let mut buf = vec![0u8; 0x20000]; // Read first 128KB
    let bytes_read = read_up_to(&mut file, &mut buf).ok()?;

    if bytes_read < 512 {
        return None;
    }

    // THE LOAD-BEARING LINE. Without it every `buf.len()` test in every
    // parser below is vacuously true, because `buf` is a 128 KiB zero-filled
    // `Vec` whose length never depended on the device at all.
    //
    // The parsers are already careful: each one guards each field with
    // `if buf.len() >= <end of that field>` before reading it, and the output
    // path already suppresses an empty label, an empty UUID and a zero block
    // size. Both halves of the check were written. Neither could fire, so a
    // device that stopped short of a field still produced one -- read out of
    // the zero padding and printed as measured fact.
    //
    // What that looked like: a 1082-byte image carrying only the ext4 magic
    // reported `UUID="00000000-0000-0000-0000-000000000000"`. Not merely
    // wrong -- EVERY short device reported that same UUID, so `findfs UUID=`
    // matched whichever it enumerated first. A UUID's one job is to be
    // unique, and the zero padding manufactured collisions.
    //
    // `bytes_read` was measured and then thrown away; this hands it to the
    // parsers, which is what they were already written to expect.
    buf.truncate(bytes_read);

    let mut info = BlkidInfo {
        device: device_path.to_path_buf(),
        fs_type: String::new(),
        label: String::new(),
        uuid: String::new(),
        partuuid: String::new(),
        part_label: String::new(),
        block_size: 0,
        fs_size: 0,
    };

    // Check magic signatures
    for magic in FS_MAGICS {
        if buf
            .get(magic.offset..)
            .is_some_and(|tail| tail.starts_with(magic.magic))
        {
            info.fs_type = magic.fs_type.to_string();

            // Extract more info based on FS type
            match magic.fs_type {
                "ext4" => parse_ext4_info(&buf, &mut info),
                "vfat" => parse_fat_info(&buf, &mut info),
                "ntfs" => parse_ntfs_info(&buf, &mut info),
                "xfs" => parse_xfs_info(&buf, &mut info),
                "swap" => parse_swap_info(&buf, &mut info),
                _ => {}
            }

            return Some(info);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Bounded reads
//
// Every superblock field below is at a fixed offset chosen by the ON-DISK
// FORMAT, and the buffer is however much of the device we managed to read.
// Those two facts are independent, so each read has to state what it needs
// and tolerate not getting it. `get(offset..)?.get(..N)?` says exactly that
// in one expression, and has no arithmetic to overflow.
//
// The alternative -- one `if buf.len() >= X` guard covering a run of fields --
// is what this file used to do, and it is how a whole parser ends up
// reporting nothing because its last field was missing. Per-field reads
// degrade one field at a time.
// ---------------------------------------------------------------------------

/// The `N` bytes at `offset`, or `None` if the device is too short.
#[inline]
fn bytes_at<const N: usize>(buf: &[u8], offset: usize) -> Option<[u8; N]> {
    buf.get(offset..)?.get(..N)?.try_into().ok()
}

/// The `len`-byte field at `offset`, or `None` if the device is too short.
#[inline]
fn field_at(buf: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    buf.get(offset..)?.get(..len)
}

fn u16_le(buf: &[u8], offset: usize) -> Option<u16> {
    bytes_at::<2>(buf, offset).map(u16::from_le_bytes)
}

fn u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    bytes_at::<4>(buf, offset).map(u32::from_le_bytes)
}

fn u32_be(buf: &[u8], offset: usize) -> Option<u32> {
    bytes_at::<4>(buf, offset).map(u32::from_be_bytes)
}

fn u64_le(buf: &[u8], offset: usize) -> Option<u64> {
    bytes_at::<8>(buf, offset).map(u64::from_le_bytes)
}

/// Format 16 raw bytes as a canonical 8-4-4-4-12 lowercase UUID.
///
/// ext4, XFS and swap all store a UUID this way and all three used to format
/// it with a sixteen-argument `format!`, which is where 48 of this crate's 91
/// indexing warnings came from. One loop replaces all three.
fn format_uuid(bytes: [u8; 16]) -> String {
    use core::fmt::Write as _;
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        // Ignored: `write!` to a `String` is infallible -- `fmt::Error` exists
        // for writers that can fail, and `String`'s impl never returns it.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn parse_ext4_info(buf: &[u8], info: &mut BlkidInfo) {
    // The ext2/3/4 superblock sits at a fixed 1024-byte offset.
    let Some(sb) = buf.get(0x400..) else {
        return;
    };

    // Block size: 1024 << s_log_block_size (offset 0x18)
    if let Some(log_block_size) = u32_le(sb, 0x18) {
        info.block_size = 1024u64.checked_shl(log_block_size).unwrap_or(4096);
    }

    // Volume label: offset 0x78, 16 bytes
    if let Some(label) = field_at(sb, 0x78, 16) {
        info.label = String::from_utf8_lossy(label)
            .trim_end_matches('\0')
            .to_string();
    }

    // UUID: offset 0x68, 16 bytes
    if let Some(uuid) = bytes_at::<16>(sb, 0x68) {
        info.uuid = format_uuid(uuid);
    }

    // ext2 / ext3 / ext4 are told apart by their feature flags, not by the
    // magic -- all three share it. Without the flags the family name from
    // the magic table stands, which is the honest answer for a device that
    // stops before offset 0x464.
    if let (Some(compat), Some(incompat)) = (u32_le(sb, 0x5C), u32_le(sb, 0x60)) {
        info.fs_type = if incompat & 0x0040 != 0 {
            "ext4"
        } else if compat & 0x0004 != 0 {
            "ext3"
        } else {
            "ext2"
        }
        .to_string();
    }
}

fn parse_fat_info(buf: &[u8], info: &mut BlkidInfo) {
    info.fs_type = "vfat".to_string();

    // A zero 16-bit total-sector count means the real count lives in the
    // 32-bit field, which is what distinguishes FAT32 from FAT12/16. The two
    // layouts then put the label and serial in different places.
    let Some(total_sectors_16) = u16_le(buf, 19) else {
        return;
    };
    let (label_off, serial_off) = if total_sectors_16 == 0 {
        (71, 67) // FAT32
    } else {
        (43, 39) // FAT12/16
    };

    if let Some(label) = field_at(buf, label_off, 11) {
        info.label = String::from_utf8_lossy(label).trim().to_string();
    }
    if let Some([b0, b1, b2, b3]) = bytes_at::<4>(buf, serial_off) {
        // Shown high half first, as blkid(8) prints it.
        info.uuid = format!("{b3:02X}{b2:02X}-{b1:02X}{b0:02X}");
    }
}

fn parse_ntfs_info(buf: &[u8], info: &mut BlkidInfo) {
    // Volume serial at offset 0x48
    if let Some(serial) = u64_le(buf, 0x48) {
        info.uuid = format!("{serial:016X}");
    }
}

fn parse_xfs_info(buf: &[u8], info: &mut BlkidInfo) {
    // Block size at offset 4, big-endian
    if let Some(block_size) = u32_be(buf, 4) {
        info.block_size = u64::from(block_size);
    }

    // UUID at offset 32, 16 bytes
    if let Some(uuid) = bytes_at::<16>(buf, 32) {
        info.uuid = format_uuid(uuid);
    }

    // Label at offset 0x6C, 12 bytes
    if let Some(label) = field_at(buf, 0x6C, 12) {
        info.label = String::from_utf8_lossy(label)
            .trim_end_matches('\0')
            .to_string();
    }
}

fn parse_swap_info(buf: &[u8], info: &mut BlkidInfo) {
    // Linux swap header has "SWAPSPACE2" at end of first page
    // UUID at offset 0x40C
    if let Some(uuid) = bytes_at::<16>(buf, 0x40C) {
        info.uuid = format_uuid(uuid);
    }
    // Label at offset 0x41C, 16 bytes
    if let Some(label) = field_at(buf, 0x41C, 16) {
        info.label = String::from_utf8_lossy(label)
            .trim_end_matches('\0')
            .to_string();
    }
}

// ---------------------------------------------------------------------------
// Device enumeration
// ---------------------------------------------------------------------------

fn enumerate_block_devices() -> Vec<PathBuf> {
    let mut devices = Vec::new();

    // Scan /dev/ for block devices
    if let Ok(entries) = std::fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Common block device patterns
            if name.starts_with("sd")
                || name.starts_with("hd")
                || name.starts_with("vd")
                || name.starts_with("nvme")
                || name.starts_with("loop")
                || name.starts_with("dm-")
            {
                devices.push(entry.path());
            }
        }
    }

    // Also check /sys/block for more devices
    if let Ok(entries) = std::fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let dev_path = PathBuf::from("/dev").join(entry.file_name());
            if dev_path.exists() && !devices.contains(&dev_path) {
                devices.push(dev_path);
            }
        }
    }

    devices.sort();
    devices
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    devices: Vec<PathBuf>,
    output_format: OutputFormat,
    tag_filter: Option<(String, String)>, // TAG=VALUE
    show_all: bool,
    cache_file: Option<PathBuf>,
    no_encoding: bool,
    show_help: bool,
    show_version: bool,
    // findfs
    findfs_spec: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Default,   // blkid standard
    ValueOnly, // -o value
    Full,      // -o full
    List,      // -o list
    Export,    // -o export
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Blkid,
            devices: Vec::new(),
            output_format: OutputFormat::Default,
            tag_filter: None,
            show_all: false,
            cache_file: None,
            no_encoding: false,
            show_help: false,
            show_version: false,
            findfs_spec: None,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Blkid);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut i = 1;

    while i < args.len() {
        let Some(arg) = args.get(i) else {
            break;
        };
        match personality {
            Personality::Blkid => match arg.as_str() {
                "-o" => {
                    i = i.saturating_add(1);
                    let fmt = args.get(i).ok_or("-o requires a format")?;
                    cfg.output_format = match fmt.as_str() {
                        "value" => OutputFormat::ValueOnly,
                        "full" => OutputFormat::Full,
                        "list" => OutputFormat::List,
                        "export" => OutputFormat::Export,
                        "device" => OutputFormat::Default,
                        other => return Err(format!("unknown output format: {other}")),
                    };
                }
                "-s" => {
                    i = i.saturating_add(1);
                    // Show specific tag only
                    if let Some(tag) = args.get(i) {
                        cfg.tag_filter = Some((tag.clone(), String::new()));
                    }
                }
                "-t" => {
                    i = i.saturating_add(1);
                    if let Some(spec) = args.get(i)
                        && let Some((tag, val)) = spec.split_once('=')
                    {
                        cfg.tag_filter = Some((tag.to_string(), val.to_string()));
                    }
                }
                "-c" => {
                    i = i.saturating_add(1);
                    cfg.cache_file = args.get(i).map(PathBuf::from);
                }
                "-p" | "--probe" => cfg.show_all = true,
                "-g" | "--garbage-collect" => {} // no-op
                "-n" | "--no-encoding" => cfg.no_encoding = true,
                "-h" | "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("blkid: unknown option: {other}"));
                }
                _ => cfg.devices.push(PathBuf::from(arg)),
            },
            Personality::Findfs => {
                if arg == "-h" || arg == "--help" {
                    cfg.show_help = true;
                } else if arg == "-V" || arg == "--version" {
                    cfg.show_version = true;
                } else if !arg.starts_with('-') {
                    cfg.findfs_spec = Some(arg.clone());
                }
            }
        }
        i = i.saturating_add(1);
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn run_blkid(cfg: &Config, writer: &mut dyn Write) -> io::Result<i32> {
    let devices = if cfg.devices.is_empty() {
        enumerate_block_devices()
    } else {
        cfg.devices.clone()
    };

    let mut found_any = false;

    for device in &devices {
        if let Some(info) = detect_filesystem(device) {
            // Tag filter
            if let Some((ref tag, ref val)) = cfg.tag_filter {
                let tag_val = match tag.to_uppercase().as_str() {
                    "TYPE" => &info.fs_type,
                    "LABEL" => &info.label,
                    "UUID" => &info.uuid,
                    "PARTUUID" => &info.partuuid,
                    _ => continue,
                };
                if !val.is_empty() && tag_val != val {
                    continue;
                }
            }

            match cfg.output_format {
                OutputFormat::Default | OutputFormat::Full => {
                    write!(writer, "{}: ", info.device.display())?;
                    let mut parts = Vec::new();
                    if !info.label.is_empty() {
                        parts.push(format!("LABEL=\"{}\"", info.label));
                    }
                    if !info.uuid.is_empty() {
                        parts.push(format!("UUID=\"{}\"", info.uuid));
                    }
                    if !info.partuuid.is_empty() {
                        parts.push(format!("PARTUUID=\"{}\"", info.partuuid));
                    }
                    if info.block_size > 0 {
                        parts.push(format!("BLOCK_SIZE=\"{}\"", info.block_size));
                    }
                    parts.push(format!("TYPE=\"{}\"", info.fs_type));
                    writeln!(writer, "{}", parts.join(" "))?;
                }
                OutputFormat::ValueOnly => {
                    if let Some((ref tag, _)) = cfg.tag_filter {
                        let val = match tag.to_uppercase().as_str() {
                            "TYPE" => &info.fs_type,
                            "LABEL" => &info.label,
                            "UUID" => &info.uuid,
                            _ => &info.fs_type,
                        };
                        writeln!(writer, "{val}")?;
                    } else {
                        if !info.label.is_empty() {
                            writeln!(writer, "{}", info.label)?;
                        }
                        if !info.uuid.is_empty() {
                            writeln!(writer, "{}", info.uuid)?;
                        }
                        writeln!(writer, "{}", info.fs_type)?;
                    }
                }
                OutputFormat::List => {
                    writeln!(
                        writer,
                        "{:<20} {:<10} {:<36} {}",
                        info.device.display(),
                        info.fs_type,
                        info.uuid,
                        info.label
                    )?;
                }
                OutputFormat::Export => {
                    writeln!(writer, "DEVNAME={}", info.device.display())?;
                    if !info.label.is_empty() {
                        writeln!(writer, "LABEL={}", info.label)?;
                    }
                    if !info.uuid.is_empty() {
                        writeln!(writer, "UUID={}", info.uuid)?;
                    }
                    writeln!(writer, "TYPE={}", info.fs_type)?;
                    if info.block_size > 0 {
                        writeln!(writer, "BLOCK_SIZE={}", info.block_size)?;
                    }
                    writeln!(writer)?;
                }
            }
            found_any = true;
        }
    }

    Ok(if found_any { 0 } else { 2 })
}

fn run_findfs(cfg: &Config, writer: &mut dyn Write) -> io::Result<i32> {
    let spec = match &cfg.findfs_spec {
        Some(s) => s.clone(),
        None => {
            writeln!(writer, "findfs: usage: findfs LABEL=<label> | UUID=<uuid>")?;
            return Ok(1);
        }
    };

    let (tag, value) = match spec.split_once('=') {
        Some((t, v)) => (t.to_uppercase(), v.to_string()),
        None => {
            writeln!(writer, "findfs: invalid spec: {spec}")?;
            return Ok(1);
        }
    };

    let devices = enumerate_block_devices();

    for device in &devices {
        if let Some(info) = detect_filesystem(device) {
            let matches = match tag.as_str() {
                "LABEL" => info.label == value,
                "UUID" => info.uuid == value,
                "PARTUUID" => info.partuuid == value,
                "PARTLABEL" => info.part_label == value,
                "TYPE" => info.fs_type == value,
                _ => false,
            };
            if matches {
                writeln!(writer, "{}", info.device.display())?;
                return Ok(0);
            }
        }
    }

    writeln!(writer, "findfs: unable to resolve '{spec}'")?;
    Ok(1)
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

// Reachable only from `main`, which the test harness replaces, so this is dead
// in the test build and live in the real one. Scoped to `test` rather than
// allowed outright, so a genuinely dead item here is still reported.
#[cfg_attr(test, allow(dead_code))]
fn print_help(personality: Personality) {
    match personality {
        Personality::Blkid => {
            println!("Usage: blkid [OPTIONS] [device...]");
            println!();
            println!("Locate/print block device attributes.");
            println!();
            println!("Options:");
            println!("  -o <format>   Output format (value, full, list, export)");
            println!("  -s <tag>      Show only specified tag (TYPE, LABEL, UUID)");
            println!("  -t <spec>     Find device by tag (e.g., TYPE=ext4)");
            println!("  -c <file>     Cache file (default: /etc/blkid.tab)");
            println!("  -p            Low-level probing mode");
            println!("  -h, --help    Show this help");
            println!("  -V, --version Show version");
        }
        Personality::Findfs => {
            println!("Usage: findfs LABEL=<label> | UUID=<uuid> | PARTUUID=<uuid>");
            println!();
            println!("Find a filesystem by label or UUID.");
        }
    }
}

#[cfg_attr(test, allow(dead_code))]
fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Blkid => "blkid",
        Personality::Findfs => "findfs",
    };
    println!("{name} (Slate OS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help(cfg.personality);
        return 0;
    }

    if cfg.show_version {
        print_version(cfg.personality);
        return 0;
    }

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let result = match cfg.personality {
        Personality::Blkid => run_blkid(&cfg, &mut writer),
        Personality::Findfs => run_findfs(&cfg, &mut writer),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("blkid: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point in a test: an `unwrap` that fires is
    // a failed assertion with a stack trace, which is what a test is for.
    // CLAUDE.md allows the defensive lints to be switched off here for
    // exactly that reason.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn test_detect_personality() {
        assert_eq!(detect_personality("blkid"), Personality::Blkid);
        assert_eq!(detect_personality("findfs"), Personality::Findfs);
        assert_eq!(detect_personality("/sbin/blkid"), Personality::Blkid);
    }

    #[test]
    fn test_parse_args_basic() {
        let args = vec!["blkid".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Blkid);
        assert!(cfg.devices.is_empty());
    }

    #[test]
    fn test_parse_args_device() {
        let args = vec!["blkid".to_string(), "/dev/sda1".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.devices.len(), 1);
    }

    #[test]
    fn test_parse_args_output_format() {
        for (fmt, expected) in [
            ("value", OutputFormat::ValueOnly),
            ("full", OutputFormat::Full),
            ("list", OutputFormat::List),
            ("export", OutputFormat::Export),
        ] {
            let args = vec!["blkid".to_string(), "-o".to_string(), fmt.to_string()];
            let cfg = parse_args(&args).unwrap();
            assert_eq!(cfg.output_format, expected);
        }
    }

    #[test]
    fn test_parse_args_tag_filter() {
        let args = vec![
            "blkid".to_string(),
            "-t".to_string(),
            "TYPE=ext4".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(
            cfg.tag_filter,
            Some(("TYPE".to_string(), "ext4".to_string()))
        );
    }

    #[test]
    fn test_parse_args_findfs() {
        let args = vec!["findfs".to_string(), "UUID=abc-123".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.findfs_spec, Some("UUID=abc-123".to_string()));
    }

    #[test]
    fn test_parse_args_help() {
        for name in &["blkid", "findfs"] {
            let args = vec![name.to_string(), "--help".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_help);
        }
    }

    #[test]
    fn test_parse_ext4_info() {
        // Create a minimal ext4-like superblock
        let mut buf = vec![0u8; 0x500];
        // Magic at 0x438
        buf[0x438] = 0x53;
        buf[0x439] = 0xEF;
        // Block size = 2 (4096)
        buf[0x418] = 2;
        // Label at 0x478
        buf[0x478] = b'T';
        buf[0x479] = b'E';
        buf[0x47A] = b'S';
        buf[0x47B] = b'T';
        // Incompat flags (extents = 0x40)
        buf[0x460] = 0x40;

        let mut info = BlkidInfo {
            device: PathBuf::from("/dev/test"),
            fs_type: "ext4".to_string(),
            label: String::new(),
            uuid: String::new(),
            partuuid: String::new(),
            part_label: String::new(),
            block_size: 0,
            fs_size: 0,
        };
        parse_ext4_info(&buf, &mut info);
        assert_eq!(info.block_size, 4096);
        assert_eq!(info.fs_type, "ext4");
        assert!(info.label.starts_with("TEST"));
    }

    /// A device that stops short of a field must not report that field.
    ///
    /// Both images carry a valid ext4 magic at 0x438, so both are detected.
    /// They differ only in whether the superblock's UUID (0x468..0x478) and
    /// label (0x478..0x488) are actually present on the device.
    ///
    /// The truncated case is the regression: before `buf.truncate`, the
    /// parser read those offsets out of the zero padding of a 128 KiB buffer
    /// and reported an all-zero UUID as fact.
    /// Sixteen distinguishable bytes: every position differs, so a parser
    /// that transposes two of them fails rather than coincidentally passing.
    const UUID_BYTES: [u8; 16] = [
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        0x20,
    ];
    const UUID_TEXT: &str = "11121314-1516-1718-191a-1b1c1d1e1f20";

    /// `size` zero bytes with `fields` written into them -- a device image.
    fn image(size: usize, fields: &[(usize, &[u8])]) -> Vec<u8> {
        let mut buf = vec![0u8; size];
        for (off, data) in fields {
            buf[*off..*off + data.len()].copy_from_slice(data);
        }
        buf
    }

    fn blank(fs_type: &str) -> BlkidInfo {
        BlkidInfo {
            device: PathBuf::from("/dev/test"),
            fs_type: fs_type.to_string(),
            label: String::new(),
            uuid: String::new(),
            partuuid: String::new(),
            part_label: String::new(),
            block_size: 0,
            fs_size: 0,
        }
    }

    #[test]
    fn format_uuid_groups_the_bytes_8_4_4_4_12() {
        assert_eq!(format_uuid(UUID_BYTES), UUID_TEXT);
        assert_eq!(
            format_uuid([0; 16]),
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(
            format_uuid([0xFF; 16]),
            "ffffffff-ffff-ffff-ffff-ffffffffffff"
        );
    }

    /// Each parser gets both probes: a device carrying the field, and one
    /// that ends before it. Without the second, a parser that silently gave
    /// up would pass; without the first, one that never ran would.
    #[test]
    fn xfs_reads_block_size_uuid_and_label() {
        let mut info = blank("xfs");
        parse_xfs_info(
            &image(
                0x100,
                &[
                    (0, b"XFSB"),
                    (4, &[0x00, 0x00, 0x10, 0x00]), // big-endian 4096
                    (32, &UUID_BYTES),
                    (0x6C, b"XFSLABEL"),
                ],
            ),
            &mut info,
        );
        assert_eq!(info.block_size, 4096);
        assert_eq!(info.uuid, UUID_TEXT);
        assert_eq!(info.label, "XFSLABEL");

        // Ends at 40: the block size (needs 8) is there, the UUID (needs 48)
        // is not. The fields degrade one at a time, not all together.
        let mut info = blank("xfs");
        parse_xfs_info(&image(40, &[(4, &[0x00, 0x00, 0x10, 0x00])]), &mut info);
        assert_eq!(info.block_size, 4096);
        assert!(info.uuid.is_empty(), "uuid was {:?}", info.uuid);
        assert!(info.label.is_empty(), "label was {:?}", info.label);
    }

    #[test]
    fn swap_reads_uuid_and_label() {
        let mut info = blank("swap");
        parse_swap_info(
            &image(0x500, &[(0x40C, &UUID_BYTES), (0x41C, b"SWAPLBL")]),
            &mut info,
        );
        assert_eq!(info.uuid, UUID_TEXT);
        assert_eq!(info.label, "SWAPLBL");

        let mut info = blank("swap");
        parse_swap_info(&image(0x410, &[]), &mut info);
        assert!(info.uuid.is_empty(), "uuid was {:?}", info.uuid);
        assert!(info.label.is_empty(), "label was {:?}", info.label);
    }

    #[test]
    fn ntfs_reads_the_volume_serial() {
        let mut info = blank("ntfs");
        parse_ntfs_info(
            &image(0x100, &[(0x48, &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])]),
            &mut info,
        );
        assert_eq!(info.uuid, "EFCDAB8967452301");

        let mut info = blank("ntfs");
        parse_ntfs_info(&image(0x4F, &[]), &mut info);
        assert!(info.uuid.is_empty(), "uuid was {:?}", info.uuid);
    }

    #[test]
    fn fat_picks_its_layout_from_the_16_bit_sector_count() {
        // FAT32: the 16-bit count is zero, so label and serial are at the
        // FAT32 offsets. Serial bytes are printed high half first.
        let mut info = blank("vfat");
        parse_fat_info(
            &image(
                0x100,
                &[
                    (19, &[0x00, 0x00]),
                    (67, &[0x78, 0x56, 0x34, 0x12]),
                    (71, b"FATLABEL   "),
                ],
            ),
            &mut info,
        );
        assert_eq!(info.label, "FATLABEL");
        assert_eq!(info.uuid, "1234-5678");

        // FAT16: a non-zero count moves both fields, and reading the FAT32
        // offsets here would give the wrong answer rather than no answer --
        // which is why the two cases carry different label text.
        let mut info = blank("vfat");
        parse_fat_info(
            &image(
                0x100,
                &[
                    (19, &[0x10, 0x00]),
                    (39, &[0x78, 0x56, 0x34, 0x12]),
                    (43, b"FAT16LBL   "),
                ],
            ),
            &mut info,
        );
        assert_eq!(info.label, "FAT16LBL");
        assert_eq!(info.uuid, "1234-5678");

        // Ends before the sector count at 19: nothing is knowable but the
        // type, which came from the magic rather than from this parser.
        let mut info = blank("vfat");
        parse_fat_info(&image(20, &[]), &mut info);
        assert_eq!(info.fs_type, "vfat");
        assert!(info.uuid.is_empty(), "uuid was {:?}", info.uuid);
        assert!(info.label.is_empty(), "label was {:?}", info.label);
    }

    #[test]
    fn ext4_family_is_decided_by_feature_flags_not_by_the_magic() {
        // The magic is shared by all three, so the flags at 0x5C/0x60 are
        // what separates them.
        let probe = |compat: u32, incompat: u32| {
            let mut info = blank("ext4");
            parse_ext4_info(
                &image(
                    0x500,
                    &[
                        (0x45C, &compat.to_le_bytes()),
                        (0x460, &incompat.to_le_bytes()),
                    ],
                ),
                &mut info,
            );
            info.fs_type
        };
        assert_eq!(probe(0, 0x0040), "ext4");
        assert_eq!(probe(0x0004, 0), "ext3");
        assert_eq!(probe(0, 0), "ext2");

        // Without the flags the family name from the magic table stands,
        // rather than a conclusion drawn from absent bits.
        let mut info = blank("ext4");
        parse_ext4_info(&image(0x43A, &[]), &mut info);
        assert_eq!(info.fs_type, "ext4");
    }

    #[test]
    fn short_device_reports_no_uuid_or_label() {
        let dir = std::env::temp_dir().join("blkid-short-device-test");
        // Ignored: a leftover directory from a previous run is fine, and any
        // real failure to create it surfaces on the `write` below.
        let _ = std::fs::create_dir_all(&dir);

        let write = |name: &str, size: usize, fields: &[(usize, &[u8])]| -> PathBuf {
            let path = dir.join(name);
            std::fs::write(&path, image(size, fields)).expect("write test image");
            path
        };

        let full = write(
            "full.img",
            0x1000,
            &[
                (0x438, &[0x53, 0xEF]),
                (0x468, &UUID_BYTES),
                (0x478, b"REALLABEL"),
            ],
        );
        // One byte past the magic: the UUID at 0x468 is beyond end-of-file.
        let short = write("short.img", 0x43A, &[(0x438, &[0x53, 0xEF])]);

        // Probe one: the parser RUNS. Without this the test below would pass
        // against a blkid that simply never parsed anything.
        let got = detect_filesystem(&full).expect("full image is detected");
        assert_eq!(got.uuid, UUID_TEXT);
        assert_eq!(got.label, "REALLABEL");

        // Probe two: the parser REFUSES. The fields are not on the device, so
        // no value may be reported for them -- least of all a zero one.
        let got = detect_filesystem(&short).expect("short image is still detected");
        assert!(
            got.uuid.is_empty(),
            "reported UUID {:?} for a device that ends before the UUID field",
            got.uuid
        );
        assert!(
            got.label.is_empty(),
            "reported label {:?} for a device that ends before the label field",
            got.label
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_run_blkid_no_devices() {
        let cfg = Config {
            personality: Personality::Blkid,
            ..Default::default()
        };
        let mut buf = Vec::new();
        let code = run_blkid(&cfg, &mut buf).unwrap();
        // Likely returns 2 (no devices found on test system)
        assert!(code == 0 || code == 2);
    }

    #[test]
    fn test_run_findfs_no_spec() {
        let cfg = Config {
            personality: Personality::Findfs,
            findfs_spec: None,
            ..Default::default()
        };
        let mut buf = Vec::new();
        let code = run_findfs(&cfg, &mut buf).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_findfs_invalid() {
        let cfg = Config {
            personality: Personality::Findfs,
            findfs_spec: Some("badspec".to_string()),
            ..Default::default()
        };
        let mut buf = Vec::new();
        let code = run_findfs(&cfg, &mut buf).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.output_format, OutputFormat::Default);
        assert!(cfg.devices.is_empty());
        assert!(!cfg.show_all);
    }
}
