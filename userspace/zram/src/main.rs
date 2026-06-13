#![deny(clippy::all)]

//! zram — SlateOS compressed RAM block device management
//!
//! Multi-personality binary for managing zram (compressed swap in RAM).
//! Detected via argv[0]:
//!
//! - `zramctl` (default) — manage zram devices
//! - `zram-generator` — automatic zram setup from config

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _ZRAM_SYS_DIR: &str = "/sys/block";
const _ZRAM_CONF: &str = "/etc/systemd/zram-generator.conf";
const _ZRAM_MODULE: &str = "/sys/class/zram-control";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ZramDevice {
    name: String,
    disksize: u64,
    algorithm: CompressionAlgo,
    orig_data_size: u64,
    compr_data_size: u64,
    mem_used_total: u64,
    _mem_limit: u64,
    _mem_used_max: u64,
    zero_pages: u64,
    _same_pages: u64,
    num_reads: u64,
    num_writes: u64,
    _back_dev: Option<String>,
    streams: u32,
    _mountpoint: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CompressionAlgo {
    Lzo,
    LzoRle,
    Lz4,
    Lz4hc,
    Zstd,
    _Deflate,
    _842,
}

impl std::fmt::Display for CompressionAlgo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lzo => write!(f, "lzo"),
            Self::LzoRle => write!(f, "lzo-rle"),
            Self::Lz4 => write!(f, "lz4"),
            Self::Lz4hc => write!(f, "lz4hc"),
            Self::Zstd => write!(f, "zstd"),
            Self::_Deflate => write!(f, "deflate"),
            Self::_842 => write!(f, "842"),
        }
    }
}

impl CompressionAlgo {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "lzo" => Some(Self::Lzo),
            "lzo-rle" => Some(Self::LzoRle),
            "lz4" => Some(Self::Lz4),
            "lz4hc" => Some(Self::Lz4hc),
            "zstd" => Some(Self::Zstd),
            "deflate" => Some(Self::_Deflate),
            "842" => Some(Self::_842),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct ZramGenConfig {
    _zram_fraction: f64,
    _max_zram_size_mb: Option<u64>,
    _compression_algorithm: CompressionAlgo,
    _swap_priority: i32,
    _num_devices: u32,
}

impl Default for ZramGenConfig {
    fn default() -> Self {
        Self {
            _zram_fraction: 0.5,
            _max_zram_size_mb: Some(8192),
            _compression_algorithm: CompressionAlgo::Zstd,
            _swap_priority: 100,
            _num_devices: 1,
        }
    }
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_zram_devices() -> Vec<ZramDevice> {
    vec![
        ZramDevice {
            name: "zram0".to_string(),
            disksize: 8_589_934_592,  // 8 GiB
            algorithm: CompressionAlgo::Zstd,
            orig_data_size: 4_294_967_296,  // 4 GiB uncompressed
            compr_data_size: 1_073_741_824, // 1 GiB compressed (4:1)
            mem_used_total: 1_107_296_256,  // slightly more than compressed
            _mem_limit: 0,
            _mem_used_max: 1_200_000_000,
            zero_pages: 262_144,
            _same_pages: 65_536,
            num_reads: 12_345_678,
            num_writes: 9_876_543,
            _back_dev: None,
            streams: 8,
            _mountpoint: Some("[SWAP]".to_string()),
        },
    ]
}

fn available_algorithms() -> Vec<&'static str> {
    vec!["lzo", "lzo-rle", "lz4", "lz4hc", "zstd", "deflate", "842"]
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

// ── zramctl personality ───────────────────────────────────────────────

fn run_zramctl(args: Vec<String>) -> i32 {
    if args.is_empty() {
        return zramctl_list(false);
    }

    let cmd = args.first().cloned().unwrap_or_default();
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: zramctl [OPTIONS] [DEVICE]");
            println!();
            println!("Manage zram (compressed RAM) devices.");
            println!();
            println!("Options:");
            println!("  (no args)            List all zram devices (default)");
            println!("  -f, --find           Find first unused zram device");
            println!("  -s, --size SIZE      Set disk size (e.g., 4G, 512M)");
            println!("  -a, --algorithm ALG  Set compression algorithm");
            println!("  -t, --streams N      Set number of compression streams");
            println!("  -o, --output COLS    Output columns (NAME,DISKSIZE,ALGORITHM,...)");
            println!("  --raw                Machine-readable output");
            println!("  -r, --reset DEVICE   Reset a zram device");
            println!("  --algorithms         Show available compression algorithms");
            println!("  --version            Show version");
            0
        }
        "--version" | "-V" => {
            println!("zramctl 0.1.0 (SlateOS)");
            0
        }
        "--algorithms" | "algorithms" => {
            println!("Available compression algorithms:");
            for algo in available_algorithms() {
                let current = if algo == "zstd" { " (current default)" } else { "" };
                println!("  {}{}", algo, current);
            }
            0
        }
        "-f" | "--find" => {
            println!("/dev/zram1");
            0
        }
        "-r" | "--reset" => {
            let dev = cmd_args.first().map(|s| s.as_str()).unwrap_or("zram0");
            println!("zramctl: resetting /dev/{}", dev);
            println!("Reset complete");
            0
        }
        "--raw" => zramctl_list(true),
        s if s.starts_with("-s") || s == "--size" => {
            zramctl_setup(&cmd, &cmd_args)
        }
        s if s.starts_with("/dev/") || s.starts_with("zram") => {
            zramctl_show_device(s)
        }
        _ => {
            // Try as device name or setup flags
            zramctl_setup(&cmd, &cmd_args)
        }
    }
}

fn zramctl_list(raw: bool) -> i32 {
    let devices = read_zram_devices();

    if devices.is_empty() {
        if !raw {
            println!("No zram devices found.");
        }
        return 0;
    }

    if raw {
        for dev in &devices {
            println!("{} {} {} {} {} {} {}",
                dev.name, dev.disksize, dev.algorithm,
                dev.orig_data_size, dev.compr_data_size,
                dev.mem_used_total, dev.streams);
        }
    } else {
        println!("{:<8} {:<10} {:<10} {:<10} {:<10} {:<10} {:<8} MOUNTPOINT",
            "NAME", "ALGORITHM", "DISKSIZE", "DATA", "COMPR", "TOTAL", "STREAMS");
        for dev in &devices {
            let ratio = if dev.compr_data_size > 0 {
                format!("{:.1}:1", dev.orig_data_size as f64 / dev.compr_data_size as f64)
            } else {
                "N/A".to_string()
            };
            println!("{:<8} {:<10} {:<10} {:<10} {:<10} {:<10} {:<8} {} ({})",
                format!("/dev/{}", dev.name),
                dev.algorithm,
                format_bytes(dev.disksize),
                format_bytes(dev.orig_data_size),
                format_bytes(dev.compr_data_size),
                format_bytes(dev.mem_used_total),
                dev.streams,
                dev._mountpoint.as_deref().unwrap_or("-"),
                ratio);
        }
    }
    0
}

fn zramctl_show_device(dev_name: &str) -> i32 {
    let devices = read_zram_devices();
    let clean_name = dev_name.strip_prefix("/dev/").unwrap_or(dev_name);

    match devices.iter().find(|d| d.name == clean_name) {
        Some(dev) => {
            println!("Device: /dev/{}", dev.name);
            println!("  Disk size:       {}", format_bytes(dev.disksize));
            println!("  Algorithm:       {}", dev.algorithm);
            println!("  Streams:         {}", dev.streams);
            println!("  Original data:   {}", format_bytes(dev.orig_data_size));
            println!("  Compressed data: {}", format_bytes(dev.compr_data_size));
            println!("  Memory used:     {}", format_bytes(dev.mem_used_total));
            if dev.compr_data_size > 0 {
                println!("  Compression:     {:.1}:1",
                    dev.orig_data_size as f64 / dev.compr_data_size as f64);
            }
            println!("  Zero pages:      {}", dev.zero_pages);
            println!("  Reads:           {}", dev.num_reads);
            println!("  Writes:          {}", dev.num_writes);
            if let Some(ref mp) = dev._mountpoint {
                println!("  Mount point:     {}", mp);
            }
            0
        }
        None => {
            eprintln!("zramctl: /dev/{}: not found", clean_name);
            1
        }
    }
}

fn zramctl_setup(first_arg: &str, args: &[String]) -> i32 {
    let mut device = "zram0";
    let mut size: Option<&str> = None;
    let mut algo: Option<&str> = None;
    let mut streams: Option<&str> = None;

    // Parse mixed args
    let all_args: Vec<&str> = std::iter::once(first_arg).chain(args.iter().map(|s| s.as_str())).collect();
    let mut i = 0;
    while i < all_args.len() {
        match all_args[i] {
            "-s" | "--size" => {
                if let Some(&val) = all_args.get(i + 1) { size = Some(val); i += 1; }
            }
            "-a" | "--algorithm" => {
                if let Some(&val) = all_args.get(i + 1) { algo = Some(val); i += 1; }
            }
            "-t" | "--streams" => {
                if let Some(&val) = all_args.get(i + 1) { streams = Some(val); i += 1; }
            }
            s if s.starts_with("/dev/") || s.starts_with("zram") => {
                device = s;
            }
            _ => {}
        }
        i += 1;
    }

    println!("zramctl: setting up /dev/{}", device);
    if let Some(s) = size { println!("  Size: {}", s); }
    if let Some(a) = algo {
        if CompressionAlgo::from_str(a).is_none() {
            eprintln!("zramctl: unknown algorithm '{}'", a);
            return 1;
        }
        println!("  Algorithm: {}", a);
    }
    if let Some(t) = streams { println!("  Streams: {}", t); }
    println!("Device configured (simulated)");
    0
}

// ── zram-generator personality ────────────────────────────────────────

fn run_generator(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "run".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: zram-generator [OPTIONS]");
            println!();
            println!("Automatic zram device setup from configuration.");
            println!();
            println!("Options:");
            println!("  run           Apply zram configuration (default)");
            println!("  show-config   Show current configuration");
            println!("  status        Show zram status");
            println!("  --version     Show version");
            0
        }
        "--version" | "-V" => {
            println!("zram-generator 0.1.0 (SlateOS)");
            0
        }
        "run" => generator_run(),
        "show-config" => generator_show_config(),
        "status" => {
            zramctl_list(false)
        }
        other => {
            eprintln!("zram-generator: unknown command '{}'", other);
            1
        }
    }
}

fn generator_run() -> i32 {
    let config = ZramGenConfig::default();
    let total_ram_gb = 32; // simulated

    let zram_size_mb = std::cmp::min(
        (total_ram_gb as f64 * 1024.0 * config._zram_fraction) as u64,
        config._max_zram_size_mb.unwrap_or(u64::MAX),
    );

    println!("zram-generator: configuring zram devices");
    println!("  System RAM: {} GB", total_ram_gb);
    println!("  Fraction: {:.0}%", config._zram_fraction * 100.0);
    println!("  Max size: {} MB", config._max_zram_size_mb.unwrap_or(0));
    println!("  Computed size: {} MB", zram_size_mb);
    println!("  Algorithm: {}", config._compression_algorithm);
    println!("  Swap priority: {}", config._swap_priority);
    println!();
    println!("zram-generator: creating zram0 with {} MB, algorithm {}", zram_size_mb, config._compression_algorithm);
    println!("zram-generator: mkswap /dev/zram0");
    println!("zram-generator: swapon -p {} /dev/zram0", config._swap_priority);
    println!("zram-generator: done");
    0
}

fn generator_show_config() -> i32 {
    let config = ZramGenConfig::default();
    println!("zram-generator configuration:");
    println!("  Config file: {}", _ZRAM_CONF);
    println!("  zram-fraction: {:.1}", config._zram_fraction);
    println!("  max-zram-size: {} MB", config._max_zram_size_mb.unwrap_or(0));
    println!("  compression-algorithm: {}", config._compression_algorithm);
    println!("  swap-priority: {}", config._swap_priority);
    println!("  num-devices: {}", config._num_devices);
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("zramctl");
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

    let code = match prog_name.as_str() {
        "zram-generator" => run_generator(rest),
        _ => run_zramctl(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_zram_devices() {
        let devs = read_zram_devices();
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].name, "zram0");
        assert_eq!(devs[0].algorithm, CompressionAlgo::Zstd);
    }

    #[test]
    fn test_compression_ratio() {
        let dev = &read_zram_devices()[0];
        let ratio = dev.orig_data_size as f64 / dev.compr_data_size as f64;
        assert!(ratio > 3.0 && ratio < 5.0);
    }

    #[test]
    fn test_algo_display() {
        assert_eq!(format!("{}", CompressionAlgo::Zstd), "zstd");
        assert_eq!(format!("{}", CompressionAlgo::Lz4), "lz4");
        assert_eq!(format!("{}", CompressionAlgo::LzoRle), "lzo-rle");
    }

    #[test]
    fn test_algo_from_str() {
        assert_eq!(CompressionAlgo::from_str("zstd"), Some(CompressionAlgo::Zstd));
        assert_eq!(CompressionAlgo::from_str("lz4"), Some(CompressionAlgo::Lz4));
        assert_eq!(CompressionAlgo::from_str("lzo-rle"), Some(CompressionAlgo::LzoRle));
        assert_eq!(CompressionAlgo::from_str("invalid"), None);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(1024), "1.0K");
        assert_eq!(format_bytes(1_048_576), "1.0M");
        assert_eq!(format_bytes(1_073_741_824), "1.0G");
    }

    #[test]
    fn test_available_algorithms() {
        let algos = available_algorithms();
        assert!(algos.len() >= 5);
        assert!(algos.contains(&"zstd"));
        assert!(algos.contains(&"lz4"));
        assert!(algos.contains(&"lzo"));
    }

    #[test]
    fn test_default_generator_config() {
        let config = ZramGenConfig::default();
        assert!((config._zram_fraction - 0.5).abs() < 0.001);
        assert_eq!(config._compression_algorithm, CompressionAlgo::Zstd);
        assert_eq!(config._swap_priority, 100);
    }

    #[test]
    fn test_device_fields() {
        let dev = &read_zram_devices()[0];
        assert!(dev.disksize > 0);
        assert!(dev.orig_data_size > 0);
        assert!(dev.compr_data_size > 0);
        assert!(dev.mem_used_total >= dev.compr_data_size);
        assert!(dev.streams > 0);
        assert!(dev.num_reads > 0);
    }
}
