#![deny(clippy::all)]

//! nvme — OurOS NVMe drive management CLI
//!
//! Multi-personality binary for managing NVMe solid-state drives.
//! Detected via argv[0]:
//!
//! - `nvme` (default) — NVMe management tool
//! - `nvme-connect` — connect to NVMe-oF (NVMe over Fabrics) targets
//! - `nvme-discover` — discover NVMe-oF subsystems

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _NVME_DEV_DIR: &str = "/dev";
const _NVME_SYS_DIR: &str = "/sys/class/nvme";
const _NVME_FABRICS: &str = "/dev/nvme-fabrics";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct NvmeController {
    name: String,
    model: String,
    serial: String,
    firmware: String,
    _transport: NvmeTransport,
    _pci_addr: String,
    namespaces: Vec<NvmeNamespace>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NvmeTransport {
    _PCIe,
    _RDMA,
    _TCP,
    _FC,
}

impl std::fmt::Display for NvmeTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_PCIe => write!(f, "PCIe"),
            Self::_RDMA => write!(f, "RDMA"),
            Self::_TCP => write!(f, "TCP"),
            Self::_FC => write!(f, "FC"),
        }
    }
}

#[derive(Clone, Debug)]
struct NvmeNamespace {
    nsid: u32,
    _dev_path: String,
    size_bytes: u64,
    capacity_bytes: u64,
    _sector_size: u32,
    _format_lba: u8,
    _metadata_size: u16,
    _eui64: String,
    _nguid: String,
}

#[derive(Clone, Debug)]
struct SmartLog {
    critical_warning: u8,
    temperature: u16,  // Kelvin
    avail_spare: u8,
    avail_spare_threshold: u8,
    percent_used: u8,
    data_units_read: u64,
    data_units_written: u64,
    host_read_commands: u64,
    host_write_commands: u64,
    controller_busy_time: u64,
    power_cycles: u64,
    power_on_hours: u64,
    unsafe_shutdowns: u64,
    media_errors: u64,
    num_err_log_entries: u64,
    _warning_temp_time: u32,
    _critical_temp_time: u32,
}

#[derive(Clone, Debug)]
struct ErrorLog {
    _entry_count: u32,
    entries: Vec<ErrorLogEntry>,
}

#[derive(Clone, Debug)]
struct ErrorLogEntry {
    error_count: u64,
    sqid: u16,
    cmdid: u16,
    status: u16,
    _location: u16,
    lba: u64,
    _nsid: u32,
    _command_specific: u64,
}

#[derive(Clone, Debug)]
struct _FabricsTarget {
    _subnqn: String,
    _transport: NvmeTransport,
    _traddr: String,
    _trsvcid: String,
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_controllers() -> Vec<NvmeController> {
    vec![
        NvmeController {
            name: "nvme0".to_string(),
            model: "Samsung 980 PRO 1TB".to_string(),
            serial: "S5P2NG0R123456".to_string(),
            firmware: "5B2QGXA7".to_string(),
            _transport: NvmeTransport::_PCIe,
            _pci_addr: "0000:01:00.0".to_string(),
            namespaces: vec![
                NvmeNamespace {
                    nsid: 1,
                    _dev_path: "/dev/nvme0n1".to_string(),
                    size_bytes: 1_000_204_886_016,
                    capacity_bytes: 1_000_204_886_016,
                    _sector_size: 512,
                    _format_lba: 0,
                    _metadata_size: 0,
                    _eui64: "0025385b21406e53".to_string(),
                    _nguid: "0025385b21406e530001000100000001".to_string(),
                },
            ],
        },
        NvmeController {
            name: "nvme1".to_string(),
            model: "WD Black SN850X 2TB".to_string(),
            serial: "WD-WXK123456789".to_string(),
            firmware: "613200WD".to_string(),
            _transport: NvmeTransport::_PCIe,
            _pci_addr: "0000:02:00.0".to_string(),
            namespaces: vec![
                NvmeNamespace {
                    nsid: 1,
                    _dev_path: "/dev/nvme1n1".to_string(),
                    size_bytes: 2_000_398_934_016,
                    capacity_bytes: 2_000_398_934_016,
                    _sector_size: 512,
                    _format_lba: 0,
                    _metadata_size: 0,
                    _eui64: "e8238fa6bf530001".to_string(),
                    _nguid: "e8238fa6bf5300010001000100000001".to_string(),
                },
            ],
        },
    ]
}

fn read_smart_log(_ctrl: &str) -> SmartLog {
    SmartLog {
        critical_warning: 0,
        temperature: 312,  // 39°C
        avail_spare: 100,
        avail_spare_threshold: 10,
        percent_used: 2,
        data_units_read: 12_345_678,
        data_units_written: 9_876_543,
        host_read_commands: 234_567_890,
        host_write_commands: 123_456_789,
        controller_busy_time: 1234,
        power_cycles: 456,
        power_on_hours: 5678,
        unsafe_shutdowns: 12,
        media_errors: 0,
        num_err_log_entries: 3,
        _warning_temp_time: 0,
        _critical_temp_time: 0,
    }
}

fn read_error_log(_ctrl: &str) -> ErrorLog {
    ErrorLog {
        _entry_count: 2,
        entries: vec![
            ErrorLogEntry {
                error_count: 3,
                sqid: 0,
                cmdid: 0x0012,
                status: 0x4004,
                _location: 0,
                lba: 0x0000_0001_2345_6789,
                _nsid: 1,
                _command_specific: 0,
            },
            ErrorLogEntry {
                error_count: 2,
                sqid: 1,
                cmdid: 0x0034,
                status: 0x4281,
                _location: 0,
                lba: 0x0000_0000_ABCD_EF00,
                _nsid: 1,
                _command_specific: 0,
            },
        ],
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.2} TB", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_data_units(units: u64) -> String {
    // Each data unit = 512 KiB = 1000 * 512 bytes
    let bytes = units * 512 * 1000;
    format_size(bytes)
}

// ── nvme personality ──────────────────────────────────────────────────

fn run_nvme(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "list".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: nvme <command> [<device>] [OPTIONS]");
            println!();
            println!("NVMe management tool for OurOS.");
            println!();
            println!("Commands:");
            println!("  list              List all NVMe devices (default)");
            println!("  list-ns DEV       List namespaces on a controller");
            println!("  id-ctrl DEV       Show controller identify data");
            println!("  id-ns DEV         Show namespace identify data");
            println!("  smart-log DEV     Show SMART/health information");
            println!("  error-log DEV     Show error log");
            println!("  fw-log DEV        Show firmware slot info");
            println!("  get-feature DEV   Get a feature value");
            println!("  set-feature DEV   Set a feature value");
            println!("  format DEV        Format a namespace");
            println!("  sanitize DEV      Sanitize the NVM subsystem");
            println!("  reset DEV         Reset the controller");
            println!("  subsystem-reset DEV  Reset the NVM subsystem");
            println!("  fw-download DEV   Download firmware");
            println!("  fw-commit DEV     Commit firmware to a slot");
            println!("  connect           Connect to NVMe-oF target");
            println!("  discover          Discover NVMe-oF subsystems");
            println!("  disconnect        Disconnect from NVMe-oF target");
            println!("  version           Show version");
            0
        }
        "version" | "--version" | "-V" => {
            println!("nvme-cli 0.1.0 (OurOS)");
            0
        }
        "list" => cmd_list(),
        "list-ns" => cmd_list_ns(&cmd_args),
        "id-ctrl" => cmd_id_ctrl(&cmd_args),
        "id-ns" => cmd_id_ns(&cmd_args),
        "smart-log" => cmd_smart_log(&cmd_args),
        "error-log" => cmd_error_log(&cmd_args),
        "fw-log" => cmd_fw_log(&cmd_args),
        "get-feature" => cmd_get_feature(&cmd_args),
        "set-feature" => cmd_set_feature(&cmd_args),
        "format" => cmd_format(&cmd_args),
        "sanitize" => cmd_sanitize(&cmd_args),
        "reset" => cmd_reset(&cmd_args),
        "subsystem-reset" => cmd_subsystem_reset(&cmd_args),
        "fw-download" => cmd_fw_download(&cmd_args),
        "fw-commit" => cmd_fw_commit(&cmd_args),
        "connect" => cmd_connect(&cmd_args),
        "discover" => cmd_discover(&cmd_args),
        "disconnect" => cmd_disconnect(&cmd_args),
        other => {
            eprintln!("nvme: unknown command '{}'", other);
            eprintln!("Try 'nvme --help' for more information.");
            1
        }
    }
}

fn cmd_list() -> i32 {
    let controllers = read_controllers();
    println!("Node             SN                   Model                                    Namespace Usage                      Format           FW Rev");
    println!("---------------- -------------------- ---------------------------------------- --------- -------------------------- ---------------- --------");

    for ctrl in &controllers {
        for ns in &ctrl.namespaces {
            println!("/dev/{}n{:<4} {:<20} {:<40} {:<9} {:>12} / {:<12} {:>3}B + {:>3} B    {}",
                ctrl.name, ns.nsid, ctrl.serial, ctrl.model,
                ns.nsid,
                format_size(ns.capacity_bytes),
                format_size(ns.size_bytes),
                ns._sector_size, ns._metadata_size,
                ctrl.firmware);
        }
    }
    0
}

fn get_ctrl_name(args: &[String]) -> &str {
    args.first().map(|s| s.as_str()).unwrap_or("nvme0")
}

fn cmd_list_ns(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let controllers = read_controllers();
    let ctrl = controllers.iter().find(|c| c.name == ctrl_name || format!("/dev/{}", c.name) == ctrl_name);

    match ctrl {
        Some(c) => {
            println!("Namespace List for {} ({}):", c.name, c.model);
            println!("[  0]: {:#010x}", c.namespaces.first().map_or(0, |n| n.nsid));
            for ns in c.namespaces.iter().skip(1) {
                println!("[  {}]: {:#010x}", ns.nsid - 1, ns.nsid);
            }
            0
        }
        None => {
            eprintln!("nvme: controller '{}' not found", ctrl_name);
            1
        }
    }
}

fn cmd_id_ctrl(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let controllers = read_controllers();
    let ctrl = controllers.iter().find(|c| c.name == ctrl_name || format!("/dev/{}", c.name) == ctrl_name);

    match ctrl {
        Some(c) => {
            println!("NVME Identify Controller:");
            println!("vid       : 0x144d");
            println!("ssvid     : 0x144d");
            println!("sn        : {}", c.serial);
            println!("mn        : {}", c.model);
            println!("fr        : {}", c.firmware);
            println!("rab       : 6");
            println!("ieee      : 002538");
            println!("cmic      : 0");
            println!("mdts      : 9");
            println!("cntlid    : 0x5");
            println!("ver       : 0x10400");
            println!("rtd3r     : 0x493e0");
            println!("rtd3e     : 0x7a120");
            println!("oaes      : 0x200");
            println!("ctratt    : 0x4");
            println!("rrls      : 0");
            println!("cntrltype : 1");
            println!("fguid     :");
            println!("crdt1     : 0");
            println!("crdt2     : 0");
            println!("crdt3     : 0");
            println!("oacs      : 0x17");
            println!("acl       : 3");
            println!("aerl      : 7");
            println!("frmw      : 0x16");
            println!("lpa       : 0x1e");
            println!("elpe      : 63");
            println!("npss      : 4");
            println!("sqes      : 0x66");
            println!("cqes      : 0x44");
            println!("maxcmd    : 0");
            println!("nn        : {}", c.namespaces.len());
            println!("oncs      : 0x5f");
            println!("fuses     : 0");
            println!("fna       : 0x4");
            println!("vwc       : 0x7");
            println!("awun      : 255");
            println!("awupf     : 0");
            println!("nvscc     : 1");
            0
        }
        None => {
            eprintln!("nvme: controller '{}' not found", ctrl_name);
            1
        }
    }
}

fn cmd_id_ns(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let controllers = read_controllers();
    let ctrl = controllers.iter().find(|c| c.name == ctrl_name || format!("/dev/{}", c.name) == ctrl_name);

    match ctrl {
        Some(c) => {
            if let Some(ns) = c.namespaces.first() {
                println!("NVME Identify Namespace {}:", ns.nsid);
                println!("nsze      : {}", ns.size_bytes / ns._sector_size as u64);
                println!("ncap      : {}", ns.capacity_bytes / ns._sector_size as u64);
                println!("nuse      : {}", ns.capacity_bytes / ns._sector_size as u64);
                println!("nsfeat    : 0");
                println!("nlbaf     : 1");
                println!("flbas     : {}", ns._format_lba);
                println!("mc        : 0");
                println!("dpc       : 0");
                println!("dps       : 0");
                println!("nmic      : 0");
                println!("nguid     : {}", ns._nguid);
                println!("eui64     : {}", ns._eui64);
                println!("lbaf  0   : ms:{:<4} lbads:{:<2} rp:0x{:x} (in use)",
                    ns._metadata_size, (ns._sector_size as f64).log2() as u32, 0);
            }
            0
        }
        None => {
            eprintln!("nvme: controller '{}' not found", ctrl_name);
            1
        }
    }
}

fn cmd_smart_log(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let smart = read_smart_log(ctrl_name);

    println!("Smart Log for NVME device:{} namespace-id:ffffffff", ctrl_name);
    println!("critical_warning                        : {}", smart.critical_warning);
    println!("temperature                             : {} K ({} °C)",
        smart.temperature, smart.temperature.saturating_sub(273));
    println!("available_spare                         : {}%", smart.avail_spare);
    println!("available_spare_threshold               : {}%", smart.avail_spare_threshold);
    println!("percentage_used                         : {}%", smart.percent_used);
    println!("data_units_read                         : {} ({})",
        smart.data_units_read, format_data_units(smart.data_units_read));
    println!("data_units_written                      : {} ({})",
        smart.data_units_written, format_data_units(smart.data_units_written));
    println!("host_read_commands                      : {}", smart.host_read_commands);
    println!("host_write_commands                     : {}", smart.host_write_commands);
    println!("controller_busy_time                    : {} minutes", smart.controller_busy_time);
    println!("power_cycles                            : {}", smart.power_cycles);
    println!("power_on_hours                          : {}", smart.power_on_hours);
    println!("unsafe_shutdowns                        : {}", smart.unsafe_shutdowns);
    println!("media_errors                            : {}", smart.media_errors);
    println!("num_err_log_entries                     : {}", smart.num_err_log_entries);
    0
}

fn cmd_error_log(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let log = read_error_log(ctrl_name);

    println!("Error Log for device:{}", ctrl_name);
    println!();

    for (i, entry) in log.entries.iter().enumerate() {
        println!("Entry[{}]:", i);
        println!("  error_count   : {}", entry.error_count);
        println!("  sqid          : {}", entry.sqid);
        println!("  cmdid         : {:#06x}", entry.cmdid);
        println!("  status_field  : {:#06x}", entry.status);
        println!("  lba           : {:#018x}", entry.lba);
        println!();
    }
    0
}

fn cmd_fw_log(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("Firmware Log for device:{}", ctrl_name);
    println!("afi  : 0x1");
    println!("frs1 : 5B2QGXA7 (active)");
    println!("frs2 : 5B2QGXA6");
    println!("frs3 : (empty)");
    println!("frs4 : (empty)");
    0
}

fn cmd_get_feature(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let feature_id = args.get(1)
        .and_then(|s| s.strip_prefix("--feature-id=").or(Some(s.as_str())))
        .unwrap_or("0x06");

    println!("get-feature:{} feature:{} value:0x00010001", ctrl_name, feature_id);
    0
}

fn cmd_set_feature(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("set-feature:{} (simulated)", ctrl_name);
    println!("Feature set successfully");
    0
}

fn cmd_format(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("WARNING: NVMe format will destroy all data on {}!", ctrl_name);
    println!("nvme format: formatting namespace (simulated)");
    println!("Format completed successfully");
    0
}

fn cmd_sanitize(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("WARNING: NVMe sanitize will securely erase all data on {}!", ctrl_name);
    println!("nvme sanitize: block erase (simulated)");
    println!("Sanitize initiated successfully");
    0
}

fn cmd_reset(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("nvme reset: resetting controller {} (simulated)", ctrl_name);
    println!("Controller reset successfully");
    0
}

fn cmd_subsystem_reset(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("nvme subsystem-reset: resetting NVM subsystem {} (simulated)", ctrl_name);
    println!("NVM subsystem reset successfully");
    0
}

fn cmd_fw_download(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    let fw_file = args.get(1).map(|s| s.as_str()).unwrap_or("firmware.bin");
    println!("nvme fw-download: downloading {} to {} (simulated)", fw_file, ctrl_name);
    println!("Firmware download complete");
    0
}

fn cmd_fw_commit(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("nvme fw-commit: committing firmware on {} to slot 1 (simulated)", ctrl_name);
    println!("Firmware commit successful. Reboot required to activate.");
    0
}

fn cmd_connect(args: &[String]) -> i32 {
    let transport = args.iter()
        .find(|a| a.starts_with("--transport=") || a.starts_with("-t="))
        .map(|a| a.split('=').nth(1).unwrap_or("tcp"))
        .unwrap_or("tcp");
    let traddr = args.iter()
        .find(|a| a.starts_with("--traddr=") || a.starts_with("-a="))
        .map(|a| a.split('=').nth(1).unwrap_or("192.168.1.100"))
        .unwrap_or("192.168.1.100");

    println!("nvme connect: transport={} traddr={}", transport, traddr);
    println!("Connected to NVMe-oF target (simulated)");
    println!("  Controller: nvme2");
    println!("  Subsystem NQN: nqn.2025-01.com.example:nvme");
    0
}

fn cmd_discover(args: &[String]) -> i32 {
    let traddr = args.iter()
        .find(|a| a.starts_with("--traddr=") || a.starts_with("-a="))
        .map(|a| a.split('=').nth(1).unwrap_or("192.168.1.100"))
        .unwrap_or("192.168.1.100");

    println!("Discovery Log Number of Records 2, Generation counter 1");
    println!("=====Discovery Log Entry 0======");
    println!("trtype: tcp");
    println!("adrfam: ipv4");
    println!("subtype: nvme subsystem");
    println!("treq:   not specified");
    println!("portid: 1");
    println!("trsvcid: 4420");
    println!("subnqn: nqn.2025-01.com.example:nvme.subsys.0");
    println!("traddr: {}", traddr);
    println!();
    println!("=====Discovery Log Entry 1======");
    println!("trtype: tcp");
    println!("adrfam: ipv4");
    println!("subtype: nvme subsystem");
    println!("treq:   not specified");
    println!("portid: 2");
    println!("trsvcid: 4420");
    println!("subnqn: nqn.2025-01.com.example:nvme.subsys.1");
    println!("traddr: {}", traddr);
    0
}

fn cmd_disconnect(args: &[String]) -> i32 {
    let ctrl_name = get_ctrl_name(args);
    println!("nvme disconnect: disconnecting {} (simulated)", ctrl_name);
    println!("Disconnected successfully");
    0
}

// ── nvme-connect personality ──────────────────────────────────────────

fn run_connect(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: nvme-connect [OPTIONS]");
            println!();
            println!("Connect to an NVMe over Fabrics target.");
            println!();
            println!("Options:");
            println!("  -t, --transport=TRANSPORT  Transport type (tcp/rdma/fc)");
            println!("  -a, --traddr=ADDR          Target address");
            println!("  -s, --trsvcid=PORT         Transport service ID (port)");
            println!("  -n, --nqn=NQN              Target NQN");
            println!("  --hostnqn=NQN              Host NQN");
            0
        }
        _ => cmd_connect(&args),
    }
}

// ── nvme-discover personality ─────────────────────────────────────────

fn run_discover(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: nvme-discover [OPTIONS]");
            println!();
            println!("Discover NVMe over Fabrics subsystems.");
            println!();
            println!("Options:");
            println!("  -t, --transport=TRANSPORT  Transport type (tcp/rdma/fc)");
            println!("  -a, --traddr=ADDR          Discovery controller address");
            println!("  -s, --trsvcid=PORT         Transport service ID (port)");
            0
        }
        _ => cmd_discover(&args),
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("nvme");
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
        "nvme-connect" => run_connect(rest),
        "nvme-discover" => run_discover(rest),
        _ => run_nvme(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_controllers() {
        let ctrls = read_controllers();
        assert_eq!(ctrls.len(), 2);
        assert!(ctrls[0].model.contains("Samsung"));
        assert!(ctrls[1].model.contains("WD"));
    }

    #[test]
    fn test_controller_namespaces() {
        let ctrls = read_controllers();
        assert_eq!(ctrls[0].namespaces.len(), 1);
        assert_eq!(ctrls[0].namespaces[0].nsid, 1);
    }

    #[test]
    fn test_smart_log() {
        let smart = read_smart_log("nvme0");
        assert_eq!(smart.critical_warning, 0);
        assert!(smart.temperature > 273); // above 0°C
        assert_eq!(smart.avail_spare, 100);
        assert_eq!(smart.media_errors, 0);
    }

    #[test]
    fn test_error_log() {
        let log = read_error_log("nvme0");
        assert_eq!(log.entries.len(), 2);
        assert!(log.entries[0].error_count > 0);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1_500_000), "1.50 MB");
        assert_eq!(format_size(1_000_000_000), "1.00 GB");
        assert_eq!(format_size(1_000_000_000_000), "1.00 TB");
    }

    #[test]
    fn test_format_data_units() {
        // 1 data unit = 512 * 1000 bytes = 512KB
        let s = format_data_units(2_000_000);
        assert!(s.contains("TB") || s.contains("GB"));
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(format!("{}", NvmeTransport::_PCIe), "PCIe");
        assert_eq!(format!("{}", NvmeTransport::_TCP), "TCP");
        assert_eq!(format!("{}", NvmeTransport::_RDMA), "RDMA");
        assert_eq!(format!("{}", NvmeTransport::_FC), "FC");
    }

    #[test]
    fn test_smart_temperature_conversion() {
        let smart = read_smart_log("nvme0");
        let celsius = smart.temperature.saturating_sub(273);
        assert!(celsius > 0 && celsius < 100);
    }
}
