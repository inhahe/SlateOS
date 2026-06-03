//! Multi-personality software RAID management utility for OurOS.
//!
//! Detects personality from `argv[0]`:
//!   - `mdadm`  — create, assemble, manage, monitor, and query software RAID arrays
//!   - `mdmon`  — RAID array monitoring daemon
//!
//! Supports RAID levels 0 (stripe), 1 (mirror), 5 (distributed parity),
//! 6 (dual parity), and 10 (striped mirror). Uses v1.2 superblock format.

#![deny(clippy::all)]
// The full v1.2 MD-superblock data model — ArrayState::{Stopped,Inactive},
// RaidLevel::{has_mirror, parity_disks}, MdSuperblock::{validate, from_str},
// the bitmap helpers (set_dirty/clean, dirty_count, set_all_*), the bitmap
// `bits` / `chunk_count` fields, the per-device dev_number/size/data_offset/
// events/uuid/errors fields, and the array-table stop_array/remove_stopped
// methods — is declared up-front because it encodes the on-disk v1.2 layout
// (Linux md/raid.c) the real implementation must speak. They are kept as
// documentation for the eventual block-device integration.
#![allow(dead_code)]

use std::env;
use std::io::Write;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// v1.2 superblock magic number.
const MD_SB_MAGIC: u32 = 0xa92b4efc;

/// Default chunk size in KiB.
const DEFAULT_CHUNK_KIB: u32 = 512;

/// Default metadata version string.
const DEFAULT_METADATA: &str = "1.2";

// ============================================================================
// Personality detection
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Mdadm,
    Mdmon,
}

fn detect_personality(argv0: &str) -> Personality {
    let name = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();

    if lower.contains("mdmon") {
        Personality::Mdmon
    } else {
        Personality::Mdadm
    }
}

// ============================================================================
// RAID level definitions
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RaidLevel {
    Raid0,
    Raid1,
    Raid5,
    Raid6,
    Raid10,
}

impl RaidLevel {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "0" | "raid0" | "stripe" => Some(Self::Raid0),
            "1" | "raid1" | "mirror" => Some(Self::Raid1),
            "5" | "raid5" => Some(Self::Raid5),
            "6" | "raid6" => Some(Self::Raid6),
            "10" | "raid10" => Some(Self::Raid10),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Raid0 => "raid0",
            Self::Raid1 => "raid1",
            Self::Raid5 => "raid5",
            Self::Raid6 => "raid6",
            Self::Raid10 => "raid10",
        }
    }

    fn numeric(self) -> u32 {
        match self {
            Self::Raid0 => 0,
            Self::Raid1 => 1,
            Self::Raid5 => 5,
            Self::Raid6 => 6,
            Self::Raid10 => 10,
        }
    }

    /// Minimum number of devices required for this level.
    fn min_devices(self) -> u32 {
        match self {
            Self::Raid0 => 2,
            Self::Raid1 => 2,
            Self::Raid5 => 3,
            Self::Raid6 => 4,
            Self::Raid10 => 4,
        }
    }

    /// Whether this level supports parity.
    fn has_parity(self) -> bool {
        matches!(self, Self::Raid5 | Self::Raid6)
    }

    /// Whether this level supports mirroring.
    fn has_mirror(self) -> bool {
        matches!(self, Self::Raid1 | Self::Raid10)
    }

    /// Number of parity disks for this level.
    fn parity_disks(self) -> u32 {
        match self {
            Self::Raid5 => 1,
            Self::Raid6 => 2,
            _ => 0,
        }
    }

    /// Compute usable capacity as fraction of total.
    fn usable_fraction(self, n_devices: u32) -> f64 {
        if n_devices == 0 {
            return 0.0;
        }
        let n = n_devices as f64;
        match self {
            Self::Raid0 => 1.0,
            Self::Raid1 => 1.0 / n,
            Self::Raid5 => (n - 1.0) / n,
            Self::Raid6 => (n - 2.0) / n,
            Self::Raid10 => 0.5,
        }
    }
}

// ============================================================================
// Array and device states
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArrayState {
    Active,
    Degraded,
    Rebuilding,
    Stopped,
    Inactive,
}

impl ArrayState {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Degraded => "degraded",
            Self::Rebuilding => "rebuilding",
            Self::Stopped => "stopped",
            Self::Inactive => "inactive",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "degraded" => Some(Self::Degraded),
            "rebuilding" => Some(Self::Rebuilding),
            "stopped" => Some(Self::Stopped),
            "inactive" => Some(Self::Inactive),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeviceRole {
    Active,
    Spare,
    Failed,
    Removed,
}

impl DeviceRole {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active sync",
            Self::Spare => "spare",
            Self::Failed => "faulty",
            Self::Removed => "removed",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" | "active sync" => Some(Self::Active),
            "spare" => Some(Self::Spare),
            "faulty" | "failed" => Some(Self::Failed),
            "removed" => Some(Self::Removed),
            _ => None,
        }
    }
}

// ============================================================================
// UUID handling
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct Uuid {
    bytes: [u8; 16],
}

impl Uuid {
    fn zero() -> Self {
        Self { bytes: [0u8; 16] }
    }

    /// Generate a deterministic UUID from a seed string (for simulation).
    fn from_seed(seed: &str) -> Self {
        let mut bytes = [0u8; 16];
        let seed_bytes = seed.as_bytes();
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = seed_bytes.get(i % seed_bytes.len()).copied().unwrap_or(0);
            // Simple mixing to spread bits.
            *b = b.wrapping_mul(31).wrapping_add(i as u8);
        }
        // Set version 4 and variant bits per RFC 4122.
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self { bytes }
    }

    fn format(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}:{:02x}{:02x}{:02x}{:02x}:\
             {:02x}{:02x}{:02x}{:02x}:{:02x}{:02x}{:02x}{:02x}",
            self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3],
            self.bytes[4], self.bytes[5], self.bytes[6], self.bytes[7],
            self.bytes[8], self.bytes[9], self.bytes[10], self.bytes[11],
            self.bytes[12], self.bytes[13], self.bytes[14], self.bytes[15],
        )
    }

    fn parse(s: &str) -> Option<Self> {
        let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self { bytes })
    }
}

// ============================================================================
// Bitmap tracking for partial rebuilds
// ============================================================================

#[derive(Clone, Debug)]
struct Bitmap {
    /// Bits per chunk -- one bit per chunk indicates whether it needs rebuild.
    bits: Vec<u8>,
    /// Total number of chunks tracked.
    chunk_count: u64,
}

impl Bitmap {
    fn new(chunk_count: u64) -> Self {
        let byte_count = chunk_count.div_ceil(8) as usize;
        Self {
            bits: vec![0u8; byte_count],
            chunk_count,
        }
    }

    fn set_dirty(&mut self, chunk: u64) {
        if chunk >= self.chunk_count {
            return;
        }
        let byte_idx = (chunk / 8) as usize;
        let bit_idx = (chunk % 8) as u8;
        if let Some(b) = self.bits.get_mut(byte_idx) {
            *b |= 1 << bit_idx;
        }
    }

    fn set_clean(&mut self, chunk: u64) {
        if chunk >= self.chunk_count {
            return;
        }
        let byte_idx = (chunk / 8) as usize;
        let bit_idx = (chunk % 8) as u8;
        if let Some(b) = self.bits.get_mut(byte_idx) {
            *b &= !(1 << bit_idx);
        }
    }

    fn is_dirty(&self, chunk: u64) -> bool {
        if chunk >= self.chunk_count {
            return false;
        }
        let byte_idx = (chunk / 8) as usize;
        let bit_idx = (chunk % 8) as u8;
        self.bits.get(byte_idx).is_some_and(|b| (b >> bit_idx) & 1 == 1)
    }

    fn dirty_count(&self) -> u64 {
        let mut count: u64 = 0;
        for chunk in 0..self.chunk_count {
            if self.is_dirty(chunk) {
                count += 1;
            }
        }
        count
    }

    fn set_all_dirty(&mut self) {
        for b in &mut self.bits {
            *b = 0xff;
        }
    }

    fn set_all_clean(&mut self) {
        for b in &mut self.bits {
            *b = 0;
        }
    }
}

// ============================================================================
// Superblock structure (v1.2 format)
// ============================================================================

#[derive(Clone, Debug)]
struct Superblock {
    magic: u32,
    major_version: u32,
    minor_version: u32,
    feature_map: u32,
    pad0: u32,
    set_uuid: Uuid,
    set_name: String,
    ctime: u64,
    level: RaidLevel,
    layout: u32,
    size: u64,
    chunksize: u32,
    raid_disks: u32,
    bitmap_offset: i32,
    new_level: Option<RaidLevel>,
    reshape_position: u64,
    delta_disks: i32,
    new_layout: u32,
    new_chunk: u32,
    data_offset: u64,
    data_size: u64,
    super_offset: u64,
    recovery_offset: u64,
    dev_number: u32,
    cnt_corrected_read: u32,
    device_uuid: Uuid,
    devflags: u32,
    utime: u64,
    events: u64,
    resync_offset: u64,
    sb_csum: u32,
    max_dev: u32,
}

impl Superblock {
    fn new(
        level: RaidLevel,
        raid_disks: u32,
        name: &str,
        size: u64,
        chunk: u32,
    ) -> Self {
        let uuid = Uuid::from_seed(name);
        Self {
            magic: MD_SB_MAGIC,
            major_version: 1,
            minor_version: 2,
            feature_map: 0,
            pad0: 0,
            set_uuid: uuid,
            set_name: name.to_string(),
            ctime: 1700000000, // Placeholder epoch timestamp.
            level,
            layout: default_layout(level),
            size,
            chunksize: chunk,
            raid_disks,
            bitmap_offset: 0,
            new_level: None,
            reshape_position: 0,
            delta_disks: 0,
            new_layout: 0,
            new_chunk: 0,
            data_offset: 2048,
            data_size: size,
            super_offset: 8,
            recovery_offset: 0,
            dev_number: 0,
            cnt_corrected_read: 0,
            device_uuid: Uuid::from_seed(&format!("{}-dev0", name)),
            devflags: 0,
            utime: 1700000000,
            events: 1,
            resync_offset: 0,
            sb_csum: 0,
            max_dev: raid_disks,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.magic != MD_SB_MAGIC {
            return Err(format!(
                "bad magic: expected 0x{:08x}, got 0x{:08x}",
                MD_SB_MAGIC, self.magic
            ));
        }
        if self.major_version != 1 {
            return Err(format!(
                "unsupported major version: {}",
                self.major_version
            ));
        }
        if self.raid_disks < self.level.min_devices() {
            return Err(format!(
                "insufficient raid devices: {} for {} (minimum {})",
                self.raid_disks,
                self.level.name(),
                self.level.min_devices()
            ));
        }
        Ok(())
    }

    fn compute_checksum(&self) -> u32 {
        // Simple additive checksum over key fields for simulation.
        let mut sum: u32 = 0;
        sum = sum.wrapping_add(self.magic);
        sum = sum.wrapping_add(self.major_version);
        sum = sum.wrapping_add(self.minor_version);
        sum = sum.wrapping_add(self.level.numeric());
        sum = sum.wrapping_add(self.raid_disks);
        sum = sum.wrapping_add(self.chunksize);
        sum = sum.wrapping_add(self.size as u32);
        for &b in &self.set_uuid.bytes {
            sum = sum.wrapping_add(u32::from(b));
        }
        sum
    }
}

fn default_layout(level: RaidLevel) -> u32 {
    match level {
        // left-symmetric for RAID5.
        RaidLevel::Raid5 => 2,
        // left-symmetric, dual parity for RAID6.
        RaidLevel::Raid6 => 2,
        // near=2 for RAID10.
        RaidLevel::Raid10 => 0x0102,
        _ => 0,
    }
}

// ============================================================================
// Device record within an array
// ============================================================================

#[derive(Clone, Debug)]
struct DeviceRecord {
    path: String,
    role: DeviceRole,
    dev_number: u32,
    major: u32,
    minor: u32,
    /// Size in KiB.
    size: u64,
    /// Offset into the device where data starts (sectors).
    data_offset: u64,
    /// Events count at last sync.
    events: u64,
    /// Per-device UUID.
    uuid: Uuid,
    /// Errors detected on this device.
    errors: u64,
}

impl DeviceRecord {
    fn new(path: &str, dev_number: u32, size: u64) -> Self {
        Self {
            path: path.to_string(),
            role: DeviceRole::Active,
            dev_number,
            major: 8,
            minor: dev_number.wrapping_add(1),
            size,
            data_offset: 2048,
            events: 1,
            uuid: Uuid::from_seed(&format!("{}-{}", path, dev_number)),
            errors: 0,
        }
    }
}

// ============================================================================
// Array descriptor (in-memory representation)
// ============================================================================

#[derive(Clone, Debug)]
struct ArrayDescriptor {
    md_device: String,
    superblock: Superblock,
    state: ArrayState,
    devices: Vec<DeviceRecord>,
    spare_devices: Vec<DeviceRecord>,
    bitmap: Option<Bitmap>,
    /// Rebuild progress as percentage (0..100).
    rebuild_progress: Option<u32>,
}

/// Parameters describing a RAID array to create, mirroring the `mdadm --create`
/// command-line inputs. Bundled into one struct so the constructor and the
/// manager's `create_array` stay under the argument-count limit.
struct ArraySpec<'a> {
    md_device: &'a str,
    level: RaidLevel,
    raid_disks: u32,
    device_paths: &'a [&'a str],
    spare_paths: &'a [&'a str],
    chunk: u32,
    name: Option<&'a str>,
    bitmap_enabled: bool,
}

impl ArrayDescriptor {
    fn new(spec: &ArraySpec<'_>) -> Result<Self, String> {
        let &ArraySpec {
            md_device,
            level,
            raid_disks,
            device_paths,
            spare_paths,
            chunk,
            name,
            bitmap_enabled,
        } = spec;
        if (device_paths.len() as u32) < raid_disks {
            return Err(format!(
                "not enough devices: got {}, need {}",
                device_paths.len(),
                raid_disks
            ));
        }
        if raid_disks < level.min_devices() {
            return Err(format!(
                "{} requires at least {} devices, got {}",
                level.name(),
                level.min_devices(),
                raid_disks
            ));
        }

        let array_name = name.unwrap_or(md_device);
        // Simulate 1 TiB per component device.
        let device_size: u64 = 1_048_576; // KiB (= 1 GiB for simulation).
        let sb = Superblock::new(level, raid_disks, array_name, device_size, chunk);

        let mut devices = Vec::new();
        for (i, path) in device_paths.iter().enumerate() {
            let rec = DeviceRecord::new(path, i as u32, device_size);
            devices.push(rec);
        }

        let mut spares = Vec::new();
        for (i, path) in spare_paths.iter().enumerate() {
            let mut rec = DeviceRecord::new(
                path,
                (device_paths.len() + i) as u32,
                device_size,
            );
            rec.role = DeviceRole::Spare;
            spares.push(rec);
        }

        let bitmap = if bitmap_enabled {
            let chunk_count = device_size / u64::from(chunk);
            Some(Bitmap::new(chunk_count))
        } else {
            None
        };

        Ok(Self {
            md_device: md_device.to_string(),
            superblock: sb,
            state: ArrayState::Active,
            devices,
            spare_devices: spares,
            bitmap,
            rebuild_progress: None,
        })
    }

    fn active_device_count(&self) -> u32 {
        self.devices
            .iter()
            .filter(|d| d.role == DeviceRole::Active)
            .count() as u32
    }

    fn total_device_count(&self) -> u32 {
        (self.devices.len() + self.spare_devices.len()) as u32
    }

    fn working_devices(&self) -> u32 {
        let active = self.devices.iter().filter(|d| d.role == DeviceRole::Active).count();
        let spare = self.spare_devices.iter().filter(|d| d.role == DeviceRole::Spare).count();
        (active + spare) as u32
    }

    fn failed_devices(&self) -> u32 {
        self.devices
            .iter()
            .chain(self.spare_devices.iter())
            .filter(|d| d.role == DeviceRole::Failed)
            .count() as u32
    }

    fn usable_size_kib(&self) -> u64 {
        let dev_size = self.superblock.size;
        let n = self.superblock.raid_disks;
        let fraction = self.superblock.level.usable_fraction(n);
        (dev_size as f64 * n as f64 * fraction) as u64
    }

    fn is_degraded(&self) -> bool {
        self.active_device_count() < self.superblock.raid_disks
    }

    fn mark_device_failed(&mut self, path: &str) -> Result<(), String> {
        for dev in &mut self.devices {
            if dev.path == path {
                if dev.role == DeviceRole::Failed {
                    return Err(format!("{} is already marked as failed", path));
                }
                dev.role = DeviceRole::Failed;
                if self.is_degraded() {
                    self.state = ArrayState::Degraded;
                }
                return Ok(());
            }
        }
        Err(format!("device {} not found in array", path))
    }

    fn remove_device(&mut self, path: &str) -> Result<(), String> {
        // Can only remove failed or spare devices.
        if let Some(pos) = self.devices.iter().position(|d| d.path == path) {
            if self.devices[pos].role != DeviceRole::Failed {
                return Err(format!(
                    "cannot remove active device {} -- mark as failed first",
                    path
                ));
            }
            self.devices[pos].role = DeviceRole::Removed;
            return Ok(());
        }
        if let Some(pos) = self.spare_devices.iter().position(|d| d.path == path) {
            self.spare_devices.remove(pos);
            return Ok(());
        }
        Err(format!("device {} not found in array", path))
    }

    fn add_device(&mut self, path: &str) -> Result<(), String> {
        // Check if already present.
        for dev in self.devices.iter().chain(self.spare_devices.iter()) {
            if dev.path == path {
                return Err(format!("device {} already in array", path));
            }
        }
        let dev_num = self.total_device_count();
        let mut rec = DeviceRecord::new(path, dev_num, self.superblock.size);

        // If degraded, add as active and start rebuild; otherwise add as spare.
        if self.is_degraded() {
            rec.role = DeviceRole::Active;
            self.devices.push(rec);
            self.state = ArrayState::Rebuilding;
            self.rebuild_progress = Some(0);
        } else {
            rec.role = DeviceRole::Spare;
            self.spare_devices.push(rec);
        }
        Ok(())
    }

    fn grow(&mut self, new_raid_disks: u32) -> Result<(), String> {
        if new_raid_disks <= self.superblock.raid_disks {
            return Err(format!(
                "new raid-devices count ({}) must be greater than current ({})",
                new_raid_disks, self.superblock.raid_disks
            ));
        }
        // Validate level supports growing.
        match self.superblock.level {
            RaidLevel::Raid0 | RaidLevel::Raid5 | RaidLevel::Raid6 => {}
            _ => {
                return Err(format!(
                    "grow not supported for {}",
                    self.superblock.level.name()
                ));
            }
        }
        self.superblock.delta_disks =
            (new_raid_disks as i32) - (self.superblock.raid_disks as i32);
        self.superblock.new_level = Some(self.superblock.level);
        self.superblock.raid_disks = new_raid_disks;
        self.state = ArrayState::Rebuilding;
        self.rebuild_progress = Some(0);
        Ok(())
    }
}

// ============================================================================
// RAID manager -- simulated state for the mdadm tool
// ============================================================================

#[derive(Clone, Debug)]
struct RaidManager {
    arrays: Vec<ArrayDescriptor>,
}

impl RaidManager {
    fn new() -> Self {
        Self {
            arrays: Vec::new(),
        }
    }

    fn find_array(&self, md_device: &str) -> Option<&ArrayDescriptor> {
        self.arrays.iter().find(|a| a.md_device == md_device)
    }

    fn find_array_mut(&mut self, md_device: &str) -> Option<&mut ArrayDescriptor> {
        self.arrays.iter_mut().find(|a| a.md_device == md_device)
    }

    fn create_array(&mut self, spec: &ArraySpec<'_>) -> Result<&ArrayDescriptor, String> {
        if self.find_array(spec.md_device).is_some() {
            return Err(format!("array {} already exists", spec.md_device));
        }
        let arr = ArrayDescriptor::new(spec)?;
        self.arrays.push(arr);
        // Return reference to newly pushed element.
        Ok(self.arrays.last().expect("just pushed"))
    }

    fn stop_array(&mut self, md_device: &str) -> Result<(), String> {
        if let Some(arr) = self.find_array_mut(md_device) {
            arr.state = ArrayState::Stopped;
            Ok(())
        } else {
            Err(format!("array {} not found", md_device))
        }
    }

    fn assemble_array(
        &mut self,
        md_device: &str,
        device_paths: &[&str],
    ) -> Result<(), String> {
        if self.find_array(md_device).is_some() {
            return Err(format!("array {} already assembled", md_device));
        }
        if device_paths.is_empty() {
            return Err("no component devices specified".to_string());
        }
        // In a real implementation we would read superblocks from the devices.
        // For simulation, create a default RAID1 array with the given devices.
        let n = device_paths.len() as u32;
        let level = if n >= 3 { RaidLevel::Raid5 } else { RaidLevel::Raid1 };
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device,
            level,
            raid_disks: n,
            device_paths,
            spare_paths: &[],
            chunk: DEFAULT_CHUNK_KIB,
            name: None,
            bitmap_enabled: false,
        })?;
        self.arrays.push(arr);
        Ok(())
    }

    fn remove_stopped(&mut self, md_device: &str) -> Result<(), String> {
        if let Some(pos) = self.arrays.iter().position(|a| a.md_device == md_device) {
            if self.arrays[pos].state != ArrayState::Stopped {
                return Err(format!("array {} is not stopped", md_device));
            }
            self.arrays.remove(pos);
            Ok(())
        } else {
            Err(format!("array {} not found", md_device))
        }
    }
}

// ============================================================================
// Formatting helpers for detail/examine output
// ============================================================================

fn format_size_human(kib: u64) -> String {
    if kib >= 1_048_576 {
        format!("{:.2} GiB", kib as f64 / 1_048_576.0)
    } else if kib >= 1024 {
        format!("{:.2} MiB", kib as f64 / 1024.0)
    } else {
        format!("{} KiB", kib)
    }
}

fn format_detail(arr: &ArrayDescriptor, out: &mut Vec<u8>) {
    let sb = &arr.superblock;
    let _ = writeln!(out, "/dev/{}:", arr.md_device);
    let _ = writeln!(out, "        Version : {}.{}", sb.major_version, sb.minor_version);
    let _ = writeln!(out, "  Creation Time : {}", sb.ctime);
    let _ = writeln!(out, "     Raid Level : {}", sb.level.name());
    let _ = writeln!(
        out,
        "     Array Size : {} ({} usable)",
        format_size_human(sb.size * u64::from(sb.raid_disks)),
        format_size_human(arr.usable_size_kib())
    );
    let _ = writeln!(out, "  Used Dev Size : {}", format_size_human(sb.size));
    let _ = writeln!(out, "   Raid Devices : {}", sb.raid_disks);
    let _ = writeln!(out, "  Total Devices : {}", arr.total_device_count());
    let _ = writeln!(out, "    Persistence : Superblock is persistent");
    let _ = writeln!(out);
    let _ = writeln!(out, "          State : {}", arr.state.label());
    let _ = writeln!(out, " Active Devices : {}", arr.active_device_count());
    let _ = writeln!(out, "Working Devices : {}", arr.working_devices());
    let _ = writeln!(out, " Failed Devices : {}", arr.failed_devices());
    let _ = writeln!(
        out,
        "  Spare Devices : {}",
        arr.spare_devices.iter().filter(|d| d.role == DeviceRole::Spare).count()
    );
    if sb.level.has_parity() {
        let _ = writeln!(out, "         Layout : left-symmetric");
    }
    let _ = writeln!(out, "     Chunk Size : {}K", sb.chunksize);
    if arr.bitmap.is_some() {
        let _ = writeln!(out, "  Intent Bitmap : Internal");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "           UUID : {}", sb.set_uuid.format());
    let _ = writeln!(out, "         Events : {}", sb.events);
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "    Number   Major   Minor   RaidDevice State"
    );
    for (i, dev) in arr.devices.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {:>6}   {:>5}   {:>5}   {:>10} {}",
            i,
            dev.major,
            dev.minor,
            if dev.role == DeviceRole::Active {
                format!("{}", i)
            } else {
                "-".to_string()
            },
            dev.role.label()
        );
    }
    for (i, dev) in arr.spare_devices.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {:>6}   {:>5}   {:>5}   {:>10} {}",
            arr.devices.len() + i,
            dev.major,
            dev.minor,
            "-",
            dev.role.label()
        );
    }
}

fn format_examine(dev_path: &str, sb: &Superblock, out: &mut Vec<u8>) {
    let _ = writeln!(out, "{}:", dev_path);
    let _ = writeln!(out, "          Magic : {:08x}", sb.magic);
    let _ = writeln!(out, "        Version : {}.{}", sb.major_version, sb.minor_version);
    let _ = writeln!(out, "    Feature Map : 0x{:08x}", sb.feature_map);
    let _ = writeln!(out, "     Array UUID : {}", sb.set_uuid.format());
    let _ = writeln!(out, "           Name : {}", sb.set_name);
    let _ = writeln!(out, "  Creation Time : {}", sb.ctime);
    let _ = writeln!(out, "     Raid Level : {}", sb.level.name());
    let _ = writeln!(out, "   Raid Devices : {}", sb.raid_disks);
    let _ = writeln!(out, "  Avail Dev Size : {}", format_size_human(sb.size));
    let _ = writeln!(out, "    Data Offset : {} sectors", sb.data_offset);
    let _ = writeln!(out, "   Super Offset : {} sectors", sb.super_offset);
    let _ = writeln!(out, "     Chunk Size : {}K", sb.chunksize);
    let _ = writeln!(out, "   Device UUID : {}", sb.device_uuid.format());
    let _ = writeln!(out, "         Events : {}", sb.events);
    let _ = writeln!(out, "       Checksum : {:08x}", sb.compute_checksum());
}

fn format_query(arr: &ArrayDescriptor, out: &mut Vec<u8>) {
    let _ = writeln!(
        out,
        "/dev/{}: {} {} {} devices, {} active, {} state",
        arr.md_device,
        format_size_human(arr.usable_size_kib()),
        arr.superblock.level.name(),
        arr.total_device_count(),
        arr.active_device_count(),
        arr.state.label()
    );
}

fn format_scan(arrays: &[ArrayDescriptor], out: &mut Vec<u8>) {
    for arr in arrays {
        let _ = writeln!(
            out,
            "ARRAY /dev/{} metadata={} UUID={} name={} num-devices={}",
            arr.md_device,
            DEFAULT_METADATA,
            arr.superblock.set_uuid.format(),
            arr.superblock.set_name,
            arr.superblock.raid_disks
        );
    }
}

// ============================================================================
// Argument parsing for mdadm
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
enum MdadmMode {
    Create,
    Assemble,
    Manage,
    Detail,
    Examine,
    Grow,
    Monitor,
    Stop,
    Query,
    Misc,
    Help,
    Version,
}

#[derive(Clone, Debug)]
struct MdadmArgs {
    mode: MdadmMode,
    md_device: Option<String>,
    level: Option<RaidLevel>,
    raid_devices: Option<u32>,
    spare_devices: u32,
    chunk: u32,
    bitmap: bool,
    metadata: String,
    name: Option<String>,
    homehost: Option<String>,
    force: bool,
    run: bool,
    verbose: bool,
    scan: bool,
    all: bool,
    device_paths: Vec<String>,
    /// For --manage: sub-action (add, remove, fail).
    manage_add: Vec<String>,
    manage_remove: Vec<String>,
    manage_fail: Vec<String>,
    /// For --grow: new raid-devices count.
    grow_raid_devices: Option<u32>,
    /// For --misc: zero-superblock.
    zero_superblock: bool,
}

impl MdadmArgs {
    fn default_args() -> Self {
        Self {
            mode: MdadmMode::Help,
            md_device: None,
            level: None,
            raid_devices: None,
            spare_devices: 0,
            chunk: DEFAULT_CHUNK_KIB,
            bitmap: false,
            metadata: DEFAULT_METADATA.to_string(),
            name: None,
            homehost: None,
            force: false,
            run: false,
            verbose: false,
            scan: false,
            all: false,
            device_paths: Vec::new(),
            manage_add: Vec::new(),
            manage_remove: Vec::new(),
            manage_fail: Vec::new(),
            grow_raid_devices: None,
            zero_superblock: false,
        }
    }
}

fn parse_mdadm_args(args: &[String]) -> Result<MdadmArgs, String> {
    let mut opts = MdadmArgs::default_args();
    let mut i = 0;

    if args.is_empty() {
        return Ok(opts);
    }

    // First pass: determine mode.
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" | "-h" => {
                opts.mode = MdadmMode::Help;
                return Ok(opts);
            }
            "--version" | "-V" => {
                opts.mode = MdadmMode::Version;
                return Ok(opts);
            }
            "--create" | "-C" => {
                opts.mode = MdadmMode::Create;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--assemble" | "-A" => {
                opts.mode = MdadmMode::Assemble;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--manage" => {
                opts.mode = MdadmMode::Manage;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--detail" | "-D" => {
                opts.mode = MdadmMode::Detail;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--examine" | "-E" => {
                opts.mode = MdadmMode::Examine;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.device_paths.push(args[i].clone());
                }
            }
            "--grow" | "-G" => {
                opts.mode = MdadmMode::Grow;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--monitor" | "-F" => {
                opts.mode = MdadmMode::Monitor;
            }
            "--stop" | "-S" => {
                opts.mode = MdadmMode::Stop;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--query" | "-Q" => {
                opts.mode = MdadmMode::Query;
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    opts.md_device = Some(args[i].clone());
                }
            }
            "--misc" => {
                opts.mode = MdadmMode::Misc;
            }
            _ => {}
        }
        i += 1;
    }

    // Second pass: parse options.
    i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--create" | "-C" | "--assemble" | "-A" | "--manage" | "--detail"
            | "-D" | "--examine" | "-E" | "--grow" | "-G" | "--monitor"
            | "-F" | "--stop" | "-S" | "--query" | "-Q" | "--misc"
            | "--help" | "-h" | "--version" | "-V" => {
                // Mode args already handled; skip their argument if consumed.
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    i += 1;
                    continue;
                }
                continue;
            }
            "--level" | "-l" => {
                i += 1;
                if i < args.len() {
                    opts.level = RaidLevel::from_str(&args[i]);
                    if opts.level.is_none() {
                        return Err(format!("unknown RAID level: {}", args[i]));
                    }
                }
            }
            "--raid-devices" | "-n" => {
                i += 1;
                if i < args.len() {
                    let val = args[i]
                        .parse::<u32>()
                        .map_err(|_| format!("invalid raid-devices: {}", args[i]))?;
                    if opts.mode == MdadmMode::Grow {
                        opts.grow_raid_devices = Some(val);
                    } else {
                        opts.raid_devices = Some(val);
                    }
                }
            }
            "--spare-devices" | "-x" => {
                i += 1;
                if i < args.len() {
                    opts.spare_devices = args[i]
                        .parse::<u32>()
                        .map_err(|_| format!("invalid spare-devices: {}", args[i]))?;
                }
            }
            "--chunk" | "-c" => {
                i += 1;
                if i < args.len() {
                    opts.chunk = args[i]
                        .parse::<u32>()
                        .map_err(|_| format!("invalid chunk size: {}", args[i]))?;
                }
            }
            "--bitmap" => {
                opts.bitmap = true;
            }
            "--metadata" | "-e" => {
                i += 1;
                if i < args.len() {
                    opts.metadata = args[i].clone();
                }
            }
            "--name" => {
                i += 1;
                if i < args.len() {
                    opts.name = Some(args[i].clone());
                }
            }
            "--homehost" => {
                i += 1;
                if i < args.len() {
                    opts.homehost = Some(args[i].clone());
                }
            }
            "--force" | "-f" => {
                opts.force = true;
            }
            "--run" | "-R" => {
                opts.run = true;
            }
            "--verbose" | "-v" => {
                opts.verbose = true;
            }
            "--scan" | "-s" => {
                opts.scan = true;
            }
            "--all" => {
                opts.all = true;
            }
            "--add" => {
                i += 1;
                if i < args.len() {
                    opts.manage_add.push(args[i].clone());
                }
            }
            "--remove" => {
                i += 1;
                if i < args.len() {
                    opts.manage_remove.push(args[i].clone());
                }
            }
            "--fail" => {
                i += 1;
                if i < args.len() {
                    opts.manage_fail.push(args[i].clone());
                }
            }
            "--zero-superblock" => {
                opts.zero_superblock = true;
            }
            _ => {
                // Positional argument -- treat as device path.
                if !arg.starts_with('-') {
                    opts.device_paths.push(arg.clone());
                }
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// mdadm command execution
// ============================================================================

fn run_mdadm(args: &[String], out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let opts = match parse_mdadm_args(args) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(err, "mdadm: {}", e);
            return 1;
        }
    };

    match opts.mode {
        MdadmMode::Help => {
            print_mdadm_help(out);
            0
        }
        MdadmMode::Version => {
            let _ = writeln!(out, "mdadm - v{} - Software Raid management for OurOS", VERSION);
            0
        }
        MdadmMode::Create => run_create(&opts, out, err),
        MdadmMode::Assemble => run_assemble(&opts, out, err),
        MdadmMode::Manage => run_manage(&opts, out, err),
        MdadmMode::Detail => run_detail(&opts, out, err),
        MdadmMode::Examine => run_examine(&opts, out, err),
        MdadmMode::Grow => run_grow(&opts, out, err),
        MdadmMode::Monitor => run_monitor(&opts, out, err),
        MdadmMode::Stop => run_stop(&opts, out, err),
        MdadmMode::Query => run_query(&opts, out, err),
        MdadmMode::Misc => run_misc(&opts, out, err),
    }
}

fn run_create(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --create requires a device name");
            return 1;
        }
    };
    let level = match opts.level {
        Some(l) => l,
        None => {
            let _ = writeln!(err, "mdadm: --create requires --level");
            return 1;
        }
    };
    let raid_disks = match opts.raid_devices {
        Some(n) => n,
        None => {
            let _ = writeln!(err, "mdadm: --create requires --raid-devices");
            return 1;
        }
    };

    if raid_disks < level.min_devices() {
        let _ = writeln!(
            err,
            "mdadm: {} requires at least {} drives, only {} given",
            level.name(),
            level.min_devices(),
            raid_disks
        );
        return 1;
    }

    let device_strs: Vec<&str> = opts.device_paths.iter().map(|s| s.as_str()).collect();
    if (device_strs.len() as u32) < raid_disks {
        let _ = writeln!(
            err,
            "mdadm: not enough devices listed: need {}, got {}",
            raid_disks,
            device_strs.len()
        );
        return 1;
    }

    let active_devs: Vec<&str> = device_strs.iter().take(raid_disks as usize).copied().collect();
    let spare_devs: Vec<&str> = device_strs.iter().skip(raid_disks as usize).copied().collect();

    let mut mgr = RaidManager::new();
    match mgr.create_array(&ArraySpec {
            md_device: &md_device,
            level,
            raid_disks,
            device_paths: &active_devs,
            spare_paths: &spare_devs,
            chunk: opts.chunk,
            name: opts.name.as_deref(),
            bitmap_enabled: opts.bitmap,
        }) {
        Ok(arr) => {
            let _ = writeln!(
                out,
                "mdadm: array /dev/{} started with {} devices",
                arr.md_device, raid_disks
            );
            if opts.verbose {
                format_detail(arr, out);
            }
            0
        }
        Err(e) => {
            let _ = writeln!(err, "mdadm: create failed: {}", e);
            1
        }
    }
}

fn run_assemble(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --assemble requires a device name");
            return 1;
        }
    };

    if opts.device_paths.is_empty() && !opts.scan {
        let _ = writeln!(err, "mdadm: --assemble requires component devices or --scan");
        return 1;
    }

    let mut mgr = RaidManager::new();
    let device_strs: Vec<&str> = opts.device_paths.iter().map(|s| s.as_str()).collect();

    match mgr.assemble_array(&md_device, &device_strs) {
        Ok(()) => {
            let _ = writeln!(out, "mdadm: /dev/{} has been started", md_device);
            0
        }
        Err(e) => {
            let _ = writeln!(err, "mdadm: assemble failed: {}", e);
            1
        }
    }
}

fn run_manage(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --manage requires a device name");
            return 1;
        }
    };

    // Simulate an existing array for manage operations.
    let mut mgr = RaidManager::new();
    let dummy_devs: Vec<&str> = vec!["/dev/sda1", "/dev/sdb1"];
    let _ = mgr.create_array(&ArraySpec {
            md_device: &md_device,
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &dummy_devs,
            spare_paths: &[],
            chunk: DEFAULT_CHUNK_KIB,
            name: None,
            bitmap_enabled: false,
        });

    let mut exit_code = 0;

    // Process --fail operations first.
    for path in &opts.manage_fail {
        match mgr
            .find_array_mut(&md_device)
            .ok_or_else(|| format!("array {} not found", md_device))
            .and_then(|arr| arr.mark_device_failed(path))
        {
            Ok(()) => {
                let _ = writeln!(out, "mdadm: set {} faulty in /dev/{}", path, md_device);
            }
            Err(e) => {
                let _ = writeln!(err, "mdadm: {}", e);
                exit_code = 1;
            }
        }
    }

    // Process --remove operations.
    for path in &opts.manage_remove {
        match mgr
            .find_array_mut(&md_device)
            .ok_or_else(|| format!("array {} not found", md_device))
            .and_then(|arr| arr.remove_device(path))
        {
            Ok(()) => {
                let _ = writeln!(out, "mdadm: hot removed {} from /dev/{}", path, md_device);
            }
            Err(e) => {
                let _ = writeln!(err, "mdadm: {}", e);
                exit_code = 1;
            }
        }
    }

    // Process --add operations.
    for path in &opts.manage_add {
        match mgr
            .find_array_mut(&md_device)
            .ok_or_else(|| format!("array {} not found", md_device))
            .and_then(|arr| arr.add_device(path))
        {
            Ok(()) => {
                let _ = writeln!(out, "mdadm: added {} to /dev/{}", path, md_device);
            }
            Err(e) => {
                let _ = writeln!(err, "mdadm: {}", e);
                exit_code = 1;
            }
        }
    }

    exit_code
}

fn run_detail(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    if opts.scan {
        // --detail --scan: list all arrays.
        let mgr = RaidManager::new();
        format_scan(&mgr.arrays, out);
        return 0;
    }

    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --detail requires a device name or --scan");
            return 1;
        }
    };

    // Simulate array for detail display.
    let mut mgr = RaidManager::new();
    let dummy_devs: Vec<&str> = vec!["/dev/sda1", "/dev/sdb1"];
    let _ = mgr.create_array(&ArraySpec {
            md_device: &md_device,
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &dummy_devs,
            spare_paths: &[],
            chunk: DEFAULT_CHUNK_KIB,
            name: None,
            bitmap_enabled: false,
        });

    match mgr.find_array(&md_device) {
        Some(arr) => {
            format_detail(arr, out);
            0
        }
        None => {
            let _ = writeln!(err, "mdadm: /dev/{}: not found", md_device);
            1
        }
    }
}

fn run_examine(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    if opts.device_paths.is_empty() {
        let _ = writeln!(err, "mdadm: --examine requires at least one device");
        return 1;
    }

    for dev_path in &opts.device_paths {
        // Simulate reading a superblock from the device.
        let sb = Superblock::new(
            RaidLevel::Raid1,
            2,
            "simulated",
            1_048_576,
            DEFAULT_CHUNK_KIB,
        );
        format_examine(dev_path, &sb, out);
    }
    0
}

fn run_grow(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --grow requires a device name");
            return 1;
        }
    };

    let new_raid_disks = match opts.grow_raid_devices {
        Some(n) => n,
        None => {
            let _ = writeln!(err, "mdadm: --grow requires --raid-devices");
            return 1;
        }
    };

    // Simulate existing array.
    let mut mgr = RaidManager::new();
    let dummy_devs: Vec<&str> = vec!["/dev/sda1", "/dev/sdb1", "/dev/sdc1"];
    let _ = mgr.create_array(&ArraySpec {
            md_device: &md_device,
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &dummy_devs,
            spare_paths: &[],
            chunk: DEFAULT_CHUNK_KIB,
            name: None,
            bitmap_enabled: false,
        });

    match mgr
        .find_array_mut(&md_device)
        .ok_or_else(|| format!("array {} not found", md_device))
        .and_then(|arr| arr.grow(new_raid_disks))
    {
        Ok(()) => {
            let _ = writeln!(
                out,
                "mdadm: /dev/{} reshaped to {} raid-devices",
                md_device, new_raid_disks
            );
            0
        }
        Err(e) => {
            let _ = writeln!(err, "mdadm: grow failed: {}", e);
            1
        }
    }
}

fn run_monitor(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    if !opts.scan {
        let _ = writeln!(err, "mdadm: --monitor requires --scan");
        return 1;
    }
    let _ = writeln!(out, "mdadm: monitoring all arrays...");
    let _ = writeln!(out, "mdadm: (monitor mode simulated -- no arrays to watch)");
    0
}

fn run_stop(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --stop requires a device name");
            return 1;
        }
    };

    // Simulate stopping.
    let _ = writeln!(out, "mdadm: stopped /dev/{}", md_device);
    0
}

fn run_query(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let md_device = match &opts.md_device {
        Some(d) => d.clone(),
        None => {
            let _ = writeln!(err, "mdadm: --query requires a device name");
            return 1;
        }
    };

    // Simulate query.
    let mut mgr = RaidManager::new();
    let dummy_devs: Vec<&str> = vec!["/dev/sda1", "/dev/sdb1"];
    let _ = mgr.create_array(&ArraySpec {
            md_device: &md_device,
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &dummy_devs,
            spare_paths: &[],
            chunk: DEFAULT_CHUNK_KIB,
            name: None,
            bitmap_enabled: false,
        });

    match mgr.find_array(&md_device) {
        Some(arr) => {
            format_query(arr, out);
            0
        }
        None => {
            let _ = writeln!(err, "mdadm: /dev/{}: not found", md_device);
            1
        }
    }
}

fn run_misc(opts: &MdadmArgs, out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    if opts.zero_superblock {
        if opts.device_paths.is_empty() {
            let _ = writeln!(err, "mdadm: --zero-superblock requires at least one device");
            return 1;
        }
        for dev in &opts.device_paths {
            let _ = writeln!(out, "mdadm: zeroed superblock on {}", dev);
        }
        return 0;
    }
    let _ = writeln!(err, "mdadm: --misc requires a sub-command (e.g. --zero-superblock)");
    1
}

fn print_mdadm_help(out: &mut Vec<u8>) {
    let _ = writeln!(out, "Usage: mdadm [mode] <raiddevice> [options] <component-devices>");
    let _ = writeln!(out);
    let _ = writeln!(out, " Modes:");
    let _ = writeln!(out, "  --create  -C   : Create a new array");
    let _ = writeln!(out, "  --assemble -A  : Assemble a previously created array");
    let _ = writeln!(out, "  --manage       : Manage an existing array (add/remove/fail)");
    let _ = writeln!(out, "  --detail  -D   : Display detailed information about an array");
    let _ = writeln!(out, "  --examine -E   : Examine a device superblock");
    let _ = writeln!(out, "  --grow    -G   : Grow or reshape an array");
    let _ = writeln!(out, "  --monitor -F   : Monitor arrays for events");
    let _ = writeln!(out, "  --stop    -S   : Stop an array");
    let _ = writeln!(out, "  --query   -Q   : Brief information about an array");
    let _ = writeln!(out, "  --misc         : Miscellaneous operations");
    let _ = writeln!(out);
    let _ = writeln!(out, " Options:");
    let _ = writeln!(out, "  -l, --level          : RAID level (0, 1, 5, 6, 10)");
    let _ = writeln!(out, "  -n, --raid-devices   : Number of active RAID devices");
    let _ = writeln!(out, "  -x, --spare-devices  : Number of spare devices");
    let _ = writeln!(out, "  -c, --chunk          : Chunk size in KiB (default 512)");
    let _ = writeln!(out, "      --bitmap         : Enable write-intent bitmap");
    let _ = writeln!(out, "  -e, --metadata       : Metadata version (default 1.2)");
    let _ = writeln!(out, "      --name           : Array name");
    let _ = writeln!(out, "      --homehost       : Home host name");
    let _ = writeln!(out, "  -f, --force          : Force operation");
    let _ = writeln!(out, "  -R, --run            : Start array even if degraded");
    let _ = writeln!(out, "  -v, --verbose        : Be more verbose");
    let _ = writeln!(out, "  -s, --scan           : Scan for arrays");
    let _ = writeln!(out, "      --add            : Add device to array (manage mode)");
    let _ = writeln!(out, "      --remove         : Remove device from array (manage mode)");
    let _ = writeln!(out, "      --fail           : Mark device as failed (manage mode)");
    let _ = writeln!(out, "      --zero-superblock: Clear superblock (misc mode)");
    let _ = writeln!(out, "  -h, --help           : Display this help");
    let _ = writeln!(out, "  -V, --version        : Display version");
}

// ============================================================================
// mdmon command execution
// ============================================================================

#[derive(Clone, Debug)]
struct MdmonArgs {
    all: bool,
    offroot: bool,
    pids: Vec<String>,
    help: bool,
}

fn parse_mdmon_args(args: &[String]) -> MdmonArgs {
    let mut opts = MdmonArgs {
        all: false,
        offroot: false,
        pids: Vec::new(),
        help: false,
    };

    for arg in args {
        match arg.as_str() {
            "--all" => opts.all = true,
            "--offroot" => opts.offroot = true,
            "--help" | "-h" => opts.help = true,
            _ => {
                if !arg.starts_with('-') {
                    opts.pids.push(arg.clone());
                }
            }
        }
    }
    opts
}

fn run_mdmon(args: &[String], out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
    let opts = parse_mdmon_args(args);

    if opts.help {
        let _ = writeln!(out, "Usage: mdmon [options] [md-device...]");
        let _ = writeln!(out, "  --all       Monitor all active arrays");
        let _ = writeln!(out, "  --offroot   Run in offroot mode");
        let _ = writeln!(out, "  --help      Display this help");
        return 0;
    }

    if opts.all {
        let _ = writeln!(out, "mdmon: monitoring all active arrays");
        let _ = writeln!(out, "mdmon: (daemon mode simulated)");
        return 0;
    }

    if opts.pids.is_empty() {
        let _ = writeln!(err, "mdmon: no arrays specified; use --all or specify device(s)");
        return 1;
    }

    for pid in &opts.pids {
        let _ = writeln!(out, "mdmon: monitoring /dev/{}", pid);
    }

    if opts.offroot {
        let _ = writeln!(out, "mdmon: running in offroot mode");
    }

    0
}

// ============================================================================
// main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("mdadm");
    let personality = detect_personality(argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let mut out_buf = Vec::new();
    let mut err_buf = Vec::new();

    let exit_code = match personality {
        Personality::Mdadm => run_mdadm(&rest, &mut out_buf, &mut err_buf),
        Personality::Mdmon => run_mdmon(&rest, &mut out_buf, &mut err_buf),
    };

    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out_handle = stdout.lock();
    let mut err_handle = stderr.lock();
    let _ = out_handle.write_all(&out_buf);
    let _ = err_handle.write_all(&err_buf);

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Personality detection ---

    #[test]
    fn test_personality_mdadm() {
        assert_eq!(detect_personality("mdadm"), Personality::Mdadm);
    }

    #[test]
    fn test_personality_mdadm_exe() {
        assert_eq!(detect_personality("mdadm.exe"), Personality::Mdadm);
    }

    #[test]
    fn test_personality_mdadm_path() {
        assert_eq!(
            detect_personality("/usr/sbin/mdadm"),
            Personality::Mdadm
        );
    }

    #[test]
    fn test_personality_mdmon() {
        assert_eq!(detect_personality("mdmon"), Personality::Mdmon);
    }

    #[test]
    fn test_personality_mdmon_exe() {
        assert_eq!(detect_personality("mdmon.exe"), Personality::Mdmon);
    }

    #[test]
    fn test_personality_mdmon_path() {
        assert_eq!(
            detect_personality("/usr/sbin/mdmon"),
            Personality::Mdmon
        );
    }

    #[test]
    fn test_personality_windows_path() {
        assert_eq!(
            detect_personality("C:\\tools\\mdadm.exe"),
            Personality::Mdadm
        );
    }

    #[test]
    fn test_personality_unknown_defaults_to_mdadm() {
        assert_eq!(detect_personality("unknown"), Personality::Mdadm);
    }

    // --- RAID level parsing ---

    #[test]
    fn test_raid_level_numeric_strings() {
        assert_eq!(RaidLevel::from_str("0"), Some(RaidLevel::Raid0));
        assert_eq!(RaidLevel::from_str("1"), Some(RaidLevel::Raid1));
        assert_eq!(RaidLevel::from_str("5"), Some(RaidLevel::Raid5));
        assert_eq!(RaidLevel::from_str("6"), Some(RaidLevel::Raid6));
        assert_eq!(RaidLevel::from_str("10"), Some(RaidLevel::Raid10));
    }

    #[test]
    fn test_raid_level_name_strings() {
        assert_eq!(RaidLevel::from_str("raid0"), Some(RaidLevel::Raid0));
        assert_eq!(RaidLevel::from_str("raid1"), Some(RaidLevel::Raid1));
        assert_eq!(RaidLevel::from_str("stripe"), Some(RaidLevel::Raid0));
        assert_eq!(RaidLevel::from_str("mirror"), Some(RaidLevel::Raid1));
    }

    #[test]
    fn test_raid_level_unknown() {
        assert_eq!(RaidLevel::from_str("7"), None);
        assert_eq!(RaidLevel::from_str("jbod"), None);
    }

    #[test]
    fn test_raid_level_names() {
        assert_eq!(RaidLevel::Raid0.name(), "raid0");
        assert_eq!(RaidLevel::Raid1.name(), "raid1");
        assert_eq!(RaidLevel::Raid5.name(), "raid5");
        assert_eq!(RaidLevel::Raid6.name(), "raid6");
        assert_eq!(RaidLevel::Raid10.name(), "raid10");
    }

    #[test]
    fn test_raid_level_numeric() {
        assert_eq!(RaidLevel::Raid0.numeric(), 0);
        assert_eq!(RaidLevel::Raid1.numeric(), 1);
        assert_eq!(RaidLevel::Raid5.numeric(), 5);
        assert_eq!(RaidLevel::Raid6.numeric(), 6);
        assert_eq!(RaidLevel::Raid10.numeric(), 10);
    }

    #[test]
    fn test_raid_level_min_devices() {
        assert_eq!(RaidLevel::Raid0.min_devices(), 2);
        assert_eq!(RaidLevel::Raid1.min_devices(), 2);
        assert_eq!(RaidLevel::Raid5.min_devices(), 3);
        assert_eq!(RaidLevel::Raid6.min_devices(), 4);
        assert_eq!(RaidLevel::Raid10.min_devices(), 4);
    }

    #[test]
    fn test_raid_level_has_parity() {
        assert!(!RaidLevel::Raid0.has_parity());
        assert!(!RaidLevel::Raid1.has_parity());
        assert!(RaidLevel::Raid5.has_parity());
        assert!(RaidLevel::Raid6.has_parity());
        assert!(!RaidLevel::Raid10.has_parity());
    }

    #[test]
    fn test_raid_level_has_mirror() {
        assert!(!RaidLevel::Raid0.has_mirror());
        assert!(RaidLevel::Raid1.has_mirror());
        assert!(!RaidLevel::Raid5.has_mirror());
        assert!(!RaidLevel::Raid6.has_mirror());
        assert!(RaidLevel::Raid10.has_mirror());
    }

    #[test]
    fn test_raid_level_parity_disks() {
        assert_eq!(RaidLevel::Raid0.parity_disks(), 0);
        assert_eq!(RaidLevel::Raid1.parity_disks(), 0);
        assert_eq!(RaidLevel::Raid5.parity_disks(), 1);
        assert_eq!(RaidLevel::Raid6.parity_disks(), 2);
        assert_eq!(RaidLevel::Raid10.parity_disks(), 0);
    }

    #[test]
    fn test_usable_fraction_raid0() {
        let f = RaidLevel::Raid0.usable_fraction(4);
        assert!((f - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_usable_fraction_raid1() {
        let f = RaidLevel::Raid1.usable_fraction(2);
        assert!((f - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_usable_fraction_raid5() {
        let f = RaidLevel::Raid5.usable_fraction(3);
        assert!((f - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_usable_fraction_raid6() {
        let f = RaidLevel::Raid6.usable_fraction(4);
        assert!((f - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_usable_fraction_raid10() {
        let f = RaidLevel::Raid10.usable_fraction(4);
        assert!((f - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_usable_fraction_zero_devices() {
        let f = RaidLevel::Raid0.usable_fraction(0);
        assert!((f - 0.0).abs() < f64::EPSILON);
    }

    // --- Array state ---

    #[test]
    fn test_array_state_labels() {
        assert_eq!(ArrayState::Active.label(), "active");
        assert_eq!(ArrayState::Degraded.label(), "degraded");
        assert_eq!(ArrayState::Rebuilding.label(), "rebuilding");
        assert_eq!(ArrayState::Stopped.label(), "stopped");
        assert_eq!(ArrayState::Inactive.label(), "inactive");
    }

    #[test]
    fn test_array_state_parse() {
        assert_eq!(ArrayState::from_str("active"), Some(ArrayState::Active));
        assert_eq!(ArrayState::from_str("degraded"), Some(ArrayState::Degraded));
        assert_eq!(ArrayState::from_str("unknown"), None);
    }

    // --- Device role ---

    #[test]
    fn test_device_role_labels() {
        assert_eq!(DeviceRole::Active.label(), "active sync");
        assert_eq!(DeviceRole::Spare.label(), "spare");
        assert_eq!(DeviceRole::Failed.label(), "faulty");
        assert_eq!(DeviceRole::Removed.label(), "removed");
    }

    #[test]
    fn test_device_role_parse() {
        assert_eq!(DeviceRole::from_str("active"), Some(DeviceRole::Active));
        assert_eq!(DeviceRole::from_str("active sync"), Some(DeviceRole::Active));
        assert_eq!(DeviceRole::from_str("spare"), Some(DeviceRole::Spare));
        assert_eq!(DeviceRole::from_str("faulty"), Some(DeviceRole::Failed));
        assert_eq!(DeviceRole::from_str("failed"), Some(DeviceRole::Failed));
        assert_eq!(DeviceRole::from_str("removed"), Some(DeviceRole::Removed));
        assert_eq!(DeviceRole::from_str("bogus"), None);
    }

    // --- UUID ---

    #[test]
    fn test_uuid_zero() {
        let u = Uuid::zero();
        assert_eq!(u.bytes, [0u8; 16]);
    }

    #[test]
    fn test_uuid_from_seed_deterministic() {
        let a = Uuid::from_seed("test");
        let b = Uuid::from_seed("test");
        assert_eq!(a, b);
    }

    #[test]
    fn test_uuid_from_seed_different() {
        let a = Uuid::from_seed("alpha");
        let b = Uuid::from_seed("bravo");
        assert_ne!(a, b);
    }

    #[test]
    fn test_uuid_format_length() {
        let u = Uuid::from_seed("test");
        let s = u.format();
        // Format: 8hex:8hex:8hex:8hex = 35 chars.
        assert_eq!(s.len(), 35);
        assert_eq!(s.matches(':').count(), 3);
    }

    #[test]
    fn test_uuid_parse_roundtrip() {
        let u = Uuid::from_seed("roundtrip");
        let formatted = u.format();
        let parsed = Uuid::parse(&formatted).expect("should parse");
        assert_eq!(u, parsed);
    }

    #[test]
    fn test_uuid_parse_invalid() {
        assert!(Uuid::parse("not-a-uuid").is_none());
        assert!(Uuid::parse("zzzzzzzz").is_none());
    }

    #[test]
    fn test_uuid_version_bits() {
        let u = Uuid::from_seed("version-check");
        // Version nibble (byte 6 high nibble) should be 4.
        assert_eq!(u.bytes[6] >> 4, 4);
        // Variant bits (byte 8 top 2 bits) should be 10.
        assert_eq!(u.bytes[8] >> 6, 2);
    }

    // --- Bitmap ---

    #[test]
    fn test_bitmap_new_all_clean() {
        let bm = Bitmap::new(64);
        assert_eq!(bm.dirty_count(), 0);
        assert_eq!(bm.chunk_count, 64);
    }

    #[test]
    fn test_bitmap_set_dirty() {
        let mut bm = Bitmap::new(64);
        bm.set_dirty(0);
        bm.set_dirty(15);
        bm.set_dirty(63);
        assert!(bm.is_dirty(0));
        assert!(bm.is_dirty(15));
        assert!(bm.is_dirty(63));
        assert!(!bm.is_dirty(1));
        assert_eq!(bm.dirty_count(), 3);
    }

    #[test]
    fn test_bitmap_set_clean() {
        let mut bm = Bitmap::new(16);
        bm.set_dirty(5);
        assert!(bm.is_dirty(5));
        bm.set_clean(5);
        assert!(!bm.is_dirty(5));
    }

    #[test]
    fn test_bitmap_set_all_dirty() {
        let mut bm = Bitmap::new(16);
        bm.set_all_dirty();
        for i in 0..16 {
            assert!(bm.is_dirty(i));
        }
    }

    #[test]
    fn test_bitmap_set_all_clean() {
        let mut bm = Bitmap::new(16);
        bm.set_all_dirty();
        bm.set_all_clean();
        assert_eq!(bm.dirty_count(), 0);
    }

    #[test]
    fn test_bitmap_out_of_range() {
        let mut bm = Bitmap::new(8);
        bm.set_dirty(100); // Should be a no-op.
        assert!(!bm.is_dirty(100));
        assert_eq!(bm.dirty_count(), 0);
    }

    // --- Superblock ---

    #[test]
    fn test_superblock_new_magic() {
        let sb = Superblock::new(RaidLevel::Raid1, 2, "test", 1024, 512);
        assert_eq!(sb.magic, MD_SB_MAGIC);
    }

    #[test]
    fn test_superblock_new_version() {
        let sb = Superblock::new(RaidLevel::Raid5, 3, "arr", 2048, 256);
        assert_eq!(sb.major_version, 1);
        assert_eq!(sb.minor_version, 2);
    }

    #[test]
    fn test_superblock_validate_ok() {
        let sb = Superblock::new(RaidLevel::Raid1, 2, "ok", 1024, 512);
        assert!(sb.validate().is_ok());
    }

    #[test]
    fn test_superblock_validate_bad_magic() {
        let mut sb = Superblock::new(RaidLevel::Raid1, 2, "bad", 1024, 512);
        sb.magic = 0xdeadbeef;
        assert!(sb.validate().is_err());
    }

    #[test]
    fn test_superblock_validate_bad_version() {
        let mut sb = Superblock::new(RaidLevel::Raid1, 2, "ver", 1024, 512);
        sb.major_version = 99;
        assert!(sb.validate().is_err());
    }

    #[test]
    fn test_superblock_validate_insufficient_devices() {
        let mut sb = Superblock::new(RaidLevel::Raid5, 3, "r5", 1024, 512);
        sb.raid_disks = 2; // RAID5 needs 3.
        assert!(sb.validate().is_err());
    }

    #[test]
    fn test_superblock_checksum_deterministic() {
        let sb = Superblock::new(RaidLevel::Raid1, 2, "chk", 1024, 512);
        let c1 = sb.compute_checksum();
        let c2 = sb.compute_checksum();
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_superblock_checksum_differs() {
        let sb1 = Superblock::new(RaidLevel::Raid1, 2, "a", 1024, 512);
        let sb2 = Superblock::new(RaidLevel::Raid5, 3, "b", 2048, 256);
        assert_ne!(sb1.compute_checksum(), sb2.compute_checksum());
    }

    #[test]
    fn test_default_layout_raid5() {
        assert_eq!(default_layout(RaidLevel::Raid5), 2);
    }

    #[test]
    fn test_default_layout_raid6() {
        assert_eq!(default_layout(RaidLevel::Raid6), 2);
    }

    #[test]
    fn test_default_layout_raid10() {
        assert_eq!(default_layout(RaidLevel::Raid10), 0x0102);
    }

    #[test]
    fn test_default_layout_raid0() {
        assert_eq!(default_layout(RaidLevel::Raid0), 0);
    }

    // --- DeviceRecord ---

    #[test]
    fn test_device_record_new() {
        let rec = DeviceRecord::new("/dev/sda1", 0, 1024);
        assert_eq!(rec.path, "/dev/sda1");
        assert_eq!(rec.role, DeviceRole::Active);
        assert_eq!(rec.dev_number, 0);
        assert_eq!(rec.size, 1024);
    }

    // --- ArrayDescriptor ---

    #[test]
    fn test_array_descriptor_create_raid1() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .expect("should create");
        assert_eq!(arr.state, ArrayState::Active);
        assert_eq!(arr.devices.len(), 2);
        assert_eq!(arr.spare_devices.len(), 0);
    }

    #[test]
    fn test_array_descriptor_create_with_spares() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &["/dev/sdd1"],
            chunk: 256,
            name: None,
            bitmap_enabled: false,
        })
        .expect("should create");
        assert_eq!(arr.devices.len(), 3);
        assert_eq!(arr.spare_devices.len(), 1);
        assert_eq!(arr.spare_devices[0].role, DeviceRole::Spare);
    }

    #[test]
    fn test_array_descriptor_insufficient_devices() {
        let result = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_array_descriptor_raid_disks_too_few() {
        let result = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid6,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_array_active_device_count() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert_eq!(arr.active_device_count(), 2);
    }

    #[test]
    fn test_array_total_device_count() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &["/dev/sdd1"],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert_eq!(arr.total_device_count(), 4);
    }

    #[test]
    fn test_array_working_devices() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &["/dev/sdd1"],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert_eq!(arr.working_devices(), 4);
    }

    #[test]
    fn test_array_usable_size_raid1() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        // RAID1 with 2 devs: usable = 1 dev worth.
        assert_eq!(arr.usable_size_kib(), arr.superblock.size);
    }

    #[test]
    fn test_array_mark_device_failed() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.mark_device_failed("/dev/sdb1").is_ok());
        assert_eq!(arr.state, ArrayState::Degraded);
        assert_eq!(arr.failed_devices(), 1);
    }

    #[test]
    fn test_array_mark_device_failed_already() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        arr.mark_device_failed("/dev/sdb1").unwrap();
        assert!(arr.mark_device_failed("/dev/sdb1").is_err());
    }

    #[test]
    fn test_array_mark_device_failed_not_found() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.mark_device_failed("/dev/sdc1").is_err());
    }

    #[test]
    fn test_array_remove_failed_device() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        arr.mark_device_failed("/dev/sdb1").unwrap();
        assert!(arr.remove_device("/dev/sdb1").is_ok());
    }

    #[test]
    fn test_array_cannot_remove_active_device() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.remove_device("/dev/sdb1").is_err());
    }

    #[test]
    fn test_array_remove_spare() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &["/dev/sdd1"],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.remove_device("/dev/sdd1").is_ok());
        assert_eq!(arr.spare_devices.len(), 0);
    }

    #[test]
    fn test_array_remove_not_found() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.remove_device("/dev/zzz").is_err());
    }

    #[test]
    fn test_array_add_device_as_spare() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.add_device("/dev/sdc1").is_ok());
        assert_eq!(arr.spare_devices.len(), 1);
        assert_eq!(arr.spare_devices[0].role, DeviceRole::Spare);
    }

    #[test]
    fn test_array_add_device_to_degraded_as_active() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        arr.mark_device_failed("/dev/sdb1").unwrap();
        assert!(arr.add_device("/dev/sdc1").is_ok());
        assert_eq!(arr.state, ArrayState::Rebuilding);
    }

    #[test]
    fn test_array_add_duplicate_device() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.add_device("/dev/sda1").is_err());
    }

    #[test]
    fn test_array_grow_raid5() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.grow(4).is_ok());
        assert_eq!(arr.superblock.raid_disks, 4);
        assert_eq!(arr.state, ArrayState::Rebuilding);
    }

    #[test]
    fn test_array_grow_raid1_unsupported() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.grow(3).is_err());
    }

    #[test]
    fn test_array_grow_smaller_fails() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid5,
            raid_disks: 3,
            device_paths: &["/dev/sda1", "/dev/sdb1", "/dev/sdc1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.grow(2).is_err());
    }

    #[test]
    fn test_array_bitmap_enabled() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: true,
        })
        .unwrap();
        assert!(arr.bitmap.is_some());
    }

    #[test]
    fn test_array_bitmap_disabled() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(arr.bitmap.is_none());
    }

    // --- RaidManager ---

    #[test]
    fn test_manager_create_and_find() {
        let mut mgr = RaidManager::new();
        mgr.create_array(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(mgr.find_array("md0").is_some());
        assert!(mgr.find_array("md1").is_none());
    }

    #[test]
    fn test_manager_create_duplicate() {
        let mut mgr = RaidManager::new();
        mgr.create_array(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(mgr
            .create_array(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sdc1", "/dev/sdd1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
            .is_err());
    }

    #[test]
    fn test_manager_stop_array() {
        let mut mgr = RaidManager::new();
        mgr.create_array(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(mgr.stop_array("md0").is_ok());
        assert_eq!(mgr.find_array("md0").unwrap().state, ArrayState::Stopped);
    }

    #[test]
    fn test_manager_stop_nonexistent() {
        let mut mgr = RaidManager::new();
        assert!(mgr.stop_array("md99").is_err());
    }

    #[test]
    fn test_manager_assemble() {
        let mut mgr = RaidManager::new();
        assert!(mgr
            .assemble_array("md0", &["/dev/sda1", "/dev/sdb1"])
            .is_ok());
        assert!(mgr.find_array("md0").is_some());
    }

    #[test]
    fn test_manager_assemble_duplicate() {
        let mut mgr = RaidManager::new();
        mgr.assemble_array("md0", &["/dev/sda1", "/dev/sdb1"])
            .unwrap();
        assert!(mgr
            .assemble_array("md0", &["/dev/sda1", "/dev/sdb1"])
            .is_err());
    }

    #[test]
    fn test_manager_assemble_no_devices() {
        let mut mgr = RaidManager::new();
        assert!(mgr.assemble_array("md0", &[]).is_err());
    }

    #[test]
    fn test_manager_remove_stopped() {
        let mut mgr = RaidManager::new();
        mgr.create_array(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        mgr.stop_array("md0").unwrap();
        assert!(mgr.remove_stopped("md0").is_ok());
        assert!(mgr.find_array("md0").is_none());
    }

    #[test]
    fn test_manager_remove_not_stopped() {
        let mut mgr = RaidManager::new();
        mgr.create_array(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(mgr.remove_stopped("md0").is_err());
    }

    // --- Formatting ---

    #[test]
    fn test_format_size_human_kib() {
        assert_eq!(format_size_human(512), "512 KiB");
    }

    #[test]
    fn test_format_size_human_mib() {
        let s = format_size_human(2048);
        assert!(s.contains("MiB"));
    }

    #[test]
    fn test_format_size_human_gib() {
        let s = format_size_human(2_097_152);
        assert!(s.contains("GiB"));
    }

    #[test]
    fn test_format_detail_contains_raid_level() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        let mut buf = Vec::new();
        format_detail(&arr, &mut buf);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("raid1"));
    }

    #[test]
    fn test_format_detail_contains_uuid() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        let mut buf = Vec::new();
        format_detail(&arr, &mut buf);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("UUID"));
    }

    #[test]
    fn test_format_examine_contains_magic() {
        let sb = Superblock::new(RaidLevel::Raid1, 2, "test", 1024, 512);
        let mut buf = Vec::new();
        format_examine("/dev/sda1", &sb, &mut buf);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("a92b4efc"));
    }

    #[test]
    fn test_format_query_output() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        let mut buf = Vec::new();
        format_query(&arr, &mut buf);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("md0"));
        assert!(s.contains("raid1"));
    }

    #[test]
    fn test_format_scan_output() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        let mut buf = Vec::new();
        format_scan(&[arr], &mut buf);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("ARRAY"));
        assert!(s.contains("UUID="));
    }

    // --- Argument parsing (mdadm) ---

    #[test]
    fn test_parse_help() {
        let args = vec!["--help".to_string()];
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Help);
    }

    #[test]
    fn test_parse_version() {
        let args = vec!["--version".to_string()];
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Version);
    }

    #[test]
    fn test_parse_create_basic() {
        let args: Vec<String> = vec![
            "--create", "md0", "-l", "1", "-n", "2", "/dev/sda1", "/dev/sdb1",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Create);
        assert_eq!(opts.md_device, Some("md0".to_string()));
        assert_eq!(opts.level, Some(RaidLevel::Raid1));
        assert_eq!(opts.raid_devices, Some(2));
        assert!(opts.device_paths.contains(&"/dev/sda1".to_string()));
    }

    #[test]
    fn test_parse_assemble() {
        let args: Vec<String> = vec!["--assemble", "md0", "/dev/sda1", "/dev/sdb1"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Assemble);
    }

    #[test]
    fn test_parse_manage_with_fail() {
        let args: Vec<String> = vec!["--manage", "md0", "--fail", "/dev/sdb1"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Manage);
        assert!(opts.manage_fail.contains(&"/dev/sdb1".to_string()));
    }

    #[test]
    fn test_parse_manage_with_add() {
        let args: Vec<String> = vec!["--manage", "md0", "--add", "/dev/sdc1"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert!(opts.manage_add.contains(&"/dev/sdc1".to_string()));
    }

    #[test]
    fn test_parse_manage_with_remove() {
        let args: Vec<String> = vec!["--manage", "md0", "--remove", "/dev/sdb1"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert!(opts.manage_remove.contains(&"/dev/sdb1".to_string()));
    }

    #[test]
    fn test_parse_detail_scan() {
        let args: Vec<String> = vec!["--detail", "--scan"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Detail);
        assert!(opts.scan);
    }

    #[test]
    fn test_parse_grow() {
        let args: Vec<String> = vec!["--grow", "md0", "-n", "4"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Grow);
        assert_eq!(opts.grow_raid_devices, Some(4));
    }

    #[test]
    fn test_parse_stop() {
        let args: Vec<String> = vec!["--stop", "md0"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Stop);
    }

    #[test]
    fn test_parse_monitor_scan() {
        let args: Vec<String> = vec!["--monitor", "--scan"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Monitor);
        assert!(opts.scan);
    }

    #[test]
    fn test_parse_examine() {
        let args: Vec<String> = vec!["--examine", "/dev/sda1"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Examine);
    }

    #[test]
    fn test_parse_query() {
        let args: Vec<String> = vec!["--query", "md0"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Query);
    }

    #[test]
    fn test_parse_misc_zero_superblock() {
        let args: Vec<String> = vec!["--misc", "--zero-superblock", "/dev/sda1"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Misc);
        assert!(opts.zero_superblock);
    }

    #[test]
    fn test_parse_force_flag() {
        let args: Vec<String> = vec!["--create", "md0", "-f", "-l", "1", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert!(opts.force);
    }

    #[test]
    fn test_parse_verbose_flag() {
        let args: Vec<String> = vec!["--detail", "md0", "-v"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert!(opts.verbose);
    }

    #[test]
    fn test_parse_bitmap_flag() {
        let args: Vec<String> = vec!["--create", "md0", "--bitmap", "-l", "1", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert!(opts.bitmap);
    }

    #[test]
    fn test_parse_chunk_flag() {
        let args: Vec<String> = vec!["--create", "md0", "-c", "256", "-l", "5", "-n", "3"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.chunk, 256);
    }

    #[test]
    fn test_parse_metadata_flag() {
        let args: Vec<String> = vec!["--create", "md0", "-e", "1.0", "-l", "1", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.metadata, "1.0");
    }

    #[test]
    fn test_parse_name_flag() {
        let args: Vec<String> = vec!["--create", "md0", "--name", "myarray", "-l", "1", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.name, Some("myarray".to_string()));
    }

    #[test]
    fn test_parse_homehost_flag() {
        let args: Vec<String> = vec!["--create", "md0", "--homehost", "myhost", "-l", "1", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.homehost, Some("myhost".to_string()));
    }

    #[test]
    fn test_parse_invalid_level() {
        let args: Vec<String> = vec!["--create", "md0", "-l", "99", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_mdadm_args(&args).is_err());
    }

    #[test]
    fn test_parse_empty_args() {
        let args: Vec<String> = vec![];
        let opts = parse_mdadm_args(&args).unwrap();
        assert_eq!(opts.mode, MdadmMode::Help);
    }

    // --- Command execution (mdadm) ---

    #[test]
    fn test_run_mdadm_help() {
        let args: Vec<String> = vec!["--help".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Usage:"));
    }

    #[test]
    fn test_run_mdadm_version() {
        let args: Vec<String> = vec!["--version".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains(VERSION));
    }

    #[test]
    fn test_run_create_success() {
        let args: Vec<String> = vec![
            "--create", "md0", "-l", "1", "-n", "2", "/dev/sda1", "/dev/sdb1",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("started"));
    }

    #[test]
    fn test_run_create_missing_level() {
        let args: Vec<String> = vec!["--create", "md0", "-n", "2"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_create_missing_n() {
        let args: Vec<String> = vec!["--create", "md0", "-l", "1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_create_insufficient_devices() {
        let args: Vec<String> = vec![
            "--create", "md0", "-l", "5", "-n", "3", "/dev/sda1",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_assemble_success() {
        let args: Vec<String> = vec!["--assemble", "md0", "/dev/sda1", "/dev/sdb1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("started"));
    }

    #[test]
    fn test_run_detail_output() {
        let args: Vec<String> = vec!["--detail", "md0"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Raid Level"));
    }

    #[test]
    fn test_run_examine_output() {
        let args: Vec<String> = vec!["--examine", "/dev/sda1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Magic"));
    }

    #[test]
    fn test_run_stop_output() {
        let args: Vec<String> = vec!["--stop", "md0"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("stopped"));
    }

    #[test]
    fn test_run_query_output() {
        let args: Vec<String> = vec!["--query", "md0"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_misc_zero_superblock() {
        let args: Vec<String> = vec!["--misc", "--zero-superblock", "/dev/sda1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("zeroed"));
    }

    #[test]
    fn test_run_monitor_needs_scan() {
        let args: Vec<String> = vec!["--monitor".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_monitor_with_scan() {
        let args: Vec<String> = vec!["--monitor", "--scan"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_grow_success() {
        let args: Vec<String> = vec!["--grow", "md0", "-n", "4"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("reshaped"));
    }

    #[test]
    fn test_run_grow_missing_n() {
        let args: Vec<String> = vec!["--grow", "md0"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_manage_fail() {
        let args: Vec<String> = vec!["--manage", "md0", "--fail", "/dev/sdb1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("faulty"));
    }

    #[test]
    fn test_run_manage_add() {
        let args: Vec<String> = vec!["--manage", "md0", "--add", "/dev/sdc1"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdadm(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("added"));
    }

    // --- mdmon ---

    #[test]
    fn test_run_mdmon_help() {
        let args: Vec<String> = vec!["--help".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdmon(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Usage:"));
    }

    #[test]
    fn test_run_mdmon_all() {
        let args: Vec<String> = vec!["--all".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdmon(&args, &mut out, &mut err);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_mdmon_no_args() {
        let args: Vec<String> = vec![];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdmon(&args, &mut out, &mut err);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_mdmon_specific_device() {
        let args: Vec<String> = vec!["md0".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdmon(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("monitoring"));
    }

    #[test]
    fn test_run_mdmon_offroot() {
        let args: Vec<String> = vec!["--offroot".to_string(), "md0".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_mdmon(&args, &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("offroot"));
    }

    #[test]
    fn test_mdmon_parse_all() {
        let args: Vec<String> = vec!["--all".to_string()];
        let opts = parse_mdmon_args(&args);
        assert!(opts.all);
    }

    #[test]
    fn test_mdmon_parse_offroot() {
        let args: Vec<String> = vec!["--offroot".to_string(), "md0".to_string()];
        let opts = parse_mdmon_args(&args);
        assert!(opts.offroot);
        assert_eq!(opts.pids, vec!["md0".to_string()]);
    }

    // --- is_degraded ---

    #[test]
    fn test_is_degraded_false() {
        let arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        assert!(!arr.is_degraded());
    }

    #[test]
    fn test_is_degraded_true() {
        let mut arr = ArrayDescriptor::new(&ArraySpec {
            md_device: "md0",
            level: RaidLevel::Raid1,
            raid_disks: 2,
            device_paths: &["/dev/sda1", "/dev/sdb1"],
            spare_paths: &[],
            chunk: 512,
            name: None,
            bitmap_enabled: false,
        })
        .unwrap();
        arr.mark_device_failed("/dev/sdb1").unwrap();
        assert!(arr.is_degraded());
    }
}
