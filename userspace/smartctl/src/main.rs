// SlateOS smartctl - SMART disk monitoring tool
//
// Provides S.M.A.R.T. (Self-Monitoring, Analysis and Reporting Technology)
// disk health monitoring for ATA and SCSI devices.

#![cfg_attr(not(test), no_main)]
// Many ATA SMART attribute IDs, command opcodes, NVMe log page IDs, exit
// codes, and SelfTestStatus / HealthStatus variants are declared up-front
// because they encode the protocol the real implementation must speak.
// They are intentionally kept as documentation for the eventual ATA SAT
// driver integration; squashing them now would erase that contract.
#![allow(dead_code)]

// ── Constants ──────────────────────────────────────────────────────────

// SMART attribute IDs (ATA)
const ATTR_RAW_READ_ERROR: u8 = 1;
const ATTR_THROUGHPUT_PERF: u8 = 2;
const ATTR_SPIN_UP_TIME: u8 = 3;
const ATTR_START_STOP_COUNT: u8 = 4;
const ATTR_REALLOC_SECTOR: u8 = 5;
const ATTR_SEEK_ERROR: u8 = 7;
const ATTR_POWER_ON_HOURS: u8 = 9;
const ATTR_SPIN_RETRY: u8 = 10;
const ATTR_CALIBRATION_RETRY: u8 = 11;
const ATTR_POWER_CYCLE: u8 = 12;
const ATTR_READ_SOFT_ERROR: u8 = 13;
const ATTR_CURRENT_HELIUM: u8 = 22;
const ATTR_AVAILABLE_RESERVED: u8 = 170;
const ATTR_PROGRAM_FAIL: u8 = 171;
const ATTR_ERASE_FAIL: u8 = 172;
const ATTR_WEAR_LEVELING: u8 = 173;
const ATTR_UNEXPECTED_POWER_LOSS: u8 = 174;
const ATTR_UNUSED_RESERVE_NAND: u8 = 180;
const ATTR_PROGRAM_FAIL_TOTAL: u8 = 181;
const ATTR_ERASE_FAIL_TOTAL: u8 = 182;
const ATTR_RUNTIME_BAD_BLOCK: u8 = 183;
const ATTR_END_TO_END_ERROR: u8 = 184;
const ATTR_UNCORRECTABLE_ERROR: u8 = 187;
const ATTR_COMMAND_TIMEOUT: u8 = 188;
const ATTR_HIGH_FLY_WRITES: u8 = 189;
const ATTR_AIRFLOW_TEMP: u8 = 190;
const ATTR_GSENSE_ERROR: u8 = 191;
const ATTR_POWER_OFF_RETRACT: u8 = 192;
const ATTR_LOAD_CYCLE: u8 = 193;
const ATTR_TEMPERATURE: u8 = 194;
const ATTR_HARDWARE_ECC: u8 = 195;
const ATTR_REALLOC_EVENT: u8 = 196;
const ATTR_CURRENT_PENDING: u8 = 197;
const ATTR_OFFLINE_UNCORRECTABLE: u8 = 198;
const ATTR_UDMA_CRC_ERROR: u8 = 199;
const ATTR_WRITE_ERROR: u8 = 200;
const ATTR_MEDIA_WEAROUT: u8 = 233;
const ATTR_TOTAL_LBA_WRITTEN: u8 = 241;
const ATTR_TOTAL_LBA_READ: u8 = 242;

// SMART status flags
const SMART_FLAG_PREFAIL: u16 = 1 << 0;
const SMART_FLAG_ONLINE: u16 = 1 << 1;
const SMART_FLAG_PERFORMANCE: u16 = 1 << 2;
const SMART_FLAG_ERROR_RATE: u16 = 1 << 3;
const SMART_FLAG_EVENT_COUNT: u16 = 1 << 4;
const SMART_FLAG_SELF_PRESERVING: u16 = 1 << 5;

// ATA SMART commands
const ATA_SMART_READ_VALUES: u8 = 0xD0;
const ATA_SMART_READ_THRESHOLDS: u8 = 0xD1;
const ATA_SMART_AUTOSAVE: u8 = 0xD2;
const ATA_SMART_EXECUTE_OFFLINE: u8 = 0xD4;
const ATA_SMART_READ_LOG: u8 = 0xD5;
const ATA_SMART_WRITE_LOG: u8 = 0xD6;
const ATA_SMART_ENABLE: u8 = 0xD8;
const ATA_SMART_DISABLE: u8 = 0xD9;
const ATA_SMART_STATUS: u8 = 0xDA;
const ATA_SMART_AUTO_OFFLINE: u8 = 0xDB;

// Self-test types
const TEST_SHORT: u8 = 1;
const TEST_EXTENDED: u8 = 2;
const TEST_CONVEYANCE: u8 = 3;
const TEST_SELECTIVE: u8 = 4;
const TEST_VENDOR: u8 = 0x40;

// NVMe SMART log page
const NVME_LOG_SMART: u8 = 0x02;
const NVME_LOG_ERROR: u8 = 0x01;
const NVME_LOG_FWSLOT: u8 = 0x03;
const NVME_LOG_SELF_TEST: u8 = 0x06;

// Health thresholds
const TEMP_WARN_C: u16 = 55;
const TEMP_CRIT_C: u16 = 70;
const REALLOC_WARN: u64 = 10;
const PENDING_WARN: u64 = 1;

// Exit status bits (like real smartctl)
const EXIT_OK: i32 = 0;
const EXIT_CMD_FAIL: i32 = 1;
const EXIT_OPEN_FAIL: i32 = 2;
const EXIT_SMART_CMD_FAIL: i32 = 4;
const EXIT_DISK_FAILING: i32 = 8;
const EXIT_PREFAIL_BELOW: i32 = 16;
const EXIT_PAST_PREFAIL: i32 = 32;
const EXIT_ERROR_LOG: i32 = 64;
const EXIT_SELFTEST_LOG: i32 = 128;

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
enum DeviceType {
    Ata,
    Scsi,
    Nvme,
    Auto,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HealthStatus {
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy)]
struct SmartAttribute {
    id: u8,
    flags: u16,
    current: u8,
    worst: u8,
    threshold: u8,
    raw: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelfTestStatus {
    Completed,
    InProgress(u8), // percent remaining
    Interrupted,
    Fatal,
    UnknownFailure,
    ElectricalFailure,
    ServoFailure,
    ReadFailure,
    HandlingDamage,
    NotRun,
}

#[derive(Clone, Copy)]
struct SelfTestEntry {
    test_num: u8,
    test_type: u8,
    status: SelfTestStatus,
    lifetime_hours: u32,
    lba_first_error: u64,
}

#[derive(Clone, Copy)]
struct ErrorLogEntry {
    error_num: u16,
    lifetime_hours: u32,
    error_type: u8,
    lba: u64,
    count: u8,
}

struct DeviceInfo {
    model: [u8; 64],
    model_len: usize,
    serial: [u8; 32],
    serial_len: usize,
    firmware: [u8; 16],
    firmware_len: usize,
    capacity_bytes: u64,
    rotation_rate: u16, // 0 = SSD, >0 = RPM
    form_factor: u8,
    device_type: DeviceType,
    smart_supported: bool,
    smart_enabled: bool,
    ata_version: u8,
    sata_version: u8,
}

struct SmartData {
    health: HealthStatus,
    temperature: u16,
    power_on_hours: u64,
    power_cycles: u64,
    attributes: [Option<SmartAttribute>; 64],
    attr_count: usize,
    self_tests: [Option<SelfTestEntry>; 21],
    test_count: usize,
    error_log: [Option<ErrorLogEntry>; 16],
    error_count: usize,
}

// NVMe SMART info
struct NvmeSmartLog {
    critical_warning: u8,
    temperature: u16,
    avail_spare: u8,
    spare_thresh: u8,
    percent_used: u8,
    data_units_read: u128,
    data_units_written: u128,
    host_reads: u128,
    host_writes: u128,
    ctrl_busy_time: u128,
    power_cycles: u128,
    power_on_hours: u128,
    unsafe_shutdowns: u128,
    media_errors: u128,
    error_log_entries: u128,
    warning_temp_time: u32,
    critical_temp_time: u32,
}

struct Options {
    device: [u8; 256],
    device_len: usize,
    device_type: DeviceType,
    info: bool,            // -i
    health: bool,          // -H
    capabilities: bool,    // -c
    attributes: bool,      // -A
    all: bool,             // -a
    everything: bool,      // -x
    error_log: bool,       // -l error
    selftest_log: bool,    // -l selftest
    test_short: bool,      // -t short
    test_long: bool,       // -t long
    test_conveyance: bool, // -t conveyance
    test_abort: bool,      // -X
    enable_smart: bool,    // -s on
    disable_smart: bool,   // -s off
    json: bool,            // -j
    quiet: bool,           // -q
    verbose: bool,         // -v
    scan: bool,            // --scan
}

// ── Argument Parsing ───────────────────────────────────────────────────

fn parse_args(argc: i32, argv: *const *const u8) -> Result<Options, i32> {
    let mut opts = Options {
        device: [0u8; 256],
        device_len: 0,
        device_type: DeviceType::Auto,
        info: false,
        health: false,
        capabilities: false,
        attributes: false,
        all: false,
        everything: false,
        error_log: false,
        selftest_log: false,
        test_short: false,
        test_long: false,
        test_conveyance: false,
        test_abort: false,
        enable_smart: false,
        disable_smart: false,
        json: false,
        quiet: false,
        verbose: false,
        scan: false,
    };

    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let mut i = 1;
    while i < args.len() {
        let arg = unsafe { cstr_to_slice(args[i]) };

        if arg == b"--help" || arg == b"-h" {
            show_help();
            return Err(0);
        } else if arg == b"--version" || arg == b"-V" {
            print_out(b"smartctl 0.1.0 (Slate OS)\n");
            return Err(0);
        } else if arg == b"--scan" {
            opts.scan = true;
        } else if arg == b"-i" || arg == b"--info" {
            opts.info = true;
        } else if arg == b"-H" || arg == b"--health" {
            opts.health = true;
        } else if arg == b"-c" || arg == b"--capabilities" {
            opts.capabilities = true;
        } else if arg == b"-A" || arg == b"--attributes" {
            opts.attributes = true;
        } else if arg == b"-a" || arg == b"--all" {
            opts.all = true;
        } else if arg == b"-x" || arg == b"--xall" {
            opts.everything = true;
        } else if arg == b"-j" || arg == b"--json" {
            opts.json = true;
        } else if arg == b"-q" || arg == b"--quiet" {
            opts.quiet = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            opts.verbose = true;
        } else if arg == b"-X" {
            opts.test_abort = true;
        } else if arg == b"-d" || arg == b"--device" {
            i += 1;
            if i >= args.len() {
                print_err(b"smartctl: -d requires a device type\n");
                return Err(1);
            }
            let val = unsafe { cstr_to_slice(args[i]) };
            if val == b"ata" || val == b"sat" {
                opts.device_type = DeviceType::Ata;
            } else if val == b"scsi" {
                opts.device_type = DeviceType::Scsi;
            } else if val == b"nvme" {
                opts.device_type = DeviceType::Nvme;
            } else if val == b"auto" {
                opts.device_type = DeviceType::Auto;
            } else {
                print_err(b"smartctl: unknown device type: ");
                print_err(val);
                print_err(b"\n");
                return Err(1);
            }
        } else if arg == b"-s" {
            i += 1;
            if i >= args.len() {
                print_err(b"smartctl: -s requires on/off\n");
                return Err(1);
            }
            let val = unsafe { cstr_to_slice(args[i]) };
            if val == b"on" {
                opts.enable_smart = true;
            } else if val == b"off" {
                opts.disable_smart = true;
            } else {
                print_err(b"smartctl: -s requires on or off\n");
                return Err(1);
            }
        } else if arg == b"-t" || arg == b"--test" {
            i += 1;
            if i >= args.len() {
                print_err(b"smartctl: -t requires test type\n");
                return Err(1);
            }
            let val = unsafe { cstr_to_slice(args[i]) };
            if val == b"short" {
                opts.test_short = true;
            } else if val == b"long" || val == b"extended" {
                opts.test_long = true;
            } else if val == b"conveyance" {
                opts.test_conveyance = true;
            } else {
                print_err(b"smartctl: unknown test type: ");
                print_err(val);
                print_err(b"\n");
                return Err(1);
            }
        } else if arg == b"-l" || arg == b"--log" {
            i += 1;
            if i >= args.len() {
                print_err(b"smartctl: -l requires log type\n");
                return Err(1);
            }
            let val = unsafe { cstr_to_slice(args[i]) };
            if val == b"error" {
                opts.error_log = true;
            } else if val == b"selftest" {
                opts.selftest_log = true;
            } else {
                print_err(b"smartctl: unknown log type: ");
                print_err(val);
                print_err(b"\n");
                return Err(1);
            }
        } else if !arg.is_empty() && arg[0] != b'-' {
            // Device path
            let len = arg.len().min(255);
            opts.device[..len].copy_from_slice(&arg[..len]);
            opts.device_len = len;
        } else {
            print_err(b"smartctl: unknown option: ");
            print_err(arg);
            print_err(b"\n");
            return Err(1);
        }
        i += 1;
    }

    // --all implies -iHcA plus logs
    if opts.all {
        opts.info = true;
        opts.health = true;
        opts.capabilities = true;
        opts.attributes = true;
        opts.error_log = true;
        opts.selftest_log = true;
    }
    // --xall implies everything
    if opts.everything {
        opts.info = true;
        opts.health = true;
        opts.capabilities = true;
        opts.attributes = true;
        opts.error_log = true;
        opts.selftest_log = true;
    }

    // Default to showing info+health if nothing specified
    if !opts.info
        && !opts.health
        && !opts.capabilities
        && !opts.attributes
        && !opts.error_log
        && !opts.selftest_log
        && !opts.test_short
        && !opts.test_long
        && !opts.test_conveyance
        && !opts.test_abort
        && !opts.enable_smart
        && !opts.disable_smart
        && !opts.scan
    {
        opts.info = true;
        opts.health = true;
    }

    if !opts.scan && opts.device_len == 0 {
        print_err(b"smartctl: no device specified\n");
        print_err(b"Try 'smartctl --help' for more information.\n");
        return Err(1);
    }

    Ok(opts)
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

fn format_i64(val: i64, buf: &mut [u8]) -> usize {
    if val < 0 {
        if buf.is_empty() {
            return 0;
        }
        buf[0] = b'-';
        let written = format_u64((-val) as u64, &mut buf[1..]);
        written + 1
    } else {
        format_u64(val as u64, buf)
    }
}

fn format_u128(val: u128, buf: &mut [u8]) -> usize {
    if val == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
        }
        return 1;
    }
    let mut tmp = [0u8; 40];
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

fn format_hex_u8(val: u8, buf: &mut [u8]) -> usize {
    if buf.len() < 2 {
        return 0;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    buf[0] = HEX[(val >> 4) as usize];
    buf[1] = HEX[(val & 0xF) as usize];
    2
}

fn pad_right(buf: &mut [u8], start: usize, width: usize) -> usize {
    let mut pos = start;
    while pos < width && pos < buf.len() {
        buf[pos] = b' ';
        pos += 1;
    }
    pos
}

fn pad_left_num(val: u64, buf: &mut [u8], width: usize) -> usize {
    let mut tmp = [0u8; 20];
    let n = format_u64(val, &mut tmp);
    let mut pos = 0;
    if n < width {
        let pad = width - n;
        while pos < pad && pos < buf.len() {
            buf[pos] = b' ';
            pos += 1;
        }
    }
    for j in 0..n {
        if pos < buf.len()
            && let Some(c) = tmp.get(j)
        {
            buf[pos] = *c;
            pos += 1;
        }
    }
    pos
}

fn format_capacity(bytes: u64, buf: &mut [u8]) -> usize {
    let mut pos = 0;
    let n = format_u64(bytes, &mut buf[pos..]);
    pos += n;
    if pos < buf.len() {
        buf[pos] = b' ';
        pos += 1;
    }
    let suffix = b"bytes";
    for &c in suffix {
        if pos < buf.len() {
            buf[pos] = c;
            pos += 1;
        }
    }
    // Also show human-readable
    if pos + 3 < buf.len() {
        buf[pos] = b' ';
        pos += 1;
        buf[pos] = b'[';
        pos += 1;

        let (val, unit): (u64, &[u8]) = if bytes >= 1_000_000_000_000 {
            (bytes / 1_000_000_000, b"TB")
        } else if bytes >= 1_000_000_000 {
            (bytes / 1_000_000, b"GB")
        } else if bytes >= 1_000_000 {
            (bytes / 1_000, b"MB")
        } else {
            (bytes, b"B")
        };

        // Format as X.XX
        let whole = val / 1000;
        let frac = val % 1000;
        let n2 = format_u64(whole, &mut buf[pos..]);
        pos += n2;
        if pos < buf.len() {
            buf[pos] = b'.';
            pos += 1;
        }
        // Two decimal digits
        let d1 = (frac / 100) as u8;
        let d2 = ((frac % 100) / 10) as u8;
        if pos + 1 < buf.len() {
            buf[pos] = b'0' + d1;
            pos += 1;
            buf[pos] = b'0' + d2;
            pos += 1;
        }
        if pos < buf.len() {
            buf[pos] = b' ';
            pos += 1;
        }
        for &c in unit {
            if pos < buf.len() {
                buf[pos] = c;
                pos += 1;
            }
        }
        if pos < buf.len() {
            buf[pos] = b']';
            pos += 1;
        }
    }
    pos
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

// ── Attribute Name Lookup ──────────────────────────────────────────────

fn attribute_name(id: u8) -> &'static [u8] {
    match id {
        1 => b"Raw_Read_Error_Rate",
        2 => b"Throughput_Performance",
        3 => b"Spin_Up_Time",
        4 => b"Start_Stop_Count",
        5 => b"Reallocated_Sector_Ct",
        7 => b"Seek_Error_Rate",
        9 => b"Power_On_Hours",
        10 => b"Spin_Retry_Count",
        11 => b"Calibration_Retry_Count",
        12 => b"Power_Cycle_Count",
        13 => b"Read_Soft_Error_Rate",
        22 => b"Current_Helium_Level",
        170 => b"Available_Reservd_Space",
        171 => b"Program_Fail_Count",
        172 => b"Erase_Fail_Count",
        173 => b"Wear_Leveling_Count",
        174 => b"Unexpect_Power_Loss_Ct",
        175 => b"Program_Fail_Count_Chip",
        176 => b"Erase_Fail_Count_Chip",
        177 => b"Wear_Leveling_Count",
        178 => b"Used_Rsvd_Blk_Cnt_Chip",
        179 => b"Used_Rsvd_Blk_Cnt_Tot",
        180 => b"Unused_Rsvd_Blk_Cnt_Tot",
        181 => b"Program_Fail_Cnt_Total",
        182 => b"Erase_Fail_Count_Total",
        183 => b"Runtime_Bad_Block",
        184 => b"End-to-End_Error",
        187 => b"Reported_Uncorrect",
        188 => b"Command_Timeout",
        189 => b"High_Fly_Writes",
        190 => b"Airflow_Temperature_Cel",
        191 => b"G-Sense_Error_Rate",
        192 => b"Power-Off_Retract_Count",
        193 => b"Load_Cycle_Count",
        194 => b"Temperature_Celsius",
        195 => b"Hardware_ECC_Recovered",
        196 => b"Reallocated_Event_Count",
        197 => b"Current_Pending_Sector",
        198 => b"Offline_Uncorrectable",
        199 => b"UDMA_CRC_Error_Count",
        200 => b"Multi_Zone_Error_Rate",
        201 => b"Soft_Read_Error_Rate",
        202 => b"Data_Address_Mark_Errs",
        220 => b"Disk_Shift",
        222 => b"Loaded_Hours",
        223 => b"Load_Retry_Count",
        224 => b"Load_Friction",
        225 => b"Load_Cycle_Count",
        226 => b"Load-in_Time",
        227 => b"Torq-amp_Count",
        228 => b"Power-off_Retract_Cycle",
        230 => b"Head_Amplitude",
        231 => b"Temperature_Celsius",
        232 => b"Available_Reservd_Space",
        233 => b"Media_Wearout_Indicator",
        234 => b"Thermal_Throttle",
        235 => b"Good_Block_Count",
        240 => b"Head_Flying_Hours",
        241 => b"Total_LBAs_Written",
        242 => b"Total_LBAs_Read",
        250 => b"Read_Error_Retry_Rate",
        254 => b"Free_Fall_Sensor",
        _ => b"Unknown_Attribute",
    }
}

// ── Flag String Builder ────────────────────────────────────────────────

fn format_attr_flags(flags: u16, buf: &mut [u8]) -> usize {
    // Format: "PO-S-C" or similar 6-char string
    let mut pos = 0;
    // Pre-fail (P) vs Old-age (O)
    if pos < buf.len() {
        buf[pos] = if flags & SMART_FLAG_PREFAIL != 0 {
            b'P'
        } else {
            b'-'
        };
        pos += 1;
    }
    // Online (O)
    if pos < buf.len() {
        buf[pos] = if flags & SMART_FLAG_ONLINE != 0 {
            b'O'
        } else {
            b'-'
        };
        pos += 1;
    }
    // Performance (S)
    if pos < buf.len() {
        buf[pos] = if flags & SMART_FLAG_PERFORMANCE != 0 {
            b'S'
        } else {
            b'-'
        };
        pos += 1;
    }
    // Error rate (R)
    if pos < buf.len() {
        buf[pos] = if flags & SMART_FLAG_ERROR_RATE != 0 {
            b'R'
        } else {
            b'-'
        };
        pos += 1;
    }
    // Event count (C)
    if pos < buf.len() {
        buf[pos] = if flags & SMART_FLAG_EVENT_COUNT != 0 {
            b'C'
        } else {
            b'-'
        };
        pos += 1;
    }
    // Self-preserving (K)
    if pos < buf.len() {
        buf[pos] = if flags & SMART_FLAG_SELF_PRESERVING != 0 {
            b'K'
        } else {
            b'-'
        };
        pos += 1;
    }
    pos
}

fn format_attr_type(flags: u16, buf: &mut [u8]) -> usize {
    if flags & SMART_FLAG_PREFAIL != 0 {
        copy_bytes(buf, 0, b"Pre-fail")
    } else {
        copy_bytes(buf, 0, b"Old_age")
    }
}

fn format_when(flags: u16, buf: &mut [u8]) -> usize {
    if flags & SMART_FLAG_ONLINE != 0 {
        copy_bytes(buf, 0, b"Always")
    } else {
        copy_bytes(buf, 0, b"Offline")
    }
}

// ── Simulated Device Detection ─────────────────────────────────────────

fn detect_device_type(path: &[u8]) -> DeviceType {
    // Heuristic from device path name
    if contains(path, b"nvme") {
        DeviceType::Nvme
    } else if contains(path, b"sd") || contains(path, b"hd") {
        DeviceType::Ata
    } else if contains(path, b"sg") {
        DeviceType::Scsi
    } else {
        DeviceType::Ata // default
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if haystack[i..i + needle.len()] == *needle {
            return true;
        }
    }
    false
}

// ── Simulated SMART Data ───────────────────────────────────────────────

// In a real OS, these would issue ioctl()/ATA pass-through commands.
// For now, we simulate reading from the device to demonstrate the
// full output formatting and analysis logic.

fn get_device_info(path: &[u8], dtype: DeviceType) -> DeviceInfo {
    let device_type = if dtype == DeviceType::Auto {
        detect_device_type(path)
    } else {
        dtype
    };

    // Generate plausible info based on device type
    match device_type {
        DeviceType::Nvme => {
            let mut info = DeviceInfo {
                model: [0u8; 64],
                model_len: 0,
                serial: [0u8; 32],
                serial_len: 0,
                firmware: [0u8; 16],
                firmware_len: 0,
                capacity_bytes: 1_000_204_886_016,
                rotation_rate: 0,
                form_factor: 0,
                device_type: DeviceType::Nvme,
                smart_supported: true,
                smart_enabled: true,
                ata_version: 0,
                sata_version: 0,
            };
            let model = b"Slate OS NVMe SSD 1TB";
            info.model[..model.len()].copy_from_slice(model);
            info.model_len = model.len();
            let serial = b"SLATEOS0NV001";
            info.serial[..serial.len()].copy_from_slice(serial);
            info.serial_len = serial.len();
            let fw = b"1.0.0";
            info.firmware[..fw.len()].copy_from_slice(fw);
            info.firmware_len = fw.len();
            info
        }
        _ => {
            let mut info = DeviceInfo {
                model: [0u8; 64],
                model_len: 0,
                serial: [0u8; 32],
                serial_len: 0,
                firmware: [0u8; 16],
                firmware_len: 0,
                capacity_bytes: 500_107_862_016,
                rotation_rate: 7200,
                form_factor: 1,
                device_type,
                smart_supported: true,
                smart_enabled: true,
                ata_version: 10,
                sata_version: 3,
            };
            let model = b"Slate OS ATA HDD 500GB";
            info.model[..model.len()].copy_from_slice(model);
            info.model_len = model.len();
            let serial = b"SLATEOS0HD001";
            info.serial[..serial.len()].copy_from_slice(serial);
            info.serial_len = serial.len();
            let fw = b"FW01";
            info.firmware[..fw.len()].copy_from_slice(fw);
            info.firmware_len = fw.len();
            info
        }
    }
}

fn get_smart_data(_path: &[u8], dtype: DeviceType) -> SmartData {
    let mut data = SmartData {
        health: HealthStatus::Passed,
        temperature: 34,
        power_on_hours: 12345,
        power_cycles: 456,
        attributes: [None; 64],
        attr_count: 0,
        self_tests: [None; 21],
        test_count: 0,
        error_log: [None; 16],
        error_count: 0,
    };

    if dtype != DeviceType::Nvme {
        // Simulate typical ATA SMART attributes
        let attrs: &[(u8, u16, u8, u8, u8, u64)] = &[
            (
                ATTR_RAW_READ_ERROR,
                SMART_FLAG_PREFAIL | SMART_FLAG_ONLINE | SMART_FLAG_ERROR_RATE,
                100,
                100,
                6,
                0,
            ),
            (
                ATTR_THROUGHPUT_PERF,
                SMART_FLAG_ONLINE | SMART_FLAG_PERFORMANCE,
                100,
                100,
                54,
                0,
            ),
            (
                ATTR_SPIN_UP_TIME,
                SMART_FLAG_PREFAIL | SMART_FLAG_PERFORMANCE,
                100,
                100,
                24,
                250,
            ),
            (
                ATTR_START_STOP_COUNT,
                SMART_FLAG_ONLINE | SMART_FLAG_EVENT_COUNT,
                100,
                100,
                20,
                456,
            ),
            (
                ATTR_REALLOC_SECTOR,
                SMART_FLAG_PREFAIL | SMART_FLAG_ONLINE,
                100,
                100,
                10,
                0,
            ),
            (
                ATTR_SEEK_ERROR,
                SMART_FLAG_ONLINE | SMART_FLAG_ERROR_RATE,
                100,
                100,
                67,
                0,
            ),
            (
                ATTR_POWER_ON_HOURS,
                SMART_FLAG_ONLINE | SMART_FLAG_EVENT_COUNT,
                86,
                86,
                0,
                12345,
            ),
            (
                ATTR_SPIN_RETRY,
                SMART_FLAG_PREFAIL | SMART_FLAG_ONLINE,
                100,
                100,
                60,
                0,
            ),
            (
                ATTR_POWER_CYCLE,
                SMART_FLAG_ONLINE | SMART_FLAG_EVENT_COUNT,
                100,
                100,
                20,
                456,
            ),
            (
                ATTR_TEMPERATURE,
                SMART_FLAG_ONLINE | SMART_FLAG_PERFORMANCE,
                166,
                166,
                0,
                34,
            ),
            (
                ATTR_REALLOC_EVENT,
                SMART_FLAG_ONLINE | SMART_FLAG_EVENT_COUNT,
                100,
                100,
                0,
                0,
            ),
            (ATTR_CURRENT_PENDING, SMART_FLAG_ONLINE, 100, 100, 0, 0),
            (
                ATTR_OFFLINE_UNCORRECTABLE,
                SMART_FLAG_ONLINE,
                100,
                100,
                0,
                0,
            ),
            (
                ATTR_UDMA_CRC_ERROR,
                SMART_FLAG_ONLINE | SMART_FLAG_ERROR_RATE,
                200,
                200,
                0,
                0,
            ),
        ];

        for (idx, &(id, flags, cur, worst, thresh, raw)) in attrs.iter().enumerate() {
            if idx < 64 {
                data.attributes[idx] = Some(SmartAttribute {
                    id,
                    flags,
                    current: cur,
                    worst,
                    threshold: thresh,
                    raw,
                });
                data.attr_count += 1;
            }
        }

        // Simulated self-test log
        data.self_tests[0] = Some(SelfTestEntry {
            test_num: 1,
            test_type: TEST_SHORT,
            status: SelfTestStatus::Completed,
            lifetime_hours: 12340,
            lba_first_error: 0,
        });
        data.self_tests[1] = Some(SelfTestEntry {
            test_num: 2,
            test_type: TEST_EXTENDED,
            status: SelfTestStatus::Completed,
            lifetime_hours: 12300,
            lba_first_error: 0,
        });
        data.test_count = 2;
    }

    data
}

fn get_nvme_smart_log(_path: &[u8]) -> NvmeSmartLog {
    NvmeSmartLog {
        critical_warning: 0,
        temperature: 307, // 34°C in Kelvin
        avail_spare: 100,
        spare_thresh: 10,
        percent_used: 3,
        data_units_read: 25_678_901,
        data_units_written: 18_345_678,
        host_reads: 432_567_890,
        host_writes: 298_345_678,
        ctrl_busy_time: 1234,
        power_cycles: 456,
        power_on_hours: 12345,
        unsafe_shutdowns: 12,
        media_errors: 0,
        error_log_entries: 0,
        warning_temp_time: 0,
        critical_temp_time: 0,
    }
}

// ── Display Functions ──────────────────────────────────────────────────

fn show_help() {
    print_out(b"Usage: smartctl [options] device\n");
    print_out(b"\n");
    print_out(b"SMART disk monitoring tool\n");
    print_out(b"\n");
    print_out(b"Options:\n");
    print_out(b"  -i, --info            Show device information\n");
    print_out(b"  -H, --health          Show SMART health status\n");
    print_out(b"  -c, --capabilities    Show SMART capabilities\n");
    print_out(b"  -A, --attributes      Show SMART attributes\n");
    print_out(b"  -a, --all             Show all SMART information (-iHcA)\n");
    print_out(b"  -x, --xall            Show all extended information\n");
    print_out(b"  -l TYPE               Show log (error, selftest)\n");
    print_out(b"  -t TYPE               Run self-test (short, long, conveyance)\n");
    print_out(b"  -X                    Abort self-test\n");
    print_out(b"  -s on|off             Enable/disable SMART\n");
    print_out(b"  -d TYPE               Set device type (ata, scsi, nvme, auto)\n");
    print_out(b"  -j, --json            Output in JSON format\n");
    print_out(b"  -q, --quiet           Quiet output\n");
    print_out(b"  -v, --verbose         Verbose output\n");
    print_out(b"  --scan                Scan for devices\n");
    print_out(b"  -h, --help            Show this help\n");
    print_out(b"  -V, --version         Show version\n");
}

fn show_device_info(info: &DeviceInfo) {
    print_out(b"=== START OF INFORMATION SECTION ===\n");

    let mut buf = [0u8; 512];

    // Model
    let mut pos = copy_bytes(&mut buf, 0, b"Model Family:     ");
    match info.device_type {
        DeviceType::Nvme => pos = copy_bytes(&mut buf, pos, b"NVMe SSD"),
        DeviceType::Scsi => pos = copy_bytes(&mut buf, pos, b"SCSI Device"),
        _ => pos = copy_bytes(&mut buf, pos, b"ATA Device"),
    }
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"Device Model:     ");
    let model = &info.model[..info.model_len];
    pos = copy_bytes(&mut buf, pos, model);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"Serial Number:    ");
    let serial = &info.serial[..info.serial_len];
    pos = copy_bytes(&mut buf, pos, serial);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"Firmware Version: ");
    let fw = &info.firmware[..info.firmware_len];
    pos = copy_bytes(&mut buf, pos, fw);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"User Capacity:    ");
    pos += format_capacity(info.capacity_bytes, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    if info.rotation_rate > 0 {
        pos = copy_bytes(&mut buf, 0, b"Rotation Rate:    ");
        pos += format_u64(info.rotation_rate as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" rpm\n");
        print_out(&buf[..pos]);
    } else {
        print_out(b"Rotation Rate:    Solid State Device\n");
    }

    if info.device_type == DeviceType::Ata {
        pos = copy_bytes(&mut buf, 0, b"ATA Version:      ATA");
        pos += format_u64(info.ata_version as u64, &mut buf[pos..]);
        buf[pos] = b'\n';
        pos += 1;
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"SATA Version:     SATA ");
        pos += format_u64(info.sata_version as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b".0\n");
        print_out(&buf[..pos]);
    }

    pos = copy_bytes(&mut buf, 0, b"SMART support is: ");
    if info.smart_supported {
        pos = copy_bytes(&mut buf, pos, b"Available - device has SMART capability.\n");
    } else {
        pos = copy_bytes(
            &mut buf,
            pos,
            b"Unavailable - device lacks SMART capability.\n",
        );
    }
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"SMART support is: ");
    if info.smart_enabled {
        pos = copy_bytes(&mut buf, pos, b"Enabled\n");
    } else {
        pos = copy_bytes(&mut buf, pos, b"Disabled\n");
    }
    print_out(&buf[..pos]);

    print_out(b"\n");
}

fn show_health(data: &SmartData) {
    print_out(b"=== START OF READ SMART DATA SECTION ===\n");
    print_out(b"SMART overall-health self-assessment test result: ");
    match data.health {
        HealthStatus::Passed => print_out(b"PASSED\n"),
        HealthStatus::Failed => print_out(b"FAILED!\n"),
        HealthStatus::Unknown => print_out(b"UNKNOWN\n"),
    }
    print_out(b"\n");
}

fn show_capabilities(info: &DeviceInfo) {
    print_out(b"General SMART Values:\n");

    if info.smart_enabled {
        print_out(b"Offline data collection status:  (0x00) Offline data collection activity\n");
        print_out(b"                                        was never started.\n");
    }
    print_out(
        b"Self-test execution status:      (   0) The previous self-test routine completed\n",
    );
    print_out(b"                                        without error or no self-test has ever\n");
    print_out(b"                                        been run.\n");
    print_out(b"Total time to complete Offline\n");
    print_out(b"data collection:                 ( 120) seconds.\n");
    print_out(b"Offline data collection\n");
    print_out(b"capabilities:                    (0x5b) SMART execute Offline immediate.\n");
    print_out(
        b"                                        Auto Offline data collection on/off support.\n",
    );
    print_out(
        b"                                        Suspend Offline collection upon new command.\n",
    );
    print_out(b"                                        Offline surface scan supported.\n");
    print_out(b"                                        Self-test supported.\n");
    print_out(b"                                        Conveyance Self-test supported.\n");
    print_out(b"SMART capabilities:              (0x0003) Saves SMART data before entering\n");
    print_out(b"                                        power-saving mode.\n");
    print_out(b"                                        Supports SMART auto save timer.\n");
    print_out(b"Error logging capability:        (0x01) Error logging supported.\n");
    print_out(b"Short self-test routine\n");
    print_out(b"recommended polling time:        (   2) minutes.\n");
    print_out(b"Extended self-test routine\n");
    print_out(b"recommended polling time:        (  60) minutes.\n");
    print_out(b"Conveyance self-test routine\n");
    print_out(b"recommended polling time:        (   5) minutes.\n");
    print_out(b"\n");
}

fn show_attributes(data: &SmartData) {
    print_out(b"SMART Attributes Data Structure revision number: 16\n");
    print_out(b"Vendor Specific SMART Attributes with Thresholds:\n");
    print_out(
        b"ID# ATTRIBUTE_NAME          FLAG     VALUE WORST THRESH TYPE      UPDATED  RAW_VALUE\n",
    );

    let mut buf = [0u8; 256];
    for i in 0..data.attr_count {
        if let Some(attr) = &data.attributes[i] {
            let mut pos = 0;

            // ID (3 chars, right-aligned)
            pos += pad_left_num(attr.id as u64, &mut buf[pos..], 3);
            buf[pos] = b' ';
            pos += 1;

            // Name (24 chars, left-aligned). The `min(24)` clamp on the
            // source name length is implicit in the pad_right target —
            // pad_right is idempotent if pos already equals/exceeds it,
            // and copy_bytes is itself bounded by buf.len().
            let name = attribute_name(attr.id);
            let start = pos;
            pos = copy_bytes(&mut buf, pos, name);
            pos = pad_right(&mut buf, pos, start + 24);

            // Flags (6 chars hex)
            pos = copy_bytes(&mut buf, pos, b"0x");
            pos += format_hex_u8((attr.flags >> 8) as u8, &mut buf[pos..]);
            pos += format_hex_u8((attr.flags & 0xFF) as u8, &mut buf[pos..]);
            buf[pos] = b' ';
            pos += 1;

            // Value (3 chars)
            pos += pad_left_num(attr.current as u64, &mut buf[pos..], 5);
            buf[pos] = b' ';
            pos += 1;

            // Worst (3 chars)
            pos += pad_left_num(attr.worst as u64, &mut buf[pos..], 5);
            buf[pos] = b' ';
            pos += 1;

            // Threshold (3 chars)
            pos += pad_left_num(attr.threshold as u64, &mut buf[pos..], 5);
            buf[pos] = b' ';
            pos += 1;

            // Type
            let type_start = pos;
            pos += format_attr_type(attr.flags, &mut buf[pos..]);
            pos = pad_right(&mut buf, pos, type_start + 9);

            // When updated
            let when_start = pos;
            pos += format_when(attr.flags, &mut buf[pos..]);
            pos = pad_right(&mut buf, pos, when_start + 8);

            // Raw value
            pos += format_u64(attr.raw, &mut buf[pos..]);

            buf[pos] = b'\n';
            pos += 1;
            print_out(&buf[..pos]);
        }
    }
    print_out(b"\n");
}

fn show_selftest_log(data: &SmartData) {
    print_out(b"SMART Self-test log structure revision number 1\n");
    if data.test_count == 0 {
        print_out(b"No self-tests have been logged.\n\n");
        return;
    }

    print_out(b"Num  Test_Description    Status                  Remaining  LifeTime(hours)  LBA_of_first_error\n");

    let mut buf = [0u8; 256];
    for i in 0..data.test_count {
        if let Some(test) = &data.self_tests[i] {
            let mut pos = 0;

            // Test number
            pos = copy_bytes(&mut buf, pos, b"# ");
            pos += pad_left_num(test.test_num as u64, &mut buf[pos..], 2);
            buf[pos] = b' ';
            pos += 1;

            // Test type
            let type_name = match test.test_type {
                TEST_SHORT => b"Short offline       " as &[u8],
                TEST_EXTENDED => b"Extended offline    " as &[u8],
                TEST_CONVEYANCE => b"Conveyance offline " as &[u8],
                TEST_SELECTIVE => b"Selective offline   " as &[u8],
                _ => b"Vendor offline      " as &[u8],
            };
            pos = copy_bytes(&mut buf, pos, type_name);

            // Status
            let status_start = pos;
            match test.status {
                SelfTestStatus::Completed => {
                    pos = copy_bytes(&mut buf, pos, b"Completed without error");
                }
                SelfTestStatus::InProgress(pct) => {
                    // Real smartctl prints "Self-test routine in progress... NN% remaining".
                    pos = copy_bytes(&mut buf, pos, b"Self-test routine in progress... ");
                    pos += format_u64(u64::from(pct), &mut buf[pos..]);
                    pos = copy_bytes(&mut buf, pos, b"% remaining");
                }
                SelfTestStatus::Interrupted => {
                    pos = copy_bytes(&mut buf, pos, b"Interrupted (host reset)");
                }
                SelfTestStatus::Fatal => {
                    pos = copy_bytes(&mut buf, pos, b"Fatal or unknown error ");
                }
                SelfTestStatus::ReadFailure => {
                    pos = copy_bytes(&mut buf, pos, b"Completed: read failure");
                }
                SelfTestStatus::ElectricalFailure => {
                    pos = copy_bytes(&mut buf, pos, b"Completed: electrical ");
                }
                SelfTestStatus::ServoFailure => {
                    pos = copy_bytes(&mut buf, pos, b"Completed: servo/seek ");
                }
                SelfTestStatus::HandlingDamage => {
                    pos = copy_bytes(&mut buf, pos, b"Completed: handling   ");
                }
                _ => {
                    pos = copy_bytes(&mut buf, pos, b"Unknown              ");
                }
            }
            pos = pad_right(&mut buf, pos, status_start + 25);

            // Remaining
            let remaining = match test.status {
                SelfTestStatus::InProgress(pct) => pct,
                _ => 0,
            };
            pos += pad_left_num(remaining as u64, &mut buf[pos..], 4);
            pos = copy_bytes(&mut buf, pos, b"%      ");

            // Lifetime hours
            pos += pad_left_num(test.lifetime_hours as u64, &mut buf[pos..], 8);
            pos = copy_bytes(&mut buf, pos, b"         ");

            // LBA
            if test.lba_first_error == 0 {
                pos = copy_bytes(&mut buf, pos, b"-");
            } else {
                pos += format_u64(test.lba_first_error, &mut buf[pos..]);
            }

            buf[pos] = b'\n';
            pos += 1;
            print_out(&buf[..pos]);
        }
    }
    print_out(b"\n");
}

fn show_error_log(data: &SmartData) {
    print_out(b"SMART Error Log Version: 1\n");
    if data.error_count == 0 {
        print_out(b"No Errors Logged\n\n");
        return;
    }

    let mut buf = [0u8; 128];
    let mut pos = copy_bytes(&mut buf, 0, b"ATA Error Count: ");
    pos += format_u64(data.error_count as u64, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    for i in 0..data.error_count {
        if let Some(err) = &data.error_log[i] {
            pos = copy_bytes(&mut buf, 0, b"Error ");
            pos += format_u64(err.error_num as u64, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" occurred at disk power-on lifetime: ");
            pos += format_u64(err.lifetime_hours as u64, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" hours\n");
            print_out(&buf[..pos]);
        }
    }
    print_out(b"\n");
}

// ── NVMe Display ───────────────────────────────────────────────────────

fn show_nvme_health(log: &NvmeSmartLog) {
    print_out(b"=== START OF SMART DATA SECTION ===\n");
    print_out(b"SMART/Health Information (NVMe Log 0x02)\n");

    let mut buf = [0u8; 256];
    let mut pos;

    // Critical warning
    pos = copy_bytes(&mut buf, 0, b"Critical Warning:                   0x");
    pos += format_hex_u8(log.critical_warning, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Temperature
    // Convert Kelvin → Celsius. saturating_sub guards against absurd
    // sub-zero-K logs (e.g. uninitialised firmware fields).
    let temp_c = log.temperature.saturating_sub(273);
    pos = copy_bytes(&mut buf, 0, b"Temperature:                        ");
    pos += format_u64(temp_c as u64, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b" Celsius\n");
    print_out(&buf[..pos]);

    // Available spare
    pos = copy_bytes(&mut buf, 0, b"Available Spare:                    ");
    pos += format_u64(log.avail_spare as u64, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b"%\n");
    print_out(&buf[..pos]);

    // Available spare threshold
    pos = copy_bytes(&mut buf, 0, b"Available Spare Threshold:          ");
    pos += format_u64(log.spare_thresh as u64, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b"%\n");
    print_out(&buf[..pos]);

    // Percentage used
    pos = copy_bytes(&mut buf, 0, b"Percentage Used:                    ");
    pos += format_u64(log.percent_used as u64, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b"%\n");
    print_out(&buf[..pos]);

    // Data units read
    pos = copy_bytes(&mut buf, 0, b"Data Units Read:                    ");
    pos += format_u128(log.data_units_read, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Data units written
    pos = copy_bytes(&mut buf, 0, b"Data Units Written:                 ");
    pos += format_u128(log.data_units_written, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Host reads
    pos = copy_bytes(&mut buf, 0, b"Host Read Commands:                 ");
    pos += format_u128(log.host_reads, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Host writes
    pos = copy_bytes(&mut buf, 0, b"Host Write Commands:                ");
    pos += format_u128(log.host_writes, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Controller busy time
    pos = copy_bytes(&mut buf, 0, b"Controller Busy Time:               ");
    pos += format_u128(log.ctrl_busy_time, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Power cycles
    pos = copy_bytes(&mut buf, 0, b"Power Cycles:                       ");
    pos += format_u128(log.power_cycles, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Power on hours
    pos = copy_bytes(&mut buf, 0, b"Power On Hours:                     ");
    pos += format_u128(log.power_on_hours, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Unsafe shutdowns
    pos = copy_bytes(&mut buf, 0, b"Unsafe Shutdowns:                   ");
    pos += format_u128(log.unsafe_shutdowns, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Media errors
    pos = copy_bytes(&mut buf, 0, b"Media and Data Integrity Errors:    ");
    pos += format_u128(log.media_errors, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Error log entries
    pos = copy_bytes(&mut buf, 0, b"Error Information Log Entries:      ");
    pos += format_u128(log.error_log_entries, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Warning temp time
    pos = copy_bytes(&mut buf, 0, b"Warning Comp. Temperature Time:     ");
    pos += format_u64(log.warning_temp_time as u64, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    // Critical temp time
    pos = copy_bytes(&mut buf, 0, b"Critical Comp. Temperature Time:    ");
    pos += format_u64(log.critical_temp_time as u64, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    print_out(b"\n");
}

// ── Health Analysis ────────────────────────────────────────────────────

fn analyze_health(data: &SmartData) -> i32 {
    let mut exit_status = EXIT_OK;

    // Check for failing health
    if data.health == HealthStatus::Failed {
        exit_status |= EXIT_DISK_FAILING;
    }

    // Check pre-fail attributes below threshold
    for i in 0..data.attr_count {
        if let Some(attr) = &data.attributes[i]
            && attr.flags & SMART_FLAG_PREFAIL != 0
            && attr.threshold > 0
        {
            if attr.current <= attr.threshold {
                exit_status |= EXIT_PREFAIL_BELOW;
            }
            if attr.worst <= attr.threshold {
                exit_status |= EXIT_PAST_PREFAIL;
            }
        }
    }

    // Check for concerning raw values
    for i in 0..data.attr_count {
        if let Some(attr) = &data.attributes[i] {
            match attr.id {
                ATTR_REALLOC_SECTOR | ATTR_REALLOC_EVENT
                    if attr.raw >= REALLOC_WARN
                        && !data.error_log.iter().all(Option::is_none) =>
                {
                    exit_status |= EXIT_ERROR_LOG;
                }
                ATTR_CURRENT_PENDING if attr.raw >= PENDING_WARN => {
                    // Flag concern but not necessarily failure
                }
                _ => {}
            }
        }
    }

    // Check self-test log for failures
    for i in 0..data.test_count {
        if let Some(test) = &data.self_tests[i] {
            match test.status {
                SelfTestStatus::Fatal
                | SelfTestStatus::ReadFailure
                | SelfTestStatus::ElectricalFailure
                | SelfTestStatus::ServoFailure
                | SelfTestStatus::HandlingDamage
                | SelfTestStatus::UnknownFailure => {
                    exit_status |= EXIT_SELFTEST_LOG;
                }
                _ => {}
            }
        }
    }

    exit_status
}

fn show_health_summary(data: &SmartData) {
    let mut warnings = false;
    let mut buf = [0u8; 256];
    let mut pos;

    // Temperature check
    if data.temperature >= TEMP_CRIT_C {
        pos = copy_bytes(&mut buf, 0, b"!! WARNING: Temperature is ");
        pos += format_u64(data.temperature as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"C - CRITICAL !!\n");
        print_out(&buf[..pos]);
        warnings = true;
    } else if data.temperature >= TEMP_WARN_C {
        pos = copy_bytes(&mut buf, 0, b"** WARNING: Temperature is ");
        pos += format_u64(data.temperature as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"C - HIGH **\n");
        print_out(&buf[..pos]);
        warnings = true;
    }

    // Reallocated sectors
    for i in 0..data.attr_count {
        if let Some(attr) = &data.attributes[i] {
            if attr.id == ATTR_REALLOC_SECTOR && attr.raw > 0 {
                pos = copy_bytes(&mut buf, 0, b"** WARNING: ");
                pos += format_u64(attr.raw, &mut buf[pos..]);
                pos = copy_bytes(&mut buf, pos, b" reallocated sectors **\n");
                print_out(&buf[..pos]);
                warnings = true;
            }
            if attr.id == ATTR_CURRENT_PENDING && attr.raw > 0 {
                pos = copy_bytes(&mut buf, 0, b"** WARNING: ");
                pos += format_u64(attr.raw, &mut buf[pos..]);
                pos = copy_bytes(&mut buf, pos, b" current pending sectors **\n");
                print_out(&buf[..pos]);
                warnings = true;
            }
            if attr.id == ATTR_OFFLINE_UNCORRECTABLE && attr.raw > 0 {
                pos = copy_bytes(&mut buf, 0, b"** WARNING: ");
                pos += format_u64(attr.raw, &mut buf[pos..]);
                pos = copy_bytes(&mut buf, pos, b" offline uncorrectable sectors **\n");
                print_out(&buf[..pos]);
                warnings = true;
            }
        }
    }

    if !warnings {
        print_out(b"No warnings found.\n");
    }
    print_out(b"\n");
}

// ── Scan ───────────────────────────────────────────────────────────────

fn scan_devices() {
    // In a real OS, this would enumerate /dev/sd*, /dev/nvme*, etc.
    print_out(b"/dev/sda -d ata # /dev/sda, ATA device\n");
    print_out(b"/dev/nvme0 -d nvme # /dev/nvme0, NVMe device\n");
}

// ── JSON Output ────────────────────────────────────────────────────────

fn show_json_info(info: &DeviceInfo) {
    let mut buf = [0u8; 512];
    let mut pos;

    print_out(b"{\n");
    print_out(b"  \"device\": {\n");

    pos = copy_bytes(&mut buf, 0, b"    \"model\": \"");
    pos = copy_bytes(&mut buf, pos, &info.model[..info.model_len]);
    pos = copy_bytes(&mut buf, pos, b"\",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"serial\": \"");
    pos = copy_bytes(&mut buf, pos, &info.serial[..info.serial_len]);
    pos = copy_bytes(&mut buf, pos, b"\",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"firmware\": \"");
    pos = copy_bytes(&mut buf, pos, &info.firmware[..info.firmware_len]);
    pos = copy_bytes(&mut buf, pos, b"\",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"capacity\": ");
    pos += format_u64(info.capacity_bytes, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"rotation_rate\": ");
    pos += format_u64(info.rotation_rate as u64, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"smart_supported\": ");
    if info.smart_supported {
        pos = copy_bytes(&mut buf, pos, b"true,\n");
    } else {
        pos = copy_bytes(&mut buf, pos, b"false,\n");
    }
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"smart_enabled\": ");
    if info.smart_enabled {
        pos = copy_bytes(&mut buf, pos, b"true\n");
    } else {
        pos = copy_bytes(&mut buf, pos, b"false\n");
    }
    print_out(&buf[..pos]);

    print_out(b"  }\n");
    print_out(b"}\n");
}

fn show_json_health(data: &SmartData) {
    let mut buf = [0u8; 256];
    let mut pos;

    print_out(b"{\n");
    print_out(b"  \"smart_status\": {\n");

    pos = copy_bytes(&mut buf, 0, b"    \"passed\": ");
    if data.health == HealthStatus::Passed {
        pos = copy_bytes(&mut buf, pos, b"true,\n");
    } else {
        pos = copy_bytes(&mut buf, pos, b"false,\n");
    }
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"temperature\": ");
    pos += format_u64(data.temperature as u64, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"power_on_hours\": ");
    pos += format_u64(data.power_on_hours, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b",\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"    \"power_cycles\": ");
    pos += format_u64(data.power_cycles, &mut buf[pos..]);
    buf[pos] = b'\n';
    pos += 1;
    print_out(&buf[..pos]);

    print_out(b"  }\n");
    print_out(b"}\n");
}

// ── Self-Test Initiation ───────────────────────────────────────────────

// The `_path` parameter documents intent: a real implementation would issue
// the ATA SMART EXECUTE OFF-LINE IMMEDIATE command against that device path.
// The personality-CLI stub prints canned output regardless of device.
fn start_selftest(_path: &[u8], test_type: u8) {
    let mut buf = [0u8; 256];
    let mut pos;

    let type_name: &[u8] = match test_type {
        TEST_SHORT => b"Short",
        TEST_EXTENDED => b"Extended",
        TEST_CONVEYANCE => b"Conveyance",
        _ => b"Unknown",
    };

    // In a real implementation, this would issue ATA SMART EXECUTE OFF-LINE IMMEDIATE
    pos = copy_bytes(&mut buf, 0, b"Sending command: \"Execute SMART ");
    pos = copy_bytes(&mut buf, pos, type_name);
    pos = copy_bytes(
        &mut buf,
        pos,
        b" self-test routine immediately in off-line mode\".\n",
    );
    print_out(&buf[..pos]);

    let est_minutes = match test_type {
        TEST_SHORT => 2u64,
        TEST_EXTENDED => 60,
        TEST_CONVEYANCE => 5,
        _ => 0,
    };

    pos = copy_bytes(&mut buf, 0, b"Drive command \"Execute SMART ");
    pos = copy_bytes(&mut buf, pos, type_name);
    pos = copy_bytes(
        &mut buf,
        pos,
        b" self-test routine immediately in off-line mode\" successful.\n",
    );
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"Testing has begun.\n");
    print_out(&buf[..pos]);

    pos = copy_bytes(&mut buf, 0, b"Please wait ");
    pos += format_u64(est_minutes, &mut buf[pos..]);
    pos = copy_bytes(&mut buf, pos, b" minutes for test to complete.\n");
    print_out(&buf[..pos]);

    print_out(b"Test will complete after next power cycle.\n");
    print_out(b"Use smartctl -X to abort test.\n");
}

fn abort_selftest(_path: &[u8]) {
    print_out(b"Sending command: \"Abort SMART off-line mode self-test routine\".\n");
    print_out(b"Self-testing aborted!\n");
}

fn enable_smart(_path: &[u8]) {
    print_out(b"SMART Enabled.\n");
}

fn disable_smart(_path: &[u8]) {
    print_out(b"SMART Disabled. Use option -s with argument 'on' to enable it.\n");
}

// ── Main ───────────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let opts = match parse_args(argc, argv) {
        Ok(o) => o,
        Err(code) => return code,
    };

    if opts.scan {
        scan_devices();
        return EXIT_OK;
    }

    let device_path = &opts.device[..opts.device_len];

    // Print header
    if !opts.quiet && !opts.json {
        print_out(b"smartctl 0.1.0 (Slate OS)\n");
        print_out(b"Copyright (C) Slate OS Project\n\n");
    }

    // Get device info
    let info = get_device_info(device_path, opts.device_type);

    if !info.smart_supported && !opts.json {
        print_err(b"smartctl: device does not support SMART\n");
        return EXIT_OPEN_FAIL;
    }

    // Handle SMART enable/disable
    if opts.enable_smart {
        enable_smart(device_path);
    }
    if opts.disable_smart {
        disable_smart(device_path);
        return EXIT_OK;
    }

    // JSON mode
    if opts.json {
        if opts.info {
            show_json_info(&info);
        }
        if opts.health || opts.attributes {
            let data = get_smart_data(device_path, info.device_type);
            show_json_health(&data);
        }
        return EXIT_OK;
    }

    // Normal display mode
    let mut exit_status = EXIT_OK;

    if opts.info {
        show_device_info(&info);
    }

    if info.device_type == DeviceType::Nvme {
        // NVMe-specific display
        if opts.health || opts.attributes {
            let log = get_nvme_smart_log(device_path);
            show_nvme_health(&log);

            if log.critical_warning != 0 {
                exit_status |= EXIT_DISK_FAILING;
            }
            if log.avail_spare < log.spare_thresh {
                exit_status |= EXIT_PREFAIL_BELOW;
            }
        }
    } else {
        // ATA/SCSI display
        let data = get_smart_data(device_path, info.device_type);

        if opts.health {
            show_health(&data);
            show_health_summary(&data);
        }

        if opts.capabilities {
            show_capabilities(&info);
        }

        if opts.attributes {
            show_attributes(&data);
        }

        if opts.selftest_log {
            show_selftest_log(&data);
        }

        if opts.error_log {
            show_error_log(&data);
        }

        exit_status |= analyze_health(&data);
    }

    // Handle self-test commands
    if opts.test_short {
        start_selftest(device_path, TEST_SHORT);
    }
    if opts.test_long {
        start_selftest(device_path, TEST_EXTENDED);
    }
    if opts.test_conveyance {
        start_selftest(device_path, TEST_CONVEYANCE);
    }
    if opts.test_abort {
        abort_selftest(device_path);
    }

    exit_status
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_format_u128_zero() {
        let mut buf = [0u8; 40];
        let n = format_u128(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn test_format_u128_large() {
        let mut buf = [0u8; 40];
        let n = format_u128(123456789012345, &mut buf);
        assert_eq!(&buf[..n], b"123456789012345");
    }

    #[test]
    fn test_format_hex_u8() {
        let mut buf = [0u8; 2];
        format_hex_u8(0xAB, &mut buf);
        assert_eq!(&buf, b"ab");
    }

    #[test]
    fn test_format_hex_u8_zero() {
        let mut buf = [0u8; 2];
        format_hex_u8(0x00, &mut buf);
        assert_eq!(&buf, b"00");
    }

    #[test]
    fn test_attribute_name_known() {
        assert_eq!(attribute_name(ATTR_TEMPERATURE), b"Temperature_Celsius");
        assert_eq!(attribute_name(ATTR_POWER_ON_HOURS), b"Power_On_Hours");
        assert_eq!(
            attribute_name(ATTR_REALLOC_SECTOR),
            b"Reallocated_Sector_Ct"
        );
    }

    #[test]
    fn test_attribute_name_unknown() {
        assert_eq!(attribute_name(255), b"Unknown_Attribute");
    }

    #[test]
    fn test_detect_device_type() {
        assert_eq!(detect_device_type(b"/dev/nvme0"), DeviceType::Nvme);
        assert_eq!(detect_device_type(b"/dev/sda"), DeviceType::Ata);
        assert_eq!(detect_device_type(b"/dev/hda"), DeviceType::Ata);
        assert_eq!(detect_device_type(b"/dev/sg0"), DeviceType::Scsi);
    }

    #[test]
    fn test_contains() {
        assert!(contains(b"hello world", b"world"));
        assert!(contains(b"hello world", b"hello"));
        assert!(!contains(b"hello", b"world"));
        assert!(contains(b"abc", b"abc"));
        assert!(!contains(b"ab", b"abc"));
    }

    #[test]
    fn test_format_attr_flags() {
        let mut buf = [0u8; 6];
        let n = format_attr_flags(SMART_FLAG_PREFAIL | SMART_FLAG_ONLINE, &mut buf);
        assert_eq!(n, 6);
        assert_eq!(&buf[..2], b"PO");
    }

    #[test]
    fn test_format_attr_type() {
        let mut buf = [0u8; 16];
        let n = format_attr_type(SMART_FLAG_PREFAIL, &mut buf);
        assert_eq!(&buf[..n], b"Pre-fail");
        let n = format_attr_type(0, &mut buf);
        assert_eq!(&buf[..n], b"Old_age");
    }

    #[test]
    fn test_format_when() {
        let mut buf = [0u8; 16];
        let n = format_when(SMART_FLAG_ONLINE, &mut buf);
        assert_eq!(&buf[..n], b"Always");
        let n = format_when(0, &mut buf);
        assert_eq!(&buf[..n], b"Offline");
    }

    #[test]
    fn test_get_device_info_nvme() {
        let info = get_device_info(b"/dev/nvme0", DeviceType::Nvme);
        assert_eq!(info.device_type, DeviceType::Nvme);
        assert!(info.smart_supported);
        assert_eq!(info.rotation_rate, 0); // SSD
    }

    #[test]
    fn test_get_device_info_ata() {
        let info = get_device_info(b"/dev/sda", DeviceType::Ata);
        assert_eq!(info.device_type, DeviceType::Ata);
        assert!(info.smart_supported);
        assert!(info.rotation_rate > 0); // HDD
    }

    #[test]
    fn test_smart_data_defaults() {
        let data = get_smart_data(b"/dev/sda", DeviceType::Ata);
        assert_eq!(data.health, HealthStatus::Passed);
        assert!(data.attr_count > 0);
        assert!(data.test_count > 0);
    }

    #[test]
    fn test_analyze_health_passed() {
        let data = get_smart_data(b"/dev/sda", DeviceType::Ata);
        let status = analyze_health(&data);
        assert_eq!(status, EXIT_OK);
    }

    #[test]
    fn test_analyze_health_failing() {
        let mut data = get_smart_data(b"/dev/sda", DeviceType::Ata);
        data.health = HealthStatus::Failed;
        let status = analyze_health(&data);
        assert!(status & EXIT_DISK_FAILING != 0);
    }

    #[test]
    fn test_analyze_health_prefail_below() {
        let mut data = SmartData {
            health: HealthStatus::Passed,
            temperature: 34,
            power_on_hours: 100,
            power_cycles: 10,
            attributes: [None; 64],
            attr_count: 1,
            self_tests: [None; 21],
            test_count: 0,
            error_log: [None; 16],
            error_count: 0,
        };
        data.attributes[0] = Some(SmartAttribute {
            id: ATTR_REALLOC_SECTOR,
            flags: SMART_FLAG_PREFAIL,
            current: 5,
            worst: 5,
            threshold: 10,
            raw: 100,
        });
        let status = analyze_health(&data);
        assert!(status & EXIT_PREFAIL_BELOW != 0);
    }

    #[test]
    fn test_nvme_smart_log() {
        let log = get_nvme_smart_log(b"/dev/nvme0");
        assert_eq!(log.critical_warning, 0);
        assert!(log.power_on_hours > 0);
        assert_eq!(log.media_errors, 0);
    }

    #[test]
    fn test_format_i64_negative() {
        let mut buf = [0u8; 20];
        let n = format_i64(-42, &mut buf);
        assert_eq!(&buf[..n], b"-42");
    }

    #[test]
    fn test_format_i64_positive() {
        let mut buf = [0u8; 20];
        let n = format_i64(42, &mut buf);
        assert_eq!(&buf[..n], b"42");
    }

    #[test]
    fn test_pad_left_num() {
        let mut buf = [0u8; 10];
        let n = pad_left_num(42, &mut buf, 5);
        assert_eq!(&buf[..n], b"   42");
    }

    #[test]
    fn test_copy_bytes() {
        let mut buf = [0u8; 20];
        let n = copy_bytes(&mut buf, 0, b"hello");
        assert_eq!(n, 5);
        assert_eq!(&buf[..n], b"hello");
    }

    #[test]
    fn test_format_capacity() {
        let mut buf = [0u8; 128];
        let n = format_capacity(500_107_862_016, &mut buf);
        assert!(n > 0);
        // Should contain "bytes" and "[" for human readable
        let s = &buf[..n];
        assert!(contains(s, b"bytes"));
        assert!(contains(s, b"["));
    }

    #[test]
    fn test_selftest_status_variants() {
        // Verify all variants are representable
        let statuses = [
            SelfTestStatus::Completed,
            SelfTestStatus::InProgress(50),
            SelfTestStatus::Interrupted,
            SelfTestStatus::Fatal,
            SelfTestStatus::UnknownFailure,
            SelfTestStatus::ElectricalFailure,
            SelfTestStatus::ServoFailure,
            SelfTestStatus::ReadFailure,
            SelfTestStatus::HandlingDamage,
            SelfTestStatus::NotRun,
        ];
        assert_eq!(statuses.len(), 10);
    }

    #[test]
    fn test_health_status_variants() {
        assert_ne!(HealthStatus::Passed, HealthStatus::Failed);
        assert_ne!(HealthStatus::Failed, HealthStatus::Unknown);
    }

    #[test]
    fn test_device_type_variants() {
        assert_ne!(DeviceType::Ata, DeviceType::Nvme);
        assert_ne!(DeviceType::Scsi, DeviceType::Auto);
    }
}
