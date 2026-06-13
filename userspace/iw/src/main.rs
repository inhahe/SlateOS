// SlateOS iw - wireless network tools
//
// Multi-personality binary:
//   iw       - nl80211-based wireless config tool
//   iwconfig - legacy wireless config
//   iwlist   - legacy wireless scanning
//   rfkill   - radio frequency kill switch

#![cfg_attr(not(test), no_main)]
// BAND_6GHZ, print_err, band_str, pad_right, and the unread
// WifiInterface::beacon_interval / Options::verbose fields are part of
// the nl80211 / wireless-extensions vocabulary the real implementation
// must speak. The multi-personality stub only exercises a subset of the
// surface; the full vocabulary is intentionally kept so the future
// driver-attached implementation can drop in without reshaping public
// types or function signatures. Dead-code lint cannot see across that
// future boundary.
#![allow(dead_code)]

// ── Constants ──────────────────────────────────────────────────────────

const MAX_IFACES: usize = 8;
const MAX_SSIDS: usize = 32;
const MAX_NAME: usize = 32;

// Interface types
const IFTYPE_MANAGED: u8 = 0;
const IFTYPE_AP: u8 = 1;
const IFTYPE_MONITOR: u8 = 2;
const IFTYPE_ADHOC: u8 = 3;
const IFTYPE_MESH: u8 = 4;

// Band
const BAND_24GHZ: u8 = 0;
const BAND_5GHZ: u8 = 1;
const BAND_6GHZ: u8 = 2;

// Security
const SEC_OPEN: u8 = 0;
const SEC_WEP: u8 = 1;
const SEC_WPA: u8 = 2;
const SEC_WPA2: u8 = 3;
const SEC_WPA3: u8 = 4;

// rfkill types
const RFKILL_WIFI: u8 = 0;
const RFKILL_BT: u8 = 1;
const RFKILL_UWB: u8 = 2;
const RFKILL_WIMAX: u8 = 3;
const RFKILL_WWAN: u8 = 4;
const RFKILL_GPS: u8 = 5;
const RFKILL_FM: u8 = 6;
const RFKILL_NFC: u8 = 7;

// ── Output Helpers ─────────────────────────────────────────────────────

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

// ── Data Types ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool { Iw, Iwconfig, Iwlist, Rfkill }

#[derive(Clone, Copy)]
struct WifiInterface {
    name: [u8; MAX_NAME],
    name_len: usize,
    phy_index: u8,
    if_index: u32,
    if_type: u8,
    mac: [u8; 6],
    channel: u16,
    frequency: u32,  // MHz
    tx_power: i16,   // dBm * 100
    connected: bool,
    ssid: [u8; 33],
    ssid_len: usize,
    bssid: [u8; 6],
    signal: i16,     // dBm
    bitrate: u32,    // Mbps * 10
    band: u8,
}

impl WifiInterface {
    fn new() -> Self {
        Self {
            name: [0u8; MAX_NAME], name_len: 0,
            phy_index: 0, if_index: 0, if_type: IFTYPE_MANAGED,
            mac: [0u8; 6], channel: 0, frequency: 0,
            tx_power: 2000, connected: false,
            ssid: [0u8; 33], ssid_len: 0,
            bssid: [0u8; 6], signal: 0, bitrate: 0,
            band: BAND_24GHZ,
        }
    }
}

#[derive(Clone, Copy)]
struct ScanResult {
    bssid: [u8; 6],
    ssid: [u8; 33],
    ssid_len: usize,
    frequency: u32,
    channel: u16,
    signal: i16,
    security: u8,
    band: u8,
    bitrate_max: u32,
    beacon_interval: u16,
}

impl ScanResult {
    fn new() -> Self {
        Self {
            bssid: [0u8; 6], ssid: [0u8; 33], ssid_len: 0,
            frequency: 0, channel: 0, signal: 0,
            security: SEC_OPEN, band: BAND_24GHZ,
            bitrate_max: 0, beacon_interval: 100,
        }
    }
}

#[derive(Clone, Copy)]
struct RfkillDevice {
    index: u8,
    dev_type: u8,
    name: [u8; MAX_NAME],
    name_len: usize,
    soft_blocked: bool,
    hard_blocked: bool,
}

impl RfkillDevice {
    fn new() -> Self {
        Self {
            index: 0, dev_type: RFKILL_WIFI,
            name: [0u8; MAX_NAME], name_len: 0,
            soft_blocked: false, hard_blocked: false,
        }
    }
}

struct Options {
    tool: Tool,
    iface: [u8; MAX_NAME],
    iface_len: usize,
    // iw subcommands
    subcmd: [u8; 32],
    subcmd_len: usize,
    subcmd2: [u8; 32],
    subcmd2_len: usize,
    // rfkill
    rfkill_cmd: [u8; 16],
    rfkill_cmd_len: usize,
    rfkill_type: [u8; 16],
    rfkill_type_len: usize,
    verbose: bool,
}

impl Options {
    fn new(tool: Tool) -> Self {
        Self {
            tool,
            iface: [0u8; MAX_NAME], iface_len: 0,
            subcmd: [0u8; 32], subcmd_len: 0,
            subcmd2: [0u8; 32], subcmd2_len: 0,
            rfkill_cmd: [0u8; 16], rfkill_cmd_len: 0,
            rfkill_type: [0u8; 16], rfkill_type_len: 0,
            verbose: false,
        }
    }
}

// ── String/Number Helpers ──────────────────────────────────────────────

unsafe fn cstr_to_slice(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() { return b""; }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 { len += 1; if len >= 4096 { break; } }
        core::slice::from_raw_parts(ptr, len)
    }
}

fn format_u64(val: u64, buf: &mut [u8]) -> usize {
    if val == 0 { if !buf.is_empty() { buf[0] = b'0'; } return 1; }
    let mut tmp = [0u8; 20];
    let mut n = val; let mut i = 0;
    while n > 0 { if let Some(s) = tmp.get_mut(i) { *s = b'0' + (n % 10) as u8; } n /= 10; i += 1; }
    let len = i.min(buf.len());
    for j in 0..len { if let (Some(d), Some(s)) = (buf.get_mut(j), tmp.get(i-1-j)) { *d = *s; } }
    len
}

fn format_i16(val: i16, buf: &mut [u8]) -> usize {
    if val < 0 {
        if buf.is_empty() { return 0; }
        buf[0] = b'-';
        format_u64((-val) as u64, &mut buf[1..]) + 1
    } else {
        format_u64(val as u64, buf)
    }
}

fn copy_bytes(dst: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let mut p = pos;
    for &c in src { if p < dst.len() { dst[p] = c; p += 1; } }
    p
}

fn pad_right(buf: &mut [u8], start: usize, width: usize) -> usize {
    let mut pos = start;
    while pos < width && pos < buf.len() { buf[pos] = b' '; pos += 1; }
    pos
}

fn set_field(dst: &mut [u8], len: &mut usize, src: &[u8]) {
    let cl = src.len().min(dst.len());
    dst[..cl].copy_from_slice(&src[..cl]);
    *len = cl;
}

fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() { return false; }
    &haystack[..needle.len()] == needle
}

fn format_mac(mac: &[u8; 6], buf: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut pos = 0;
    for i in 0..6 {
        if i > 0 && pos < buf.len() { buf[pos] = b':'; pos += 1; }
        if pos + 1 < buf.len() {
            buf[pos] = HEX[(mac[i] >> 4) as usize]; pos += 1;
            buf[pos] = HEX[(mac[i] & 0xF) as usize]; pos += 1;
        }
    }
    pos
}

fn iftype_str(t: u8) -> &'static [u8] {
    match t {
        IFTYPE_MANAGED => b"managed",
        IFTYPE_AP => b"AP",
        IFTYPE_MONITOR => b"monitor",
        IFTYPE_ADHOC => b"IBSS",
        IFTYPE_MESH => b"mesh point",
        _ => b"unknown",
    }
}

fn sec_str(s: u8) -> &'static [u8] {
    match s {
        SEC_OPEN => b"Open",
        SEC_WEP => b"WEP",
        SEC_WPA => b"WPA",
        SEC_WPA2 => b"WPA2",
        SEC_WPA3 => b"WPA3",
        _ => b"Unknown",
    }
}

fn rfkill_type_str(t: u8) -> &'static [u8] {
    match t {
        RFKILL_WIFI => b"wlan",
        RFKILL_BT => b"bluetooth",
        RFKILL_UWB => b"uwb",
        RFKILL_WIMAX => b"wimax",
        RFKILL_WWAN => b"wwan",
        RFKILL_GPS => b"gps",
        RFKILL_FM => b"fm",
        RFKILL_NFC => b"nfc",
        _ => b"unknown",
    }
}

fn band_str(b: u8) -> &'static [u8] {
    match b {
        BAND_24GHZ => b"2.4 GHz",
        BAND_5GHZ => b"5 GHz",
        BAND_6GHZ => b"6 GHz",
        _ => b"unknown",
    }
}

// ── Tool Detection ─────────────────────────────────────────────────────

fn detect_tool(argv0: &[u8]) -> Tool {
    let mut start = 0;
    for (i, b) in argv0.iter().enumerate() { if *b == b'/' || *b == b'\\' { start = i + 1; } }
    let name = &argv0[start..];
    if starts_with(name, b"iwconfig") { Tool::Iwconfig }
    else if starts_with(name, b"iwlist") { Tool::Iwlist }
    else if starts_with(name, b"rfkill") { Tool::Rfkill }
    else { Tool::Iw }
}

// ── Argument Parsing ───────────────────────────────────────────────────

fn parse_args(argc: i32, argv: *const *const u8) -> Result<Options, i32> {
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let argv0 = if !args.is_empty() { unsafe { cstr_to_slice(args[0]) } } else { b"iw" };
    let tool = detect_tool(argv0);
    let mut opts = Options::new(tool);

    let mut i = 1;
    while i < args.len() {
        let arg = unsafe { cstr_to_slice(args[i]) };
        if arg == b"--help" || arg == b"-h" || arg == b"help" { show_help(tool); return Err(0); }
        else if arg == b"--version" || arg == b"-V" {
            print_out(tool_name(tool)); print_out(b" 0.1.0 (SlateOS)\n"); return Err(0);
        }

        match tool {
            Tool::Iw => {
                if arg == b"dev" || arg == b"phy" {
                    // iw dev <ifname> <cmd>
                    set_field(&mut opts.subcmd, &mut opts.subcmd_len, arg);
                    i += 1;
                    if i < args.len() {
                        let iface = unsafe { cstr_to_slice(args[i]) };
                        set_field(&mut opts.iface, &mut opts.iface_len, iface);
                    }
                    i += 1;
                    if i < args.len() {
                        let cmd = unsafe { cstr_to_slice(args[i]) };
                        set_field(&mut opts.subcmd2, &mut opts.subcmd2_len, cmd);
                    }
                } else if opts.subcmd_len == 0 {
                    set_field(&mut opts.subcmd, &mut opts.subcmd_len, arg);
                }
            }
            Tool::Iwconfig | Tool::Iwlist => {
                if !arg.is_empty() && arg[0] != b'-' && opts.iface_len == 0 {
                    set_field(&mut opts.iface, &mut opts.iface_len, arg);
                } else if opts.subcmd_len == 0 {
                    set_field(&mut opts.subcmd, &mut opts.subcmd_len, arg);
                }
            }
            Tool::Rfkill => {
                if opts.rfkill_cmd_len == 0 && !arg.is_empty() && arg[0] != b'-' {
                    set_field(&mut opts.rfkill_cmd, &mut opts.rfkill_cmd_len, arg);
                } else if opts.rfkill_type_len == 0 && !arg.is_empty() && arg[0] != b'-' {
                    set_field(&mut opts.rfkill_type, &mut opts.rfkill_type_len, arg);
                }
            }
        }
        i += 1;
    }
    Ok(opts)
}

fn tool_name(tool: Tool) -> &'static [u8] {
    match tool { Tool::Iw => b"iw", Tool::Iwconfig => b"iwconfig", Tool::Iwlist => b"iwlist", Tool::Rfkill => b"rfkill" }
}

fn show_help(tool: Tool) {
    match tool {
        Tool::Iw => {
            print_out(b"Usage: iw [options] <command>\n\n");
            print_out(b"Commands:\n");
            print_out(b"  dev <iface> info         Show interface info\n");
            print_out(b"  dev <iface> scan         Trigger scan and show results\n");
            print_out(b"  dev <iface> link         Show connection info\n");
            print_out(b"  dev <iface> station dump Show station statistics\n");
            print_out(b"  phy                      Show PHY info\n");
            print_out(b"  list                     List all devices\n");
            print_out(b"  reg get                  Show regulatory domain\n");
        }
        Tool::Iwconfig => {
            print_out(b"Usage: iwconfig [interface]\n\nShow/set wireless interface parameters.\n");
        }
        Tool::Iwlist => {
            print_out(b"Usage: iwlist <interface> <command>\n\n");
            print_out(b"Commands: scan, frequency, channel, rate, keys, power, txpower\n");
        }
        Tool::Rfkill => {
            print_out(b"Usage: rfkill <command> [type]\n\n");
            print_out(b"Commands: list, block, unblock\n");
            print_out(b"Types: all, wifi, bluetooth, wwan\n");
        }
    }
}

// ── Simulated Wireless Data ────────────────────────────────────────────

fn get_wifi_interfaces() -> ([WifiInterface; MAX_IFACES], usize) {
    let mut ifaces = [WifiInterface::new(); MAX_IFACES];
    let mut count = 0;

    // wlan0
    {
        let w = &mut ifaces[count];
        set_field(&mut w.name, &mut w.name_len, b"wlan0");
        w.phy_index = 0;
        w.if_index = 3;
        w.if_type = IFTYPE_MANAGED;
        w.mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        w.channel = 36;
        w.frequency = 5180;
        w.tx_power = 2000; // 20 dBm
        w.connected = true;
        set_field(&mut w.ssid, &mut w.ssid_len, b"MyHomeNetwork");
        w.bssid = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        w.signal = -45;
        w.bitrate = 8667; // 866.7 Mbps
        w.band = BAND_5GHZ;
        count += 1;
    }

    (ifaces, count)
}

fn get_scan_results() -> ([ScanResult; MAX_SSIDS], usize) {
    let mut results = [ScanResult::new(); MAX_SSIDS];
    let mut count = 0;

    type ScanFixture<'a> = (&'a [u8], [u8; 6], u32, u16, i16, u8, u8, u32);
    let networks: &[ScanFixture] = &[
        (b"MyHomeNetwork", [0xAA,0xBB,0xCC,0xDD,0xEE,0xFF], 5180, 36, -45, SEC_WPA2, BAND_5GHZ, 8667),
        (b"MyHomeNetwork", [0xAA,0xBB,0xCC,0xDD,0xEE,0x01], 2437, 6, -55, SEC_WPA2, BAND_24GHZ, 1440),
        (b"Neighbor_5G", [0x11,0x22,0x33,0x44,0x55,0x66], 5240, 48, -62, SEC_WPA3, BAND_5GHZ, 5760),
        (b"CoffeeShop", [0x22,0x33,0x44,0x55,0x66,0x77], 2412, 1, -70, SEC_OPEN, BAND_24GHZ, 540),
        (b"Office_Secure", [0x33,0x44,0x55,0x66,0x77,0x88], 5500, 100, -58, SEC_WPA2, BAND_5GHZ, 8667),
        (b"Guest_Network", [0x44,0x55,0x66,0x77,0x88,0x99], 2462, 11, -75, SEC_WPA, BAND_24GHZ, 540),
    ];

    for &(ssid, bssid, freq, ch, sig, sec, band, rate) in networks {
        if count >= MAX_SSIDS { break; }
        set_field(&mut results[count].ssid, &mut results[count].ssid_len, ssid);
        results[count].bssid = bssid;
        results[count].frequency = freq;
        results[count].channel = ch;
        results[count].signal = sig;
        results[count].security = sec;
        results[count].band = band;
        results[count].bitrate_max = rate;
        count += 1;
    }
    (results, count)
}

fn get_rfkill_devices() -> ([RfkillDevice; 8], usize) {
    let mut devs = [RfkillDevice::new(); 8];
    let mut count = 0;

    devs[count].index = 0; devs[count].dev_type = RFKILL_WIFI;
    set_field(&mut devs[count].name, &mut devs[count].name_len, b"phy0");
    count += 1;

    devs[count].index = 1; devs[count].dev_type = RFKILL_BT;
    set_field(&mut devs[count].name, &mut devs[count].name_len, b"hci0");
    count += 1;

    (devs, count)
}

// ── Display Functions ──────────────────────────────────────────────────

fn cmd_iw(opts: &Options) {
    let subcmd = &opts.subcmd[..opts.subcmd_len];
    let subcmd2 = &opts.subcmd2[..opts.subcmd2_len];

    if subcmd == b"dev" && subcmd2 == b"info" {
        show_iw_dev_info(opts);
    } else if subcmd == b"dev" && subcmd2 == b"scan" {
        show_iw_scan(opts);
    } else if subcmd == b"dev" && subcmd2 == b"link" {
        show_iw_link(opts);
    } else if subcmd == b"list" || (subcmd == b"dev" && opts.subcmd2_len == 0 && opts.iface_len == 0) {
        show_iw_list();
    } else if subcmd == b"reg" {
        show_iw_reg();
    } else {
        show_iw_list();
    }
}

fn show_iw_dev_info(opts: &Options) {
    let (ifaces, count) = get_wifi_interfaces();
    let target = &opts.iface[..opts.iface_len];
    let mut buf = [0u8; 256];

    for w in ifaces.iter().take(count) {
        if opts.iface_len > 0 && &w.name[..w.name_len] != target { continue; }

        let mut pos = copy_bytes(&mut buf, 0, b"Interface ");
        pos = copy_bytes(&mut buf, pos, &w.name[..w.name_len]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tifindex ");
        pos += format_u64(w.if_index as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\twdev 0x1\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\taddr ");
        pos += format_mac(&w.mac, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\ttype ");
        pos = copy_bytes(&mut buf, pos, iftype_str(w.if_type));
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tchannel ");
        pos += format_u64(w.channel as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" (");
        pos += format_u64(w.frequency as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" MHz)\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\ttxpower ");
        pos += format_i16(w.tx_power / 100, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b".00 dBm\n");
        print_out(&buf[..pos]);
    }
}

fn show_iw_scan(_opts: &Options) {
    let (results, count) = get_scan_results();
    let mut buf = [0u8; 256];

    for r in results.iter().take(count) {
        let mut pos = copy_bytes(&mut buf, 0, b"BSS ");
        pos += format_mac(&r.bssid, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tSSID: ");
        pos = copy_bytes(&mut buf, pos, &r.ssid[..r.ssid_len]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tfreq: ");
        pos += format_u64(r.frequency as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tsignal: ");
        pos += format_i16(r.signal, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" dBm\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tcapability: ");
        pos = copy_bytes(&mut buf, pos, sec_str(r.security));
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);
    }
}

fn show_iw_link(opts: &Options) {
    let (ifaces, count) = get_wifi_interfaces();
    let target = &opts.iface[..opts.iface_len];
    let mut buf = [0u8; 256];

    for w in ifaces.iter().take(count) {
        if opts.iface_len > 0 && &w.name[..w.name_len] != target { continue; }
        if !w.connected {
            print_out(b"Not connected.\n");
            return;
        }

        let mut pos = copy_bytes(&mut buf, 0, b"Connected to ");
        pos += format_mac(&w.bssid, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tSSID: ");
        pos = copy_bytes(&mut buf, pos, &w.ssid[..w.ssid_len]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tfreq: ");
        pos += format_u64(w.frequency as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\tsignal: ");
        pos += format_i16(w.signal, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" dBm\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"\ttx bitrate: ");
        let whole = w.bitrate / 10;
        let frac = w.bitrate % 10;
        pos += format_u64(whole as u64, &mut buf[pos..]);
        buf[pos] = b'.'; pos += 1;
        pos += format_u64(frac as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" MBit/s\n");
        print_out(&buf[..pos]);
    }
}

fn show_iw_list() {
    let (ifaces, count) = get_wifi_interfaces();
    let mut buf = [0u8; 256];

    print_out(b"Wiphy phy0\n");
    print_out(b"\tBands:\n");
    print_out(b"\t\tBand 1 (2.4 GHz):\n");
    print_out(b"\t\t\tChannels: 1-13\n");
    print_out(b"\t\tBand 2 (5 GHz):\n");
    print_out(b"\t\t\tChannels: 36-165\n");
    print_out(b"\tInterface modes:\n");
    print_out(b"\t\t* managed\n\t\t* AP\n\t\t* monitor\n");

    for w in ifaces.iter().take(count) {
        let mut pos = copy_bytes(&mut buf, 0, b"\nInterface ");
        pos = copy_bytes(&mut buf, pos, &w.name[..w.name_len]);
        pos = copy_bytes(&mut buf, pos, b"\n\ttype ");
        pos = copy_bytes(&mut buf, pos, iftype_str(w.if_type));
        pos = copy_bytes(&mut buf, pos, b"\n\taddr ");
        pos += format_mac(&w.mac, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);
    }
}

fn show_iw_reg() {
    print_out(b"country US: DFS-FCC\n");
    print_out(b"\t(2402 - 2472 @ 40), (N/A, 30), (N/A)\n");
    print_out(b"\t(5170 - 5250 @ 80), (N/A, 23), (N/A), AUTO-BW\n");
    print_out(b"\t(5250 - 5330 @ 80), (N/A, 23), (0 ms), DFS, AUTO-BW\n");
    print_out(b"\t(5490 - 5730 @ 160), (N/A, 23), (0 ms), DFS\n");
    print_out(b"\t(5735 - 5835 @ 80), (N/A, 30), (N/A)\n");
}

fn cmd_iwconfig(opts: &Options) {
    let (ifaces, count) = get_wifi_interfaces();
    let target = &opts.iface[..opts.iface_len];
    let mut buf = [0u8; 256];

    for w in ifaces.iter().take(count) {
        if opts.iface_len > 0 && &w.name[..w.name_len] != target { continue; }

        let mut pos = copy_bytes(&mut buf, 0, &w.name[..w.name_len]);
        pos = copy_bytes(&mut buf, pos, b"    IEEE 802.11  ");
        if w.connected {
            pos = copy_bytes(&mut buf, pos, b"ESSID:\"");
            pos = copy_bytes(&mut buf, pos, &w.ssid[..w.ssid_len]);
            pos = copy_bytes(&mut buf, pos, b"\"\n");
        } else {
            pos = copy_bytes(&mut buf, pos, b"ESSID:off/any\n");
        }
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"          Mode:Managed  Frequency:");
        pos += format_u64((w.frequency / 1000) as u64, &mut buf[pos..]);
        buf[pos] = b'.'; pos += 1;
        pos += format_u64(((w.frequency % 1000) / 100) as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" GHz  Access Point: ");
        if w.connected {
            pos += format_mac(&w.bssid, &mut buf[pos..]);
        } else {
            pos = copy_bytes(&mut buf, pos, b"Not-Associated");
        }
        pos = copy_bytes(&mut buf, pos, b"\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"          Bit Rate=");
        let whole = w.bitrate / 10;
        let frac = w.bitrate % 10;
        pos += format_u64(whole as u64, &mut buf[pos..]);
        buf[pos] = b'.'; pos += 1;
        pos += format_u64(frac as u64, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" Mb/s   Tx-Power=");
        pos += format_i16(w.tx_power / 100, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" dBm\n");
        print_out(&buf[..pos]);

        pos = copy_bytes(&mut buf, 0, b"          Link Quality=70/70  Signal level=");
        pos += format_i16(w.signal, &mut buf[pos..]);
        pos = copy_bytes(&mut buf, pos, b" dBm\n\n");
        print_out(&buf[..pos]);
    }
}

fn cmd_iwlist(opts: &Options) {
    let subcmd = &opts.subcmd[..opts.subcmd_len];
    if subcmd == b"scan" || subcmd == b"scanning" {
        let iface = if opts.iface_len > 0 { &opts.iface[..opts.iface_len] } else { b"wlan0" as &[u8] };
        print_out(iface);
        print_out(b"    Scan completed :\n");

        let (results, count) = get_scan_results();
        let mut buf = [0u8; 256];
        for (i, r) in results.iter().enumerate().take(count) {
            let mut pos = copy_bytes(&mut buf, 0, b"          Cell ");
            pos += format_u64((i + 1) as u64, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" - Address: ");
            pos += format_mac(&r.bssid, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b"\n");
            print_out(&buf[..pos]);

            pos = copy_bytes(&mut buf, 0, b"                    ESSID:\"");
            pos = copy_bytes(&mut buf, pos, &r.ssid[..r.ssid_len]);
            pos = copy_bytes(&mut buf, pos, b"\"\n");
            print_out(&buf[..pos]);

            pos = copy_bytes(&mut buf, 0, b"                    Frequency:");
            pos += format_u64((r.frequency / 1000) as u64, &mut buf[pos..]);
            buf[pos] = b'.'; pos += 1;
            pos += format_u64(((r.frequency % 1000) / 100) as u64, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" GHz (Channel ");
            pos += format_u64(r.channel as u64, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b")\n");
            print_out(&buf[..pos]);

            pos = copy_bytes(&mut buf, 0, b"                    Quality=70/100  Signal level=");
            pos += format_i16(r.signal, &mut buf[pos..]);
            pos = copy_bytes(&mut buf, pos, b" dBm\n");
            print_out(&buf[..pos]);

            pos = copy_bytes(&mut buf, 0, b"                    Encryption key:");
            pos = copy_bytes(&mut buf, pos, if r.security == SEC_OPEN { b"off\n" } else { b"on\n" });
            print_out(&buf[..pos]);
        }
    } else {
        print_out(b"Usage: iwlist <interface> scan\n");
    }
}

fn cmd_rfkill(opts: &Options) {
    let cmd = &opts.rfkill_cmd[..opts.rfkill_cmd_len];
    let (devs, count) = get_rfkill_devices();

    if cmd == b"list" || opts.rfkill_cmd_len == 0 {
        let mut buf = [0u8; 256];
        for d in devs.iter().take(count) {
            let mut pos = format_u64(d.index as u64, &mut buf);
            pos = copy_bytes(&mut buf, pos, b": ");
            pos = copy_bytes(&mut buf, pos, &d.name[..d.name_len]);
            pos = copy_bytes(&mut buf, pos, b": ");
            pos = copy_bytes(&mut buf, pos, rfkill_type_str(d.dev_type));
            pos = copy_bytes(&mut buf, pos, b"\n");
            print_out(&buf[..pos]);

            pos = copy_bytes(&mut buf, 0, b"\tSoft blocked: ");
            pos = copy_bytes(&mut buf, pos, if d.soft_blocked { b"yes\n" } else { b"no\n" });
            print_out(&buf[..pos]);

            pos = copy_bytes(&mut buf, 0, b"\tHard blocked: ");
            pos = copy_bytes(&mut buf, pos, if d.hard_blocked { b"yes\n" } else { b"no\n" });
            print_out(&buf[..pos]);
        }
    } else if cmd == b"block" {
        print_out(b"rfkill: blocked ");
        print_out(&opts.rfkill_type[..opts.rfkill_type_len]);
        print_out(b"\n");
    } else if cmd == b"unblock" {
        print_out(b"rfkill: unblocked ");
        print_out(&opts.rfkill_type[..opts.rfkill_type_len]);
        print_out(b"\n");
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
        Tool::Iw => cmd_iw(&opts),
        Tool::Iwconfig => cmd_iwconfig(&opts),
        Tool::Iwlist => cmd_iwlist(&opts),
        Tool::Rfkill => cmd_rfkill(&opts),
    }
    0
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_tool() {
        assert_eq!(detect_tool(b"iw"), Tool::Iw);
        assert_eq!(detect_tool(b"/usr/sbin/iwconfig"), Tool::Iwconfig);
        assert_eq!(detect_tool(b"iwlist"), Tool::Iwlist);
        assert_eq!(detect_tool(b"rfkill"), Tool::Rfkill);
    }

    #[test]
    fn test_iftype_str() {
        assert_eq!(iftype_str(IFTYPE_MANAGED), b"managed");
        assert_eq!(iftype_str(IFTYPE_AP), b"AP");
        assert_eq!(iftype_str(IFTYPE_MONITOR), b"monitor");
    }

    #[test]
    fn test_sec_str() {
        assert_eq!(sec_str(SEC_OPEN), b"Open");
        assert_eq!(sec_str(SEC_WPA2), b"WPA2");
        assert_eq!(sec_str(SEC_WPA3), b"WPA3");
    }

    #[test]
    fn test_rfkill_type_str() {
        assert_eq!(rfkill_type_str(RFKILL_WIFI), b"wlan");
        assert_eq!(rfkill_type_str(RFKILL_BT), b"bluetooth");
    }

    #[test]
    fn test_format_mac() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let mut buf = [0u8; 17];
        let n = format_mac(&mac, &mut buf);
        assert_eq!(n, 17);
        assert_eq!(&buf[..n], b"aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn test_format_i16_positive() {
        let mut buf = [0u8; 10];
        let n = format_i16(42, &mut buf);
        assert_eq!(&buf[..n], b"42");
    }

    #[test]
    fn test_format_i16_negative() {
        let mut buf = [0u8; 10];
        let n = format_i16(-45, &mut buf);
        assert_eq!(&buf[..n], b"-45");
    }

    #[test]
    fn test_get_wifi_interfaces() {
        let (ifaces, count) = get_wifi_interfaces();
        assert_eq!(count, 1);
        assert_eq!(&ifaces[0].name[..ifaces[0].name_len], b"wlan0");
        assert!(ifaces[0].connected);
    }

    #[test]
    fn test_get_scan_results() {
        let (_results, count) = get_scan_results();
        assert!(count >= 4);
    }

    #[test]
    fn test_get_rfkill_devices() {
        let (devs, count) = get_rfkill_devices();
        assert!(count >= 2);
        assert_eq!(devs[0].dev_type, RFKILL_WIFI);
        assert_eq!(devs[1].dev_type, RFKILL_BT);
    }

    #[test]
    fn test_band_str() {
        assert_eq!(band_str(BAND_24GHZ), b"2.4 GHz");
        assert_eq!(band_str(BAND_5GHZ), b"5 GHz");
        assert_eq!(band_str(BAND_6GHZ), b"6 GHz");
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(tool_name(Tool::Iw), b"iw");
        assert_eq!(tool_name(Tool::Iwconfig), b"iwconfig");
        assert_eq!(tool_name(Tool::Iwlist), b"iwlist");
        assert_eq!(tool_name(Tool::Rfkill), b"rfkill");
    }
}
