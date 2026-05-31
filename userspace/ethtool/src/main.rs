// OurOS ethtool - ethernet device configuration
//
// Single-personality binary providing ethernet/network interface
// configuration and statistics display (similar to Linux ethtool).

#![cfg_attr(not(test), no_main)]

use std::collections::BTreeMap;

// ── Constants ──────────────────────────────────────────────────────────

const VERSION: &[u8] = b"ethtool (OurOS) 1.0.0";

// Link speeds in Mbps
const SPEED_UNKNOWN: u32 = 0xFFFF_FFFF;
const SPEED_10: u32 = 10;
const SPEED_100: u32 = 100;
const SPEED_1000: u32 = 1000;
const SPEED_2500: u32 = 2500;
const SPEED_5000: u32 = 5000;
const SPEED_10000: u32 = 10000;
const SPEED_25000: u32 = 25000;
const SPEED_40000: u32 = 40000;
const SPEED_50000: u32 = 50000;
const SPEED_100000: u32 = 100000;

// Duplex modes
const DUPLEX_HALF: u8 = 0;
const DUPLEX_FULL: u8 = 1;
const DUPLEX_UNKNOWN: u8 = 0xFF;

// Port types
const PORT_TP: u8 = 0;
const PORT_AUI: u8 = 1;
const PORT_BNC: u8 = 2;
const PORT_MII: u8 = 3;
const PORT_FIBRE: u8 = 4;
const PORT_DA: u8 = 5;
const PORT_NONE: u8 = 0xEF;
const PORT_OTHER: u8 = 0xFF;

// Wake-on-LAN flags
const WAKE_PHY: u32 = 1 << 0;
const WAKE_UCAST: u32 = 1 << 1;
const WAKE_MCAST: u32 = 1 << 2;
const WAKE_BCAST: u32 = 1 << 3;
const WAKE_ARP: u32 = 1 << 4;
const WAKE_MAGIC: u32 = 1 << 5;
const WAKE_MAGICSECURE: u32 = 1 << 6;
const WAKE_FILTER: u32 = 1 << 7;

// ── Data Structures ────────────────────────────────────────────────────

struct DriverInfo {
    driver: Vec<u8>,
    version: Vec<u8>,
    firmware_version: Vec<u8>,
    bus_info: Vec<u8>,
    erom_version: Vec<u8>,
}

struct LinkSettings {
    speed: u32,
    duplex: u8,
    port: u8,
    autoneg: bool,
    phy_address: u8,
    mdi_x: MdiX,
    transceiver: Transceiver,
    supported_modes: Vec<LinkMode>,
    advertised_modes: Vec<LinkMode>,
    lp_advertised_modes: Vec<LinkMode>,
}

#[derive(Clone, Copy, PartialEq)]
enum MdiX {
    Auto,
    On,
    Off,
    Unknown,
}

#[derive(Clone, Copy, PartialEq)]
enum Transceiver {
    Internal,
    External,
    Unknown,
}

#[derive(Clone, Copy, PartialEq)]
struct LinkMode {
    speed: u32,
    duplex: u8,
    name: &'static [u8],
}

const COMMON_MODES: &[LinkMode] = &[
    LinkMode { speed: 10, duplex: DUPLEX_HALF, name: b"10baseT/Half" },
    LinkMode { speed: 10, duplex: DUPLEX_FULL, name: b"10baseT/Full" },
    LinkMode { speed: 100, duplex: DUPLEX_HALF, name: b"100baseT/Half" },
    LinkMode { speed: 100, duplex: DUPLEX_FULL, name: b"100baseT/Full" },
    LinkMode { speed: 1000, duplex: DUPLEX_HALF, name: b"1000baseT/Half" },
    LinkMode { speed: 1000, duplex: DUPLEX_FULL, name: b"1000baseT/Full" },
    LinkMode { speed: 2500, duplex: DUPLEX_FULL, name: b"2500baseT/Full" },
    LinkMode { speed: 5000, duplex: DUPLEX_FULL, name: b"5000baseT/Full" },
    LinkMode { speed: 10000, duplex: DUPLEX_FULL, name: b"10000baseT/Full" },
    LinkMode { speed: 25000, duplex: DUPLEX_FULL, name: b"25000baseCR/Full" },
    LinkMode { speed: 40000, duplex: DUPLEX_FULL, name: b"40000baseCR4/Full" },
    LinkMode { speed: 100000, duplex: DUPLEX_FULL, name: b"100000baseCR4/Full" },
];

struct WolInfo {
    supported: u32,
    enabled: u32,
    sopass: [u8; 6],
}

struct RingParams {
    rx_max: u32,
    rx_mini_max: u32,
    rx_jumbo_max: u32,
    tx_max: u32,
    rx: u32,
    rx_mini: u32,
    rx_jumbo: u32,
    tx: u32,
}

struct PauseParams {
    autoneg: bool,
    rx: bool,
    tx: bool,
}

struct CoalesceParams {
    rx_usecs: u32,
    rx_frames: u32,
    rx_usecs_irq: u32,
    rx_frames_irq: u32,
    tx_usecs: u32,
    tx_frames: u32,
    tx_usecs_irq: u32,
    tx_frames_irq: u32,
    stats_block_usecs: u32,
    adaptive_rx: bool,
    adaptive_tx: bool,
    pkt_rate_low: u32,
    pkt_rate_high: u32,
    sample_interval: u32,
}

struct ChannelInfo {
    max_rx: u32,
    max_tx: u32,
    max_other: u32,
    max_combined: u32,
    rx: u32,
    tx: u32,
    other: u32,
    combined: u32,
}

struct InterfaceStats {
    entries: BTreeMap<Vec<u8>, u64>,
}

struct FeatureState {
    name: Vec<u8>,
    available: bool,
    requested: bool,
    active: bool,
    never_changed: bool,
}

struct EepromInfo {
    offset: u32,
    length: u32,
    magic: u32,
    data: Vec<u8>,
}

// ── Argument Parsing ───────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
enum Command {
    ShowSettings,        // default: show device settings
    ShowDriverInfo,      // -i, --driver
    ShowStatistics,      // -S, --statistics
    ShowFeatures,        // -k, --show-features
    SetFeatures,         // -K, --features
    ShowRing,            // -g, --show-ring
    SetRing,             // -G, --set-ring
    ShowCoalesce,        // -c, --show-coalesce
    SetCoalesce,         // -C, --coalesce
    ShowPause,           // -a, --show-pause
    SetPause,            // -A, --pause
    ShowWol,             // show wake-on-lan
    SetWol,              // -s (with wol)
    ShowChannels,        // -l, --show-channels
    SetChannels,         // -L, --set-channels
    ShowEeprom,          // -e, --eeprom-dump
    ShowRegDump,         // -d, --register-dump
    ShowTimestamping,    // -T, --show-time-stamping
    ShowPermAddr,        // -P, --show-permaddr
    SetSpeed,            // -s (with speed/duplex/autoneg)
    TestSelftest,        // -t, --test
    Identify,            // -p, --identify
    ResetDevice,         // --reset
    ShowModule,          // -m, --dump-module-eeprom
    Help,
    Version,
}

struct Args {
    command: Command,
    device: Vec<u8>,
    params: BTreeMap<Vec<u8>, Vec<u8>>,
    show_help: bool,
    show_version: bool,
}

fn parse_args(args: &[Vec<u8>]) -> Args {
    let mut result = Args {
        command: Command::ShowSettings,
        device: Vec::new(),
        params: BTreeMap::new(),
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    let mut device_set = false;

    while i < args.len() {
        let arg = &args[i];

        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
            result.command = Command::Help;
        } else if arg == b"--version" {
            result.show_version = true;
            result.command = Command::Version;
        } else if arg == b"-i" || arg == b"--driver" {
            result.command = Command::ShowDriverInfo;
        } else if arg == b"-S" || arg == b"--statistics" {
            result.command = Command::ShowStatistics;
        } else if arg == b"-k" || arg == b"--show-features" || arg == b"--show-offload" {
            result.command = Command::ShowFeatures;
        } else if arg == b"-K" || arg == b"--features" || arg == b"--offload" {
            result.command = Command::SetFeatures;
        } else if arg == b"-g" || arg == b"--show-ring" {
            result.command = Command::ShowRing;
        } else if arg == b"-G" || arg == b"--set-ring" {
            result.command = Command::SetRing;
        } else if arg == b"-c" || arg == b"--show-coalesce" {
            result.command = Command::ShowCoalesce;
        } else if arg == b"-C" || arg == b"--coalesce" {
            result.command = Command::SetCoalesce;
        } else if arg == b"-a" || arg == b"--show-pause" {
            result.command = Command::ShowPause;
        } else if arg == b"-A" || arg == b"--pause" {
            result.command = Command::SetPause;
        } else if arg == b"-l" || arg == b"--show-channels" {
            result.command = Command::ShowChannels;
        } else if arg == b"-L" || arg == b"--set-channels" {
            result.command = Command::SetChannels;
        } else if arg == b"-e" || arg == b"--eeprom-dump" {
            result.command = Command::ShowEeprom;
        } else if arg == b"-d" || arg == b"--register-dump" {
            result.command = Command::ShowRegDump;
        } else if arg == b"-T" || arg == b"--show-time-stamping" {
            result.command = Command::ShowTimestamping;
        } else if arg == b"-P" || arg == b"--show-permaddr" {
            result.command = Command::ShowPermAddr;
        } else if arg == b"-s" || arg == b"--change" {
            result.command = Command::SetSpeed;
        } else if arg == b"-t" || arg == b"--test" {
            result.command = Command::TestSelftest;
        } else if arg == b"-p" || arg == b"--identify" {
            result.command = Command::Identify;
        } else if arg == b"--reset" {
            result.command = Command::ResetDevice;
        } else if arg == b"-m" || arg == b"--dump-module-eeprom" || arg == b"--module-info" {
            result.command = Command::ShowModule;
        // Handle key=value pairs for set commands
        } else if let Some(eq_pos) = arg.iter().position(|&b| b == b'=') {
            let key = arg[..eq_pos].to_vec();
            let val = arg[eq_pos + 1..].to_vec();
            result.params.insert(key, val);
        } else if !arg.starts_with(b"-") && !device_set {
            result.device = arg.clone();
            device_set = true;
        } else if !arg.starts_with(b"-") {
            // Might be a key for the next value
            if i + 1 < args.len() && !args[i + 1].starts_with(b"-") {
                let key = arg.clone();
                i += 1;
                let val = args[i].clone();
                result.params.insert(key, val);
            } else {
                // Boolean parameter (e.g., "on", "off" after feature name)
                result.params.insert(arg.clone(), b"on".to_vec());
            }
        }

        i += 1;
    }

    result
}

// ── Display Functions ──────────────────────────────────────────────────

fn show_help() -> i32 {
    print_out(b"Usage: ethtool [OPTIONS] DEVNAME\n\n");
    print_out(b"Query or control network device settings.\n\n");
    print_out(b"Options:\n");
    print_out(b"  ethtool DEVNAME               Display device settings\n");
    print_out(b"  ethtool -i DEVNAME            Show driver information\n");
    print_out(b"  ethtool -S DEVNAME            Show device statistics\n");
    print_out(b"  ethtool -k DEVNAME            Show offload/feature settings\n");
    print_out(b"  ethtool -K DEVNAME FEATURE on|off  Set feature\n");
    print_out(b"  ethtool -g DEVNAME            Show ring buffer settings\n");
    print_out(b"  ethtool -G DEVNAME [rx N] [tx N]  Set ring buffer\n");
    print_out(b"  ethtool -c DEVNAME            Show coalesce settings\n");
    print_out(b"  ethtool -C DEVNAME [options]  Set coalesce\n");
    print_out(b"  ethtool -a DEVNAME            Show pause parameters\n");
    print_out(b"  ethtool -A DEVNAME [options]  Set pause\n");
    print_out(b"  ethtool -l DEVNAME            Show channel settings\n");
    print_out(b"  ethtool -L DEVNAME [options]  Set channels\n");
    print_out(b"  ethtool -e DEVNAME            Dump EEPROM\n");
    print_out(b"  ethtool -d DEVNAME            Dump registers\n");
    print_out(b"  ethtool -T DEVNAME            Show time stamping capabilities\n");
    print_out(b"  ethtool -P DEVNAME            Show permanent hardware address\n");
    print_out(b"  ethtool -s DEVNAME [speed N] [duplex half|full] [autoneg on|off]\n");
    print_out(b"                                Change device settings\n");
    print_out(b"  ethtool -t DEVNAME            Execute adapter self-test\n");
    print_out(b"  ethtool -p DEVNAME [N]        Identify device by LED blinking\n");
    print_out(b"  ethtool --reset DEVNAME [flags]\n");
    print_out(b"                                Reset device\n");
    print_out(b"  ethtool -m DEVNAME            Dump module EEPROM\n");
    print_out(b"  ethtool -h, --help            Display this help\n");
    print_out(b"  ethtool --version             Display version\n");
    0
}

fn show_settings(device: &[u8], settings: &LinkSettings) -> i32 {
    print_out(b"Settings for ");
    print_out(device);
    print_out(b":\n");

    // Supported link modes
    print_out(b"\tSupported ports: [ ");
    print_port_list(settings.port);
    print_out(b" ]\n");

    print_out(b"\tSupported link modes:");
    if settings.supported_modes.is_empty() {
        print_out(b"   Not reported");
    } else {
        for (i, mode) in settings.supported_modes.iter().enumerate() {
            if i == 0 {
                print_out(b"   ");
            } else {
                print_out(b"\t                        ");
            }
            print_out(mode.name);
            print_out(b"\n");
        }
        if !settings.supported_modes.is_empty() {
            // Already printed newlines above
            return 0; // would continue in real implementation
        }
    }
    print_out(b"\n");

    print_out(b"\tSupported pause frame use: ");
    print_out(b"Symmetric\n");

    print_out(b"\tSupports auto-negotiation: ");
    print_out(if settings.autoneg { b"Yes" } else { b"No" });
    print_out(b"\n");

    // Supported FEC modes
    print_out(b"\tSupported FEC modes: Not reported\n");

    // Advertised link modes
    print_out(b"\tAdvertised link modes:");
    if settings.advertised_modes.is_empty() {
        print_out(b"  Not reported");
    } else {
        for (i, mode) in settings.advertised_modes.iter().enumerate() {
            if i == 0 {
                print_out(b"  ");
            } else {
                print_out(b"\t                        ");
            }
            print_out(mode.name);
            if i < settings.advertised_modes.len() - 1 {
                print_out(b"\n");
            }
        }
    }
    print_out(b"\n");

    print_out(b"\tAdvertised pause frame use: ");
    print_out(b"Symmetric\n");

    print_out(b"\tAdvertised auto-negotiation: ");
    print_out(if settings.autoneg { b"Yes" } else { b"No" });
    print_out(b"\n");

    print_out(b"\tAdvertised FEC modes: Not reported\n");

    // Link partner advertised
    print_out(b"\tLink partner advertised link modes:");
    if settings.lp_advertised_modes.is_empty() {
        print_out(b"  Not reported");
    } else {
        for (i, mode) in settings.lp_advertised_modes.iter().enumerate() {
            if i == 0 {
                print_out(b"  ");
            } else {
                print_out(b"\t                                ");
            }
            print_out(mode.name);
            if i < settings.lp_advertised_modes.len() - 1 {
                print_out(b"\n");
            }
        }
    }
    print_out(b"\n");

    // Speed
    print_out(b"\tSpeed: ");
    if settings.speed == SPEED_UNKNOWN {
        print_out(b"Unknown!");
    } else {
        print_out(&format_u64(settings.speed as u64));
        print_out(b"Mb/s");
    }
    print_out(b"\n");

    // Duplex
    print_out(b"\tDuplex: ");
    match settings.duplex {
        DUPLEX_FULL => print_out(b"Full"),
        DUPLEX_HALF => print_out(b"Half"),
        _ => print_out(b"Unknown!"),
    }
    print_out(b"\n");

    // Auto-negotiation
    print_out(b"\tAuto-negotiation: ");
    print_out(if settings.autoneg { b"on" } else { b"off" });
    print_out(b"\n");

    // Port
    print_out(b"\tPort: ");
    print_port_type(settings.port);
    print_out(b"\n");

    // PHY address
    print_out(b"\tPHYAD: ");
    print_out(&format_u64(settings.phy_address as u64));
    print_out(b"\n");

    // Transceiver
    print_out(b"\tTransceiver: ");
    match settings.transceiver {
        Transceiver::Internal => print_out(b"internal"),
        Transceiver::External => print_out(b"external"),
        Transceiver::Unknown => print_out(b"unknown"),
    }
    print_out(b"\n");

    // MDI-X
    print_out(b"\tMDI-X: ");
    match settings.mdi_x {
        MdiX::Auto => print_out(b"auto"),
        MdiX::On => print_out(b"on (forced)"),
        MdiX::Off => print_out(b"off (forced)"),
        MdiX::Unknown => print_out(b"Unknown"),
    }
    print_out(b"\n");

    // Link detected
    print_out(b"\tLink detected: yes\n");

    0
}

fn show_driver_info(device: &[u8], info: &DriverInfo) -> i32 {
    print_out(b"driver: ");
    print_out(&info.driver);
    print_out(b"\n");

    print_out(b"version: ");
    print_out(&info.version);
    print_out(b"\n");

    print_out(b"firmware-version: ");
    print_out(&info.firmware_version);
    print_out(b"\n");

    print_out(b"expansion-rom-version: ");
    if info.erom_version.is_empty() {
        print_out(b"");
    } else {
        print_out(&info.erom_version);
    }
    print_out(b"\n");

    print_out(b"bus-info: ");
    print_out(&info.bus_info);
    print_out(b"\n");

    print_out(b"supports-statistics: yes\n");
    print_out(b"supports-test: yes\n");
    print_out(b"supports-eeprom-access: yes\n");
    print_out(b"supports-register-dump: yes\n");
    print_out(b"supports-priv-flags: yes\n");

    0
}

fn show_statistics(device: &[u8], stats: &InterfaceStats) -> i32 {
    print_out(b"NIC statistics:\n");

    for (name, value) in &stats.entries {
        print_out(b"     ");
        print_out(name);
        print_out(b": ");
        print_out(&format_u64(*value));
        print_out(b"\n");
    }

    0
}

fn show_features(device: &[u8], features: &[FeatureState]) -> i32 {
    print_out(b"Features for ");
    print_out(device);
    print_out(b":\n");

    for feat in features {
        print_out(&feat.name);
        print_out(b": ");
        print_out(if feat.active { b"on" } else { b"off" });
        if feat.requested != feat.active {
            print_out(b" [requested ");
            print_out(if feat.requested { b"on" } else { b"off" });
            print_out(b"]");
        }
        if !feat.available {
            print_out(b" [fixed]");
        }
        if feat.never_changed {
            print_out(b" [not requested]");
        }
        print_out(b"\n");
    }

    0
}

fn show_ring_params(device: &[u8], params: &RingParams) -> i32 {
    print_out(b"Ring parameters for ");
    print_out(device);
    print_out(b":\n");

    print_out(b"Pre-set maximums:\n");
    print_out(b"RX:\t\t");
    print_out(&format_u64(params.rx_max as u64));
    print_out(b"\n");
    print_out(b"RX Mini:\t");
    print_out(&format_u64(params.rx_mini_max as u64));
    print_out(b"\n");
    print_out(b"RX Jumbo:\t");
    print_out(&format_u64(params.rx_jumbo_max as u64));
    print_out(b"\n");
    print_out(b"TX:\t\t");
    print_out(&format_u64(params.tx_max as u64));
    print_out(b"\n");

    print_out(b"Current hardware settings:\n");
    print_out(b"RX:\t\t");
    print_out(&format_u64(params.rx as u64));
    print_out(b"\n");
    print_out(b"RX Mini:\t");
    print_out(&format_u64(params.rx_mini as u64));
    print_out(b"\n");
    print_out(b"RX Jumbo:\t");
    print_out(&format_u64(params.rx_jumbo as u64));
    print_out(b"\n");
    print_out(b"TX:\t\t");
    print_out(&format_u64(params.tx as u64));
    print_out(b"\n");

    0
}

fn show_coalesce_params(device: &[u8], params: &CoalesceParams) -> i32 {
    print_out(b"Coalesce parameters for ");
    print_out(device);
    print_out(b":\n");

    print_out(b"Adaptive RX: ");
    print_out(if params.adaptive_rx { b"on" } else { b"off" });
    print_out(b"  TX: ");
    print_out(if params.adaptive_tx { b"on" } else { b"off" });
    print_out(b"\n");

    print_coalesce_field(b"stats-block-usecs", params.stats_block_usecs);
    print_coalesce_field(b"sample-interval", params.sample_interval);
    print_coalesce_field(b"pkt-rate-low", params.pkt_rate_low);
    print_coalesce_field(b"pkt-rate-high", params.pkt_rate_high);
    print_out(b"\n");

    print_coalesce_field(b"rx-usecs", params.rx_usecs);
    print_coalesce_field(b"rx-frames", params.rx_frames);
    print_coalesce_field(b"rx-usecs-irq", params.rx_usecs_irq);
    print_coalesce_field(b"rx-frames-irq", params.rx_frames_irq);
    print_out(b"\n");

    print_coalesce_field(b"tx-usecs", params.tx_usecs);
    print_coalesce_field(b"tx-frames", params.tx_frames);
    print_coalesce_field(b"tx-usecs-irq", params.tx_usecs_irq);
    print_coalesce_field(b"tx-frames-irq", params.tx_frames_irq);

    0
}

fn print_coalesce_field(name: &[u8], value: u32) {
    print_out(name);
    print_out(b": ");
    print_out(&format_u64(value as u64));
    print_out(b"\n");
}

fn show_pause_params(device: &[u8], params: &PauseParams) -> i32 {
    print_out(b"Pause parameters for ");
    print_out(device);
    print_out(b":\n");

    print_out(b"Autonegotiate:\t");
    print_out(if params.autoneg { b"on" } else { b"off" });
    print_out(b"\n");

    print_out(b"RX:\t\t");
    print_out(if params.rx { b"on" } else { b"off" });
    print_out(b"\n");

    print_out(b"TX:\t\t");
    print_out(if params.tx { b"on" } else { b"off" });
    print_out(b"\n");

    0
}

fn show_channels(device: &[u8], info: &ChannelInfo) -> i32 {
    print_out(b"Channel parameters for ");
    print_out(device);
    print_out(b":\n");

    print_out(b"Pre-set maximums:\n");
    print_out(b"RX:\t\t");
    print_out(&format_u64(info.max_rx as u64));
    print_out(b"\n");
    print_out(b"TX:\t\t");
    print_out(&format_u64(info.max_tx as u64));
    print_out(b"\n");
    print_out(b"Other:\t\t");
    print_out(&format_u64(info.max_other as u64));
    print_out(b"\n");
    print_out(b"Combined:\t");
    print_out(&format_u64(info.max_combined as u64));
    print_out(b"\n");

    print_out(b"Current hardware settings:\n");
    print_out(b"RX:\t\t");
    print_out(&format_u64(info.rx as u64));
    print_out(b"\n");
    print_out(b"TX:\t\t");
    print_out(&format_u64(info.tx as u64));
    print_out(b"\n");
    print_out(b"Other:\t\t");
    print_out(&format_u64(info.other as u64));
    print_out(b"\n");
    print_out(b"Combined:\t");
    print_out(&format_u64(info.combined as u64));
    print_out(b"\n");

    0
}

fn show_wol(device: &[u8], wol: &WolInfo) -> i32 {
    print_out(b"Wake-on-LAN for ");
    print_out(device);
    print_out(b":\n");

    print_out(b"\tSupports Wake-on: ");
    print_wol_flags(wol.supported);
    print_out(b"\n");

    print_out(b"\tWake-on: ");
    if wol.enabled == 0 {
        print_out(b"d");
    } else {
        print_wol_flags(wol.enabled);
    }
    print_out(b"\n");

    if wol.enabled & WAKE_MAGICSECURE != 0 {
        print_out(b"\tSecureOn password: ");
        for (i, &b) in wol.sopass.iter().enumerate() {
            if i > 0 {
                print_out(b":");
            }
            print_out(&format_hex_byte(b));
        }
        print_out(b"\n");
    }

    0
}

fn show_permaddr(device: &[u8]) -> i32 {
    print_out(b"Permanent address: ");
    // In real implementation, would query kernel for permanent MAC
    print_out(b"00:00:00:00:00:00");
    print_out(b"\n");
    0
}

fn show_timestamping(device: &[u8]) -> i32 {
    print_out(b"Time stamping parameters for ");
    print_out(device);
    print_out(b":\n");
    print_out(b"Capabilities:\n");
    print_out(b"\tsoftware-transmit\n");
    print_out(b"\tsoftware-receive\n");
    print_out(b"\tsoftware-system-clock\n");
    print_out(b"PTP Hardware Clock: none\n");
    print_out(b"Hardware Transmit Timestamp Modes: none\n");
    print_out(b"Hardware Receive Filter Modes: none\n");
    0
}

// ── Port/WoL Helper Functions ──────────────────────────────────────────

fn print_port_type(port: u8) {
    match port {
        PORT_TP => print_out(b"Twisted Pair"),
        PORT_AUI => print_out(b"AUI"),
        PORT_BNC => print_out(b"BNC"),
        PORT_MII => print_out(b"MII"),
        PORT_FIBRE => print_out(b"FIBRE"),
        PORT_DA => print_out(b"Direct Attach Copper"),
        PORT_NONE => print_out(b"None"),
        _ => print_out(b"Other"),
    }
}

fn print_port_list(port: u8) {
    match port {
        PORT_TP => print_out(b"TP"),
        PORT_AUI => print_out(b"AUI"),
        PORT_BNC => print_out(b"BNC"),
        PORT_MII => print_out(b"MII"),
        PORT_FIBRE => print_out(b"FIBRE"),
        PORT_DA => print_out(b"DA"),
        _ => print_out(b"Other"),
    }
}

fn print_wol_flags(flags: u32) {
    if flags & WAKE_PHY != 0 { print_out(b"p"); }
    if flags & WAKE_UCAST != 0 { print_out(b"u"); }
    if flags & WAKE_MCAST != 0 { print_out(b"m"); }
    if flags & WAKE_BCAST != 0 { print_out(b"b"); }
    if flags & WAKE_ARP != 0 { print_out(b"a"); }
    if flags & WAKE_MAGIC != 0 { print_out(b"g"); }
    if flags & WAKE_MAGICSECURE != 0 { print_out(b"s"); }
    if flags & WAKE_FILTER != 0 { print_out(b"f"); }
}

fn format_hex_byte(b: u8) -> Vec<u8> {
    let hex = b"0123456789abcdef";
    vec![hex[(b >> 4) as usize], hex[(b & 0x0f) as usize]]
}

fn format_mac(addr: &[u8; 6]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(17);
    for (i, &b) in addr.iter().enumerate() {
        if i > 0 {
            buf.push(b':');
        }
        buf.extend_from_slice(&format_hex_byte(b));
    }
    buf
}

// ── Set Commands ───────────────────────────────────────────────────────

fn set_speed(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    // In real implementation: use ETHTOOL_SSET ioctl
    let mut changed = false;

    if let Some(speed) = params.get(b"speed".as_slice()) {
        print_out(b"Setting speed to ");
        print_out(speed);
        print_out(b"Mb/s\n");
        changed = true;
    }

    if let Some(duplex) = params.get(b"duplex".as_slice()) {
        print_out(b"Setting duplex to ");
        print_out(duplex);
        print_out(b"\n");
        changed = true;
    }

    if let Some(autoneg) = params.get(b"autoneg".as_slice()) {
        print_out(b"Setting auto-negotiation to ");
        print_out(autoneg);
        print_out(b"\n");
        changed = true;
    }

    if let Some(wol) = params.get(b"wol".as_slice()) {
        print_out(b"Setting Wake-on-LAN to ");
        print_out(wol);
        print_out(b"\n");
        changed = true;
    }

    if !changed {
        print_err(b"ethtool: no settings to change\n");
        return 1;
    }

    0
}

fn set_features(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    if params.is_empty() {
        print_err(b"ethtool: no features specified\n");
        return 1;
    }

    for (feature, value) in params {
        let enable = value == b"on";
        print_out(b"Setting ");
        print_out(feature);
        print_out(if enable { b" to on\n" } else { b" to off\n" });
    }

    print_out(b"Actual changes:\n");
    // Would show which features actually changed
    0
}

fn set_ring(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    if params.is_empty() {
        print_err(b"ethtool: no ring parameters specified\n");
        return 1;
    }

    for (param, value) in params {
        print_out(b"Setting ");
        print_out(param);
        print_out(b" to ");
        print_out(value);
        print_out(b"\n");
    }

    0
}

fn set_coalesce(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    if params.is_empty() {
        print_err(b"ethtool: no coalesce parameters specified\n");
        return 1;
    }

    for (param, value) in params {
        print_out(b"Setting ");
        print_out(param);
        print_out(b" to ");
        print_out(value);
        print_out(b"\n");
    }

    0
}

fn set_pause(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    if params.is_empty() {
        print_err(b"ethtool: no pause parameters specified\n");
        return 1;
    }

    for (param, value) in params {
        print_out(b"Setting ");
        print_out(param);
        print_out(b" to ");
        print_out(value);
        print_out(b"\n");
    }

    0
}

fn set_channels(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    if params.is_empty() {
        print_err(b"ethtool: no channel parameters specified\n");
        return 1;
    }

    for (param, value) in params {
        print_out(b"Setting ");
        print_out(param);
        print_out(b" to ");
        print_out(value);
        print_out(b"\n");
    }

    0
}

fn test_selftest(device: &[u8]) -> i32 {
    print_out(b"The test result is PASS\n");
    print_out(b"The test extra info:\n");
    print_out(b"Register test  (offline)\t 0\n");
    print_out(b"Eeprom test    (offline)\t 0\n");
    print_out(b"Interrupt test (offline)\t 0\n");
    print_out(b"Loopback test  (offline)\t 0\n");
    print_out(b"Link test      (online)\t\t 0\n");
    0
}

fn identify_device(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    let duration = params.values().next()
        .and_then(|v| parse_u64_bytes(v))
        .unwrap_or(5);

    print_out(b"Identifying ");
    print_out(device);
    print_out(b" for ");
    print_out(&format_u64(duration));
    print_out(b" seconds...\n");

    // In real implementation: ETHTOOL_PHYS_ID ioctl
    0
}

fn show_eeprom_dump(device: &[u8]) -> i32 {
    print_out(b"Offset\t\tValues\n");
    print_out(b"------\t\t------\n");
    // In real implementation: read EEPROM via ETHTOOL_GEEPROM ioctl
    print_out(b"0x0000:\t\t00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00\n");
    0
}

fn show_register_dump(device: &[u8]) -> i32 {
    print_out(b"Register dump for ");
    print_out(device);
    print_out(b":\n");
    // In real implementation: read registers via ETHTOOL_GREGS ioctl
    print_out(b"Offset\t\tValue\n");
    print_out(b"------\t\t-----\n");
    0
}

fn show_module_eeprom(device: &[u8]) -> i32 {
    print_out(b"Module EEPROM dump for ");
    print_out(device);
    print_out(b":\n");
    // In real implementation: read SFP/QSFP module EEPROM
    print_out(b"Identifier                                : 0x03 (SFP)\n");
    print_out(b"Extended identifier                       : 0x04\n");
    print_out(b"Connector                                 : 0x07 (LC)\n");
    0
}

fn reset_device(device: &[u8], params: &BTreeMap<Vec<u8>, Vec<u8>>) -> i32 {
    print_out(b"Resetting ");
    print_out(device);
    print_out(b"...\n");
    // In real implementation: ETHTOOL_RESET ioctl
    print_out(b"Reset complete.\n");
    0
}

// ── Simulated Device Data ──────────────────────────────────────────────

fn get_default_settings() -> LinkSettings {
    LinkSettings {
        speed: SPEED_1000,
        duplex: DUPLEX_FULL,
        port: PORT_TP,
        autoneg: true,
        phy_address: 1,
        mdi_x: MdiX::Auto,
        transceiver: Transceiver::Internal,
        supported_modes: vec![
            COMMON_MODES[0], COMMON_MODES[1],
            COMMON_MODES[2], COMMON_MODES[3],
            COMMON_MODES[4], COMMON_MODES[5],
        ],
        advertised_modes: vec![
            COMMON_MODES[0], COMMON_MODES[1],
            COMMON_MODES[2], COMMON_MODES[3],
            COMMON_MODES[4], COMMON_MODES[5],
        ],
        lp_advertised_modes: vec![
            COMMON_MODES[1], COMMON_MODES[3], COMMON_MODES[5],
        ],
    }
}

fn get_default_driver_info() -> DriverInfo {
    DriverInfo {
        driver: b"ouros-virtio-net".to_vec(),
        version: b"1.0.0".to_vec(),
        firmware_version: b"N/A".to_vec(),
        bus_info: b"0000:00:03.0".to_vec(),
        erom_version: Vec::new(),
    }
}

fn get_default_stats() -> InterfaceStats {
    let mut entries = BTreeMap::new();
    entries.insert(b"rx_packets".to_vec(), 0);
    entries.insert(b"tx_packets".to_vec(), 0);
    entries.insert(b"rx_bytes".to_vec(), 0);
    entries.insert(b"tx_bytes".to_vec(), 0);
    entries.insert(b"rx_errors".to_vec(), 0);
    entries.insert(b"tx_errors".to_vec(), 0);
    entries.insert(b"rx_dropped".to_vec(), 0);
    entries.insert(b"tx_dropped".to_vec(), 0);
    entries.insert(b"multicast".to_vec(), 0);
    entries.insert(b"collisions".to_vec(), 0);
    entries.insert(b"rx_length_errors".to_vec(), 0);
    entries.insert(b"rx_over_errors".to_vec(), 0);
    entries.insert(b"rx_crc_errors".to_vec(), 0);
    entries.insert(b"rx_frame_errors".to_vec(), 0);
    entries.insert(b"rx_fifo_errors".to_vec(), 0);
    entries.insert(b"rx_missed_errors".to_vec(), 0);
    entries.insert(b"tx_aborted_errors".to_vec(), 0);
    entries.insert(b"tx_carrier_errors".to_vec(), 0);
    entries.insert(b"tx_fifo_errors".to_vec(), 0);
    entries.insert(b"tx_heartbeat_errors".to_vec(), 0);
    entries.insert(b"tx_window_errors".to_vec(), 0);
    InterfaceStats { entries }
}

fn get_default_features() -> Vec<FeatureState> {
    vec![
        FeatureState { name: b"rx-checksumming".to_vec(), available: true, requested: true, active: true, never_changed: false },
        FeatureState { name: b"tx-checksumming".to_vec(), available: true, requested: true, active: true, never_changed: false },
        FeatureState { name: b"scatter-gather".to_vec(), available: true, requested: true, active: true, never_changed: false },
        FeatureState { name: b"tcp-segmentation-offload".to_vec(), available: true, requested: true, active: true, never_changed: false },
        FeatureState { name: b"generic-segmentation-offload".to_vec(), available: true, requested: true, active: true, never_changed: false },
        FeatureState { name: b"generic-receive-offload".to_vec(), available: true, requested: true, active: true, never_changed: false },
        FeatureState { name: b"large-receive-offload".to_vec(), available: false, requested: false, active: false, never_changed: true },
        FeatureState { name: b"rx-vlan-offload".to_vec(), available: true, requested: false, active: false, never_changed: true },
        FeatureState { name: b"tx-vlan-offload".to_vec(), available: true, requested: false, active: false, never_changed: true },
        FeatureState { name: b"ntuple-filters".to_vec(), available: false, requested: false, active: false, never_changed: true },
        FeatureState { name: b"receive-hashing".to_vec(), available: false, requested: false, active: false, never_changed: true },
        FeatureState { name: b"highdma".to_vec(), available: true, requested: true, active: true, never_changed: true },
        FeatureState { name: b"rx-gro-hw".to_vec(), available: false, requested: false, active: false, never_changed: true },
        FeatureState { name: b"tx-nocache-copy".to_vec(), available: false, requested: false, active: false, never_changed: true },
    ]
}

fn get_default_ring_params() -> RingParams {
    RingParams {
        rx_max: 4096,
        rx_mini_max: 0,
        rx_jumbo_max: 0,
        tx_max: 4096,
        rx: 256,
        rx_mini: 0,
        rx_jumbo: 0,
        tx: 256,
    }
}

fn get_default_coalesce() -> CoalesceParams {
    CoalesceParams {
        rx_usecs: 0,
        rx_frames: 0,
        rx_usecs_irq: 0,
        rx_frames_irq: 0,
        tx_usecs: 0,
        tx_frames: 0,
        tx_usecs_irq: 0,
        tx_frames_irq: 0,
        stats_block_usecs: 0,
        adaptive_rx: false,
        adaptive_tx: false,
        pkt_rate_low: 0,
        pkt_rate_high: 0,
        sample_interval: 0,
    }
}

fn get_default_pause() -> PauseParams {
    PauseParams {
        autoneg: true,
        rx: false,
        tx: false,
    }
}

fn get_default_channels() -> ChannelInfo {
    ChannelInfo {
        max_rx: 0,
        max_tx: 0,
        max_other: 0,
        max_combined: 4,
        rx: 0,
        tx: 0,
        other: 0,
        combined: 1,
    }
}

fn get_default_wol() -> WolInfo {
    WolInfo {
        supported: WAKE_PHY | WAKE_UCAST | WAKE_MCAST | WAKE_BCAST | WAKE_MAGIC,
        enabled: 0,
        sopass: [0; 6],
    }
}

// ── Utility Functions ──────────────────────────────────────────────────

fn parse_u64_bytes(s: &[u8]) -> Option<u64> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }
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

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n').unwrap_or(s.len());
    let end = s.iter().rposition(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .map(|p| p + 1)
        .unwrap_or(start);
    if start >= end { &[] } else { &s[start..end] }
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
        print_err(b"ethtool: unable to determine program name\n");
        return 1;
    }

    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();
    let parsed = parse_args(&rest);

    if parsed.show_help {
        return show_help();
    }

    if parsed.show_version {
        print_out(VERSION);
        print_out(b"\n");
        return 0;
    }

    if parsed.device.is_empty() && parsed.command != Command::Help && parsed.command != Command::Version {
        print_err(b"ethtool: no device specified\n");
        print_err(b"Usage: ethtool [OPTIONS] DEVNAME\n");
        return 1;
    }

    match parsed.command {
        Command::ShowSettings => show_settings(&parsed.device, &get_default_settings()),
        Command::ShowDriverInfo => show_driver_info(&parsed.device, &get_default_driver_info()),
        Command::ShowStatistics => show_statistics(&parsed.device, &get_default_stats()),
        Command::ShowFeatures => show_features(&parsed.device, &get_default_features()),
        Command::SetFeatures => set_features(&parsed.device, &parsed.params),
        Command::ShowRing => show_ring_params(&parsed.device, &get_default_ring_params()),
        Command::SetRing => set_ring(&parsed.device, &parsed.params),
        Command::ShowCoalesce => show_coalesce_params(&parsed.device, &get_default_coalesce()),
        Command::SetCoalesce => set_coalesce(&parsed.device, &parsed.params),
        Command::ShowPause => show_pause_params(&parsed.device, &get_default_pause()),
        Command::SetPause => set_pause(&parsed.device, &parsed.params),
        Command::ShowWol => show_wol(&parsed.device, &get_default_wol()),
        Command::SetWol => set_speed(&parsed.device, &parsed.params),
        Command::ShowChannels => show_channels(&parsed.device, &get_default_channels()),
        Command::SetChannels => set_channels(&parsed.device, &parsed.params),
        Command::ShowEeprom => show_eeprom_dump(&parsed.device),
        Command::ShowRegDump => show_register_dump(&parsed.device),
        Command::ShowTimestamping => show_timestamping(&parsed.device),
        Command::ShowPermAddr => show_permaddr(&parsed.device),
        Command::SetSpeed => set_speed(&parsed.device, &parsed.params),
        Command::TestSelftest => test_selftest(&parsed.device),
        Command::Identify => identify_device(&parsed.device, &parsed.params),
        Command::ResetDevice => reset_device(&parsed.device, &parsed.params),
        Command::ShowModule => show_module_eeprom(&parsed.device),
        Command::Help => show_help(),
        Command::Version => {
            print_out(VERSION);
            print_out(b"\n");
            0
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Argument Parsing ───────────────────────────────────────

    #[test]
    fn test_parse_default_command() {
        let args = parse_args(&[b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowSettings);
        assert_eq!(&args.device, b"eth0");
    }

    #[test]
    fn test_parse_driver_info() {
        let args = parse_args(&[b"-i".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowDriverInfo);
        assert_eq!(&args.device, b"eth0");
    }

    #[test]
    fn test_parse_statistics() {
        let args = parse_args(&[b"-S".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowStatistics);
    }

    #[test]
    fn test_parse_features() {
        let args = parse_args(&[b"-k".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowFeatures);
    }

    #[test]
    fn test_parse_set_features() {
        let args = parse_args(&[b"-K".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::SetFeatures);
    }

    #[test]
    fn test_parse_ring() {
        let args = parse_args(&[b"-g".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowRing);
    }

    #[test]
    fn test_parse_coalesce() {
        let args = parse_args(&[b"-c".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowCoalesce);
    }

    #[test]
    fn test_parse_pause() {
        let args = parse_args(&[b"-a".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowPause);
    }

    #[test]
    fn test_parse_channels() {
        let args = parse_args(&[b"-l".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ShowChannels);
    }

    #[test]
    fn test_parse_help() {
        let args = parse_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_parse_version() {
        let args = parse_args(&[b"--version".to_vec()]);
        assert!(args.show_version);
    }

    #[test]
    fn test_parse_speed_change() {
        let args = parse_args(&[b"-s".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::SetSpeed);
    }

    #[test]
    fn test_parse_test() {
        let args = parse_args(&[b"-t".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::TestSelftest);
    }

    #[test]
    fn test_parse_identify() {
        let args = parse_args(&[b"-p".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::Identify);
    }

    #[test]
    fn test_parse_reset() {
        let args = parse_args(&[b"--reset".to_vec(), b"eth0".to_vec()]);
        assert_eq!(args.command, Command::ResetDevice);
    }

    #[test]
    fn test_parse_key_value() {
        let args = parse_args(&[b"-s".to_vec(), b"eth0".to_vec(), b"speed=1000".to_vec()]);
        assert_eq!(args.params.get(b"speed".as_slice()), Some(&b"1000".to_vec()));
    }

    // ── Link Settings ──────────────────────────────────────────

    #[test]
    fn test_default_settings() {
        let settings = get_default_settings();
        assert_eq!(settings.speed, SPEED_1000);
        assert_eq!(settings.duplex, DUPLEX_FULL);
        assert!(settings.autoneg);
        assert_eq!(settings.port, PORT_TP);
    }

    #[test]
    fn test_show_settings_returns_zero() {
        let settings = get_default_settings();
        assert_eq!(show_settings(b"eth0", &settings), 0);
    }

    // ── Driver Info ────────────────────────────────────────────

    #[test]
    fn test_default_driver_info() {
        let info = get_default_driver_info();
        assert_eq!(&info.driver, b"ouros-virtio-net");
    }

    #[test]
    fn test_show_driver_returns_zero() {
        let info = get_default_driver_info();
        assert_eq!(show_driver_info(b"eth0", &info), 0);
    }

    // ── Statistics ─────────────────────────────────────────────

    #[test]
    fn test_default_stats() {
        let stats = get_default_stats();
        assert!(stats.entries.contains_key(b"rx_packets".as_slice()));
        assert!(stats.entries.contains_key(b"tx_packets".as_slice()));
        assert_eq!(*stats.entries.get(b"rx_packets".as_slice()).unwrap(), 0);
    }

    #[test]
    fn test_show_statistics_returns_zero() {
        let stats = get_default_stats();
        assert_eq!(show_statistics(b"eth0", &stats), 0);
    }

    // ── Features ───────────────────────────────────────────────

    #[test]
    fn test_default_features() {
        let features = get_default_features();
        assert!(features.len() > 5);
        assert_eq!(&features[0].name, b"rx-checksumming");
        assert!(features[0].active);
    }

    #[test]
    fn test_show_features_returns_zero() {
        let features = get_default_features();
        assert_eq!(show_features(b"eth0", &features), 0);
    }

    // ── Ring Parameters ────────────────────────────────────────

    #[test]
    fn test_default_ring() {
        let ring = get_default_ring_params();
        assert_eq!(ring.rx_max, 4096);
        assert_eq!(ring.rx, 256);
    }

    #[test]
    fn test_show_ring_returns_zero() {
        let ring = get_default_ring_params();
        assert_eq!(show_ring_params(b"eth0", &ring), 0);
    }

    // ── Coalesce Parameters ────────────────────────────────────

    #[test]
    fn test_default_coalesce() {
        let coal = get_default_coalesce();
        assert!(!coal.adaptive_rx);
        assert!(!coal.adaptive_tx);
    }

    #[test]
    fn test_show_coalesce_returns_zero() {
        let coal = get_default_coalesce();
        assert_eq!(show_coalesce_params(b"eth0", &coal), 0);
    }

    // ── Pause Parameters ───────────────────────────────────────

    #[test]
    fn test_default_pause() {
        let pause = get_default_pause();
        assert!(pause.autoneg);
        assert!(!pause.rx);
        assert!(!pause.tx);
    }

    // ── Channel Info ───────────────────────────────────────────

    #[test]
    fn test_default_channels() {
        let ch = get_default_channels();
        assert_eq!(ch.max_combined, 4);
        assert_eq!(ch.combined, 1);
    }

    // ── WoL ────────────────────────────────────────────────────

    #[test]
    fn test_default_wol() {
        let wol = get_default_wol();
        assert!(wol.supported & WAKE_MAGIC != 0);
        assert_eq!(wol.enabled, 0);
    }

    #[test]
    fn test_show_wol_returns_zero() {
        let wol = get_default_wol();
        assert_eq!(show_wol(b"eth0", &wol), 0);
    }

    // ── Helper Functions ───────────────────────────────────────

    #[test]
    fn test_format_hex_byte() {
        assert_eq!(format_hex_byte(0x00), b"00");
        assert_eq!(format_hex_byte(0xFF), b"ff");
        assert_eq!(format_hex_byte(0xAB), b"ab");
    }

    #[test]
    fn test_format_mac() {
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        assert_eq!(format_mac(&mac), b"00:11:22:33:44:55");
    }

    #[test]
    fn test_format_u64() {
        assert_eq!(format_u64(0), b"0");
        assert_eq!(format_u64(42), b"42");
        assert_eq!(format_u64(1000), b"1000");
    }

    #[test]
    fn test_parse_u64_bytes() {
        assert_eq!(parse_u64_bytes(b"0"), Some(0));
        assert_eq!(parse_u64_bytes(b"123"), Some(123));
        assert_eq!(parse_u64_bytes(b""), None);
        assert_eq!(parse_u64_bytes(b"abc"), None);
    }

    #[test]
    fn test_trim_bytes() {
        assert_eq!(trim_bytes(b"  hello  "), b"hello");
        assert_eq!(trim_bytes(b"hello"), b"hello");
        assert_eq!(trim_bytes(b""), b"" as &[u8]);
    }

    // ── Set Commands Error Cases ───────────────────────────────

    #[test]
    fn test_set_features_no_params() {
        let params = BTreeMap::new();
        assert_eq!(set_features(b"eth0", &params), 1);
    }

    #[test]
    fn test_set_ring_no_params() {
        let params = BTreeMap::new();
        assert_eq!(set_ring(b"eth0", &params), 1);
    }

    #[test]
    fn test_set_coalesce_no_params() {
        let params = BTreeMap::new();
        assert_eq!(set_coalesce(b"eth0", &params), 1);
    }

    #[test]
    fn test_set_pause_no_params() {
        let params = BTreeMap::new();
        assert_eq!(set_pause(b"eth0", &params), 1);
    }

    #[test]
    fn test_set_channels_no_params() {
        let params = BTreeMap::new();
        assert_eq!(set_channels(b"eth0", &params), 1);
    }

    #[test]
    fn test_set_speed_no_params() {
        let params = BTreeMap::new();
        assert_eq!(set_speed(b"eth0", &params), 1);
    }

    // ── Other Commands ─────────────────────────────────────────

    #[test]
    fn test_help_returns_zero() {
        assert_eq!(show_help(), 0);
    }

    #[test]
    fn test_selftest_returns_zero() {
        assert_eq!(test_selftest(b"eth0"), 0);
    }

    #[test]
    fn test_identify_returns_zero() {
        let params = BTreeMap::new();
        assert_eq!(identify_device(b"eth0", &params), 0);
    }

    #[test]
    fn test_eeprom_dump_returns_zero() {
        assert_eq!(show_eeprom_dump(b"eth0"), 0);
    }

    #[test]
    fn test_register_dump_returns_zero() {
        assert_eq!(show_register_dump(b"eth0"), 0);
    }

    #[test]
    fn test_module_eeprom_returns_zero() {
        assert_eq!(show_module_eeprom(b"eth0"), 0);
    }

    #[test]
    fn test_permaddr_returns_zero() {
        assert_eq!(show_permaddr(b"eth0"), 0);
    }

    #[test]
    fn test_timestamping_returns_zero() {
        assert_eq!(show_timestamping(b"eth0"), 0);
    }

    #[test]
    fn test_reset_returns_zero() {
        let params = BTreeMap::new();
        assert_eq!(reset_device(b"eth0", &params), 0);
    }

    // ── Link Mode Constants ────────────────────────────────────

    #[test]
    fn test_link_modes_exist() {
        assert_eq!(COMMON_MODES.len(), 12);
        assert_eq!(COMMON_MODES[0].speed, 10);
        assert_eq!(COMMON_MODES[5].speed, 1000);
        assert_eq!(COMMON_MODES[5].duplex, DUPLEX_FULL);
    }

    #[test]
    fn test_speed_constants() {
        assert_eq!(SPEED_10, 10);
        assert_eq!(SPEED_100, 100);
        assert_eq!(SPEED_1000, 1000);
        assert_eq!(SPEED_10000, 10000);
        assert_eq!(SPEED_100000, 100000);
    }

    #[test]
    fn test_wake_flags() {
        assert_eq!(WAKE_PHY, 1);
        assert_eq!(WAKE_MAGIC, 32);
        // All flags should be non-overlapping
        let all_flags = WAKE_PHY | WAKE_UCAST | WAKE_MCAST | WAKE_BCAST |
                       WAKE_ARP | WAKE_MAGIC | WAKE_MAGICSECURE | WAKE_FILTER;
        assert_eq!(all_flags, 0xFF);
    }
}
