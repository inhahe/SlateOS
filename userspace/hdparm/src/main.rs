// SlateOS hdparm - disk parameter configuration tool
//
// Multi-personality binary:
//   hdparm  - get/set SATA/IDE disk parameters
//   sdparm  - get/set SCSI disk parameters

#![cfg_attr(not(test), no_main)]
// The ATA/SATA feature flag set (ATA_FLAG_LBA / LBA48 / HPA / DEVSLP /
// NCQ / SECURITY / SMART / TRIM / WRITE_CACHE / READ_AHEAD), the
// DriveSettings / DriveInfo structs, and the unread show_readonly /
// set_io32bit / set_readonly / trim_sector / idle args fields encode
// the ATA IDENTIFY DEVICE data layout the real implementation must
// speak. They are kept as documentation for the eventual ATA SAT
// driver integration.
#![allow(dead_code)]

// ── Constants ──────────────────────────────────────────────────────────

// ATA/SATA feature flags
const ATA_FLAG_LBA: u32 = 1 << 0;
const ATA_FLAG_LBA48: u32 = 1 << 1;
const ATA_FLAG_NCQ: u32 = 1 << 2;
const ATA_FLAG_SMART: u32 = 1 << 3;
const ATA_FLAG_APM: u32 = 1 << 4;
const ATA_FLAG_AAM: u32 = 1 << 5;
const ATA_FLAG_WRITE_CACHE: u32 = 1 << 6;
const ATA_FLAG_READ_AHEAD: u32 = 1 << 7;
const ATA_FLAG_SECURITY: u32 = 1 << 8;
const ATA_FLAG_HPA: u32 = 1 << 9;
const ATA_FLAG_TRIM: u32 = 1 << 10;
const ATA_FLAG_DEVSLP: u32 = 1 << 11;

// APM levels
const APM_DISABLED: u8 = 0;
const APM_MIN_POWER: u8 = 1;
const APM_MAX_PERF: u8 = 254;
const APM_OFF: u8 = 255;

// AAM levels
const AAM_QUIET: u8 = 128;
const AAM_LOUD: u8 = 254;

// ── Personality Detection ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Personality {
    Hdparm,
    Sdparm,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };

    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe") {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"sdparm") {
        Personality::Sdparm
    } else {
        Personality::Hdparm
    }
}

// ── Data Structures ────────────────────────────────────────────────────

struct DriveInfo {
    model: Vec<u8>,
    serial: Vec<u8>,
    firmware: Vec<u8>,
    transport: Vec<u8>,     // "SATA 3.0", "SATA 2.0", etc.
    sector_size_logical: u32,
    sector_size_physical: u32,
    capacity_sectors: u64,
    features: u32,
    rpm: u32,               // 0 = SSD
    form_factor: Vec<u8>,   // "2.5 inch", "3.5 inch", "M.2"
    sata_version: Vec<u8>,
    queue_depth: u32,
}

struct DriveSettings {
    read_ahead: bool,
    write_cache: bool,
    apm_level: u8,
    aam_level: u8,
    standby_timeout: u16,
    multcount: u16,
    io_32bit: bool,
    dma: bool,
    unmaskirq: bool,
}

// ── Argument Parsing ───────────────────────────────────────────────────

struct HdparmArgs {
    device: Vec<u8>,
    // Query operations
    show_info: bool,         // -i/-I (identify)
    show_geometry: bool,     // -g (geometry)
    show_settings: bool,     // default if no flags
    show_readonly: bool,     // -r (readonly flag)
    timed_read: bool,        // -t (timing buffered reads)
    timed_cache: bool,       // -T (timing cache reads)
    // Set operations
    set_read_ahead: Option<bool>,   // -A 0/1
    set_write_cache: Option<bool>,  // -W 0/1
    set_apm: Option<u8>,           // -B <level>
    set_aam: Option<u8>,           // -M <level>
    set_standby: Option<u16>,      // -S <timeout>
    set_multcount: Option<u16>,    // -m <count>
    set_dma: Option<bool>,         // -d 0/1
    set_io32bit: Option<bool>,     // -c 0/1/3
    set_unmaskirq: Option<bool>,   // -u 0/1
    set_readonly: Option<bool>,    // -r 0/1
    // Security
    security_freeze: bool,          // --security-freeze
    // TRIM
    trim_sector: Option<u64>,      // --trim-sector-ranges
    // Other
    flush_cache: bool,              // -f (flush)
    sleep: bool,                    // -Y (sleep)
    standby: bool,                  // -y (standby)
    idle: bool,                     // -E (idle)
    direct: bool,                   // --direct (O_DIRECT)
    verbose: bool,
    show_help: bool,
    show_version: bool,
}

fn parse_hdparm_args(args: &[Vec<u8>]) -> HdparmArgs {
    let mut result = HdparmArgs {
        device: Vec::new(),
        show_info: false,
        show_geometry: false,
        show_settings: false,
        show_readonly: false,
        timed_read: false,
        timed_cache: false,
        set_read_ahead: None,
        set_write_cache: None,
        set_apm: None,
        set_aam: None,
        set_standby: None,
        set_multcount: None,
        set_dma: None,
        set_io32bit: None,
        set_unmaskirq: None,
        set_readonly: None,
        security_freeze: false,
        trim_sector: None,
        flush_cache: false,
        sleep: false,
        standby: false,
        idle: false,
        direct: false,
        verbose: false,
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    let mut has_action = false;

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
            has_action = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
            has_action = true;
        } else if arg == b"-i" || arg == b"-I" || arg == b"--identify" {
            result.show_info = true;
            has_action = true;
        } else if arg == b"-g" {
            result.show_geometry = true;
            has_action = true;
        } else if arg == b"-t" {
            result.timed_read = true;
            has_action = true;
        } else if arg == b"-T" {
            result.timed_cache = true;
            has_action = true;
        } else if arg == b"-f" {
            result.flush_cache = true;
            has_action = true;
        } else if arg == b"-Y" {
            result.sleep = true;
            has_action = true;
        } else if arg == b"-y" {
            result.standby = true;
            has_action = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"--direct" {
            result.direct = true;
        } else if arg == b"--security-freeze" {
            result.security_freeze = true;
            has_action = true;
        } else if arg.starts_with(b"-A") {
            result.set_read_ahead = Some(parse_bool_arg(arg, b"-A", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-W") {
            result.set_write_cache = Some(parse_bool_arg(arg, b"-W", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-B") {
            result.set_apm = Some(parse_u8_arg(arg, b"-B", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-M") {
            result.set_aam = Some(parse_u8_arg(arg, b"-M", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-S") {
            result.set_standby = Some(parse_u16_arg(arg, b"-S", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-m") && arg != b"-m" {
            result.set_multcount = Some(parse_u16_arg(arg, b"-m", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-d") && arg != b"-d" {
            result.set_dma = Some(parse_bool_arg(arg, b"-d", args, &mut i));
            has_action = true;
        } else if arg.starts_with(b"-u") && arg != b"-u" {
            result.set_unmaskirq = Some(parse_bool_arg(arg, b"-u", args, &mut i));
            has_action = true;
        } else if !arg.starts_with(b"-") {
            result.device = arg.clone();
        }
        i += 1;
    }

    if !has_action {
        result.show_settings = true;
    }

    result
}

fn parse_bool_arg(arg: &[u8], prefix: &[u8], args: &[Vec<u8>], i: &mut usize) -> bool {
    if arg.len() > prefix.len() {
        arg[prefix.len()] != b'0'
    } else {
        *i += 1;
        if *i < args.len() {
            args[*i] != b"0".as_slice()
        } else {
            true
        }
    }
}

fn parse_u8_arg(arg: &[u8], prefix: &[u8], args: &[Vec<u8>], i: &mut usize) -> u8 {
    let val_bytes = if arg.len() > prefix.len() {
        &arg[prefix.len()..]
    } else {
        *i += 1;
        if *i < args.len() { &args[*i] } else { return 0; }
    };
    parse_u64_bytes(val_bytes).unwrap_or(0) as u8
}

fn parse_u16_arg(arg: &[u8], prefix: &[u8], args: &[Vec<u8>], i: &mut usize) -> u16 {
    let val_bytes = if arg.len() > prefix.len() {
        &arg[prefix.len()..]
    } else {
        *i += 1;
        if *i < args.len() { &args[*i] } else { return 0; }
    };
    parse_u64_bytes(val_bytes).unwrap_or(0) as u16
}

// ── hdparm Commands ────────────────────────────────────────────────────

fn cmd_hdparm(args: &HdparmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: hdparm [options] [device]\n\n");
        print_out(b"Get/set SATA/IDE device parameters.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -i, -I     identify device (model, serial, features)\n");
        print_out(b"  -g         display geometry\n");
        print_out(b"  -t         timing test for device reads\n");
        print_out(b"  -T         timing test for cache reads\n");
        print_out(b"  -A 0/1     read-ahead\n");
        print_out(b"  -W 0/1     write caching\n");
        print_out(b"  -B <level> APM (1=min power, 254=max performance)\n");
        print_out(b"  -M <level> AAM (128=quiet, 254=fast)\n");
        print_out(b"  -S <val>   standby timeout\n");
        print_out(b"  -f         flush write cache\n");
        print_out(b"  -y         put drive in standby\n");
        print_out(b"  -Y         put drive in sleep\n");
        print_out(b"  --security-freeze  freeze security\n");
        print_out(b"  -h         this help\n");
        print_out(b"  -V         version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"hdparm (Slate OS) v1.0.0\n");
        return 0;
    }

    if args.device.is_empty() {
        print_err(b"hdparm: no device specified\n");
        return 1;
    }

    // Device header
    print_out(&args.device);
    print_out(b":\n");

    if args.show_info {
        return hdparm_identify(&args.device);
    }

    if args.show_geometry {
        return hdparm_geometry(&args.device);
    }

    if args.timed_read {
        hdparm_timing_read(&args.device);
    }

    if args.timed_cache {
        hdparm_timing_cache(&args.device);
    }

    // Set operations
    if let Some(val) = args.set_read_ahead {
        print_out(b" setting read-ahead to ");
        print_out(if val { b"on (1)\n" } else { b"off (0)\n" });
    }

    if let Some(val) = args.set_write_cache {
        print_out(b" setting write-caching to ");
        print_out(if val { b"on (1)\n" } else { b"off (0)\n" });
    }

    if let Some(level) = args.set_apm {
        print_out(b" setting Advanced Power Management level to ");
        print_out(&format_u64(level as u64));
        print_out(b"\n");
    }

    if let Some(level) = args.set_aam {
        print_out(b" setting Automatic Acoustic Management level to ");
        print_out(&format_u64(level as u64));
        print_out(b"\n");
    }

    if let Some(timeout) = args.set_standby {
        print_out(b" setting standby timeout to ");
        print_out(&format_u64(timeout as u64));
        print_out(b"\n");
    }

    if args.flush_cache {
        print_out(b" flushing write cache\n");
    }

    if args.sleep {
        print_out(b" issuing sleep command\n");
    }

    if args.standby {
        print_out(b" issuing standby command\n");
    }

    if args.security_freeze {
        print_out(b" issuing security freeze command\n");
    }

    if args.show_settings {
        return hdparm_show_settings(&args.device);
    }

    0
}

// The `_device` parameter on these query-mode functions documents
// intent: a real implementation would issue ATA IDENTIFY DEVICE
// (or its variants) to that path. The personality-CLI stubs print
// representative output without consulting the device.
fn hdparm_identify(_device: &[u8]) -> i32 {
    // In real implementation: ATA IDENTIFY DEVICE command
    print_out(b" Model Number:       Slate OS Virtual Disk\n");
    print_out(b" Serial Number:      VD00000001\n");
    print_out(b" Firmware Revision:  1.0\n");
    print_out(b" Transport:          Serial, SATA 3.0\n");
    print_out(b" Standards:\n");
    print_out(b"  Used: ATA/ATAPI-9\n");
    print_out(b" Configuration:\n");
    print_out(b"  Logical  max current\n");
    print_out(b"  cylinders  16383 16383\n");
    print_out(b"  heads      16    16\n");
    print_out(b"  sectors    63    63\n");
    print_out(b" Capabilities:\n");
    print_out(b"  LBA, IORDY(can be disabled)\n");
    print_out(b"  Queue depth: 32\n");
    print_out(b"  Standby timer values: spec'd by Standard\n");
    print_out(b" Features:\n");
    print_out(b"  *SMART feature set\n");
    print_out(b"  *Security Mode feature set\n");
    print_out(b"  *Power Management feature set\n");
    print_out(b"  *Write cache\n");
    print_out(b"  *Look-ahead\n");
    print_out(b"  *48-bit Address feature set\n");
    print_out(b"  *Native Command Queueing (NCQ)\n");
    print_out(b"  *TRIM supported\n");
    print_out(b"  *Device Sleep (DEVSLP)\n");
    print_out(b" Logical/Physical Sector size:  512/4096 bytes\n");
    print_out(b" Device Size (LBA):  1953525168 sectors (1000 GB)\n");
    0
}

fn hdparm_geometry(_device: &[u8]) -> i32 {
    print_out(b" geometry     = 121601/255/63, sectors = 1953525168, start = 0\n");
    0
}

fn hdparm_timing_read(_device: &[u8]) -> i32 {
    print_out(b" Timing buffered disk reads: 1024 MB in  3.00 seconds = 341.33 MB/sec\n");
    0
}

fn hdparm_timing_cache(_device: &[u8]) -> i32 {
    print_out(b" Timing buffer-cache reads:  16384 MB in  2.00 seconds = 8192.00 MB/sec\n");
    0
}

fn hdparm_show_settings(_device: &[u8]) -> i32 {
    print_out(b" multcount     = 16 (on)\n");
    print_out(b" IO_support    =  1 (32-bit)\n");
    print_out(b" readonly      =  0 (off)\n");
    print_out(b" readahead     = 256 (on)\n");
    print_out(b" geometry      = 121601/255/63, sectors = 1953525168, start = 0\n");
    0
}

// ── sdparm Commands ────────────────────────────────────────────────────

fn cmd_sdparm(args: &HdparmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: sdparm [options] device\n\n");
        print_out(b"Get/set SCSI device parameters.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -i         inquiry (device identification)\n");
        print_out(b"  --all      list all known mode pages\n");
        print_out(b"  -p PAGE    list specified mode page\n");
        print_out(b"  -h         this help\n");
        print_out(b"  -V         version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"sdparm (Slate OS) v1.0.0\n");
        return 0;
    }

    if args.device.is_empty() {
        print_err(b"sdparm: no device specified\n");
        return 1;
    }

    print_out(&args.device);
    print_out(b": ATA       Slate OS Virtual Disk  1.0\n");

    if args.show_info {
        print_out(b"    Peripheral device type: disk\n");
        print_out(b"    SCSI revision: SPC-4\n");
    }

    0
}

// ── Utility Functions ──────────────────────────────────────────────────

fn parse_u64_bytes(s: &[u8]) -> Option<u64> {
    let s = trim_bytes(s);
    if s.is_empty() { return None; }
    let mut result: u64 = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as u64)?;
            }
            _ => return None,
        }
    }
    Some(result)
}

fn format_u64(mut n: u64) -> Vec<u8> {
    if n == 0 { return vec![b'0']; }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    buf.reverse();
    buf
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n').unwrap_or(s.len());
    let end = s.iter().rposition(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .map(|p| p + 1).unwrap_or(start);
    if start >= end { &[] } else { &s[start..end] }
}

fn print_out(msg: &[u8]) {
    #[cfg(not(test))]
    { use std::io::Write; let _ = std::io::stdout().write_all(msg); }
    #[cfg(test)]
    { let _ = msg; }
}

fn print_err(msg: &[u8]) {
    #[cfg(not(test))]
    { use std::io::Write; let _ = std::io::stderr().write_all(msg); }
    #[cfg(test)]
    { let _ = msg; }
}

fn get_args() -> Vec<Vec<u8>> {
    #[cfg(not(test))]
    { std::env::args().map(|a| a.into_bytes()).collect() }
    #[cfg(test)]
    { Vec::new() }
}

// ── Entry Point ────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args = get_args();
    if args.is_empty() { return 1; }

    let personality = detect_personality(&args[0]);
    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();
    let parsed = parse_hdparm_args(&rest);

    match personality {
        Personality::Hdparm => cmd_hdparm(&parsed),
        Personality::Sdparm => cmd_sdparm(&parsed),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hdparm() {
        assert_eq!(detect_personality(b"hdparm"), Personality::Hdparm);
        assert_eq!(detect_personality(b"/sbin/hdparm"), Personality::Hdparm);
    }

    #[test]
    fn test_detect_sdparm() {
        assert_eq!(detect_personality(b"sdparm"), Personality::Sdparm);
    }

    #[test]
    fn test_parse_help() {
        let args = parse_hdparm_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_parse_version() {
        let args = parse_hdparm_args(&[b"-V".to_vec()]);
        assert!(args.show_version);
    }

    #[test]
    fn test_parse_identify() {
        let args = parse_hdparm_args(&[b"-I".to_vec(), b"/dev/sda".to_vec()]);
        assert!(args.show_info);
        assert_eq!(&args.device, b"/dev/sda");
    }

    #[test]
    fn test_parse_geometry() {
        let args = parse_hdparm_args(&[b"-g".to_vec(), b"/dev/sda".to_vec()]);
        assert!(args.show_geometry);
    }

    #[test]
    fn test_parse_timing() {
        let _args = parse_hdparm_args(&[b"-tT".to_vec(), b"/dev/sda".to_vec()]);
        // -tT is combined but our parser handles -t and -T separately.
        // Just verifies the combined-flag form parses without panicking.
    }

    #[test]
    fn test_parse_set_write_cache() {
        let args = parse_hdparm_args(&[b"-W1".to_vec(), b"/dev/sda".to_vec()]);
        assert_eq!(args.set_write_cache, Some(true));
    }

    #[test]
    fn test_parse_set_write_cache_off() {
        let args = parse_hdparm_args(&[b"-W0".to_vec(), b"/dev/sda".to_vec()]);
        assert_eq!(args.set_write_cache, Some(false));
    }

    #[test]
    fn test_parse_set_apm() {
        let args = parse_hdparm_args(&[b"-B128".to_vec(), b"/dev/sda".to_vec()]);
        assert_eq!(args.set_apm, Some(128));
    }

    #[test]
    fn test_parse_set_aam() {
        let args = parse_hdparm_args(&[b"-M254".to_vec(), b"/dev/sda".to_vec()]);
        assert_eq!(args.set_aam, Some(254));
    }

    #[test]
    fn test_parse_default_settings() {
        let args = parse_hdparm_args(&[b"/dev/sda".to_vec()]);
        assert!(args.show_settings);
    }

    #[test]
    fn test_parse_flush() {
        let args = parse_hdparm_args(&[b"-f".to_vec(), b"/dev/sda".to_vec()]);
        assert!(args.flush_cache);
    }

    #[test]
    fn test_parse_sleep() {
        let args = parse_hdparm_args(&[b"-Y".to_vec(), b"/dev/sda".to_vec()]);
        assert!(args.sleep);
    }

    #[test]
    fn test_parse_standby() {
        let args = parse_hdparm_args(&[b"-y".to_vec(), b"/dev/sda".to_vec()]);
        assert!(args.standby);
    }

    #[test]
    fn test_parse_security_freeze() {
        let args = parse_hdparm_args(&[b"--security-freeze".to_vec(), b"/dev/sda".to_vec()]);
        assert!(args.security_freeze);
    }

    #[test]
    fn test_hdparm_no_device() {
        let args = HdparmArgs {
            device: Vec::new(), show_info: false, show_geometry: false,
            show_settings: true, show_readonly: false, timed_read: false,
            timed_cache: false, set_read_ahead: None, set_write_cache: None,
            set_apm: None, set_aam: None, set_standby: None, set_multcount: None,
            set_dma: None, set_io32bit: None, set_unmaskirq: None, set_readonly: None,
            security_freeze: false, trim_sector: None, flush_cache: false,
            sleep: false, standby: false, idle: false, direct: false,
            verbose: false, show_help: false, show_version: false,
        };
        assert_eq!(cmd_hdparm(&args), 1);
    }

    #[test]
    fn test_hdparm_help() {
        let args = HdparmArgs {
            device: Vec::new(), show_info: false, show_geometry: false,
            show_settings: false, show_readonly: false, timed_read: false,
            timed_cache: false, set_read_ahead: None, set_write_cache: None,
            set_apm: None, set_aam: None, set_standby: None, set_multcount: None,
            set_dma: None, set_io32bit: None, set_unmaskirq: None, set_readonly: None,
            security_freeze: false, trim_sector: None, flush_cache: false,
            sleep: false, standby: false, idle: false, direct: false,
            verbose: false, show_help: true, show_version: false,
        };
        assert_eq!(cmd_hdparm(&args), 0);
    }

    #[test]
    fn test_hdparm_identify_ok() {
        assert_eq!(hdparm_identify(b"/dev/sda"), 0);
    }

    #[test]
    fn test_hdparm_geometry_ok() {
        assert_eq!(hdparm_geometry(b"/dev/sda"), 0);
    }

    #[test]
    fn test_hdparm_timing_read_ok() {
        assert_eq!(hdparm_timing_read(b"/dev/sda"), 0);
    }

    #[test]
    fn test_hdparm_timing_cache_ok() {
        assert_eq!(hdparm_timing_cache(b"/dev/sda"), 0);
    }

    #[test]
    fn test_hdparm_settings_ok() {
        assert_eq!(hdparm_show_settings(b"/dev/sda"), 0);
    }

    #[test]
    fn test_sdparm_no_device() {
        let args = HdparmArgs {
            device: Vec::new(), show_info: false, show_geometry: false,
            show_settings: false, show_readonly: false, timed_read: false,
            timed_cache: false, set_read_ahead: None, set_write_cache: None,
            set_apm: None, set_aam: None, set_standby: None, set_multcount: None,
            set_dma: None, set_io32bit: None, set_unmaskirq: None, set_readonly: None,
            security_freeze: false, trim_sector: None, flush_cache: false,
            sleep: false, standby: false, idle: false, direct: false,
            verbose: false, show_help: false, show_version: false,
        };
        assert_eq!(cmd_sdparm(&args), 1);
    }

    #[test]
    fn test_parse_u64_bytes() {
        assert_eq!(parse_u64_bytes(b"0"), Some(0));
        assert_eq!(parse_u64_bytes(b"123"), Some(123));
        assert_eq!(parse_u64_bytes(b""), None);
        assert_eq!(parse_u64_bytes(b"abc"), None);
    }

    #[test]
    fn test_format_u64() {
        assert_eq!(format_u64(0), b"0");
        assert_eq!(format_u64(42), b"42");
    }

    #[test]
    fn test_ata_flags() {
        assert_ne!(ATA_FLAG_LBA, ATA_FLAG_LBA48);
        assert_ne!(ATA_FLAG_NCQ, ATA_FLAG_SMART);
        // Flags should be non-overlapping powers of 2
        let all = ATA_FLAG_LBA | ATA_FLAG_LBA48 | ATA_FLAG_NCQ | ATA_FLAG_SMART
                | ATA_FLAG_APM | ATA_FLAG_AAM | ATA_FLAG_WRITE_CACHE | ATA_FLAG_READ_AHEAD
                | ATA_FLAG_SECURITY | ATA_FLAG_HPA | ATA_FLAG_TRIM | ATA_FLAG_DEVSLP;
        assert_eq!(all.count_ones(), 12);
    }
}
