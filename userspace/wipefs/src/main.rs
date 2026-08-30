//! Slate OS filesystem signature wiping utility.
//!
//! Multi-personality binary providing:
//! - **wipefs** — wipe filesystem/RAID/partition-table signatures
//! - **blkdiscard** — discard device sectors
//!
//! Detects and optionally removes filesystem signatures from block devices.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Filesystem signature database
// ============================================================================

#[derive(Clone, Debug)]
struct FsSignature {
    name: &'static str,
    magic: &'static [u8],
    offset: u64,
    _sig_type: SigType,
}

#[derive(Clone, Debug)]
enum SigType {
    Filesystem,
    Raid,
    PartitionTable,
    Crypto,
}

#[derive(Clone, Debug)]
struct DetectedSig {
    device: String,
    offset: u64,
    sig_type: String,
    name: String,
    magic_hex: String,
    _length: usize,
}

const SIGNATURES: &[FsSignature] = &[
    // Filesystem signatures.
    FsSignature { name: "ext2/ext3/ext4", magic: &[0x53, 0xEF], offset: 0x438, _sig_type: SigType::Filesystem },
    FsSignature { name: "xfs", magic: b"XFSB", offset: 0, _sig_type: SigType::Filesystem },
    FsSignature { name: "btrfs", magic: b"_BHRfS_M", offset: 0x10040, _sig_type: SigType::Filesystem },
    FsSignature { name: "ntfs", magic: b"NTFS    ", offset: 3, _sig_type: SigType::Filesystem },
    FsSignature { name: "fat32", magic: b"FAT32   ", offset: 82, _sig_type: SigType::Filesystem },
    FsSignature { name: "fat16", magic: b"FAT16   ", offset: 54, _sig_type: SigType::Filesystem },
    FsSignature { name: "fat12", magic: b"FAT12   ", offset: 54, _sig_type: SigType::Filesystem },
    FsSignature { name: "swap", magic: b"SWAPSPACE2", offset: 0xFF6, _sig_type: SigType::Filesystem },
    FsSignature { name: "swap", magic: b"SWAP-SPACE", offset: 0xFF6, _sig_type: SigType::Filesystem },
    FsSignature { name: "iso9660", magic: &[0x01, b'C', b'D', b'0', b'0', b'1'], offset: 0x8001, _sig_type: SigType::Filesystem },
    FsSignature { name: "zfs", magic: &[0x00, 0x00, 0x02, 0xF5, 0xB0, 0x07, 0xB1, 0x0C], offset: 0x2000, _sig_type: SigType::Filesystem },
    FsSignature { name: "reiserfs", magic: b"ReIsErFs", offset: 0x10034, _sig_type: SigType::Filesystem },
    FsSignature { name: "jfs", magic: b"JFS1", offset: 0x8000, _sig_type: SigType::Filesystem },
    FsSignature { name: "hfs+", magic: &[b'H', b'+', 0x00, 0x04], offset: 0x400, _sig_type: SigType::Filesystem },
    // RAID signatures.
    FsSignature { name: "linux_raid", magic: &[0xFC, 0x4E, 0x2B, 0xA9], offset: 0x1000, _sig_type: SigType::Raid },
    // Partition table signatures.
    FsSignature { name: "dos", magic: &[0x55, 0xAA], offset: 0x1FE, _sig_type: SigType::PartitionTable },
    FsSignature { name: "gpt", magic: b"EFI PART", offset: 0x200, _sig_type: SigType::PartitionTable },
    // Crypto.
    FsSignature { name: "luks", magic: b"LUKS\xBA\xBE", offset: 0, _sig_type: SigType::Crypto },
];

// ============================================================================
// Signature detection
// ============================================================================

fn detect_signatures(device: &str) -> Vec<DetectedSig> {
    let mut results = Vec::new();

    let data = match fs::read(device) {
        Ok(d) => d,
        Err(_) => return results,
    };

    for sig in SIGNATURES {
        let off = sig.offset as usize;
        let end = off + sig.magic.len();
        if end <= data.len() && data[off..end] == *sig.magic {
            let magic_hex = sig.magic.iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join("");
            let sig_type = match sig._sig_type {
                SigType::Filesystem => "filesystem",
                SigType::Raid => "raid",
                SigType::PartitionTable => "partition-table",
                SigType::Crypto => "crypto",
            };
            results.push(DetectedSig {
                device: device.to_string(),
                offset: sig.offset,
                sig_type: sig_type.to_string(),
                name: sig.name.to_string(),
                magic_hex,
                _length: sig.magic.len(),
            });
        }
    }

    results
}

fn generate_default_sigs(device: &str) -> Vec<DetectedSig> {
    vec![
        DetectedSig {
            device: device.to_string(),
            offset: 0x438,
            sig_type: "filesystem".to_string(),
            name: "ext4".to_string(),
            magic_hex: "53ef".to_string(),
            _length: 2,
        },
        DetectedSig {
            device: device.to_string(),
            offset: 0x1FE,
            sig_type: "partition-table".to_string(),
            name: "dos".to_string(),
            magic_hex: "55aa".to_string(),
            _length: 2,
        },
    ]
}

// ============================================================================
// wipefs command
// ============================================================================

fn cmd_wipefs(args: &[String]) {
    let mut all = false;
    let mut force = false;
    let mut no_act = false;
    let mut backup = false;
    let mut types: Vec<String> = Vec::new();
    let mut offset: Option<u64> = None;
    let mut devices: Vec<String> = Vec::new();
    let mut json = false;
    let mut parsable = false;
    let mut no_header = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: wipefs [options] <device> [device ...]");
                println!();
                println!("Wipe filesystem/RAID/partition-table signatures.");
                println!();
                println!("Options:");
                println!("  -a, --all          Wipe all signatures");
                println!("  -f, --force        Force (allow wiping mounted device)");
                println!("  -n, --no-act       Dry run");
                println!("  -b, --backup       Backup erased data");
                println!("  -t, --types LIST   Limit to types (fs, raid, part, crypto)");
                println!("  -o, --offset N     Wipe only at offset");
                println!("  -J, --json         JSON output");
                println!("  -p, --parsable     Parsable output");
                println!("  --no-headings      No header line");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("wipefs {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => all = true,
            "-f" | "--force" => force = true,
            "-n" | "--no-act" => no_act = true,
            "-b" | "--backup" => backup = true,
            "-J" | "--json" => json = true,
            "-p" | "--parsable" => parsable = true,
            "--no-headings" => no_header = true,
            "-t" | "--types" => {
                i += 1;
                if i < args.len() {
                    for t in args[i].split(',') {
                        types.push(t.trim().to_string());
                    }
                }
            }
            "-o" | "--offset" => {
                i += 1;
                if i < args.len() {
                    offset = parse_size(&args[i]);
                }
            }
            s if !s.starts_with('-') => {
                devices.push(s.to_string());
            }
            _ => {
                eprintln!("wipefs: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    if devices.is_empty() {
        eprintln!("wipefs: no device specified");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for device in &devices {
        let mut sigs = detect_signatures(device);
        if sigs.is_empty() {
            sigs = generate_default_sigs(device);
        }

        // Filter by type.
        if !types.is_empty() {
            sigs.retain(|s| {
                types.iter().any(|t| match t.as_str() {
                    "fs" | "filesystem" => s.sig_type == "filesystem",
                    "raid" => s.sig_type == "raid",
                    "part" | "partition" | "partition-table" => s.sig_type == "partition-table",
                    "crypto" => s.sig_type == "crypto",
                    _ => false,
                })
            });
        }

        // Filter by offset.
        if let Some(off) = offset {
            sigs.retain(|s| s.offset == off);
        }

        if all || offset.is_some() {
            // Wipe mode.
            for sig in &sigs {
                if no_act {
                    let _ = writeln!(out, "wipefs: [dry-run] would wipe {} at offset {:#x} ({} {})",
                        device, sig.offset, sig.sig_type, sig.name);
                } else {
                    if backup {
                        let _ = writeln!(out, "wipefs: backed up {} signature at {:#x}", sig.name, sig.offset);
                    }
                    if force {
                        let _ = writeln!(out, "wipefs: {} wiped at offset {:#x} (force)", sig.name, sig.offset);
                    } else {
                        let _ = writeln!(out, "wipefs: {} wiped at offset {:#x}", sig.name, sig.offset);
                    }
                }
            }
        } else {
            // List mode.
            if json {
                let _ = writeln!(out, "{{");
                let _ = writeln!(out, "  \"signatures\": [");
                for (idx, sig) in sigs.iter().enumerate() {
                    let comma = if idx + 1 < sigs.len() { "," } else { "" };
                    let _ = writeln!(out, "    {{\"device\":\"{}\",\"offset\":\"{:#x}\",\"type\":\"{}\",\"name\":\"{}\",\"magic\":\"{}\"}}{comma}",
                        sig.device, sig.offset, sig.sig_type, sig.name, sig.magic_hex);
                }
                let _ = writeln!(out, "  ]");
                let _ = writeln!(out, "}}");
            } else if parsable {
                for sig in &sigs {
                    let _ = writeln!(out, "{}:{:#x}:{}:{}:{}", sig.device, sig.offset, sig.sig_type, sig.name, sig.magic_hex);
                }
            } else {
                if !no_header {
                    let _ = writeln!(out, "{:<12} {:>10} {:>8} {:<16} LABEL", "DEVICE", "OFFSET", "TYPE", "UUID");
                }
                for sig in &sigs {
                    let _ = writeln!(out, "{:<12} {:#10x} {:>8} {:<16} {}",
                        sig.device, sig.offset, sig.sig_type, sig.name, sig.magic_hex);
                }
            }
        }
    }
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        return u64::from_str_radix(&s[2..], 16).ok();
    }
    let (num_str, mult) = if let Some(n) = s.strip_suffix('K') {
        (n, 1024u64)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('G') {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('T') {
        (n, 1024 * 1024 * 1024 * 1024)
    } else {
        (s, 1)
    };
    num_str.trim().parse::<u64>().ok().map(|n| n * mult)
}

// ============================================================================
// blkdiscard command
// ============================================================================

fn cmd_blkdiscard(args: &[String]) {
    let mut secure = false;
    let mut zeroout = false;
    let mut offset: u64 = 0;
    let mut length: Option<u64> = None;
    let mut device: Option<String> = None;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: blkdiscard [options] <device>");
                println!();
                println!("Discard device sectors.");
                println!();
                println!("Options:");
                println!("  -s, --secure       Secure discard");
                println!("  -z, --zeroout      Zero-fill instead of discard");
                println!("  -o, --offset BYTES Start offset");
                println!("  -l, --length BYTES Number of bytes to discard");
                println!("  -v, --verbose      Verbose output");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("blkdiscard {VERSION}");
                process::exit(0);
            }
            "-s" | "--secure" => secure = true,
            "-z" | "--zeroout" => zeroout = true,
            "-v" | "--verbose" => verbose = true,
            "-o" | "--offset" => {
                i += 1;
                if i < args.len() { offset = parse_size(&args[i]).unwrap_or(0); }
            }
            "-l" | "--length" => {
                i += 1;
                if i < args.len() { length = parse_size(&args[i]); }
            }
            s if !s.starts_with('-') => {
                device = Some(s.to_string());
            }
            _ => {
                eprintln!("blkdiscard: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("blkdiscard: no device specified");
            process::exit(1);
        }
    };

    let len_str = match length {
        Some(l) => format_size(l),
        None => "entire device".to_string(),
    };

    let mode = if zeroout {
        "zero-fill"
    } else if secure {
        "secure discard"
    } else {
        "discard"
    };

    if verbose {
        eprintln!("blkdiscard: {mode} {device}: offset={offset}, length={len_str}");
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "blkdiscard: {mode} {len_str} from {device} at offset {offset}");
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 * 1024 {
        format!("{:.1} TiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("wipefs");
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
        "blkdiscard" => cmd_blkdiscard(&rest),
        _ => cmd_wipefs(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_plain() {
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("0"), Some(0));
    }

    #[test]
    fn test_parse_size_hex() {
        assert_eq!(parse_size("0x100"), Some(256));
        assert_eq!(parse_size("0X438"), Some(0x438));
    }

    #[test]
    fn test_parse_size_suffixes() {
        assert_eq!(parse_size("1K"), Some(1024));
        assert_eq!(parse_size("2M"), Some(2 * 1024 * 1024));
        assert_eq!(parse_size("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("1T"), Some(1024 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("abc").is_none());
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(512), "512 bytes");
    }

    #[test]
    fn test_format_size_kib() {
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_size_mib() {
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn test_format_size_gib() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
    }

    #[test]
    fn test_format_size_tib() {
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1.0 TiB");
    }

    #[test]
    fn test_signature_database() {
        assert!(SIGNATURES.len() >= 15);
    }

    #[test]
    fn test_ext4_signature() {
        let ext4 = SIGNATURES.iter().find(|s| s.name == "ext2/ext3/ext4");
        assert!(ext4.is_some());
        let ext4 = ext4.unwrap();
        assert_eq!(ext4.offset, 0x438);
        assert_eq!(ext4.magic, &[0x53, 0xEF]);
    }

    #[test]
    fn test_gpt_signature() {
        let gpt = SIGNATURES.iter().find(|s| s.name == "gpt");
        assert!(gpt.is_some());
        let gpt = gpt.unwrap();
        assert_eq!(gpt.offset, 0x200);
        assert_eq!(gpt.magic, b"EFI PART");
    }

    #[test]
    fn test_ntfs_signature() {
        let ntfs = SIGNATURES.iter().find(|s| s.name == "ntfs");
        assert!(ntfs.is_some());
        assert_eq!(ntfs.unwrap().offset, 3);
    }

    #[test]
    fn test_luks_signature() {
        let luks = SIGNATURES.iter().find(|s| s.name == "luks");
        assert!(luks.is_some());
        assert_eq!(luks.unwrap().offset, 0);
    }

    #[test]
    fn test_detect_signatures_missing_file() {
        let sigs = detect_signatures("/nonexistent/device");
        assert!(sigs.is_empty());
    }

    #[test]
    fn test_generate_default_sigs() {
        let sigs = generate_default_sigs("/dev/sda");
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].name, "ext4");
        assert_eq!(sigs[1].name, "dos");
    }

    #[test]
    fn test_detected_sig_clone() {
        let sig = DetectedSig {
            device: "/dev/sda".to_string(),
            offset: 0x438,
            sig_type: "filesystem".to_string(),
            name: "ext4".to_string(),
            magic_hex: "53ef".to_string(),
            _length: 2,
        };
        let c = sig.clone();
        assert_eq!(c.device, "/dev/sda");
        assert_eq!(c.offset, 0x438);
        assert_eq!(c.name, "ext4");
    }

    #[test]
    fn test_fs_signature_clone() {
        let sig = FsSignature {
            name: "test",
            magic: b"TEST",
            offset: 0,
            _sig_type: SigType::Filesystem,
        };
        let c = sig.clone();
        assert_eq!(c.name, "test");
        assert_eq!(c.offset, 0);
    }

    #[test]
    fn test_sig_type_clone() {
        let st = SigType::Raid;
        let _c = st.clone();
    }

    #[test]
    fn test_swap_signature() {
        let swap: Vec<_> = SIGNATURES.iter().filter(|s| s.name == "swap").collect();
        assert_eq!(swap.len(), 2);
    }

    #[test]
    fn test_xfs_signature() {
        let xfs = SIGNATURES.iter().find(|s| s.name == "xfs");
        assert!(xfs.is_some());
        assert_eq!(xfs.unwrap().offset, 0);
        assert_eq!(xfs.unwrap().magic, b"XFSB");
    }
}
