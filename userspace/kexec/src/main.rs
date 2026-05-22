#![deny(clippy::all)]

//! kexec — OurOS kexec tools for fast kernel replacement and kdump
//!
//! Multi-personality binary for loading new kernels and crash dump setup.
//! Detected via argv[0]:
//!
//! - `kexec` (default) — load and execute a new kernel
//! - `kdump` — crash dump configuration and management
//! - `makedumpfile` — convert/filter kernel crash dumps
//! - `vmcore-dmesg` — extract dmesg from vmcore crash dumps

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _KEXEC_LOADED: &str = "/sys/kernel/kexec_loaded";
const _KEXEC_CRASH_LOADED: &str = "/sys/kernel/kexec_crash_loaded";
const _CRASH_KERNEL_PARAM: &str = "/proc/cmdline";
const _VMLINUZ_PATH: &str = "/boot/vmlinuz";
const _INITRD_PATH: &str = "/boot/initrd.img";
const _KDUMP_CONF: &str = "/etc/kdump.conf";
const _KDUMP_DIR: &str = "/var/crash";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct KexecImage {
    kernel: String,
    initrd: Option<String>,
    cmdline: String,
    _image_type: KexecType,
    loaded: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum KexecType {
    _Normal,
    Crash,
}

impl std::fmt::Display for KexecType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Normal => write!(f, "normal"),
            Self::Crash => write!(f, "crash"),
        }
    }
}

#[derive(Clone, Debug)]
struct KdumpConfig {
    _path: String,
    _core_collector: String,
    _default_action: String,
    _crash_size: String,
    _target: KdumpTarget,
}

#[derive(Clone, Debug)]
enum KdumpTarget {
    _Local(String),
    _Nfs(String),
    _Ssh(String),
}

impl std::fmt::Display for KdumpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Local(path) => write!(f, "local: {}", path),
            Self::_Nfs(addr) => write!(f, "nfs: {}", addr),
            Self::_Ssh(host) => write!(f, "ssh: {}", host),
        }
    }
}

#[derive(Clone, Debug)]
struct _CrashDump {
    _path: String,
    _timestamp: u64,
    _kernel_version: String,
    _size_bytes: u64,
    _compressed: bool,
}

#[derive(Clone, Debug)]
struct DumpFilter {
    zero_pages: bool,
    cache_pages: bool,
    user_pages: bool,
    free_pages: bool,
    _level: u32,
}

impl Default for DumpFilter {
    fn default() -> Self {
        Self {
            zero_pages: true,
            cache_pages: true,
            user_pages: false,
            free_pages: true,
            _level: 31,
        }
    }
}

// ── Simulated state ───────────────────────────────────────────────────

fn current_kexec_state() -> (Option<KexecImage>, Option<KexecImage>) {
    // Normal kexec image (not loaded)
    let normal: Option<KexecImage> = None;

    // Crash kernel (simulated as loaded)
    let crash = Some(KexecImage {
        kernel: "/boot/vmlinuz-6.1.0-ouros".to_string(),
        initrd: Some("/boot/initrd.img-6.1.0-ouros".to_string()),
        cmdline: "root=/dev/sda2 ro crashkernel=256M irqpoll maxcpus=1 reset_devices".to_string(),
        _image_type: KexecType::Crash,
        loaded: true,
    });

    (normal, crash)
}

fn default_kdump_config() -> KdumpConfig {
    KdumpConfig {
        _path: _KDUMP_CONF.to_string(),
        _core_collector: "makedumpfile -l --message-level 7 -d 31".to_string(),
        _default_action: "reboot".to_string(),
        _crash_size: "256M".to_string(),
        _target: KdumpTarget::_Local(_KDUMP_DIR.to_string()),
    }
}

fn list_crash_dumps() -> Vec<(String, String, u64)> {
    vec![
        ("/var/crash/2025-05-20-142355".to_string(), "6.1.0-ouros".to_string(), 128_000_000),
        ("/var/crash/2025-05-15-091200".to_string(), "6.1.0-ouros".to_string(), 256_000_000),
    ]
}

// ── kexec personality ─────────────────────────────────────────────────

fn run_kexec(args: Vec<String>) -> i32 {
    if args.is_empty() {
        print_kexec_help();
        return 0;
    }

    let cmd = args.first().cloned().unwrap_or_default();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            print_kexec_help();
            0
        }
        "--version" | "-V" => {
            println!("kexec-tools 0.1.0 (OurOS)");
            0
        }
        "-l" | "--load" => kexec_load(&args),
        "-p" | "--load-panic" => kexec_load_panic(&args),
        "-u" | "--unload" => kexec_unload(&args),
        "-e" | "--exec" => kexec_exec(),
        "-s" | "--status" => kexec_status(),
        other => {
            // If first arg is a path, treat as kernel image
            if !other.starts_with('-') {
                kexec_load(&args)
            } else {
                eprintln!("kexec: unknown option '{}'", other);
                1
            }
        }
    }
}

fn print_kexec_help() {
    println!("Usage: kexec [OPTIONS] [KERNEL]");
    println!();
    println!("Load and execute a new kernel, or set up crash kernel.");
    println!();
    println!("Options:");
    println!("  -l, --load KERNEL       Load a new kernel for later execution");
    println!("  -p, --load-panic KERNEL Load crash kernel (kdump)");
    println!("  -u, --unload            Unload the loaded kernel");
    println!("  -e, --exec              Execute the loaded kernel");
    println!("  -s, --status            Show kexec/kdump status");
    println!("  --initrd=FILE           Specify initrd/initramfs");
    println!("  --append=STRING         Append to kernel command line");
    println!("  --reuse-cmdline         Use current kernel's command line");
    println!("  --version               Show version");
}

fn kexec_load(args: &[String]) -> i32 {
    let mut kernel: Option<&str> = None;
    let mut initrd: Option<&str> = None;
    let mut append: Option<&str> = None;
    let mut reuse_cmdline = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-l" | "--load" => {}
            "--reuse-cmdline" => reuse_cmdline = true,
            s if s.starts_with("--initrd=") => {
                initrd = Some(&s[9..]);
            }
            s if s.starts_with("--append=") => {
                append = Some(&s[9..]);
            }
            s if !s.starts_with('-') => {
                kernel = Some(s);
            }
            _ => {}
        }
        i += 1;
    }

    let kernel = match kernel {
        Some(k) => k,
        None => {
            eprintln!("kexec: no kernel image specified");
            return 1;
        }
    };

    println!("kexec: loading kernel image: {}", kernel);
    if let Some(rd) = initrd {
        println!("  initrd: {}", rd);
    }
    if let Some(cmd) = append {
        println!("  append: {}", cmd);
    }
    if reuse_cmdline {
        println!("  reusing current kernel command line");
    }
    println!();
    println!("kexec: kernel loaded successfully (simulated)");
    println!("  Use 'kexec -e' to execute, or reboot to trigger");
    0
}

fn kexec_load_panic(args: &[String]) -> i32 {
    let kernel = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(_VMLINUZ_PATH);

    println!("kexec: loading crash kernel: {}", kernel);
    println!("  Crash memory reserved: 256M");
    println!("  Kernel command line: irqpoll maxcpus=1 reset_devices");
    println!();
    println!("kexec: crash kernel loaded successfully (simulated)");
    println!("  System will capture vmcore on kernel panic/oops");
    0
}

fn kexec_unload(args: &[String]) -> i32 {
    let panic_mode = args.iter().any(|a| a == "-p" || a == "--load-panic");

    if panic_mode {
        println!("kexec: unloading crash kernel");
    } else {
        println!("kexec: unloading kexec kernel");
    }
    println!("kexec: kernel unloaded (simulated)");
    0
}

fn kexec_exec() -> i32 {
    let (normal, _) = current_kexec_state();
    if normal.is_none() {
        eprintln!("kexec: no kernel loaded for execution");
        eprintln!("  Load a kernel first with 'kexec -l <kernel>'");
        return 1;
    }
    println!("kexec: executing loaded kernel...");
    println!("  WARNING: This would reboot into the new kernel!");
    println!("  (simulated — not actually rebooting)");
    0
}

fn kexec_status() -> i32 {
    let (normal, crash) = current_kexec_state();

    println!("Kexec Status");
    println!("============");
    println!();

    match normal {
        Some(ref img) if img.loaded => {
            println!("Kexec kernel: loaded");
            println!("  Kernel: {}", img.kernel);
            if let Some(ref rd) = img.initrd {
                println!("  Initrd: {}", rd);
            }
            println!("  Cmdline: {}", img.cmdline);
        }
        _ => {
            println!("Kexec kernel: not loaded");
        }
    }
    println!();

    match crash {
        Some(ref img) if img.loaded => {
            println!("Crash kernel: loaded");
            println!("  Kernel: {}", img.kernel);
            if let Some(ref rd) = img.initrd {
                println!("  Initrd: {}", rd);
            }
            println!("  Cmdline: {}", img.cmdline);
        }
        _ => {
            println!("Crash kernel: not loaded");
        }
    }
    0
}

// ── kdump personality ─────────────────────────────────────────────────

fn run_kdump(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "status".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: kdump [COMMAND]");
            println!();
            println!("Crash dump configuration and management.");
            println!();
            println!("Commands:");
            println!("  status          Show kdump status (default)");
            println!("  start           Enable kdump (load crash kernel)");
            println!("  stop            Disable kdump (unload crash kernel)");
            println!("  restart         Restart kdump");
            println!("  config          Show kdump configuration");
            println!("  list            List captured crash dumps");
            println!("  propagate       Propagate kdump initramfs to all kernels");
            println!("  estimate        Estimate memory needed for crash kernel");
            0
        }
        "status" => kdump_status(),
        "start" => {
            println!("kdump: loading crash kernel...");
            println!("kdump: crash kernel loaded");
            println!("kdump: service started");
            0
        }
        "stop" => {
            println!("kdump: unloading crash kernel...");
            println!("kdump: crash kernel unloaded");
            println!("kdump: service stopped");
            0
        }
        "restart" => {
            println!("kdump: restarting...");
            println!("kdump: crash kernel unloaded");
            println!("kdump: crash kernel reloaded");
            println!("kdump: service restarted");
            0
        }
        "config" => kdump_config(),
        "list" => kdump_list(),
        "propagate" => {
            println!("kdump: propagating initramfs to all installed kernels...");
            println!("  Updated: /boot/initrd-kdump.img-6.1.0-ouros");
            println!("kdump: propagation complete");
            0
        }
        "estimate" => kdump_estimate(),
        other => {
            eprintln!("kdump: unknown command '{}'", other);
            1
        }
    }
}

fn kdump_status() -> i32 {
    let (_, crash) = current_kexec_state();

    println!("Kdump Status");
    println!("============");
    println!();
    println!("Service: {}", if crash.is_some() { "active" } else { "inactive" });
    println!("Crash kernel: {}", if crash.is_some() { "loaded" } else { "not loaded" });
    println!("Crash memory reserved: 256M");
    println!("Default action: reboot");
    println!("Dump target: {}", _KDUMP_DIR);
    println!("Core collector: makedumpfile -l --message-level 7 -d 31");
    0
}

fn kdump_config() -> i32 {
    let config = default_kdump_config();
    println!("Kdump Configuration");
    println!("===================");
    println!();
    println!("Config file: {}", config._path);
    println!("Target: {}", config._target);
    println!("Core collector: {}", config._core_collector);
    println!("Default action: {}", config._default_action);
    println!("Crash size: {}", config._crash_size);
    0
}

fn kdump_list() -> i32 {
    let dumps = list_crash_dumps();
    if dumps.is_empty() {
        println!("No crash dumps found.");
        return 0;
    }

    println!("Crash Dumps");
    println!("===========");
    println!();
    println!("{:<40} {:<20} {:>12}",
        "Path", "Kernel", "Size");
    println!("{}", "-".repeat(75));

    for (path, kernel, size) in &dumps {
        let size_str = if *size >= 1_000_000_000 {
            format!("{:.1} GB", *size as f64 / 1_000_000_000.0)
        } else {
            format!("{:.1} MB", *size as f64 / 1_000_000.0)
        };
        println!("{:<40} {:<20} {:>12}", path, kernel, size_str);
    }
    0
}

fn kdump_estimate() -> i32 {
    println!("Kdump Memory Estimation");
    println!("=======================");
    println!();
    println!("System memory:     32 GB");
    println!("Recommended crash: 256 MB");
    println!("  Base requirement:  160 MB");
    println!("  Per-CPU overhead:  8 MB (8 CPUs x 1 MB)");
    println!("  Driver overhead:   32 MB");
    println!("  Safety margin:     56 MB");
    println!();
    println!("Add 'crashkernel=256M' to kernel command line");
    0
}

// ── makedumpfile personality ──────────────────────────────────────────

fn run_makedumpfile(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: makedumpfile [OPTIONS] VMCORE DUMPFILE");
            println!();
            println!("Create a filtered/compressed dump from a crash vmcore.");
            println!();
            println!("Options:");
            println!("  -l              Compress with lzo");
            println!("  -p              Compress with snappy");
            println!("  -z              Compress with zlib");
            println!("  -d LEVEL        Dump level (filter pages):");
            println!("                    1  = exclude zero pages");
            println!("                    2  = exclude cache pages");
            println!("                    4  = exclude cache private pages");
            println!("                    8  = exclude user pages");
            println!("                    16 = exclude free pages");
            println!("                    31 = exclude all filterable (recommended)");
            println!("  --message-level LEVEL  Verbosity (0-31)");
            println!("  --mem-usage     Show estimated memory usage");
            println!("  --split         Split output into multiple files");
            println!("  --dry-run       Show what would be filtered");
            println!("  --version       Show version");
            0
        }
        "--version" | "-V" => {
            println!("makedumpfile 0.1.0 (OurOS)");
            0
        }
        "--mem-usage" => makedumpfile_mem_usage(),
        "--dry-run" => makedumpfile_dry_run(&args),
        _ => makedumpfile_convert(&args),
    }
}

fn makedumpfile_convert(args: &[String]) -> i32 {
    let mut dump_level: u32 = 31;
    let mut compress = "lzo";
    let mut _vmcore: Option<&str> = None;
    let mut _dumpfile: Option<&str> = None;

    let mut i = 0;
    let mut positional = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-d" => {
                if let Some(next) = args.get(i + 1) {
                    dump_level = next.parse().unwrap_or(31);
                    i += 1;
                }
            }
            "-l" => compress = "lzo",
            "-p" => compress = "snappy",
            "-z" => compress = "zlib",
            "--message-level" => { i += 1; }
            s if !s.starts_with('-') => {
                match positional {
                    0 => _vmcore = Some(s),
                    1 => _dumpfile = Some(s),
                    _ => {}
                }
                positional += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let filter = filter_from_level(dump_level);

    println!("makedumpfile: processing vmcore");
    println!("  Compression: {}", compress);
    println!("  Dump level: {} (filter: zero={} cache={} user={} free={})",
        dump_level, filter.zero_pages, filter.cache_pages,
        filter.user_pages, filter.free_pages);
    println!();
    println!("  Filtering pages...");
    println!("    Total pages:     2097152 (32 GB)");
    println!("    Zero pages:      1048576 (excluded)");
    println!("    Cache pages:      524288 (excluded)");
    println!("    Free pages:       262144 (excluded)");
    println!("    Remaining:        262144 (4 GB)");
    println!();
    println!("  Compressing...");
    println!("    Compressed size: 1.2 GB");
    println!();
    println!("makedumpfile: done (simulated)");
    0
}

fn filter_from_level(level: u32) -> DumpFilter {
    DumpFilter {
        zero_pages: level & 1 != 0,
        cache_pages: level & 2 != 0,
        user_pages: level & 8 != 0,
        free_pages: level & 16 != 0,
        _level: level,
    }
}

fn makedumpfile_mem_usage() -> i32 {
    println!("makedumpfile: estimated memory usage");
    println!();
    println!("  System memory:    32 GB");
    println!("  Bitmap size:      4 MB");
    println!("  Cycle buffer:     128 MB");
    println!("  Compression buf:  64 MB");
    println!("  Total required:   ~196 MB");
    println!();
    println!("  Recommended crashkernel= : 256M");
    0
}

fn makedumpfile_dry_run(args: &[String]) -> i32 {
    let dump_level: u32 = args.iter()
        .position(|a| a == "-d")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(31);

    let filter = filter_from_level(dump_level);

    println!("makedumpfile: dry run (dump level {})", dump_level);
    println!();
    println!("Pages that would be excluded:");
    if filter.zero_pages { println!("  [x] Zero-filled pages"); }
    else { println!("  [ ] Zero-filled pages"); }
    if filter.cache_pages { println!("  [x] Cache pages"); }
    else { println!("  [ ] Cache pages"); }
    if filter.user_pages { println!("  [x] User data pages"); }
    else { println!("  [ ] User data pages"); }
    if filter.free_pages { println!("  [x] Free pages"); }
    else { println!("  [ ] Free pages"); }

    println!();
    println!("Estimated reduction: ~87% (32 GB -> ~4 GB before compression)");
    0
}

// ── vmcore-dmesg personality ──────────────────────────────────────────

fn run_vmcore_dmesg(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: vmcore-dmesg [OPTIONS] VMCORE");
            println!();
            println!("Extract dmesg log from a kernel crash dump (vmcore).");
            println!();
            println!("Options:");
            println!("  -o FILE    Write output to FILE (default: stdout)");
            println!("  --version  Show version");
            0
        }
        "--version" | "-V" => {
            println!("vmcore-dmesg 0.1.0 (OurOS)");
            0
        }
        _ => {
            // Simulate extracting dmesg from vmcore
            println!("vmcore-dmesg: extracting kernel log from crash dump");
            println!();
            println!("[    0.000000] Linux version 6.1.0-ouros (build@ouros) (gcc 13.2.0)");
            println!("[    0.000000] Command line: root=/dev/sda2 ro crashkernel=256M");
            println!("[    0.000000] BIOS-provided physical RAM map:");
            println!("[    0.000000]  BIOS-e820: [mem 0x0000000000000000-0x000000000009ffff] usable");
            println!("[    0.000000]  BIOS-e820: [mem 0x0000000000100000-0x00000007ffffffff] usable");
            println!("[    1.234567] Memory: 32768MB total");
            println!("[    2.345678] smpboot: Estimated 8 CPUs");
            println!("...");
            println!("[  123.456789] BUG: unable to handle page fault for address: 0xdead0000beef");
            println!("[  123.456789] RIP: 0010:some_kernel_function+0x42/0x100");
            println!("[  123.456789] Call Trace:");
            println!("[  123.456789]  caller_function+0x1a/0x30");
            println!("[  123.456789]  sys_something+0x80/0xf0");
            println!("[  123.456789]  entry_SYSCALL_64+0x5c/0xa0");
            println!("[  123.456790] Kernel panic - not syncing: Fatal exception");
            println!();
            println!("vmcore-dmesg: extracted {} bytes of kernel log", 4096);
            0
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("kexec");
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
        "kdump" => run_kdump(rest),
        "makedumpfile" => run_makedumpfile(rest),
        "vmcore-dmesg" => run_vmcore_dmesg(rest),
        _ => run_kexec(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kexec_state() {
        let (normal, crash) = current_kexec_state();
        assert!(normal.is_none());
        assert!(crash.is_some());
        let crash = crash.unwrap();
        assert!(crash.loaded);
        assert_eq!(crash._image_type, KexecType::Crash);
    }

    #[test]
    fn test_kexec_type_display() {
        assert_eq!(format!("{}", KexecType::_Normal), "normal");
        assert_eq!(format!("{}", KexecType::Crash), "crash");
    }

    #[test]
    fn test_kdump_target_display() {
        let local = KdumpTarget::_Local("/var/crash".to_string());
        assert!(format!("{}", local).contains("/var/crash"));

        let nfs = KdumpTarget::_Nfs("server:/exports/crash".to_string());
        assert!(format!("{}", nfs).contains("nfs:"));

        let ssh = KdumpTarget::_Ssh("user@crashserver".to_string());
        assert!(format!("{}", ssh).contains("ssh:"));
    }

    #[test]
    fn test_filter_level_0() {
        let f = filter_from_level(0);
        assert!(!f.zero_pages);
        assert!(!f.cache_pages);
        assert!(!f.user_pages);
        assert!(!f.free_pages);
    }

    #[test]
    fn test_filter_level_31() {
        let f = filter_from_level(31);
        assert!(f.zero_pages);
        assert!(f.cache_pages);
        assert!(f.user_pages);
        assert!(f.free_pages);
    }

    #[test]
    fn test_filter_level_1() {
        let f = filter_from_level(1);
        assert!(f.zero_pages);
        assert!(!f.cache_pages);
        assert!(!f.user_pages);
        assert!(!f.free_pages);
    }

    #[test]
    fn test_filter_level_17() {
        // 17 = 16 + 1 = free + zero
        let f = filter_from_level(17);
        assert!(f.zero_pages);
        assert!(!f.cache_pages);
        assert!(!f.user_pages);
        assert!(f.free_pages);
    }

    #[test]
    fn test_default_kdump_config() {
        let config = default_kdump_config();
        assert!(config._core_collector.contains("makedumpfile"));
        assert_eq!(config._default_action, "reboot");
    }

    #[test]
    fn test_list_crash_dumps() {
        let dumps = list_crash_dumps();
        assert_eq!(dumps.len(), 2);
        assert!(dumps[0].0.contains("2025-05-20"));
    }

    #[test]
    fn test_dump_filter_default() {
        let f = DumpFilter::default();
        assert!(f.zero_pages);
        assert!(f.cache_pages);
        assert!(!f.user_pages);
        assert!(f.free_pages);
        assert_eq!(f._level, 31);
    }
}
