//! Multi-personality device mapper control utility for OurOS.
//!
//! This binary detects the tool from `argv[0]`:
//!   - `dmsetup`  — device mapper management (create, remove, suspend, etc.)
//!   - `dmstats`  — device mapper statistics regions
//!   - `kpartx`   — create device maps from partition tables
//!
//! Provides simulated device mapper state for userspace testing and
//! integration with OurOS's block-device subsystem.

#![deny(clippy::all)]
// Many items (partition parsers, kpartx data-path functions, MBR/GPT constants)
// are used only from tests and from code paths that require real block device
// access. Allow dead_code globally rather than scattering cfg(test) annotations
// on interconnected types/functions.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::io::Write;
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "1.02.197-ouros";
const DM_VERSION: &str = "4.48.0";

const KNOWN_TARGETS: &[(&str, &str)] = &[
    ("linear", "1.4.0"),
    ("striped", "1.6.0"),
    ("mirror", "1.14.0"),
    ("snapshot", "1.16.0"),
    ("snapshot-origin", "1.9.0"),
    ("thin", "1.23.0"),
    ("thin-pool", "1.23.0"),
    ("cache", "2.2.0"),
    ("crypt", "1.24.0"),
    ("zero", "1.1.0"),
    ("error", "1.5.0"),
    ("delay", "1.3.0"),
];

// MBR partition type IDs considered valid for mapping.
const MBR_LINUX_PARTITION: u8 = 0x83;
const MBR_EXTENDED_CHS: u8 = 0x05;
const MBR_EXTENDED_LBA: u8 = 0x0F;
const MBR_EXTENDED_LINUX: u8 = 0x85;
const MBR_FAT32: u8 = 0x0C;
const MBR_FAT32_LBA: u8 = 0x0B;
const MBR_NTFS: u8 = 0x07;
const MBR_SWAP: u8 = 0x82;

const GPT_MAGIC: &[u8; 8] = b"EFI PART";
const GPT_HEADER_LBA: u64 = 1;
const SECTOR_SIZE: u64 = 512;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Dmsetup,
    Dmstats,
    Kpartx,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Dmsetup => "dmsetup",
            Self::Dmstats => "dmstats",
            Self::Kpartx => "kpartx",
        }
    }
}

fn detect_tool(argv0: &str) -> Tool {
    let bytes = argv0.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &argv0[last_sep..];
    let base = base.strip_suffix(".exe").unwrap_or(base);
    let lower = base.to_ascii_lowercase();

    if lower.contains("dmstats") {
        Tool::Dmstats
    } else if lower.contains("kpartx") {
        Tool::Kpartx
    } else {
        Tool::Dmsetup
    }
}

// ---------------------------------------------------------------------------
// Device mapper data model
// ---------------------------------------------------------------------------

/// A single table entry mapping a sector range to a target.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TableEntry {
    start_sector: u64,
    length: u64,
    target_type: String,
    target_args: String,
}

impl fmt::Display for TableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.start_sector, self.length, self.target_type, self.target_args
        )
    }
}

/// Device state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeviceState {
    Active,
    Suspended,
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "ACTIVE"),
            Self::Suspended => write!(f, "SUSPENDED"),
        }
    }
}

/// A device-mapper device.
#[derive(Clone, Debug)]
struct MappedDevice {
    name: String,
    uuid: String,
    major: u32,
    minor: u32,
    state: DeviceState,
    open_count: u32,
    event_nr: u64,
    active_table: Vec<TableEntry>,
    inactive_table: Option<Vec<TableEntry>>,
}

impl MappedDevice {
    fn new(name: &str, minor: u32, table: Vec<TableEntry>) -> Self {
        Self {
            name: name.to_string(),
            uuid: format!("OUROS-{name}"),
            major: 253,
            minor,
            state: DeviceState::Active,
            open_count: 0,
            event_nr: 0,
            active_table: table,
            inactive_table: None,
        }
    }

    fn total_sectors(&self) -> u64 {
        self.active_table
            .iter()
            .map(|e| e.start_sector.saturating_add(e.length))
            .max()
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Statistics model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct StatsRegion {
    region_id: u64,
    device_name: String,
    start_sector: u64,
    length: u64,
    reads: u64,
    writes: u64,
    read_sectors: u64,
    write_sectors: u64,
    read_time_ms: u64,
    write_time_ms: u64,
    in_flight: u64,
}

impl StatsRegion {
    fn new(region_id: u64, device_name: &str, start: u64, length: u64) -> Self {
        Self {
            region_id,
            device_name: device_name.to_string(),
            start_sector: start,
            length,
            reads: 0,
            writes: 0,
            read_sectors: 0,
            write_sectors: 0,
            read_time_ms: 0,
            write_time_ms: 0,
            in_flight: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Partition table structures
// ---------------------------------------------------------------------------

/// An MBR partition entry (16 bytes in the on-disk format).
#[derive(Clone, Debug, PartialEq, Eq)]
struct MbrPartition {
    status: u8,
    part_type: u8,
    lba_start: u32,
    sector_count: u32,
}

/// A GPT partition entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GptPartition {
    type_guid: [u8; 16],
    unique_guid: [u8; 16],
    first_lba: u64,
    last_lba: u64,
    name: String,
}

/// Parsed partition table.
#[derive(Clone, Debug)]
enum PartitionTable {
    Mbr {
        primary: Vec<MbrPartition>,
        logical: Vec<MbrPartition>,
    },
    Gpt {
        entries: Vec<GptPartition>,
    },
}

// ---------------------------------------------------------------------------
// Global state container
// ---------------------------------------------------------------------------

struct DmState {
    devices: BTreeMap<String, MappedDevice>,
    next_minor: u32,
    stats_regions: Vec<StatsRegion>,
    next_region_id: u64,
}

impl DmState {
    fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
            next_minor: 0,
            stats_regions: Vec::new(),
            next_region_id: 0,
        }
    }

    fn alloc_minor(&mut self) -> u32 {
        let m = self.next_minor;
        self.next_minor = self.next_minor.wrapping_add(1);
        m
    }
}

// ---------------------------------------------------------------------------
// Table parsing
// ---------------------------------------------------------------------------

fn parse_table_line(s: &str) -> Result<TableEntry, String> {
    let s = s.trim();
    let mut parts = s.splitn(4, char::is_whitespace);

    let start_str = parts.next().ok_or("missing start sector")?;
    let len_str = parts.next().ok_or("missing length")?;
    let target = parts.next().ok_or("missing target type")?;
    let args = parts.next().unwrap_or("").trim();

    let start_sector = start_str
        .parse::<u64>()
        .map_err(|e| format!("bad start sector '{start_str}': {e}"))?;
    let length = len_str
        .parse::<u64>()
        .map_err(|e| format!("bad length '{len_str}': {e}"))?;

    if !KNOWN_TARGETS.iter().any(|&(t, _)| t == target) {
        return Err(format!("unknown target type '{target}'"));
    }

    Ok(TableEntry {
        start_sector,
        length,
        target_type: target.to_string(),
        target_args: args.to_string(),
    })
}

fn parse_table(s: &str) -> Result<Vec<TableEntry>, String> {
    let mut entries = Vec::new();
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        entries.push(parse_table_line(line)?);
    }
    if entries.is_empty() {
        return Err("empty table".to_string());
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Partition table parsing
// ---------------------------------------------------------------------------

fn is_extended_type(t: u8) -> bool {
    t == MBR_EXTENDED_CHS || t == MBR_EXTENDED_LBA || t == MBR_EXTENDED_LINUX
}

fn is_mappable_mbr_type(t: u8) -> bool {
    // Map any non-zero, non-extended partition type.
    t != 0 && !is_extended_type(t)
}

fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    let b0 = *data.get(offset).unwrap_or(&0) as u16;
    let b1 = *data.get(offset + 1).unwrap_or(&0) as u16;
    b0 | (b1 << 8)
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    let b0 = *data.get(offset).unwrap_or(&0) as u32;
    let b1 = *data.get(offset + 1).unwrap_or(&0) as u32;
    let b2 = *data.get(offset + 2).unwrap_or(&0) as u32;
    let b3 = *data.get(offset + 3).unwrap_or(&0) as u32;
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    let lo = read_u32_le(data, offset) as u64;
    let hi = read_u32_le(data, offset + 4) as u64;
    lo | (hi << 32)
}

/// Parse an MBR partition entry from 16 bytes at `offset`.
fn parse_mbr_entry(data: &[u8], offset: usize) -> MbrPartition {
    MbrPartition {
        status: *data.get(offset).unwrap_or(&0),
        part_type: *data.get(offset + 4).unwrap_or(&0),
        lba_start: read_u32_le(data, offset + 8),
        sector_count: read_u32_le(data, offset + 12),
    }
}

/// Parse a full MBR from 512 bytes, returning primary + logical partitions.
fn parse_mbr(data: &[u8]) -> Result<(Vec<MbrPartition>, Vec<MbrPartition>), String> {
    if data.len() < 512 {
        return Err("MBR too short".to_string());
    }
    // Check MBR signature.
    if read_u16_le(data, 510) != 0xAA55 {
        return Err("invalid MBR signature".to_string());
    }

    let mut primary = Vec::new();
    let mut extended_start: Option<u32> = None;

    // 4 primary entries at offsets 446, 462, 478, 494.
    for i in 0..4 {
        let entry = parse_mbr_entry(data, 446 + i * 16);
        if entry.part_type != 0 {
            if is_extended_type(entry.part_type) {
                extended_start = Some(entry.lba_start);
            } else {
                primary.push(entry);
            }
        }
    }

    // Walk extended partition chain (logical partitions).
    let mut logical = Vec::new();
    if let Some(ext_start) = extended_start {
        // In a real system we would read each EBR sector. Here we simulate
        // by noting the extended partition exists but we cannot read further
        // sectors from the provided data alone. For testing purposes, the
        // parse_extended_chain function handles the chain if enough data
        // is provided.
        parse_extended_chain(data, ext_start, ext_start, &mut logical);
    }

    Ok((primary, logical))
}

/// Walk an extended partition chain. Each EBR is at `current_lba` (absolute),
/// `ext_base` is the start of the entire extended partition.
fn parse_extended_chain(
    data: &[u8],
    current_lba: u32,
    ext_base: u32,
    logical: &mut Vec<MbrPartition>,
) {
    let offset = (current_lba as usize).saturating_mul(SECTOR_SIZE as usize);
    if offset + 512 > data.len() {
        return;
    }
    // Check EBR signature.
    if read_u16_le(data, offset + 510) != 0xAA55 {
        return;
    }

    // First entry: the logical partition (relative to current_lba).
    let entry = parse_mbr_entry(data, offset + 446);
    if entry.part_type != 0 && entry.sector_count > 0 && is_mappable_mbr_type(entry.part_type) {
        logical.push(MbrPartition {
            status: entry.status,
            part_type: entry.part_type,
            lba_start: current_lba.saturating_add(entry.lba_start),
            sector_count: entry.sector_count,
        });
    }

    // Second entry: link to next EBR (relative to ext_base).
    let next = parse_mbr_entry(data, offset + 462);
    if next.part_type != 0 && next.sector_count > 0 && is_extended_type(next.part_type) {
        let next_lba = ext_base.saturating_add(next.lba_start);
        // Guard against infinite loops.
        if next_lba != current_lba && next_lba > ext_base {
            parse_extended_chain(data, next_lba, ext_base, logical);
        }
    }
}

/// Try to parse a GPT from the provided disk image bytes.
fn parse_gpt(data: &[u8]) -> Result<Vec<GptPartition>, String> {
    // GPT header is at LBA 1.
    let hdr_offset = (GPT_HEADER_LBA as usize).saturating_mul(SECTOR_SIZE as usize);
    if data.len() < hdr_offset + 92 {
        return Err("disk image too small for GPT header".to_string());
    }

    // Verify magic.
    let magic = &data[hdr_offset..hdr_offset + 8];
    if magic != GPT_MAGIC {
        return Err("GPT magic not found".to_string());
    }

    let part_entry_lba = read_u64_le(data, hdr_offset + 72);
    let num_entries = read_u32_le(data, hdr_offset + 80);
    let entry_size = read_u32_le(data, hdr_offset + 84);

    if entry_size < 128 {
        return Err(format!("GPT entry size too small: {entry_size}"));
    }

    let entries_offset = (part_entry_lba as usize).saturating_mul(SECTOR_SIZE as usize);
    let mut entries = Vec::new();
    let zero_guid = [0u8; 16];

    for i in 0..num_entries as usize {
        let base = entries_offset + i * entry_size as usize;
        if base + 128 > data.len() {
            break;
        }

        let mut type_guid = [0u8; 16];
        type_guid.copy_from_slice(&data[base..base + 16]);
        if type_guid == zero_guid {
            continue;
        }

        let mut unique_guid = [0u8; 16];
        unique_guid.copy_from_slice(&data[base + 16..base + 32]);

        let first_lba = read_u64_le(data, base + 32);
        let last_lba = read_u64_le(data, base + 40);

        // Name is UTF-16LE at offset 56, up to 72 bytes (36 UTF-16 code units).
        let name_bytes = &data[base + 56..std::cmp::min(base + 128, data.len())];
        let name = parse_utf16le_name(name_bytes);

        entries.push(GptPartition {
            type_guid,
            unique_guid,
            first_lba,
            last_lba,
            name,
        });
    }

    Ok(entries)
}

fn parse_utf16le_name(raw: &[u8]) -> String {
    let mut chars = Vec::new();
    let mut i = 0;
    while i + 1 < raw.len() {
        let code_unit = raw[i] as u16 | ((raw[i + 1] as u16) << 8);
        if code_unit == 0 {
            break;
        }
        if let Some(ch) = char::from_u32(code_unit as u32) {
            chars.push(ch);
        }
        i += 2;
    }
    chars.into_iter().collect()
}

/// Detect and parse partition table from disk image bytes.
fn parse_partition_table(data: &[u8]) -> Result<PartitionTable, String> {
    // Try GPT first (GPT disks also have a protective MBR).
    if let Ok(entries) = parse_gpt(data)
        && !entries.is_empty()
    {
        return Ok(PartitionTable::Gpt { entries });
    }

    // Fall back to MBR.
    let (primary, logical) = parse_mbr(data)?;
    Ok(PartitionTable::Mbr { primary, logical })
}

// ---------------------------------------------------------------------------
// Name splitting (LVM)
// ---------------------------------------------------------------------------

/// Split a device-mapper name in LVM VG-LV format. The first hyphen that is
/// not part of a doubled-hyphen escape is the separator.
fn split_dm_name(name: &str) -> (String, String) {
    // LVM doubles hyphens in VG and LV names, so "my--vg-my--lv" means
    // VG="my-vg", LV="my-lv". The split point is a single hyphen.
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'-' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                // Escaped hyphen, skip both.
                i += 2;
                continue;
            }
            // Found the split point.
            let vg_raw = &name[..i];
            let lv_raw = &name[i + 1..];
            let vg = vg_raw.replace("--", "-");
            let lv = lv_raw.replace("--", "-");
            return (vg, lv);
        }
        i += 1;
    }
    // No split found — entire name is VG.
    (name.replace("--", "-"), String::new())
}

// ---------------------------------------------------------------------------
// Tree-view helper
// ---------------------------------------------------------------------------

fn print_tree(devices: &BTreeMap<String, MappedDevice>, out: &mut dyn Write) {
    for dev in devices.values() {
        let _ = writeln!(out, "{} ({}:{})", dev.name, dev.major, dev.minor);
        for entry in &dev.active_table {
            let _ = writeln!(out, "  └─ {} {}", entry.target_type, entry.target_args);
        }
    }
}

// ---------------------------------------------------------------------------
// Dependency extraction
// ---------------------------------------------------------------------------

/// Extract device references from table args. Convention: device references
/// look like major:minor pairs or /dev/xxx paths.
fn extract_deps(table: &[TableEntry]) -> Vec<String> {
    let mut deps = Vec::new();
    for entry in table {
        for token in entry.target_args.split_whitespace() {
            // major:minor pattern.
            if token.contains(':') {
                let parts: Vec<&str> = token.split(':').collect();
                if parts.len() == 2
                    && parts[0].parse::<u32>().is_ok()
                    && parts[1].parse::<u32>().is_ok()
                {
                    deps.push(token.to_string());
                }
            }
            // /dev/ path.
            if token.starts_with("/dev/") {
                deps.push(token.to_string());
            }
        }
    }
    deps
}

// ---------------------------------------------------------------------------
// dmsetup commands
// ---------------------------------------------------------------------------

fn dmsetup_create(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    let (name, table_str) = match parse_name_and_table(args) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(out, "Error: {e}");
            return 1;
        }
    };

    if state.devices.contains_key(&name) {
        let _ = writeln!(out, "Error: device '{name}' already exists");
        return 1;
    }

    let table = match parse_table(&table_str) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Error: bad table: {e}");
            return 1;
        }
    };

    let minor = state.alloc_minor();
    state
        .devices
        .insert(name.clone(), MappedDevice::new(&name, minor, table));
    0
}

fn dmsetup_remove(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let name = &args[0];
    if state.devices.remove(name).is_none() {
        let _ = writeln!(out, "Error: device '{name}' not found");
        return 1;
    }
    0
}

fn dmsetup_remove_all(state: &mut DmState, _out: &mut dyn Write) -> i32 {
    state.devices.clear();
    0
}

fn dmsetup_suspend(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    match state.devices.get_mut(&args[0]) {
        Some(dev) => {
            dev.state = DeviceState::Suspended;
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{}' not found", args[0]);
            1
        }
    }
}

fn dmsetup_resume(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    match state.devices.get_mut(&args[0]) {
        Some(dev) => {
            // If there is an inactive table, promote it.
            if let Some(new_table) = dev.inactive_table.take() {
                dev.active_table = new_table;
            }
            dev.state = DeviceState::Active;
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{}' not found", args[0]);
            1
        }
    }
}

fn dmsetup_load(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    let (name, table_str) = match parse_name_and_table(args) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(out, "Error: {e}");
            return 1;
        }
    };

    let table = match parse_table(&table_str) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Error: bad table: {e}");
            return 1;
        }
    };

    match state.devices.get_mut(&name) {
        Some(dev) => {
            dev.inactive_table = Some(table);
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{name}' not found");
            1
        }
    }
}

fn dmsetup_info(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        // Show all devices.
        if state.devices.is_empty() {
            let _ = writeln!(out, "No devices found");
            return 0;
        }
        for dev in state.devices.values() {
            print_device_info(dev, out);
        }
        return 0;
    }
    match state.devices.get(&args[0]) {
        Some(dev) => {
            print_device_info(dev, out);
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{}' not found", args[0]);
            1
        }
    }
}

fn print_device_info(dev: &MappedDevice, out: &mut dyn Write) {
    let _ = writeln!(out, "Name:              {}", dev.name);
    let _ = writeln!(out, "State:             {}", dev.state);
    let _ = writeln!(out, "Read Ahead:        256");
    let _ = writeln!(
        out,
        "Tables present:    {}",
        if dev.inactive_table.is_some() {
            "LIVE & INACTIVE"
        } else {
            "LIVE"
        }
    );
    let _ = writeln!(out, "Open count:        {}", dev.open_count);
    let _ = writeln!(out, "Event number:      {}", dev.event_nr);
    let _ = writeln!(out, "Major, minor:      {}, {}", dev.major, dev.minor);
    let _ = writeln!(out, "Number of targets:  {}", dev.active_table.len());
    let _ = writeln!(out, "UUID:              {}", dev.uuid);
    let _ = writeln!(out);
}

fn dmsetup_table(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        for dev in state.devices.values() {
            for entry in &dev.active_table {
                let _ = writeln!(out, "{}: {}", dev.name, entry);
            }
        }
        return 0;
    }
    match state.devices.get(&args[0]) {
        Some(dev) => {
            for entry in &dev.active_table {
                let _ = writeln!(out, "{}: {}", dev.name, entry);
            }
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{}' not found", args[0]);
            1
        }
    }
}

fn dmsetup_status(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        for dev in state.devices.values() {
            print_device_status(dev, out);
        }
        return 0;
    }
    match state.devices.get(&args[0]) {
        Some(dev) => {
            print_device_status(dev, out);
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{}' not found", args[0]);
            1
        }
    }
}

fn print_device_status(dev: &MappedDevice, out: &mut dyn Write) {
    for entry in &dev.active_table {
        let status_info = match entry.target_type.as_str() {
            "linear" => "0".to_string(),
            "striped" => "0".to_string(),
            "mirror" => "1 AA 0".to_string(),
            "snapshot" => "0/0 0".to_string(),
            "snapshot-origin" => "0".to_string(),
            "thin" => "0 0".to_string(),
            "thin-pool" => "0 0/0 0/0 - rw no_discard_passdown queue_if_no_space -".to_string(),
            "cache" => "0 0/0 0 0 0 0 0 0 0 writeback 2 migration_threshold 2048".to_string(),
            "crypt" => "0".to_string(),
            "zero" => "".to_string(),
            "error" => "".to_string(),
            "delay" => "0 0".to_string(),
            _ => "".to_string(),
        };
        let _ = writeln!(
            out,
            "{}: {} {} {} {}",
            dev.name, entry.start_sector, entry.length, entry.target_type, status_info
        );
    }
}

fn dmsetup_ls(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if state.devices.is_empty() {
        let _ = writeln!(out, "No devices found");
        return 0;
    }
    let tree = args.iter().any(|a| a == "--tree");
    if tree {
        print_tree(&state.devices, out);
    } else {
        for dev in state.devices.values() {
            let _ = writeln!(out, "{}\t({}:{})", dev.name, dev.major, dev.minor);
        }
    }
    0
}

fn dmsetup_deps(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        for dev in state.devices.values() {
            let deps = extract_deps(&dev.active_table);
            let _ = writeln!(
                out,
                "{}: {} dependencies: [{}]",
                dev.name,
                deps.len(),
                deps.join(", ")
            );
        }
        return 0;
    }
    match state.devices.get(&args[0]) {
        Some(dev) => {
            let deps = extract_deps(&dev.active_table);
            let _ = writeln!(out, "{} dependencies: [{}]", deps.len(), deps.join(", "));
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{}' not found", args[0]);
            1
        }
    }
}

fn dmsetup_targets(out: &mut dyn Write) -> i32 {
    for &(name, ver) in KNOWN_TARGETS {
        let _ = writeln!(out, "{:<20} v{}", name, ver);
    }
    0
}

fn dmsetup_version(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "Library version:   {DM_VERSION}");
    let _ = writeln!(out, "Driver version:    {DM_VERSION}");
    0
}

fn dmsetup_wait(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let name = &args[0];
    let expected_nr = args.get(1).and_then(|s| s.parse::<u64>().ok());

    match state.devices.get_mut(name) {
        Some(dev) => {
            // Simulate an event bump.
            dev.event_nr = dev.event_nr.saturating_add(1);
            if let Some(nr) = expected_nr
                && dev.event_nr <= nr
            {
                let _ = writeln!(
                    out,
                    "Event {} already passed (current: {})",
                    nr, dev.event_nr
                );
            }
            let _ = writeln!(out, "{}: event_nr={}", dev.name, dev.event_nr);
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{name}' not found");
            1
        }
    }
}

fn dmsetup_message(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.len() < 3 {
        let _ = writeln!(out, "Error: usage: message <name> <sector> <message>");
        return 1;
    }
    let name = &args[0];
    let sector_str = &args[1];
    let message = args[2..].join(" ");

    let _sector = match sector_str.parse::<u64>() {
        Ok(s) => s,
        Err(_) => {
            let _ = writeln!(out, "Error: bad sector '{sector_str}'");
            return 1;
        }
    };

    match state.devices.get_mut(name) {
        Some(dev) => {
            // Simulate message delivery.
            dev.event_nr = dev.event_nr.saturating_add(1);
            let _ = writeln!(out, "Message '{}' delivered to {}", message, dev.name);
            0
        }
        None => {
            let _ = writeln!(out, "Error: device '{name}' not found");
            1
        }
    }
}

fn dmsetup_setgeometry(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.len() < 5 {
        let _ = writeln!(
            out,
            "Error: usage: setgeometry <name> <cylinders> <heads> <sectors> <sector_size>"
        );
        return 1;
    }
    let name = &args[0];
    let cyl = args[1].parse::<u64>();
    let head = args[2].parse::<u64>();
    let sect = args[3].parse::<u64>();
    let sect_size = args[4].parse::<u64>();

    if cyl.is_err() || head.is_err() || sect.is_err() || sect_size.is_err() {
        let _ = writeln!(out, "Error: all geometry values must be numeric");
        return 1;
    }

    if !state.devices.contains_key(name) {
        let _ = writeln!(out, "Error: device '{name}' not found");
        return 1;
    }

    let _ = writeln!(
        out,
        "Geometry set: {} cylinders={} heads={} sectors={} sector_size={}",
        name,
        cyl.unwrap_or(0),
        head.unwrap_or(0),
        sect.unwrap_or(0),
        sect_size.unwrap_or(0)
    );
    0
}

fn dmsetup_splitname(args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let (vg, lv) = split_dm_name(&args[0]);
    let _ = writeln!(out, "  VG:   {vg}");
    let _ = writeln!(out, "  LV:   {lv}");
    let _ = writeln!(out, "  LVM:  {}", if lv.is_empty() { "no" } else { "yes" });
    0
}

fn dmsetup_udevcomplete(args: &[String], out: &mut dyn Write) -> i32 {
    let cookie = args.first().map(|s| s.as_str()).unwrap_or("0");
    let _ = writeln!(out, "udev transaction {cookie} complete");
    0
}

fn dmsetup_udevcookies(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "cookie   semid  flags");
    let _ = writeln!(out, "(no active cookies)");
    0
}

fn dmsetup_udevcreatecookie(out: &mut dyn Write) -> i32 {
    // Simulated cookie value.
    let _ = writeln!(out, "cookie: 0xd4d10000");
    0
}

fn dmsetup_help(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "Usage: dmsetup <command> [options]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Commands:");
    let _ = writeln!(
        out,
        "  create <name> --table \"<table>\"  Create a mapped device"
    );
    let _ = writeln!(
        out,
        "  remove <name>                    Remove a mapped device"
    );
    let _ = writeln!(
        out,
        "  remove_all                       Remove all mapped devices"
    );
    let _ = writeln!(
        out,
        "  suspend <name>                   Suspend a mapped device"
    );
    let _ = writeln!(
        out,
        "  resume <name>                    Resume a suspended device"
    );
    let _ = writeln!(
        out,
        "  load <name> --table \"<table>\"    Load inactive table"
    );
    let _ = writeln!(
        out,
        "  info [name]                      Display device info"
    );
    let _ = writeln!(
        out,
        "  table [name]                     Display mapping table"
    );
    let _ = writeln!(
        out,
        "  status [name]                    Display target status"
    );
    let _ = writeln!(
        out,
        "  ls [--tree]                      List mapped devices"
    );
    let _ = writeln!(
        out,
        "  deps [name]                      Display dependencies"
    );
    let _ = writeln!(
        out,
        "  targets                          List available target types"
    );
    let _ = writeln!(
        out,
        "  version                          Display version info"
    );
    let _ = writeln!(out, "  wait <name> [event_nr]           Wait for an event");
    let _ = writeln!(
        out,
        "  message <name> <sector> <msg>    Send message to target"
    );
    let _ = writeln!(
        out,
        "  setgeometry <name> <c> <h> <s> <sz>  Set device geometry"
    );
    let _ = writeln!(
        out,
        "  splitname <name>                 Split LVM VG-LV name"
    );
    let _ = writeln!(
        out,
        "  udevcomplete [cookie]            Complete udev transaction"
    );
    let _ = writeln!(out, "  udevcookies                      List udev cookies");
    let _ = writeln!(out, "  udevcreatecookie                 Create udev cookie");
    let _ = writeln!(out, "  help                             Show this help");
    0
}

// ---------------------------------------------------------------------------
// dmstats commands
// ---------------------------------------------------------------------------

fn dmstats_create(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let device = &args[0];

    if !state.devices.contains_key(device) {
        let _ = writeln!(out, "Error: device '{device}' not found");
        return 1;
    }

    let mut start: u64 = 0;
    let mut length: u64 = 0;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--start" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    start = v.parse().unwrap_or(0);
                }
            }
            "--length" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    length = v.parse().unwrap_or(0);
                }
            }
            _ => {}
        }
        i += 1;
    }

    // If length is 0, use the device's full size.
    if length == 0
        && let Some(dev) = state.devices.get(device)
    {
        length = dev.total_sectors();
    }

    let region_id = state.next_region_id;
    state.next_region_id = state.next_region_id.wrapping_add(1);
    state
        .stats_regions
        .push(StatsRegion::new(region_id, device, start, length));
    let _ = writeln!(out, "Created region {region_id} on {device}");
    0
}

fn dmstats_delete(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let device = &args[0];

    let all = args.iter().any(|a| a == "--allregions");
    if all {
        let before = state.stats_regions.len();
        state.stats_regions.retain(|r| r.device_name != *device);
        let removed = before - state.stats_regions.len();
        let _ = writeln!(out, "Deleted {removed} region(s) from {device}");
        return 0;
    }

    // Otherwise delete specific region.
    let region_id = args.get(1).and_then(|s| s.parse::<u64>().ok());
    match region_id {
        Some(rid) => {
            let before = state.stats_regions.len();
            state
                .stats_regions
                .retain(|r| !(r.device_name == *device && r.region_id == rid));
            if state.stats_regions.len() < before {
                let _ = writeln!(out, "Deleted region {rid} from {device}");
                0
            } else {
                let _ = writeln!(out, "Error: region {rid} not found on {device}");
                1
            }
        }
        None => {
            let _ = writeln!(out, "Error: specify --allregions or a region_id");
            1
        }
    }
}

fn dmstats_list(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let device = &args[0];
    let regions: Vec<&StatsRegion> = state
        .stats_regions
        .iter()
        .filter(|r| r.device_name == *device)
        .collect();

    if regions.is_empty() {
        let _ = writeln!(out, "No regions on {device}");
        return 0;
    }

    let _ = writeln!(out, "{:<10} {:<12} {:<12}", "RegionID", "Start", "Length");
    for r in regions {
        let _ = writeln!(
            out,
            "{:<10} {:<12} {:<12}",
            r.region_id, r.start_sector, r.length
        );
    }
    0
}

fn dmstats_print(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let device = &args[0];
    let filter_id = args.get(1).and_then(|s| s.parse::<u64>().ok());

    let regions: Vec<&StatsRegion> = state
        .stats_regions
        .iter()
        .filter(|r| r.device_name == *device && filter_id.is_none_or(|id| r.region_id == id))
        .collect();

    if regions.is_empty() {
        let _ = writeln!(out, "No matching regions on {device}");
        return 0;
    }

    for r in regions {
        let _ = writeln!(
            out,
            "region{}: {} {} {} {} {} {} {} {} {} {} {}",
            r.region_id,
            r.start_sector,
            r.length,
            r.reads,
            r.writes,
            r.read_sectors,
            r.write_sectors,
            r.read_time_ms,
            r.write_time_ms,
            r.in_flight,
            0, // io_ticks placeholder
            0, // time_in_queue placeholder
        );
    }
    0
}

fn dmstats_report(state: &DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: device name required");
        return 1;
    }
    let device = &args[0];
    let regions: Vec<&StatsRegion> = state
        .stats_regions
        .iter()
        .filter(|r| r.device_name == *device)
        .collect();

    if regions.is_empty() {
        let _ = writeln!(out, "No regions on {device}");
        return 0;
    }

    let _ = writeln!(
        out,
        "{:<8} {:<10} {:<10} {:<8} {:<8} {:<12} {:<12} {:<10} {:<10} {:<8}",
        "Region",
        "Start",
        "Length",
        "Reads",
        "Writes",
        "RdSectors",
        "WrSectors",
        "RdMs",
        "WrMs",
        "InFlight"
    );
    for r in regions {
        let _ = writeln!(
            out,
            "{:<8} {:<10} {:<10} {:<8} {:<8} {:<12} {:<12} {:<10} {:<10} {:<8}",
            r.region_id,
            r.start_sector,
            r.length,
            r.reads,
            r.writes,
            r.read_sectors,
            r.write_sectors,
            r.read_time_ms,
            r.write_time_ms,
            r.in_flight
        );
    }
    0
}

fn dmstats_help(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "Usage: dmstats <command> [options]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Commands:");
    let _ = writeln!(
        out,
        "  create <device> [--start <s> --length <l>]  Create stats region"
    );
    let _ = writeln!(
        out,
        "  delete <device> [--allregions | <region_id>] Delete region(s)"
    );
    let _ = writeln!(
        out,
        "  list <device>                                List regions"
    );
    let _ = writeln!(
        out,
        "  print <device> [region_id]                   Print raw counters"
    );
    let _ = writeln!(
        out,
        "  report <device>                              Formatted report"
    );
    let _ = writeln!(
        out,
        "  help                                         Show this help"
    );
    0
}

// ---------------------------------------------------------------------------
// kpartx commands
// ---------------------------------------------------------------------------

fn kpartx_add(state: &mut DmState, device: &str, disk_data: &[u8], out: &mut dyn Write) -> i32 {
    let table = match parse_partition_table(disk_data) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Error: {e}");
            return 1;
        }
    };

    let dev_base = device.rsplit(['/', '\\']).next().unwrap_or(device);

    let parts = partition_mappings(&table, dev_base);
    if parts.is_empty() {
        let _ = writeln!(out, "No partitions found on {device}");
        return 0;
    }

    for (map_name, entry) in &parts {
        if state.devices.contains_key(map_name) {
            let _ = writeln!(out, "Warning: {map_name} already exists, skipping");
            continue;
        }
        let minor = state.alloc_minor();
        state.devices.insert(
            map_name.clone(),
            MappedDevice::new(map_name, minor, vec![entry.clone()]),
        );
        let _ = writeln!(
            out,
            "add map {map_name}: {} {} linear {}",
            entry.start_sector, entry.length, entry.target_args
        );
    }
    0
}

fn kpartx_delete(state: &mut DmState, device: &str, disk_data: &[u8], out: &mut dyn Write) -> i32 {
    let table = match parse_partition_table(disk_data) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Error: {e}");
            return 1;
        }
    };

    let dev_base = device.rsplit(['/', '\\']).next().unwrap_or(device);

    let parts = partition_mappings(&table, dev_base);
    for (map_name, _) in &parts {
        if state.devices.remove(map_name).is_some() {
            let _ = writeln!(out, "del devmap: {map_name}");
        }
    }
    0
}

fn kpartx_list(device: &str, disk_data: &[u8], out: &mut dyn Write) -> i32 {
    let table = match parse_partition_table(disk_data) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Error: {e}");
            return 1;
        }
    };

    let dev_base = device.rsplit(['/', '\\']).next().unwrap_or(device);

    let parts = partition_mappings(&table, dev_base);
    if parts.is_empty() {
        let _ = writeln!(out, "No partitions found on {device}");
        return 0;
    }

    for (map_name, entry) in &parts {
        let _ = writeln!(
            out,
            "{map_name}: {} {} linear {}",
            entry.start_sector, entry.length, entry.target_args
        );
    }
    0
}

/// Generate partition map names and table entries from a parsed table.
fn partition_mappings(table: &PartitionTable, dev_base: &str) -> Vec<(String, TableEntry)> {
    let mut result = Vec::new();
    match table {
        PartitionTable::Mbr { primary, logical } => {
            let mut part_num = 1u32;
            for p in primary {
                if p.sector_count > 0 {
                    result.push((
                        format!("{dev_base}p{part_num}"),
                        TableEntry {
                            start_sector: 0,
                            length: p.sector_count as u64,
                            target_type: "linear".to_string(),
                            target_args: format!("/dev/{dev_base} {}", p.lba_start),
                        },
                    ));
                    part_num = part_num.saturating_add(1);
                }
            }
            // Logical partitions start at 5 by MBR convention.
            let mut log_num = 5u32;
            for p in logical {
                if p.sector_count > 0 {
                    result.push((
                        format!("{dev_base}p{log_num}"),
                        TableEntry {
                            start_sector: 0,
                            length: p.sector_count as u64,
                            target_type: "linear".to_string(),
                            target_args: format!("/dev/{dev_base} {}", p.lba_start),
                        },
                    ));
                    log_num = log_num.saturating_add(1);
                }
            }
        }
        PartitionTable::Gpt { entries } => {
            for (i, p) in entries.iter().enumerate() {
                let size = p.last_lba.saturating_sub(p.first_lba).saturating_add(1);
                let part_num = (i as u32).saturating_add(1);
                result.push((
                    format!("{dev_base}p{part_num}"),
                    TableEntry {
                        start_sector: 0,
                        length: size,
                        target_type: "linear".to_string(),
                        target_args: format!("/dev/{dev_base} {}", p.first_lba),
                    },
                ));
            }
        }
    }
    result
}

fn kpartx_help(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "Usage: kpartx [-a|-d|-l] <device>");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  -a <device>   Add partition mappings");
    let _ = writeln!(out, "  -d <device>   Delete partition mappings");
    let _ = writeln!(out, "  -l <device>   List partition mappings");
    let _ = writeln!(out, "  -h, --help    Show this help");
    0
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// Parse `<name> --table "<table>"` from the argument list.
fn parse_name_and_table(args: &[String]) -> Result<(String, String), String> {
    if args.is_empty() {
        return Err("device name required".to_string());
    }
    let name = args[0].clone();

    let mut table_str = String::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--table" {
            i += 1;
            if let Some(t) = args.get(i) {
                table_str = t.clone();
            } else {
                return Err("--table requires an argument".to_string());
            }
        }
        i += 1;
    }

    if table_str.is_empty() {
        return Err("--table option required".to_string());
    }

    Ok((name, table_str))
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn run_dmsetup(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        return dmsetup_help(out);
    }
    let cmd = args[0].as_str();
    let rest: Vec<String> = args[1..].to_vec();

    match cmd {
        "create" => dmsetup_create(state, &rest, out),
        "remove" => dmsetup_remove(state, &rest, out),
        "remove_all" => dmsetup_remove_all(state, out),
        "suspend" => dmsetup_suspend(state, &rest, out),
        "resume" => dmsetup_resume(state, &rest, out),
        "load" => dmsetup_load(state, &rest, out),
        "info" => dmsetup_info(state, &rest, out),
        "table" => dmsetup_table(state, &rest, out),
        "status" => dmsetup_status(state, &rest, out),
        "ls" => dmsetup_ls(state, &rest, out),
        "deps" => dmsetup_deps(state, &rest, out),
        "targets" => dmsetup_targets(out),
        "version" => dmsetup_version(out),
        "wait" => dmsetup_wait(state, &rest, out),
        "message" => dmsetup_message(state, &rest, out),
        "setgeometry" => dmsetup_setgeometry(state, &rest, out),
        "splitname" => dmsetup_splitname(&rest, out),
        "udevcomplete" => dmsetup_udevcomplete(&rest, out),
        "udevcookies" => dmsetup_udevcookies(out),
        "udevcreatecookie" => dmsetup_udevcreatecookie(out),
        "help" | "--help" | "-h" => dmsetup_help(out),
        "--version" | "-V" => dmsetup_version(out),
        _ => {
            let _ = writeln!(out, "Unknown command: {cmd}");
            let _ = writeln!(out, "Try 'dmsetup help' for usage");
            1
        }
    }
}

fn run_dmstats(state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        return dmstats_help(out);
    }
    let cmd = args[0].as_str();
    let rest: Vec<String> = args[1..].to_vec();

    match cmd {
        "create" => dmstats_create(state, &rest, out),
        "delete" => dmstats_delete(state, &rest, out),
        "list" => dmstats_list(state, &rest, out),
        "print" => dmstats_print(state, &rest, out),
        "report" => dmstats_report(state, &rest, out),
        "help" | "--help" | "-h" => dmstats_help(out),
        _ => {
            let _ = writeln!(out, "Unknown command: {cmd}");
            let _ = writeln!(out, "Try 'dmstats help' for usage");
            1
        }
    }
}

fn run_kpartx(_state: &mut DmState, args: &[String], out: &mut dyn Write) -> i32 {
    // kpartx uses flags: -a, -d, -l
    if args.is_empty() {
        return kpartx_help(out);
    }

    let flag = args[0].as_str();
    match flag {
        "-a" | "-d" | "-l" => {
            if args.len() < 2 {
                let _ = writeln!(out, "Error: device argument required");
                return 1;
            }
            let device = &args[1];
            // In a real system we'd read the device. Here we use an empty disk image
            // placeholder; real invocations would provide data via a different path.
            // For testing, we use kpartx_add_with_data etc.
            let _ = writeln!(
                out,
                "Error: cannot read device '{}' (simulated environment)",
                device
            );
            1
        }
        "-h" | "--help" | "help" => kpartx_help(out),
        _ => {
            let _ = writeln!(out, "Unknown option: {flag}");
            kpartx_help(out);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("dmsetup");
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

    let tool = detect_tool(&prog_name);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let mut state = DmState::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let code = match tool {
        Tool::Dmsetup => run_dmsetup(&mut state, &rest, &mut out),
        Tool::Dmstats => run_dmstats(&mut state, &rest, &mut out),
        Tool::Kpartx => run_kpartx(&mut state, &rest, &mut out),
    };

    process::exit(code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: run a dmsetup command sequence and capture output.
    fn run_dm(state: &mut DmState, args: &[&str]) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let code = run_dmsetup(state, &args, &mut out);
        (code, String::from_utf8_lossy(&out).to_string())
    }

    fn run_stats(state: &mut DmState, args: &[&str]) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let code = run_dmstats(state, &args, &mut out);
        (code, String::from_utf8_lossy(&out).to_string())
    }

    fn new_state() -> DmState {
        DmState::new()
    }

    fn create_test_device(state: &mut DmState, name: &str, table: &str) -> i32 {
        let (code, _) = run_dm(state, &["create", name, "--table", table]);
        code
    }

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn detect_dmsetup_plain() {
        assert_eq!(detect_tool("dmsetup"), Tool::Dmsetup);
    }

    #[test]
    fn detect_dmsetup_with_path() {
        assert_eq!(detect_tool("/usr/sbin/dmsetup"), Tool::Dmsetup);
    }

    #[test]
    fn detect_dmsetup_windows_path() {
        assert_eq!(detect_tool("C:\\bin\\dmsetup.exe"), Tool::Dmsetup);
    }

    #[test]
    fn detect_dmstats_plain() {
        assert_eq!(detect_tool("dmstats"), Tool::Dmstats);
    }

    #[test]
    fn detect_dmstats_with_path() {
        assert_eq!(detect_tool("/usr/sbin/dmstats"), Tool::Dmstats);
    }

    #[test]
    fn detect_kpartx_plain() {
        assert_eq!(detect_tool("kpartx"), Tool::Kpartx);
    }

    #[test]
    fn detect_kpartx_with_exe() {
        assert_eq!(detect_tool("kpartx.exe"), Tool::Kpartx);
    }

    #[test]
    fn detect_unknown_defaults_dmsetup() {
        assert_eq!(detect_tool("sometool"), Tool::Dmsetup);
    }

    #[test]
    fn tool_name_dmsetup() {
        assert_eq!(Tool::Dmsetup.name(), "dmsetup");
    }

    #[test]
    fn tool_name_dmstats() {
        assert_eq!(Tool::Dmstats.name(), "dmstats");
    }

    #[test]
    fn tool_name_kpartx() {
        assert_eq!(Tool::Kpartx.name(), "kpartx");
    }

    // -----------------------------------------------------------------------
    // Table parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_table_line_linear() {
        let e = parse_table_line("0 1024 linear /dev/sda1 0").unwrap();
        assert_eq!(e.start_sector, 0);
        assert_eq!(e.length, 1024);
        assert_eq!(e.target_type, "linear");
        assert_eq!(e.target_args, "/dev/sda1 0");
    }

    #[test]
    fn parse_table_line_striped() {
        let e = parse_table_line("0 2048 striped 2 64 /dev/sda 0 /dev/sdb 0").unwrap();
        assert_eq!(e.target_type, "striped");
        assert!(e.target_args.contains("/dev/sda"));
    }

    #[test]
    fn parse_table_line_zero() {
        let e = parse_table_line("0 512 zero").unwrap();
        assert_eq!(e.target_type, "zero");
        assert_eq!(e.target_args, "");
    }

    #[test]
    fn parse_table_line_error() {
        let e = parse_table_line("0 512 error").unwrap();
        assert_eq!(e.target_type, "error");
    }

    #[test]
    fn parse_table_line_unknown_target() {
        assert!(parse_table_line("0 512 nonexistent_target").is_err());
    }

    #[test]
    fn parse_table_line_bad_start() {
        assert!(parse_table_line("abc 512 linear /dev/sda 0").is_err());
    }

    #[test]
    fn parse_table_line_bad_length() {
        assert!(parse_table_line("0 xyz linear /dev/sda 0").is_err());
    }

    #[test]
    fn parse_table_line_missing_target() {
        assert!(parse_table_line("0 512").is_err());
    }

    #[test]
    fn parse_table_line_missing_length() {
        assert!(parse_table_line("0").is_err());
    }

    #[test]
    fn parse_table_line_empty() {
        assert!(parse_table_line("").is_err());
    }

    #[test]
    fn parse_table_multiline() {
        let t = parse_table("0 512 linear /dev/sda 0\n512 512 linear /dev/sdb 0").unwrap();
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn parse_table_empty_string() {
        assert!(parse_table("").is_err());
    }

    #[test]
    fn parse_table_blank_lines_ignored() {
        let t = parse_table("\n0 512 zero\n\n").unwrap();
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn parse_table_mirror() {
        let e = parse_table_line("0 4096 mirror core 1 1024 2 /dev/sda 0 /dev/sdb 0").unwrap();
        assert_eq!(e.target_type, "mirror");
    }

    #[test]
    fn parse_table_snapshot() {
        let e = parse_table_line("0 4096 snapshot /dev/origin /dev/cow P 16").unwrap();
        assert_eq!(e.target_type, "snapshot");
    }

    #[test]
    fn parse_table_thin() {
        let e = parse_table_line("0 2097152 thin 253:0 1").unwrap();
        assert_eq!(e.target_type, "thin");
    }

    #[test]
    fn parse_table_thin_pool() {
        let e = parse_table_line("0 2097152 thin-pool 253:1 253:2 128 0").unwrap();
        assert_eq!(e.target_type, "thin-pool");
    }

    #[test]
    fn parse_table_cache() {
        let e = parse_table_line("0 4096 cache 253:0 253:1 253:2 128 1 writeback smq 0").unwrap();
        assert_eq!(e.target_type, "cache");
    }

    #[test]
    fn parse_table_crypt() {
        let e =
            parse_table_line("0 2048 crypt aes-xts-plain64 0123456789abcdef 0 /dev/sda 0").unwrap();
        assert_eq!(e.target_type, "crypt");
    }

    #[test]
    fn parse_table_delay() {
        let e = parse_table_line("0 1024 delay /dev/sda 0 100").unwrap();
        assert_eq!(e.target_type, "delay");
    }

    #[test]
    fn parse_table_snapshot_origin() {
        let e = parse_table_line("0 4096 snapshot-origin /dev/sda 0").unwrap();
        assert_eq!(e.target_type, "snapshot-origin");
    }

    #[test]
    fn table_entry_display() {
        let e = TableEntry {
            start_sector: 0,
            length: 1024,
            target_type: "linear".to_string(),
            target_args: "/dev/sda 0".to_string(),
        };
        assert_eq!(format!("{e}"), "0 1024 linear /dev/sda 0");
    }

    // -----------------------------------------------------------------------
    // Device state model
    // -----------------------------------------------------------------------

    #[test]
    fn device_state_display() {
        assert_eq!(format!("{}", DeviceState::Active), "ACTIVE");
        assert_eq!(format!("{}", DeviceState::Suspended), "SUSPENDED");
    }

    #[test]
    fn mapped_device_new() {
        let dev = MappedDevice::new("test", 0, vec![]);
        assert_eq!(dev.name, "test");
        assert_eq!(dev.uuid, "OUROS-test");
        assert_eq!(dev.major, 253);
        assert_eq!(dev.minor, 0);
        assert_eq!(dev.state, DeviceState::Active);
        assert_eq!(dev.open_count, 0);
        assert_eq!(dev.event_nr, 0);
    }

    #[test]
    fn mapped_device_total_sectors() {
        let dev = MappedDevice::new(
            "test",
            0,
            vec![
                TableEntry {
                    start_sector: 0,
                    length: 512,
                    target_type: "linear".to_string(),
                    target_args: String::new(),
                },
                TableEntry {
                    start_sector: 512,
                    length: 512,
                    target_type: "linear".to_string(),
                    target_args: String::new(),
                },
            ],
        );
        assert_eq!(dev.total_sectors(), 1024);
    }

    #[test]
    fn mapped_device_total_sectors_empty() {
        let dev = MappedDevice::new("test", 0, vec![]);
        assert_eq!(dev.total_sectors(), 0);
    }

    // -----------------------------------------------------------------------
    // dmsetup: create
    // -----------------------------------------------------------------------

    #[test]
    fn create_basic() {
        let mut s = new_state();
        let (code, _) = run_dm(
            &mut s,
            &["create", "test", "--table", "0 1024 linear /dev/sda 0"],
        );
        assert_eq!(code, 0);
        assert!(s.devices.contains_key("test"));
    }

    #[test]
    fn create_sets_minor() {
        let mut s = new_state();
        create_test_device(&mut s, "a", "0 512 zero");
        create_test_device(&mut s, "b", "0 512 zero");
        assert_eq!(s.devices["a"].minor, 0);
        assert_eq!(s.devices["b"].minor, 1);
    }

    #[test]
    fn create_duplicate_fails() {
        let mut s = new_state();
        create_test_device(&mut s, "dup", "0 512 zero");
        let (code, out) = run_dm(&mut s, &["create", "dup", "--table", "0 512 zero"]);
        assert_eq!(code, 1);
        assert!(out.contains("already exists"));
    }

    #[test]
    fn create_no_name() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["create"]);
        assert_eq!(code, 1);
        assert!(out.contains("required"));
    }

    #[test]
    fn create_no_table() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["create", "test"]);
        assert_eq!(code, 1);
        assert!(out.contains("--table"));
    }

    #[test]
    fn create_bad_table() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["create", "test", "--table", "garbage"]);
        assert_eq!(code, 1);
        assert!(out.contains("bad"));
    }

    // -----------------------------------------------------------------------
    // dmsetup: remove
    // -----------------------------------------------------------------------

    #[test]
    fn remove_existing() {
        let mut s = new_state();
        create_test_device(&mut s, "gone", "0 512 zero");
        let (code, _) = run_dm(&mut s, &["remove", "gone"]);
        assert_eq!(code, 0);
        assert!(!s.devices.contains_key("gone"));
    }

    #[test]
    fn remove_nonexistent() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["remove", "nope"]);
        assert_eq!(code, 1);
        assert!(out.contains("not found"));
    }

    #[test]
    fn remove_no_name() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["remove"]);
        assert_eq!(code, 1);
        assert!(out.contains("required"));
    }

    #[test]
    fn remove_all_clears() {
        let mut s = new_state();
        create_test_device(&mut s, "a", "0 512 zero");
        create_test_device(&mut s, "b", "0 512 zero");
        let (code, _) = run_dm(&mut s, &["remove_all"]);
        assert_eq!(code, 0);
        assert!(s.devices.is_empty());
    }

    #[test]
    fn remove_all_empty_ok() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["remove_all"]);
        assert_eq!(code, 0);
    }

    // -----------------------------------------------------------------------
    // dmsetup: suspend / resume
    // -----------------------------------------------------------------------

    #[test]
    fn suspend_and_resume() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        assert_eq!(s.devices["dev"].state, DeviceState::Active);

        let (code, _) = run_dm(&mut s, &["suspend", "dev"]);
        assert_eq!(code, 0);
        assert_eq!(s.devices["dev"].state, DeviceState::Suspended);

        let (code, _) = run_dm(&mut s, &["resume", "dev"]);
        assert_eq!(code, 0);
        assert_eq!(s.devices["dev"].state, DeviceState::Active);
    }

    #[test]
    fn suspend_nonexistent() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["suspend", "nope"]);
        assert_eq!(code, 1);
        assert!(out.contains("not found"));
    }

    #[test]
    fn resume_nonexistent() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["resume", "nope"]);
        assert_eq!(code, 1);
        assert!(out.contains("not found"));
    }

    #[test]
    fn suspend_no_name() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["suspend"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn resume_no_name() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["resume"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: load (inactive table)
    // -----------------------------------------------------------------------

    #[test]
    fn load_sets_inactive_table() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        let (code, _) = run_dm(&mut s, &["load", "dev", "--table", "0 1024 error"]);
        assert_eq!(code, 0);
        assert!(s.devices["dev"].inactive_table.is_some());
        // Active table unchanged.
        assert_eq!(s.devices["dev"].active_table[0].target_type, "zero");
    }

    #[test]
    fn load_nonexistent() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["load", "nope", "--table", "0 512 zero"]);
        assert_eq!(code, 1);
        assert!(out.contains("not found"));
    }

    #[test]
    fn resume_promotes_inactive() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        run_dm(&mut s, &["suspend", "dev"]);
        run_dm(&mut s, &["load", "dev", "--table", "0 1024 error"]);
        run_dm(&mut s, &["resume", "dev"]);
        assert_eq!(s.devices["dev"].active_table[0].target_type, "error");
        assert_eq!(s.devices["dev"].active_table[0].length, 1024);
        assert!(s.devices["dev"].inactive_table.is_none());
    }

    // -----------------------------------------------------------------------
    // dmsetup: info
    // -----------------------------------------------------------------------

    #[test]
    fn info_specific_device() {
        let mut s = new_state();
        create_test_device(&mut s, "mydev", "0 512 zero");
        let (code, out) = run_dm(&mut s, &["info", "mydev"]);
        assert_eq!(code, 0);
        assert!(out.contains("mydev"));
        assert!(out.contains("ACTIVE"));
        assert!(out.contains("253"));
        assert!(out.contains("OUROS-mydev"));
    }

    #[test]
    fn info_all_devices() {
        let mut s = new_state();
        create_test_device(&mut s, "a", "0 512 zero");
        create_test_device(&mut s, "b", "0 512 error");
        let (code, out) = run_dm(&mut s, &["info"]);
        assert_eq!(code, 0);
        assert!(out.contains("a"));
        assert!(out.contains("b"));
    }

    #[test]
    fn info_no_devices() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["info"]);
        assert_eq!(code, 0);
        assert!(out.contains("No devices"));
    }

    #[test]
    fn info_nonexistent() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["info", "nope"]);
        assert_eq!(code, 1);
        assert!(out.contains("not found"));
    }

    #[test]
    fn info_shows_inactive_table_marker() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        run_dm(&mut s, &["load", "dev", "--table", "0 1024 error"]);
        let (_, out) = run_dm(&mut s, &["info", "dev"]);
        assert!(out.contains("LIVE & INACTIVE"));
    }

    // -----------------------------------------------------------------------
    // dmsetup: table
    // -----------------------------------------------------------------------

    #[test]
    fn table_specific() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 linear /dev/sda 0");
        let (code, out) = run_dm(&mut s, &["table", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("dev: 0 1024 linear /dev/sda 0"));
    }

    #[test]
    fn table_all() {
        let mut s = new_state();
        create_test_device(&mut s, "a", "0 512 zero");
        create_test_device(&mut s, "b", "0 512 error");
        let (code, out) = run_dm(&mut s, &["table"]);
        assert_eq!(code, 0);
        assert!(out.contains("a:"));
        assert!(out.contains("b:"));
    }

    #[test]
    fn table_nonexistent() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["table", "nope"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: status
    // -----------------------------------------------------------------------

    #[test]
    fn status_linear() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 linear /dev/sda 0");
        let (code, out) = run_dm(&mut s, &["status", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("dev:"));
        assert!(out.contains("linear"));
    }

    #[test]
    fn status_all() {
        let mut s = new_state();
        create_test_device(&mut s, "a", "0 512 zero");
        let (code, out) = run_dm(&mut s, &["status"]);
        assert_eq!(code, 0);
        assert!(out.contains("a:"));
    }

    #[test]
    fn status_nonexistent() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["status", "nope"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: ls
    // -----------------------------------------------------------------------

    #[test]
    fn ls_empty() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["ls"]);
        assert_eq!(code, 0);
        assert!(out.contains("No devices"));
    }

    #[test]
    fn ls_devices() {
        let mut s = new_state();
        create_test_device(&mut s, "vol1", "0 512 zero");
        create_test_device(&mut s, "vol2", "0 512 error");
        let (code, out) = run_dm(&mut s, &["ls"]);
        assert_eq!(code, 0);
        assert!(out.contains("vol1"));
        assert!(out.contains("vol2"));
    }

    #[test]
    fn ls_tree() {
        let mut s = new_state();
        create_test_device(&mut s, "vol", "0 1024 linear /dev/sda 0");
        let (code, out) = run_dm(&mut s, &["ls", "--tree"]);
        assert_eq!(code, 0);
        assert!(out.contains("vol"));
        assert!(out.contains("linear"));
    }

    // -----------------------------------------------------------------------
    // dmsetup: deps
    // -----------------------------------------------------------------------

    #[test]
    fn deps_with_refs() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 linear 253:0 0");
        let (code, out) = run_dm(&mut s, &["deps", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("253:0"));
    }

    #[test]
    fn deps_with_path() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 linear /dev/sda 0");
        let (code, out) = run_dm(&mut s, &["deps", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("/dev/sda"));
    }

    #[test]
    fn deps_no_refs() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        let (code, out) = run_dm(&mut s, &["deps", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("0 dependencies"));
    }

    #[test]
    fn deps_all() {
        let mut s = new_state();
        create_test_device(&mut s, "a", "0 512 zero");
        create_test_device(&mut s, "b", "0 512 linear /dev/sda 0");
        let (code, out) = run_dm(&mut s, &["deps"]);
        assert_eq!(code, 0);
        assert!(out.contains("a:"));
        assert!(out.contains("b:"));
    }

    #[test]
    fn deps_nonexistent() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["deps", "nope"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: targets
    // -----------------------------------------------------------------------

    #[test]
    fn targets_lists_all() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["targets"]);
        assert_eq!(code, 0);
        for &(name, _) in KNOWN_TARGETS {
            assert!(out.contains(name), "missing target: {name}");
        }
    }

    // -----------------------------------------------------------------------
    // dmsetup: version
    // -----------------------------------------------------------------------

    #[test]
    fn version_output() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["version"]);
        assert_eq!(code, 0);
        assert!(out.contains(DM_VERSION));
    }

    #[test]
    fn version_flag() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["--version"]);
        assert_eq!(code, 0);
        assert!(out.contains(DM_VERSION));
    }

    // -----------------------------------------------------------------------
    // dmsetup: wait
    // -----------------------------------------------------------------------

    #[test]
    fn wait_bumps_event_nr() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        assert_eq!(s.devices["dev"].event_nr, 0);
        let (code, out) = run_dm(&mut s, &["wait", "dev"]);
        assert_eq!(code, 0);
        assert_eq!(s.devices["dev"].event_nr, 1);
        assert!(out.contains("event_nr=1"));
    }

    #[test]
    fn wait_with_event_nr() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        let (code, _) = run_dm(&mut s, &["wait", "dev", "0"]);
        assert_eq!(code, 0);
    }

    #[test]
    fn wait_nonexistent() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["wait", "nope"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn wait_no_name() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["wait"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: message
    // -----------------------------------------------------------------------

    #[test]
    fn message_delivers() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        let (code, out) = run_dm(&mut s, &["message", "dev", "0", "hello", "world"]);
        assert_eq!(code, 0);
        assert!(out.contains("hello world"));
        assert_eq!(s.devices["dev"].event_nr, 1);
    }

    #[test]
    fn message_bad_sector() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        let (code, _) = run_dm(&mut s, &["message", "dev", "abc", "test"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn message_too_few_args() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["message", "dev"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn message_nonexistent() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["message", "nope", "0", "hi"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: setgeometry
    // -----------------------------------------------------------------------

    #[test]
    fn setgeometry_valid() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        let (code, out) = run_dm(&mut s, &["setgeometry", "dev", "100", "16", "63", "512"]);
        assert_eq!(code, 0);
        assert!(out.contains("cylinders=100"));
        assert!(out.contains("heads=16"));
    }

    #[test]
    fn setgeometry_too_few_args() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["setgeometry", "dev", "100", "16"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn setgeometry_nonexistent() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["setgeometry", "nope", "1", "2", "3", "4"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn setgeometry_nonnumeric() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 512 zero");
        let (code, _) = run_dm(&mut s, &["setgeometry", "dev", "abc", "16", "63", "512"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: splitname
    // -----------------------------------------------------------------------

    #[test]
    fn splitname_vg_lv() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["splitname", "myvg-mylv"]);
        assert_eq!(code, 0);
        assert!(out.contains("VG:   myvg"));
        assert!(out.contains("LV:   mylv"));
        assert!(out.contains("LVM:  yes"));
    }

    #[test]
    fn splitname_escaped_hyphens() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["splitname", "my--vg-my--lv"]);
        assert_eq!(code, 0);
        assert!(out.contains("VG:   my-vg"));
        assert!(out.contains("LV:   my-lv"));
    }

    #[test]
    fn splitname_no_separator() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["splitname", "justname"]);
        assert_eq!(code, 0);
        assert!(out.contains("VG:   justname"));
        assert!(out.contains("LVM:  no"));
    }

    #[test]
    fn splitname_no_args() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["splitname"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmsetup: udev commands
    // -----------------------------------------------------------------------

    #[test]
    fn udevcomplete_ok() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["udevcomplete", "42"]);
        assert_eq!(code, 0);
        assert!(out.contains("42"));
    }

    #[test]
    fn udevcookies_ok() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["udevcookies"]);
        assert_eq!(code, 0);
        assert!(out.contains("cookie"));
    }

    #[test]
    fn udevcreatecookie_ok() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["udevcreatecookie"]);
        assert_eq!(code, 0);
        assert!(out.contains("cookie:"));
    }

    // -----------------------------------------------------------------------
    // dmsetup: help and unknown command
    // -----------------------------------------------------------------------

    #[test]
    fn help_output() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["help"]);
        assert_eq!(code, 0);
        assert!(out.contains("create"));
        assert!(out.contains("remove"));
    }

    #[test]
    fn help_flag() {
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["--help"]);
        assert_eq!(code, 0);
    }

    #[test]
    fn no_args_shows_help() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &[]);
        assert_eq!(code, 0);
        assert!(out.contains("Usage:"));
    }

    #[test]
    fn unknown_command() {
        let mut s = new_state();
        let (code, out) = run_dm(&mut s, &["bogus"]);
        assert_eq!(code, 1);
        assert!(out.contains("Unknown command"));
    }

    // -----------------------------------------------------------------------
    // dmstats: create
    // -----------------------------------------------------------------------

    #[test]
    fn stats_create_basic() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        let (code, out) = run_stats(&mut s, &["create", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("Created region 0"));
        assert_eq!(s.stats_regions.len(), 1);
    }

    #[test]
    fn stats_create_with_range() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 2048 zero");
        let (code, _) = run_stats(
            &mut s,
            &["create", "dev", "--start", "512", "--length", "256"],
        );
        assert_eq!(code, 0);
        assert_eq!(s.stats_regions[0].start_sector, 512);
        assert_eq!(s.stats_regions[0].length, 256);
    }

    #[test]
    fn stats_create_multiple_regions() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 2048 zero");
        run_stats(&mut s, &["create", "dev"]);
        run_stats(
            &mut s,
            &["create", "dev", "--start", "512", "--length", "256"],
        );
        assert_eq!(s.stats_regions.len(), 2);
        assert_eq!(s.stats_regions[0].region_id, 0);
        assert_eq!(s.stats_regions[1].region_id, 1);
    }

    #[test]
    fn stats_create_nonexistent_device() {
        let mut s = new_state();
        let (code, _) = run_stats(&mut s, &["create", "nope"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn stats_create_no_device() {
        let mut s = new_state();
        let (code, _) = run_stats(&mut s, &["create"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmstats: delete
    // -----------------------------------------------------------------------

    #[test]
    fn stats_delete_specific() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        run_stats(&mut s, &["create", "dev"]);
        run_stats(&mut s, &["create", "dev"]);
        let (code, _) = run_stats(&mut s, &["delete", "dev", "0"]);
        assert_eq!(code, 0);
        assert_eq!(s.stats_regions.len(), 1);
        assert_eq!(s.stats_regions[0].region_id, 1);
    }

    #[test]
    fn stats_delete_all() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        run_stats(&mut s, &["create", "dev"]);
        run_stats(&mut s, &["create", "dev"]);
        let (code, out) = run_stats(&mut s, &["delete", "dev", "--allregions"]);
        assert_eq!(code, 0);
        assert!(out.contains("2 region"));
        assert!(s.stats_regions.is_empty());
    }

    #[test]
    fn stats_delete_nonexistent_region() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        let (code, _) = run_stats(&mut s, &["delete", "dev", "99"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn stats_delete_no_region_or_flag() {
        let mut s = new_state();
        let (code, out) = run_stats(&mut s, &["delete", "dev"]);
        assert_eq!(code, 1);
        assert!(out.contains("specify"));
    }

    // -----------------------------------------------------------------------
    // dmstats: list
    // -----------------------------------------------------------------------

    #[test]
    fn stats_list_regions() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        run_stats(&mut s, &["create", "dev"]);
        let (code, out) = run_stats(&mut s, &["list", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("RegionID"));
        assert!(out.contains("0"));
    }

    #[test]
    fn stats_list_empty() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        let (code, out) = run_stats(&mut s, &["list", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("No regions"));
    }

    #[test]
    fn stats_list_no_device() {
        let mut s = new_state();
        let (code, _) = run_stats(&mut s, &["list"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmstats: print
    // -----------------------------------------------------------------------

    #[test]
    fn stats_print_all() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        run_stats(&mut s, &["create", "dev"]);
        let (code, out) = run_stats(&mut s, &["print", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("region0:"));
    }

    #[test]
    fn stats_print_specific_region() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        run_stats(&mut s, &["create", "dev"]);
        run_stats(&mut s, &["create", "dev"]);
        let (code, out) = run_stats(&mut s, &["print", "dev", "1"]);
        assert_eq!(code, 0);
        assert!(out.contains("region1:"));
        assert!(!out.contains("region0:"));
    }

    #[test]
    fn stats_print_no_match() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        let (code, out) = run_stats(&mut s, &["print", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("No matching"));
    }

    // -----------------------------------------------------------------------
    // dmstats: report
    // -----------------------------------------------------------------------

    #[test]
    fn stats_report_formatted() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        run_stats(&mut s, &["create", "dev"]);
        let (code, out) = run_stats(&mut s, &["report", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("Region"));
        assert!(out.contains("Reads"));
        assert!(out.contains("Writes"));
    }

    #[test]
    fn stats_report_no_regions() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 1024 zero");
        let (code, out) = run_stats(&mut s, &["report", "dev"]);
        assert_eq!(code, 0);
        assert!(out.contains("No regions"));
    }

    #[test]
    fn stats_report_no_device() {
        let mut s = new_state();
        let (code, _) = run_stats(&mut s, &["report"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // dmstats: help and unknown
    // -----------------------------------------------------------------------

    #[test]
    fn stats_help() {
        let mut s = new_state();
        let (code, out) = run_stats(&mut s, &["help"]);
        assert_eq!(code, 0);
        assert!(out.contains("create"));
    }

    #[test]
    fn stats_no_args() {
        let mut s = new_state();
        let (code, _) = run_stats(&mut s, &[]);
        assert_eq!(code, 0);
    }

    #[test]
    fn stats_unknown_command() {
        let mut s = new_state();
        let (code, _) = run_stats(&mut s, &["bogus"]);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // LVM name splitting
    // -----------------------------------------------------------------------

    #[test]
    fn split_simple_vg_lv() {
        let (vg, lv) = split_dm_name("vg-lv");
        assert_eq!(vg, "vg");
        assert_eq!(lv, "lv");
    }

    #[test]
    fn split_escaped() {
        let (vg, lv) = split_dm_name("my--vg-my--lv");
        assert_eq!(vg, "my-vg");
        assert_eq!(lv, "my-lv");
    }

    #[test]
    fn split_no_hyphen() {
        let (vg, lv) = split_dm_name("nodelimiter");
        assert_eq!(vg, "nodelimiter");
        assert_eq!(lv, "");
    }

    #[test]
    fn split_double_only() {
        let (vg, lv) = split_dm_name("a--b");
        assert_eq!(vg, "a-b");
        assert_eq!(lv, "");
    }

    #[test]
    fn split_multiple_separators() {
        // First single hyphen is the split point.
        let (vg, lv) = split_dm_name("vg-lv-extra");
        assert_eq!(vg, "vg");
        assert_eq!(lv, "lv-extra");
    }

    #[test]
    fn split_empty() {
        let (vg, lv) = split_dm_name("");
        assert_eq!(vg, "");
        assert_eq!(lv, "");
    }

    // -----------------------------------------------------------------------
    // Dependency extraction
    // -----------------------------------------------------------------------

    #[test]
    fn extract_deps_major_minor() {
        let table = vec![TableEntry {
            start_sector: 0,
            length: 1024,
            target_type: "linear".to_string(),
            target_args: "253:0 0".to_string(),
        }];
        let deps = extract_deps(&table);
        assert_eq!(deps, vec!["253:0"]);
    }

    #[test]
    fn extract_deps_dev_path() {
        let table = vec![TableEntry {
            start_sector: 0,
            length: 1024,
            target_type: "linear".to_string(),
            target_args: "/dev/sda 0".to_string(),
        }];
        let deps = extract_deps(&table);
        assert_eq!(deps, vec!["/dev/sda"]);
    }

    #[test]
    fn extract_deps_empty() {
        let table = vec![TableEntry {
            start_sector: 0,
            length: 512,
            target_type: "zero".to_string(),
            target_args: String::new(),
        }];
        let deps = extract_deps(&table);
        assert!(deps.is_empty());
    }

    #[test]
    fn extract_deps_multiple() {
        let table = vec![TableEntry {
            start_sector: 0,
            length: 2048,
            target_type: "striped".to_string(),
            target_args: "2 64 /dev/sda 0 /dev/sdb 0".to_string(),
        }];
        let deps = extract_deps(&table);
        assert_eq!(deps.len(), 2);
    }

    // -----------------------------------------------------------------------
    // MBR parsing
    // -----------------------------------------------------------------------

    fn make_mbr_disk(partitions: &[(u8, u32, u32)]) -> Vec<u8> {
        let mut data = vec![0u8; 512];
        for (i, &(ptype, lba_start, sector_count)) in partitions.iter().enumerate() {
            if i >= 4 {
                break;
            }
            let offset = 446 + i * 16;
            data[offset] = 0x80; // bootable
            data[offset + 4] = ptype;
            // LBA start (little-endian).
            data[offset + 8] = (lba_start & 0xFF) as u8;
            data[offset + 9] = ((lba_start >> 8) & 0xFF) as u8;
            data[offset + 10] = ((lba_start >> 16) & 0xFF) as u8;
            data[offset + 11] = ((lba_start >> 24) & 0xFF) as u8;
            // Sector count (little-endian).
            data[offset + 12] = (sector_count & 0xFF) as u8;
            data[offset + 13] = ((sector_count >> 8) & 0xFF) as u8;
            data[offset + 14] = ((sector_count >> 16) & 0xFF) as u8;
            data[offset + 15] = ((sector_count >> 24) & 0xFF) as u8;
        }
        // MBR signature.
        data[510] = 0x55;
        data[511] = 0xAA;
        data
    }

    #[test]
    fn parse_mbr_single_linux() {
        let data = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1048576)]);
        let (primary, logical) = parse_mbr(&data).unwrap();
        assert_eq!(primary.len(), 1);
        assert!(logical.is_empty());
        assert_eq!(primary[0].lba_start, 2048);
        assert_eq!(primary[0].sector_count, 1048576);
    }

    #[test]
    fn parse_mbr_multiple_partitions() {
        let data = make_mbr_disk(&[
            (MBR_LINUX_PARTITION, 2048, 1024),
            (MBR_FAT32, 4096, 2048),
            (MBR_SWAP, 8192, 512),
        ]);
        let (primary, _) = parse_mbr(&data).unwrap();
        assert_eq!(primary.len(), 3);
    }

    #[test]
    fn parse_mbr_with_extended() {
        let data = make_mbr_disk(&[
            (MBR_LINUX_PARTITION, 2048, 1024),
            (MBR_EXTENDED_LBA, 4096, 8192),
        ]);
        let (primary, _) = parse_mbr(&data).unwrap();
        // Extended partition is not in primary list.
        assert_eq!(primary.len(), 1);
    }

    #[test]
    fn parse_mbr_empty() {
        let data = make_mbr_disk(&[]);
        let (primary, logical) = parse_mbr(&data).unwrap();
        assert!(primary.is_empty());
        assert!(logical.is_empty());
    }

    #[test]
    fn parse_mbr_too_short() {
        let data = vec![0u8; 100];
        assert!(parse_mbr(&data).is_err());
    }

    #[test]
    fn parse_mbr_bad_signature() {
        let mut data = vec![0u8; 512];
        data[510] = 0;
        data[511] = 0;
        assert!(parse_mbr(&data).is_err());
    }

    #[test]
    fn parse_mbr_ntfs_partition() {
        let data = make_mbr_disk(&[(MBR_NTFS, 2048, 4096)]);
        let (primary, _) = parse_mbr(&data).unwrap();
        assert_eq!(primary.len(), 1);
        assert_eq!(primary[0].part_type, MBR_NTFS);
    }

    #[test]
    fn parse_mbr_fat32_lba() {
        let data = make_mbr_disk(&[(MBR_FAT32_LBA, 2048, 4096)]);
        let (primary, _) = parse_mbr(&data).unwrap();
        assert_eq!(primary.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Extended chain parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_extended_chain_with_logical() {
        // Build a disk with an extended partition containing one logical partition.
        let mut data = make_mbr_disk(&[
            (MBR_LINUX_PARTITION, 2048, 1024),
            (MBR_EXTENDED_CHS, 4096, 8192),
        ]);
        // Extend data to cover the EBR at sector 4096.
        let ebr_offset = 4096 * 512;
        data.resize(ebr_offset + 512, 0);

        // EBR first entry: logical partition at offset 1 from EBR.
        let entry_off = ebr_offset + 446;
        data[entry_off] = 0x00;
        data[entry_off + 4] = MBR_LINUX_PARTITION;
        // LBA start relative to EBR = 1.
        data[entry_off + 8] = 1;
        // Sector count = 2048.
        data[entry_off + 12] = 0x00;
        data[entry_off + 13] = 0x08;
        // EBR signature.
        data[ebr_offset + 510] = 0x55;
        data[ebr_offset + 511] = 0xAA;

        let (_, logical) = parse_mbr(&data).unwrap();
        assert_eq!(logical.len(), 1);
        assert_eq!(logical[0].lba_start, 4097); // 4096 + 1
        assert_eq!(logical[0].sector_count, 2048);
    }

    // -----------------------------------------------------------------------
    // GPT parsing
    // -----------------------------------------------------------------------

    fn make_gpt_disk(partitions: &[(u64, u64)]) -> Vec<u8> {
        // Minimal GPT: protective MBR + GPT header at LBA 1 + entries at LBA 2.
        let entry_size: u32 = 128;
        let num_entries = partitions.len() as u32;
        let data_len = (2 * 512) + (num_entries as usize * entry_size as usize) + 512;
        let mut data = vec![0u8; data_len];

        // Protective MBR signature.
        data[510] = 0x55;
        data[511] = 0xAA;

        // GPT header at LBA 1 (offset 512).
        let hdr = 512;
        data[hdr..hdr + 8].copy_from_slice(GPT_MAGIC);
        // Revision 1.0.
        data[hdr + 8] = 0x00;
        data[hdr + 9] = 0x00;
        data[hdr + 10] = 0x01;
        data[hdr + 11] = 0x00;
        // Header size = 92.
        data[hdr + 12] = 92;

        // Partition entry start LBA = 2.
        data[hdr + 72] = 2;
        // Number of partition entries.
        data[hdr + 80] = (num_entries & 0xFF) as u8;
        data[hdr + 81] = ((num_entries >> 8) & 0xFF) as u8;
        // Size of partition entry.
        data[hdr + 84] = (entry_size & 0xFF) as u8;

        // Partition entries at LBA 2 (offset 1024).
        for (i, &(first_lba, last_lba)) in partitions.iter().enumerate() {
            let base = 1024 + i * entry_size as usize;
            // Non-zero type GUID (Linux filesystem GUID).
            data[base] = 0xAF;
            data[base + 4] = 0x3D;

            // First LBA.
            let first_bytes = first_lba.to_le_bytes();
            data[base + 32..base + 40].copy_from_slice(&first_bytes);

            // Last LBA.
            let last_bytes = last_lba.to_le_bytes();
            data[base + 40..base + 48].copy_from_slice(&last_bytes);

            // Name: "Part N" in UTF-16LE.
            let name = format!("Part{}", i + 1);
            for (j, ch) in name.chars().enumerate() {
                let off = base + 56 + j * 2;
                if off + 1 < data.len() {
                    data[off] = ch as u8;
                    data[off + 1] = 0;
                }
            }
        }

        data
    }

    #[test]
    fn parse_gpt_single_partition() {
        let data = make_gpt_disk(&[(2048, 1050623)]);
        let entries = parse_gpt(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].first_lba, 2048);
        assert_eq!(entries[0].last_lba, 1050623);
    }

    #[test]
    fn parse_gpt_multiple_partitions() {
        let data = make_gpt_disk(&[(2048, 1050623), (1050624, 2099199)]);
        let entries = parse_gpt(&data).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn parse_gpt_name() {
        let data = make_gpt_disk(&[(2048, 4095)]);
        let entries = parse_gpt(&data).unwrap();
        assert!(entries[0].name.starts_with("Part1"));
    }

    #[test]
    fn parse_gpt_too_short() {
        let data = vec![0u8; 100];
        assert!(parse_gpt(&data).is_err());
    }

    #[test]
    fn parse_gpt_bad_magic() {
        let mut data = vec![0u8; 2048];
        data[512..520].copy_from_slice(b"NOT GPT!");
        assert!(parse_gpt(&data).is_err());
    }

    // -----------------------------------------------------------------------
    // Partition table detection
    // -----------------------------------------------------------------------

    #[test]
    fn detect_gpt_over_mbr() {
        let data = make_gpt_disk(&[(2048, 4095)]);
        let table = parse_partition_table(&data).unwrap();
        assert!(matches!(table, PartitionTable::Gpt { .. }));
    }

    #[test]
    fn detect_mbr_fallback() {
        let data = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024)]);
        let table = parse_partition_table(&data).unwrap();
        assert!(matches!(table, PartitionTable::Mbr { .. }));
    }

    // -----------------------------------------------------------------------
    // kpartx: add/list/delete with data
    // -----------------------------------------------------------------------

    #[test]
    fn kpartx_add_mbr() {
        let mut s = new_state();
        let disk = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024), (MBR_FAT32, 4096, 2048)]);
        let mut out = Vec::new();
        let code = kpartx_add(&mut s, "/dev/sda", &disk, &mut out);
        assert_eq!(code, 0);
        assert!(s.devices.contains_key("sdap1"));
        assert!(s.devices.contains_key("sdap2"));
    }

    #[test]
    fn kpartx_add_gpt() {
        let mut s = new_state();
        let disk = make_gpt_disk(&[(2048, 1050623), (1050624, 2099199)]);
        let mut out = Vec::new();
        let code = kpartx_add(&mut s, "/dev/nvme0n1", &disk, &mut out);
        assert_eq!(code, 0);
        assert!(s.devices.contains_key("nvme0n1p1"));
        assert!(s.devices.contains_key("nvme0n1p2"));
    }

    #[test]
    fn kpartx_add_sets_linear() {
        let mut s = new_state();
        let disk = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024)]);
        let mut out = Vec::new();
        kpartx_add(&mut s, "/dev/sda", &disk, &mut out);
        let dev = &s.devices["sdap1"];
        assert_eq!(dev.active_table[0].target_type, "linear");
        assert_eq!(dev.active_table[0].length, 1024);
    }

    #[test]
    fn kpartx_add_duplicate_skips() {
        let mut s = new_state();
        let disk = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024)]);
        let mut out = Vec::new();
        kpartx_add(&mut s, "/dev/sda", &disk, &mut out);
        let mut out2 = Vec::new();
        kpartx_add(&mut s, "/dev/sda", &disk, &mut out2);
        let output = String::from_utf8_lossy(&out2);
        assert!(output.contains("already exists"));
    }

    #[test]
    fn kpartx_delete_removes() {
        let mut s = new_state();
        let disk = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024)]);
        let mut out = Vec::new();
        kpartx_add(&mut s, "/dev/sda", &disk, &mut out);
        assert!(s.devices.contains_key("sdap1"));

        let mut out2 = Vec::new();
        kpartx_delete(&mut s, "/dev/sda", &disk, &mut out2);
        assert!(!s.devices.contains_key("sdap1"));
    }

    #[test]
    fn kpartx_list_output() {
        let disk = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024), (MBR_FAT32, 4096, 2048)]);
        let mut out = Vec::new();
        let code = kpartx_list("/dev/sda", &disk, &mut out);
        assert_eq!(code, 0);
        let output = String::from_utf8_lossy(&out);
        assert!(output.contains("sdap1"));
        assert!(output.contains("sdap2"));
    }

    #[test]
    fn kpartx_list_empty_disk() {
        let disk = make_mbr_disk(&[]);
        let mut out = Vec::new();
        let code = kpartx_list("/dev/sda", &disk, &mut out);
        assert_eq!(code, 0);
        let output = String::from_utf8_lossy(&out);
        assert!(output.contains("No partitions"));
    }

    #[test]
    fn kpartx_list_gpt_sizes() {
        let disk = make_gpt_disk(&[(2048, 4095)]);
        let mut out = Vec::new();
        kpartx_list("/dev/sda", &disk, &mut out);
        let output = String::from_utf8_lossy(&out);
        // Size should be last_lba - first_lba + 1 = 2048.
        assert!(output.contains("2048"));
    }

    // -----------------------------------------------------------------------
    // kpartx: CLI dispatch
    // -----------------------------------------------------------------------

    #[test]
    fn kpartx_help_flag() {
        let mut s = new_state();
        let args: Vec<String> = vec!["-h".to_string()];
        let mut out = Vec::new();
        let code = run_kpartx(&mut s, &args, &mut out);
        assert_eq!(code, 0);
    }

    #[test]
    fn kpartx_no_args() {
        let mut s = new_state();
        let mut out = Vec::new();
        let code = run_kpartx(&mut s, &[], &mut out);
        assert_eq!(code, 0);
    }

    #[test]
    fn kpartx_unknown_flag() {
        let mut s = new_state();
        let args: Vec<String> = vec!["-x".to_string()];
        let mut out = Vec::new();
        let code = run_kpartx(&mut s, &args, &mut out);
        assert_eq!(code, 1);
    }

    #[test]
    fn kpartx_flag_no_device() {
        let mut s = new_state();
        let args: Vec<String> = vec!["-a".to_string()];
        let mut out = Vec::new();
        let code = run_kpartx(&mut s, &args, &mut out);
        assert_eq!(code, 1);
    }

    // -----------------------------------------------------------------------
    // Partition mappings
    // -----------------------------------------------------------------------

    #[test]
    fn partition_mappings_mbr() {
        let table = PartitionTable::Mbr {
            primary: vec![
                MbrPartition {
                    status: 0x80,
                    part_type: MBR_LINUX_PARTITION,
                    lba_start: 2048,
                    sector_count: 1024,
                },
                MbrPartition {
                    status: 0,
                    part_type: MBR_FAT32,
                    lba_start: 4096,
                    sector_count: 2048,
                },
            ],
            logical: vec![MbrPartition {
                status: 0,
                part_type: MBR_LINUX_PARTITION,
                lba_start: 8192,
                sector_count: 512,
            }],
        };
        let maps = partition_mappings(&table, "sda");
        assert_eq!(maps.len(), 3);
        assert_eq!(maps[0].0, "sdap1");
        assert_eq!(maps[1].0, "sdap2");
        assert_eq!(maps[2].0, "sdap5"); // Logical partitions start at 5.
    }

    #[test]
    fn partition_mappings_gpt() {
        let table = PartitionTable::Gpt {
            entries: vec![GptPartition {
                type_guid: [1; 16],
                unique_guid: [2; 16],
                first_lba: 2048,
                last_lba: 4095,
                name: "EFI".to_string(),
            }],
        };
        let maps = partition_mappings(&table, "nvme0n1");
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].0, "nvme0n1p1");
        assert_eq!(maps[0].1.length, 2048); // 4095 - 2048 + 1
    }

    #[test]
    fn partition_mappings_empty_mbr() {
        let table = PartitionTable::Mbr {
            primary: vec![],
            logical: vec![],
        };
        let maps = partition_mappings(&table, "sda");
        assert!(maps.is_empty());
    }

    // -----------------------------------------------------------------------
    // Byte readers
    // -----------------------------------------------------------------------

    #[test]
    fn read_u16_le_basic() {
        assert_eq!(read_u16_le(&[0x34, 0x12], 0), 0x1234);
    }

    #[test]
    fn read_u32_le_basic() {
        assert_eq!(read_u32_le(&[0x78, 0x56, 0x34, 0x12], 0), 0x12345678);
    }

    #[test]
    fn read_u64_le_basic() {
        let data = [0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
        assert_eq!(read_u64_le(&data, 0), 0x00000002_00000001);
    }

    #[test]
    fn read_u16_le_out_of_bounds() {
        // Graceful handling of short data.
        assert_eq!(read_u16_le(&[0x42], 0), 0x42);
    }

    #[test]
    fn read_u32_le_with_offset() {
        let data = [0x00, 0x00, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_u32_le(&data, 2), 0x12345678);
    }

    // -----------------------------------------------------------------------
    // UTF-16LE name parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_utf16le_ascii() {
        let data = [b'H', 0, b'i', 0, 0, 0];
        assert_eq!(parse_utf16le_name(&data), "Hi");
    }

    #[test]
    fn parse_utf16le_empty() {
        let data = [0, 0];
        assert_eq!(parse_utf16le_name(&data), "");
    }

    #[test]
    fn parse_utf16le_no_null_term() {
        let data = [b'A', 0, b'B', 0];
        assert_eq!(parse_utf16le_name(&data), "AB");
    }

    // -----------------------------------------------------------------------
    // Helper: is_extended_type / is_mappable_mbr_type
    // -----------------------------------------------------------------------

    #[test]
    fn extended_types() {
        assert!(is_extended_type(MBR_EXTENDED_CHS));
        assert!(is_extended_type(MBR_EXTENDED_LBA));
        assert!(is_extended_type(MBR_EXTENDED_LINUX));
        assert!(!is_extended_type(MBR_LINUX_PARTITION));
        assert!(!is_extended_type(0));
    }

    #[test]
    fn mappable_types() {
        assert!(is_mappable_mbr_type(MBR_LINUX_PARTITION));
        assert!(is_mappable_mbr_type(MBR_FAT32));
        assert!(is_mappable_mbr_type(MBR_NTFS));
        assert!(is_mappable_mbr_type(MBR_SWAP));
        assert!(!is_mappable_mbr_type(0));
        assert!(!is_mappable_mbr_type(MBR_EXTENDED_CHS));
    }

    // -----------------------------------------------------------------------
    // DmState
    // -----------------------------------------------------------------------

    #[test]
    fn alloc_minor_increments() {
        let mut s = new_state();
        assert_eq!(s.alloc_minor(), 0);
        assert_eq!(s.alloc_minor(), 1);
        assert_eq!(s.alloc_minor(), 2);
    }

    // -----------------------------------------------------------------------
    // Tree printing
    // -----------------------------------------------------------------------

    #[test]
    fn print_tree_output() {
        let mut devs = BTreeMap::new();
        devs.insert(
            "vol".to_string(),
            MappedDevice::new(
                "vol",
                0,
                vec![TableEntry {
                    start_sector: 0,
                    length: 1024,
                    target_type: "linear".to_string(),
                    target_args: "/dev/sda 0".to_string(),
                }],
            ),
        );
        let mut out = Vec::new();
        print_tree(&devs, &mut out);
        let output = String::from_utf8_lossy(&out);
        assert!(output.contains("vol"));
        assert!(output.contains("linear"));
    }

    // -----------------------------------------------------------------------
    // Integration: full workflow
    // -----------------------------------------------------------------------

    #[test]
    fn full_lifecycle() {
        let mut s = new_state();

        // Create.
        assert_eq!(
            create_test_device(&mut s, "myvol", "0 2048 linear /dev/sda 0"),
            0
        );

        // Info.
        let (_, out) = run_dm(&mut s, &["info", "myvol"]);
        assert!(out.contains("ACTIVE"));

        // Suspend.
        let (code, _) = run_dm(&mut s, &["suspend", "myvol"]);
        assert_eq!(code, 0);
        let (_, out) = run_dm(&mut s, &["info", "myvol"]);
        assert!(out.contains("SUSPENDED"));

        // Load new table.
        let (code, _) = run_dm(
            &mut s,
            &["load", "myvol", "--table", "0 4096 linear /dev/sdb 0"],
        );
        assert_eq!(code, 0);

        // Resume (promotes inactive table).
        let (code, _) = run_dm(&mut s, &["resume", "myvol"]);
        assert_eq!(code, 0);
        let (_, out) = run_dm(&mut s, &["table", "myvol"]);
        assert!(out.contains("4096"));
        assert!(out.contains("/dev/sdb"));

        // Wait.
        let (code, _) = run_dm(&mut s, &["wait", "myvol"]);
        assert_eq!(code, 0);

        // Message.
        let (code, _) = run_dm(&mut s, &["message", "myvol", "0", "hello"]);
        assert_eq!(code, 0);

        // Remove.
        let (code, _) = run_dm(&mut s, &["remove", "myvol"]);
        assert_eq!(code, 0);
        assert!(s.devices.is_empty());
    }

    #[test]
    fn stats_lifecycle() {
        let mut s = new_state();
        create_test_device(&mut s, "dev", "0 4096 zero");

        // Create two regions.
        run_stats(&mut s, &["create", "dev"]);
        run_stats(
            &mut s,
            &["create", "dev", "--start", "1024", "--length", "512"],
        );
        assert_eq!(s.stats_regions.len(), 2);

        // List.
        let (_, out) = run_stats(&mut s, &["list", "dev"]);
        assert!(out.contains("0"));
        assert!(out.contains("1"));

        // Report.
        let (_, out) = run_stats(&mut s, &["report", "dev"]);
        assert!(out.contains("Region"));

        // Delete one.
        run_stats(&mut s, &["delete", "dev", "0"]);
        assert_eq!(s.stats_regions.len(), 1);

        // Delete all.
        run_stats(&mut s, &["delete", "dev", "--allregions"]);
        assert!(s.stats_regions.is_empty());
    }

    #[test]
    fn kpartx_full_lifecycle() {
        let mut s = new_state();
        let disk = make_mbr_disk(&[(MBR_LINUX_PARTITION, 2048, 1024), (MBR_FAT32, 4096, 2048)]);

        // List.
        let mut out = Vec::new();
        kpartx_list("/dev/sda", &disk, &mut out);
        let output = String::from_utf8_lossy(&out);
        assert!(output.contains("sdap1"));

        // Add.
        let mut out = Vec::new();
        kpartx_add(&mut s, "/dev/sda", &disk, &mut out);
        assert_eq!(s.devices.len(), 2);

        // Delete.
        let mut out = Vec::new();
        kpartx_delete(&mut s, "/dev/sda", &disk, &mut out);
        assert!(s.devices.is_empty());
    }

    // -----------------------------------------------------------------------
    // Status output for various target types
    // -----------------------------------------------------------------------

    #[test]
    fn status_mirror_target() {
        let mut s = new_state();
        create_test_device(
            &mut s,
            "m",
            "0 4096 mirror core 1 1024 2 /dev/sda 0 /dev/sdb 0",
        );
        let (_, out) = run_dm(&mut s, &["status", "m"]);
        assert!(out.contains("AA"));
    }

    #[test]
    fn status_thin_pool_target() {
        let mut s = new_state();
        create_test_device(&mut s, "tp", "0 2097152 thin-pool 253:1 253:2 128 0");
        let (_, out) = run_dm(&mut s, &["status", "tp"]);
        assert!(out.contains("writeback") || out.contains("rw"));
    }

    #[test]
    fn status_snapshot_target() {
        let mut s = new_state();
        create_test_device(&mut s, "snap", "0 4096 snapshot /dev/origin /dev/cow P 16");
        let (_, out) = run_dm(&mut s, &["status", "snap"]);
        assert!(out.contains("snapshot"));
    }

    #[test]
    fn status_error_target() {
        let mut s = new_state();
        create_test_device(&mut s, "err", "0 512 error");
        let (_, out) = run_dm(&mut s, &["status", "err"]);
        assert!(out.contains("error"));
    }

    #[test]
    fn status_delay_target() {
        let mut s = new_state();
        create_test_device(&mut s, "d", "0 1024 delay /dev/sda 0 100");
        let (_, out) = run_dm(&mut s, &["status", "d"]);
        assert!(out.contains("delay"));
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn create_large_table() {
        let mut lines = Vec::new();
        for i in 0..10 {
            lines.push(format!("{} 1024 linear /dev/sda {}", i * 1024, i * 1024));
        }
        let table_str = lines.join("\n");
        let mut s = new_state();
        let (code, _) = run_dm(&mut s, &["create", "big", "--table", &table_str]);
        assert_eq!(code, 0);
        assert_eq!(s.devices["big"].active_table.len(), 10);
    }

    #[test]
    fn multiple_devices_ordered() {
        let mut s = new_state();
        create_test_device(&mut s, "zzz", "0 512 zero");
        create_test_device(&mut s, "aaa", "0 512 zero");
        create_test_device(&mut s, "mmm", "0 512 zero");
        // BTreeMap keeps sorted order.
        let names: Vec<&String> = s.devices.keys().collect();
        assert_eq!(names, vec!["aaa", "mmm", "zzz"]);
    }

    #[test]
    fn parse_name_and_table_helper() {
        let args: Vec<String> = vec![
            "mydev".to_string(),
            "--table".to_string(),
            "0 1024 zero".to_string(),
        ];
        let (name, table) = parse_name_and_table(&args).unwrap();
        assert_eq!(name, "mydev");
        assert_eq!(table, "0 1024 zero");
    }

    #[test]
    fn parse_name_and_table_no_name() {
        let args: Vec<String> = vec![];
        assert!(parse_name_and_table(&args).is_err());
    }

    #[test]
    fn parse_name_and_table_no_table_flag() {
        let args: Vec<String> = vec!["mydev".to_string()];
        assert!(parse_name_and_table(&args).is_err());
    }

    #[test]
    fn parse_name_and_table_empty_table_arg() {
        let args: Vec<String> = vec!["mydev".to_string(), "--table".to_string()];
        assert!(parse_name_and_table(&args).is_err());
    }

    #[test]
    fn gpt_partition_size_calculation() {
        // Verify first_lba=100, last_lba=199 -> size=100.
        let table = PartitionTable::Gpt {
            entries: vec![GptPartition {
                type_guid: [1; 16],
                unique_guid: [2; 16],
                first_lba: 100,
                last_lba: 199,
                name: "test".to_string(),
            }],
        };
        let maps = partition_mappings(&table, "sda");
        assert_eq!(maps[0].1.length, 100);
    }
}
