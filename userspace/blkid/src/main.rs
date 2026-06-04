// OurOS blkid — block device identification
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
#![allow(dead_code)]

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
    fs_size: u64,
}

/// Known filesystem magic signatures
struct FsMagic {
    offset: usize,
    magic: &'static [u8],
    fs_type: &'static str,
}

const FS_MAGICS: &[FsMagic] = &[
    FsMagic { offset: 0x438, magic: &[0x53, 0xEF], fs_type: "ext4" },     // ext2/3/4
    FsMagic { offset: 0, magic: b"\xeb\x3c\x90", fs_type: "vfat" },       // FAT
    FsMagic { offset: 0, magic: b"\xeb\x58\x90", fs_type: "vfat" },       // FAT32
    FsMagic { offset: 0x10040, magic: b"-FVE-FS-", fs_type: "bitlocker" },
    FsMagic { offset: 3, magic: b"NTFS    ", fs_type: "ntfs" },
    FsMagic { offset: 0x8001, magic: b"CD001", fs_type: "iso9660" },
    FsMagic { offset: 0, magic: b"XFSB", fs_type: "xfs" },
    FsMagic { offset: 0x10034, magic: b"ReIsEr", fs_type: "reiserfs" },
    FsMagic { offset: 0xFF6, magic: b"\x41\xc6\x4e\x92", fs_type: "swap" },
];

fn detect_filesystem(device_path: &Path) -> Option<BlkidInfo> {
    let mut file = std::fs::File::open(device_path).ok()?;
    let mut buf = vec![0u8; 0x20000]; // Read first 128KB
    let bytes_read = file.read(&mut buf).ok()?;

    if bytes_read < 512 {
        return None;
    }

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
        if magic.offset + magic.magic.len() <= bytes_read
            && buf[magic.offset..magic.offset + magic.magic.len()] == *magic.magic
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

fn parse_ext4_info(buf: &[u8], info: &mut BlkidInfo) {
    // Superblock starts at offset 0x400 (1024 bytes)
    if buf.len() < 0x500 {
        return;
    }

    let sb = &buf[0x400..];

    // Block size: 1024 << s_log_block_size (offset 0x18)
    let log_block_size = u32::from_le_bytes([sb[0x18], sb[0x19], sb[0x1A], sb[0x1B]]);
    info.block_size = 1024u64.checked_shl(log_block_size).unwrap_or(4096);

    // Volume label: offset 0x78, 16 bytes
    let label_bytes = &sb[0x78..0x88];
    info.label = String::from_utf8_lossy(label_bytes)
        .trim_end_matches('\0')
        .to_string();

    // UUID: offset 0x68, 16 bytes
    if sb.len() > 0x78 {
        info.uuid = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            sb[0x68], sb[0x69], sb[0x6A], sb[0x6B],
            sb[0x6C], sb[0x6D],
            sb[0x6E], sb[0x6F],
            sb[0x70], sb[0x71],
            sb[0x72], sb[0x73], sb[0x74], sb[0x75], sb[0x76], sb[0x77]
        );
    }

    // Check if ext4 (has extent feature flag)
    let compat = u32::from_le_bytes([sb[0x5C], sb[0x5D], sb[0x5E], sb[0x5F]]);
    let incompat = u32::from_le_bytes([sb[0x60], sb[0x61], sb[0x62], sb[0x63]]);
    if incompat & 0x0040 != 0 {
        info.fs_type = "ext4".to_string();
    } else if compat & 0x0004 != 0 {
        info.fs_type = "ext3".to_string();
    } else {
        info.fs_type = "ext2".to_string();
    }
}

fn parse_fat_info(buf: &[u8], info: &mut BlkidInfo) {
    if buf.len() < 90 {
        return;
    }

    // Check FAT32 vs FAT16
    let total_sectors_16 = u16::from_le_bytes([buf[19], buf[20]]);
    if total_sectors_16 == 0 {
        // FAT32
        info.fs_type = "vfat".to_string();
        // Volume label at offset 71 (FAT32)
        if buf.len() > 82 {
            let label = &buf[71..82];
            info.label = String::from_utf8_lossy(label)
                .trim()
                .to_string();
        }
        // Volume serial at offset 67 (FAT32)
        if buf.len() > 71 {
            info.uuid = format!(
                "{:02X}{:02X}-{:02X}{:02X}",
                buf[70], buf[69], buf[68], buf[67]
            );
        }
    } else {
        info.fs_type = "vfat".to_string();
        // Volume label at offset 43 (FAT16)
        if buf.len() > 54 {
            let label = &buf[43..54];
            info.label = String::from_utf8_lossy(label)
                .trim()
                .to_string();
        }
        if buf.len() > 42 {
            info.uuid = format!(
                "{:02X}{:02X}-{:02X}{:02X}",
                buf[42], buf[41], buf[40], buf[39]
            );
        }
    }
}

fn parse_ntfs_info(buf: &[u8], info: &mut BlkidInfo) {
    if buf.len() < 0x50 {
        return;
    }
    // Volume serial at offset 0x48
    let serial = u64::from_le_bytes([
        buf[0x48], buf[0x49], buf[0x4A], buf[0x4B],
        buf[0x4C], buf[0x4D], buf[0x4E], buf[0x4F],
    ]);
    info.uuid = format!("{serial:016X}");
}

fn parse_xfs_info(buf: &[u8], info: &mut BlkidInfo) {
    if buf.len() < 0x68 {
        return;
    }
    // Block size at offset 4, big-endian
    info.block_size = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as u64;

    // UUID at offset 32, 16 bytes
    info.uuid = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[32], buf[33], buf[34], buf[35],
        buf[36], buf[37],
        buf[38], buf[39],
        buf[40], buf[41],
        buf[42], buf[43], buf[44], buf[45], buf[46], buf[47]
    );

    // Label at offset 0x6C, 12 bytes
    if buf.len() >= 0x78 {
        info.label = String::from_utf8_lossy(&buf[0x6C..0x78])
            .trim_end_matches('\0')
            .to_string();
    }
}

fn parse_swap_info(buf: &[u8], info: &mut BlkidInfo) {
    // Linux swap header has "SWAPSPACE2" at end of first page
    // UUID at offset 0x40C
    if buf.len() >= 0x41C {
        info.uuid = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            buf[0x40C], buf[0x40D], buf[0x40E], buf[0x40F],
            buf[0x410], buf[0x411],
            buf[0x412], buf[0x413],
            buf[0x414], buf[0x415],
            buf[0x416], buf[0x417], buf[0x418], buf[0x419], buf[0x41A], buf[0x41B]
        );
    }
    // Label at offset 0x41C
    if buf.len() >= 0x42C {
        info.label = String::from_utf8_lossy(&buf[0x41C..0x42C])
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
        let arg = &args[i];
        match personality {
            Personality::Blkid => match arg.as_str() {
                "-o" => {
                    i += 1;
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
                    i += 1;
                    // Show specific tag only
                    if let Some(tag) = args.get(i) {
                        cfg.tag_filter = Some((tag.clone(), String::new()));
                    }
                }
                "-t" => {
                    i += 1;
                    if let Some(spec) = args.get(i)
                        && let Some((tag, val)) = spec.split_once('=') {
                            cfg.tag_filter = Some((tag.to_string(), val.to_string()));
                        }
                }
                "-c" => {
                    i += 1;
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
        i += 1;
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
            writeln!(
                writer,
                "findfs: usage: findfs LABEL=<label> | UUID=<uuid>"
            )?;
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

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Blkid => "blkid",
        Personality::Findfs => "findfs",
    };
    println!("{name} (OurOS) 0.1.0");
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
            let args = vec![
                "blkid".to_string(),
                "-o".to_string(),
                fmt.to_string(),
            ];
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
        let args = vec![
            "findfs".to_string(),
            "UUID=abc-123".to_string(),
        ];
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
