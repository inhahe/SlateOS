//! OurOS Partition Manager
//!
//! Graphical disk partition manager for creating, deleting, resizing,
//! formatting, and managing disk partitions. Features:
//! - Multi-disk sidebar with disk selection
//! - Visual disk map showing partitions proportional to size
//! - Operation queue with review-before-apply workflow
//! - Partition details panel (label, UUID, filesystem, flags, mount point)
//! - Disk info panel (model, serial, SMART health, temperature)
//! - Color-coded filesystem types (ext4, FAT32, NTFS, swap, EFI, etc.)
//! - Safety: confirmation dialogs, warning banners for system partitions
//! - Human-readable size formatting with binary units
//!
//! Uses the guitk library for UI rendering. Disk data is gathered through
//! OurOS syscalls; stubbed with representative data for initial development.

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COLOR_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COLOR_TEAL: Color = Color::from_hex(0x94E2D5);
const COLOR_SAPPHIRE: Color = Color::from_hex(0x74C7EC);
const COLOR_FLAMINGO: Color = Color::from_hex(0xF2CDCD);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const TITLE_BAR_HEIGHT: f32 = 36.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 26.0;
const SIDEBAR_WIDTH: f32 = 220.0;
const DISK_MAP_HEIGHT: f32 = 80.0;
const DISK_MAP_PADDING: f32 = 12.0;
const DISK_MAP_BAR_HEIGHT: f32 = 40.0;
const PARTITION_ROW_HEIGHT: f32 = 24.0;
const DETAIL_PANEL_WIDTH: f32 = 300.0;
const PROPERTY_ROW_HEIGHT: f32 = 22.0;
const SECTION_HEADER_HEIGHT: f32 = 28.0;
const QUEUE_PANEL_HEIGHT: f32 = 150.0;
const QUEUE_ROW_HEIGHT: f32 = 22.0;
const TOOLBAR_BTN_WIDTH: f32 = 100.0;
const TOOLBAR_BTN_HEIGHT: f32 = 28.0;
const DIALOG_WIDTH: f32 = 420.0;
const DIALOG_HEIGHT: f32 = 260.0;
const DIALOG_BTN_WIDTH: f32 = 90.0;
const DIALOG_BTN_HEIGHT: f32 = 30.0;
const SIDEBAR_DISK_ROW_HEIGHT: f32 = 48.0;
const MIN_PARTITION_BAR_WIDTH: f32 = 4.0;

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count into a human-readable string using binary units.
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = ((bytes % TIB) * 100) / TIB;
        if frac == 0 {
            return format!("{whole} TiB");
        }
        return format!("{whole}.{frac:02} TiB");
    }
    if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = ((bytes % GIB) * 100) / GIB;
        if frac == 0 {
            return format!("{whole} GiB");
        }
        return format!("{whole}.{frac:02} GiB");
    }
    if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = ((bytes % MIB) * 100) / MIB;
        if frac == 0 {
            return format!("{whole} MiB");
        }
        return format!("{whole}.{frac:02} MiB");
    }
    if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = ((bytes % KIB) * 100) / KIB;
        if frac == 0 {
            return format!("{whole} KiB");
        }
        return format!("{whole}.{frac:02} KiB");
    }
    format!("{bytes} B")
}

/// Format a sector count to bytes given a sector size.
fn sectors_to_bytes(sectors: u64, sector_size: u32) -> u64 {
    sectors.saturating_mul(sector_size as u64)
}

// ============================================================================
// Partition table type
// ============================================================================

/// Partition table type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionTableType {
    Gpt,
    Mbr,
}

impl PartitionTableType {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Gpt => "GPT",
            Self::Mbr => "MBR",
        }
    }
}

// ============================================================================
// Filesystem types
// ============================================================================

/// Filesystem type for a partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemType {
    Ext4,
    Fat32,
    Ntfs,
    Swap,
    EfiSystem,
    Unformatted,
    Unknown,
}

impl FilesystemType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Fat32 => "FAT32",
            Self::Ntfs => "NTFS",
            Self::Swap => "swap",
            Self::EfiSystem => "EFI System",
            Self::Unformatted => "Unformatted",
            Self::Unknown => "Unknown",
        }
    }

    /// Color for disk map visualization.
    pub fn color(self) -> Color {
        match self {
            Self::Ext4 => COLOR_BLUE,
            Self::Fat32 => COLOR_GREEN,
            Self::Ntfs => COLOR_LAVENDER,
            Self::Swap => COLOR_MAUVE,
            Self::EfiSystem => COLOR_PEACH,
            Self::Unformatted => COLOR_SURFACE2,
            Self::Unknown => COLOR_OVERLAY0,
        }
    }

    /// All user-selectable filesystem types for formatting.
    pub fn formattable() -> &'static [FilesystemType] {
        &[
            Self::Ext4,
            Self::Fat32,
            Self::Ntfs,
            Self::Swap,
            Self::EfiSystem,
        ]
    }

    /// All filesystem types.
    pub fn all() -> &'static [FilesystemType] {
        &[
            Self::Ext4,
            Self::Fat32,
            Self::Ntfs,
            Self::Swap,
            Self::EfiSystem,
            Self::Unformatted,
            Self::Unknown,
        ]
    }
}

// ============================================================================
// Partition flags
// ============================================================================

/// Flags that can be set on a partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionFlag {
    Boot,
    Efi,
    Swap,
    Hidden,
}

impl PartitionFlag {
    pub fn label(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Efi => "efi",
            Self::Swap => "swap",
            Self::Hidden => "hidden",
        }
    }

    pub fn all() -> &'static [PartitionFlag] {
        &[Self::Boot, Self::Efi, Self::Swap, Self::Hidden]
    }
}

// ============================================================================
// SMART health status
// ============================================================================

/// SMART health status for a disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartHealth {
    Healthy,
    Warning,
    Failing,
    Unknown,
}

impl SmartHealth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Warning => "Warning",
            Self::Failing => "Failing",
            Self::Unknown => "Unknown",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Healthy => COLOR_GREEN,
            Self::Warning => COLOR_YELLOW,
            Self::Failing => COLOR_RED,
            Self::Unknown => COLOR_OVERLAY0,
        }
    }
}

// ============================================================================
// Partition data model
// ============================================================================

/// A single partition on a disk.
#[derive(Clone, Debug)]
pub struct Partition {
    /// Partition index (1-based).
    pub index: u32,
    /// Human-readable label.
    pub label: String,
    /// Filesystem type.
    pub filesystem: FilesystemType,
    /// Start sector (inclusive).
    pub start_sector: u64,
    /// End sector (inclusive).
    pub end_sector: u64,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Flags set on this partition.
    pub flags: Vec<PartitionFlag>,
    /// Mount point (if mounted).
    pub mount_point: Option<String>,
    /// UUID string.
    pub uuid: String,
    /// Used space in bytes (if known).
    pub used_bytes: Option<u64>,
    /// Free space in bytes (if known).
    pub free_bytes: Option<u64>,
}

impl Partition {
    /// Whether this partition has the boot flag.
    pub fn is_boot(&self) -> bool {
        self.flags.contains(&PartitionFlag::Boot)
    }

    /// Whether this partition has the EFI flag.
    pub fn is_efi(&self) -> bool {
        self.flags.contains(&PartitionFlag::Efi)
    }

    /// Whether this partition is a system-critical partition (boot or EFI).
    pub fn is_system(&self) -> bool {
        self.is_boot() || self.is_efi()
    }

    /// Flags as a comma-separated string.
    pub fn flags_string(&self) -> String {
        if self.flags.is_empty() {
            return String::from("none");
        }
        self.flags.iter().map(|f| f.label()).collect::<Vec<_>>().join(", ")
    }

    /// Used percentage (0..100), or None if unknown.
    pub fn used_percent(&self) -> Option<u32> {
        let used = self.used_bytes?;
        if self.size_bytes == 0 {
            return Some(0);
        }
        Some(((used as f64 / self.size_bytes as f64) * 100.0) as u32)
    }
}

// ============================================================================
// Unallocated space
// ============================================================================

/// Represents unallocated (free) space on a disk.
#[derive(Clone, Debug)]
pub struct UnallocatedSpace {
    pub start_sector: u64,
    pub end_sector: u64,
    pub size_bytes: u64,
}

// ============================================================================
// Disk region -- partition or unallocated
// ============================================================================

/// A region on the disk: either a partition or unallocated space.
#[derive(Clone, Debug)]
pub enum DiskRegion {
    Partition(Partition),
    Unallocated(UnallocatedSpace),
}

impl DiskRegion {
    pub fn start_sector(&self) -> u64 {
        match self {
            Self::Partition(p) => p.start_sector,
            Self::Unallocated(u) => u.start_sector,
        }
    }

    pub fn end_sector(&self) -> u64 {
        match self {
            Self::Partition(p) => p.end_sector,
            Self::Unallocated(u) => u.end_sector,
        }
    }

    pub fn size_bytes(&self) -> u64 {
        match self {
            Self::Partition(p) => p.size_bytes,
            Self::Unallocated(u) => u.size_bytes,
        }
    }

    pub fn is_partition(&self) -> bool {
        matches!(self, Self::Partition(_))
    }

    pub fn is_unallocated(&self) -> bool {
        matches!(self, Self::Unallocated(_))
    }

    pub fn as_partition(&self) -> Option<&Partition> {
        match self {
            Self::Partition(p) => Some(p),
            Self::Unallocated(_) => None,
        }
    }
}

// ============================================================================
// Disk data model
// ============================================================================

/// A physical disk.
#[derive(Clone, Debug)]
pub struct Disk {
    /// Internal disk ID.
    pub id: u32,
    /// Device name (e.g. "/dev/sda").
    pub name: String,
    /// Disk model string.
    pub model: String,
    /// Serial number.
    pub serial: String,
    /// Total size in bytes.
    pub total_size_bytes: u64,
    /// Sector size in bytes.
    pub sector_size: u32,
    /// Total number of sectors.
    pub total_sectors: u64,
    /// Partition table type.
    pub table_type: PartitionTableType,
    /// Partitions on this disk.
    pub partitions: Vec<Partition>,
    /// SMART health status.
    pub smart_health: SmartHealth,
    /// Disk temperature in Celsius (if available).
    pub temperature_c: Option<u32>,
}

impl Disk {
    /// Compute all regions (partitions + unallocated gaps) sorted by start sector.
    pub fn regions(&self) -> Vec<DiskRegion> {
        let mut regions = Vec::new();
        let mut sorted_parts: Vec<&Partition> = self.partitions.iter().collect();
        sorted_parts.sort_by_key(|p| p.start_sector);

        // GPT reserve: first 34 sectors typically reserved
        let first_usable: u64 = if self.table_type == PartitionTableType::Gpt {
            34
        } else {
            1
        };
        let last_usable: u64 = self.total_sectors.saturating_sub(
            if self.table_type == PartitionTableType::Gpt { 34 } else { 1 },
        );

        let mut cursor = first_usable;

        for part in &sorted_parts {
            if part.start_sector > cursor {
                let gap_sectors = part.start_sector - cursor;
                regions.push(DiskRegion::Unallocated(UnallocatedSpace {
                    start_sector: cursor,
                    end_sector: part.start_sector.saturating_sub(1),
                    size_bytes: sectors_to_bytes(gap_sectors, self.sector_size),
                }));
            }
            regions.push(DiskRegion::Partition((*part).clone()));
            cursor = part.end_sector.saturating_add(1);
        }

        if cursor < last_usable {
            let gap_sectors = last_usable - cursor;
            regions.push(DiskRegion::Unallocated(UnallocatedSpace {
                start_sector: cursor,
                end_sector: last_usable.saturating_sub(1),
                size_bytes: sectors_to_bytes(gap_sectors, self.sector_size),
            }));
        }

        regions
    }

    /// Total used space across all partitions.
    pub fn used_space(&self) -> u64 {
        self.partitions.iter().map(|p| p.size_bytes).sum()
    }

    /// Total free (unallocated) space.
    pub fn free_space(&self) -> u64 {
        self.total_size_bytes.saturating_sub(self.used_space())
    }

    /// Number of partitions.
    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }
}

// ============================================================================
// Pending operation model
// ============================================================================

/// A queued operation that has not yet been applied to disk.
#[derive(Clone, Debug)]
pub enum PendingOperation {
    /// Create a new partition in unallocated space.
    CreatePartition {
        disk_id: u32,
        start_sector: u64,
        end_sector: u64,
        filesystem: FilesystemType,
        label: String,
    },
    /// Delete an existing partition.
    DeletePartition {
        disk_id: u32,
        partition_index: u32,
        partition_label: String,
    },
    /// Resize/move a partition.
    ResizePartition {
        disk_id: u32,
        partition_index: u32,
        new_start_sector: u64,
        new_end_sector: u64,
    },
    /// Format a partition with a new filesystem.
    FormatPartition {
        disk_id: u32,
        partition_index: u32,
        new_filesystem: FilesystemType,
    },
    /// Set the label on a partition.
    SetLabel {
        disk_id: u32,
        partition_index: u32,
        new_label: String,
    },
    /// Toggle a flag on a partition.
    SetFlag {
        disk_id: u32,
        partition_index: u32,
        flag: PartitionFlag,
        enabled: bool,
    },
    /// Change the mount point of a partition.
    SetMountPoint {
        disk_id: u32,
        partition_index: u32,
        mount_point: Option<String>,
    },
    /// Create a new partition table (destroys all data).
    CreatePartitionTable {
        disk_id: u32,
        table_type: PartitionTableType,
    },
}

impl PendingOperation {
    /// Human-readable description of this operation.
    pub fn describe(&self) -> String {
        match self {
            Self::CreatePartition { label, filesystem, start_sector, end_sector, .. } => {
                let size = end_sector.saturating_sub(*start_sector).saturating_mul(512);
                format!("Create {} partition \"{}\" ({})", filesystem.label(), label, format_size(size))
            }
            Self::DeletePartition { partition_label, partition_index, .. } => {
                format!("Delete partition {} (\"{}\")", partition_index, partition_label)
            }
            Self::ResizePartition { partition_index, new_start_sector, new_end_sector, .. } => {
                let size = new_end_sector.saturating_sub(*new_start_sector).saturating_mul(512);
                format!("Resize partition {} to {}", partition_index, format_size(size))
            }
            Self::FormatPartition { partition_index, new_filesystem, .. } => {
                format!("Format partition {} as {}", partition_index, new_filesystem.label())
            }
            Self::SetLabel { partition_index, new_label, .. } => {
                format!("Set label on partition {} to \"{}\"", partition_index, new_label)
            }
            Self::SetFlag { partition_index, flag, enabled, .. } => {
                if *enabled {
                    format!("Enable {} flag on partition {}", flag.label(), partition_index)
                } else {
                    format!("Disable {} flag on partition {}", flag.label(), partition_index)
                }
            }
            Self::SetMountPoint { partition_index, mount_point, .. } => {
                match mount_point {
                    Some(mp) => format!("Mount partition {} at {}", partition_index, mp),
                    None => format!("Unmount partition {}", partition_index),
                }
            }
            Self::CreatePartitionTable { table_type, .. } => {
                format!("Create new {} partition table (ALL DATA WILL BE LOST)", table_type.label())
            }
        }
    }

    /// Whether this operation is destructive (data loss risk).
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            Self::DeletePartition { .. }
                | Self::FormatPartition { .. }
                | Self::CreatePartitionTable { .. }
        )
    }

    /// The disk ID this operation targets.
    pub fn disk_id(&self) -> u32 {
        match self {
            Self::CreatePartition { disk_id, .. }
            | Self::DeletePartition { disk_id, .. }
            | Self::ResizePartition { disk_id, .. }
            | Self::FormatPartition { disk_id, .. }
            | Self::SetLabel { disk_id, .. }
            | Self::SetFlag { disk_id, .. }
            | Self::SetMountPoint { disk_id, .. }
            | Self::CreatePartitionTable { disk_id, .. } => *disk_id,
        }
    }
}

// ============================================================================
// Dialog types
// ============================================================================

/// Confirmation dialog state.
#[derive(Clone, Debug)]
pub struct ConfirmDialog {
    /// Title text.
    pub title: String,
    /// Body message.
    pub message: String,
    /// Text on the confirm button.
    pub confirm_text: String,
    /// Whether this is a destructive action (styles the button red).
    pub destructive: bool,
    /// Which button is hovered (0=confirm, 1=cancel).
    pub hovered_button: Option<u32>,
}

impl ConfirmDialog {
    pub fn new(title: &str, message: &str, confirm_text: &str, destructive: bool) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            confirm_text: confirm_text.to_string(),
            destructive,
            hovered_button: None,
        }
    }
}

/// Create-partition dialog state.
#[derive(Clone, Debug)]
pub struct CreatePartitionDialog {
    /// The unallocated region start sector.
    pub start_sector: u64,
    /// The unallocated region end sector.
    pub end_sector: u64,
    /// Disk sector size.
    pub sector_size: u32,
    /// Selected filesystem.
    pub filesystem_index: usize,
    /// Label being typed.
    pub label: String,
    /// Size as percentage of available space (0..100).
    pub size_percent: u32,
    /// Hovered button.
    pub hovered_button: Option<u32>,
}

impl CreatePartitionDialog {
    pub fn new(start_sector: u64, end_sector: u64, sector_size: u32) -> Self {
        Self {
            start_sector,
            end_sector,
            sector_size,
            filesystem_index: 0,
            label: String::new(),
            size_percent: 100,
            hovered_button: None,
        }
    }

    /// Total available space in bytes.
    pub fn available_bytes(&self) -> u64 {
        let sectors = self.end_sector.saturating_sub(self.start_sector);
        sectors_to_bytes(sectors, self.sector_size)
    }

    /// Selected size in bytes based on percentage.
    pub fn selected_size_bytes(&self) -> u64 {
        let avail = self.available_bytes();
        (avail as f64 * self.size_percent as f64 / 100.0) as u64
    }

    /// Selected filesystem.
    pub fn selected_filesystem(&self) -> FilesystemType {
        let formattable = FilesystemType::formattable();
        formattable.get(self.filesystem_index).copied().unwrap_or(FilesystemType::Ext4)
    }

    /// Computed end sector for the partition.
    pub fn computed_end_sector(&self) -> u64 {
        let total_sectors = self.end_sector.saturating_sub(self.start_sector);
        let use_sectors = (total_sectors as f64 * self.size_percent as f64 / 100.0) as u64;
        self.start_sector.saturating_add(use_sectors)
    }
}

/// Format-partition dialog state.
#[derive(Clone, Debug)]
pub struct FormatDialog {
    pub partition_index: u32,
    pub partition_label: String,
    pub filesystem_index: usize,
    pub hovered_button: Option<u32>,
}

impl FormatDialog {
    pub fn new(partition_index: u32, partition_label: &str) -> Self {
        Self {
            partition_index,
            partition_label: partition_label.to_string(),
            filesystem_index: 0,
            hovered_button: None,
        }
    }

    pub fn selected_filesystem(&self) -> FilesystemType {
        let formattable = FilesystemType::formattable();
        formattable.get(self.filesystem_index).copied().unwrap_or(FilesystemType::Ext4)
    }
}

/// Active dialog.
#[derive(Clone, Debug)]
pub enum ActiveDialog {
    Confirm(ConfirmDialog),
    CreatePartition(CreatePartitionDialog),
    Format(FormatDialog),
    None,
}

impl ActiveDialog {
    pub fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }
}

// ============================================================================
// Selected item
// ============================================================================

/// What is currently selected in the main view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectedItem {
    /// A partition by index.
    Partition(u32),
    /// An unallocated gap by index in the regions list.
    Unallocated(usize),
    /// Nothing selected.
    None,
}

// ============================================================================
// Sample data generation
// ============================================================================

/// Generate sample disks with realistic partition layouts for development.
fn sample_disks() -> Vec<Disk> {
    vec![
        Disk {
            id: 0,
            name: String::from("/dev/sda"),
            model: String::from("Samsung 970 EVO Plus"),
            serial: String::from("S4EVNX0R712345"),
            total_size_bytes: 500_107_862_016,
            sector_size: 512,
            total_sectors: 976_773_168,
            table_type: PartitionTableType::Gpt,
            partitions: vec![
                Partition {
                    index: 1,
                    label: String::from("EFI"),
                    filesystem: FilesystemType::EfiSystem,
                    start_sector: 2048,
                    end_sector: 1_050_623,
                    size_bytes: 536_870_912,
                    flags: vec![PartitionFlag::Boot, PartitionFlag::Efi],
                    mount_point: Some(String::from("/boot/efi")),
                    uuid: String::from("A1B2-C3D4"),
                    used_bytes: Some(42_000_000),
                    free_bytes: Some(494_870_912),
                },
                Partition {
                    index: 2,
                    label: String::from("OurOS Root"),
                    filesystem: FilesystemType::Ext4,
                    start_sector: 1_050_624,
                    end_sector: 839_909_375,
                    size_bytes: 429_367_296_000,
                    flags: vec![],
                    mount_point: Some(String::from("/")),
                    uuid: String::from("f47ac10b-58cc-4372-a567-0e02b2c3d479"),
                    used_bytes: Some(128_000_000_000),
                    free_bytes: Some(301_367_296_000),
                },
                Partition {
                    index: 3,
                    label: String::from("swap"),
                    filesystem: FilesystemType::Swap,
                    start_sector: 839_909_376,
                    end_sector: 856_686_591,
                    size_bytes: 8_589_934_592,
                    flags: vec![PartitionFlag::Swap],
                    mount_point: None,
                    uuid: String::from("d4e5f6a7-b8c9-0d1e-2f3a-4b5c6d7e8f90"),
                    used_bytes: Some(2_000_000_000),
                    free_bytes: Some(6_589_934_592),
                },
            ],
            smart_health: SmartHealth::Healthy,
            temperature_c: Some(38),
        },
        Disk {
            id: 1,
            name: String::from("/dev/sdb"),
            model: String::from("WDC WD20EARX-00P"),
            serial: String::from("WD-WMAZA1234567"),
            total_size_bytes: 2_000_398_934_016,
            sector_size: 512,
            total_sectors: 3_907_029_168,
            table_type: PartitionTableType::Gpt,
            partitions: vec![
                Partition {
                    index: 1,
                    label: String::from("Data"),
                    filesystem: FilesystemType::Ntfs,
                    start_sector: 2048,
                    end_sector: 1_953_525_167,
                    size_bytes: 1_000_202_043_392,
                    flags: vec![],
                    mount_point: Some(String::from("/mnt/data")),
                    uuid: String::from("9A8B7C6D-5E4F-3A2B-1C0D-EEFF00112233"),
                    used_bytes: Some(620_000_000_000),
                    free_bytes: Some(380_202_043_392),
                },
                Partition {
                    index: 2,
                    label: String::from("Backup"),
                    filesystem: FilesystemType::Ext4,
                    start_sector: 1_953_525_168,
                    end_sector: 3_907_029_133,
                    size_bytes: 1_000_193_851_392,
                    flags: vec![],
                    mount_point: Some(String::from("/mnt/backup")),
                    uuid: String::from("abcdef01-2345-6789-abcd-ef0123456789"),
                    used_bytes: Some(450_000_000_000),
                    free_bytes: Some(550_193_851_392),
                },
            ],
            smart_health: SmartHealth::Healthy,
            temperature_c: Some(34),
        },
        Disk {
            id: 2,
            name: String::from("/dev/sdc"),
            model: String::from("Kingston A400 SSD"),
            serial: String::from("50026B7682A12345"),
            total_size_bytes: 240_057_409_536,
            sector_size: 512,
            total_sectors: 468_862_128,
            table_type: PartitionTableType::Mbr,
            partitions: vec![
                Partition {
                    index: 1,
                    label: String::from("Windows"),
                    filesystem: FilesystemType::Ntfs,
                    start_sector: 2048,
                    end_sector: 409_602_047,
                    size_bytes: 209_715_200_000,
                    flags: vec![PartitionFlag::Boot],
                    mount_point: None,
                    uuid: String::from("1234ABCD"),
                    used_bytes: Some(95_000_000_000),
                    free_bytes: Some(114_715_200_000),
                },
            ],
            smart_health: SmartHealth::Warning,
            temperature_c: Some(42),
        },
    ]
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state for the partition manager.
pub struct PartitionManagerApp {
    /// Window dimensions.
    pub width: f32,
    pub height: f32,
    /// All detected disks.
    pub disks: Vec<Disk>,
    /// Index of the currently selected disk.
    pub selected_disk: usize,
    /// Currently selected item (partition or unallocated gap).
    pub selected_item: SelectedItem,
    /// Queue of pending operations.
    pub operation_queue: Vec<PendingOperation>,
    /// Currently active dialog.
    pub dialog: ActiveDialog,
    /// Scroll offset for the partition list.
    pub partition_scroll: f32,
    /// Scroll offset for the operation queue.
    pub queue_scroll: f32,
    /// Hovered toolbar button index.
    pub hovered_toolbar_btn: Option<usize>,
    /// Hovered sidebar disk index.
    pub hovered_sidebar_disk: Option<usize>,
    /// Hovered partition region index in the disk map.
    pub hovered_map_region: Option<usize>,
    /// Hovered operation queue row.
    pub hovered_queue_row: Option<usize>,
    /// Whether the queue panel is expanded.
    pub queue_expanded: bool,
    /// Status bar message.
    pub status_message: String,
}

impl PartitionManagerApp {
    /// Create a new application with sample data.
    pub fn new() -> Self {
        Self {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            disks: sample_disks(),
            selected_disk: 0,
            selected_item: SelectedItem::None,
            operation_queue: Vec::new(),
            dialog: ActiveDialog::None,
            partition_scroll: 0.0,
            queue_scroll: 0.0,
            hovered_toolbar_btn: None,
            hovered_sidebar_disk: None,
            hovered_map_region: None,
            hovered_queue_row: None,
            queue_expanded: true,
            status_message: String::from("Ready"),
        }
    }

    /// Get a reference to the currently selected disk.
    pub fn current_disk(&self) -> Option<&Disk> {
        self.disks.get(self.selected_disk)
    }

    /// Get the currently selected partition (if any).
    pub fn selected_partition(&self) -> Option<&Partition> {
        let disk = self.current_disk()?;
        match &self.selected_item {
            SelectedItem::Partition(idx) => {
                disk.partitions.iter().find(|p| p.index == *idx)
            }
            _ => None,
        }
    }

    /// Whether there are any pending operations.
    pub fn has_pending_operations(&self) -> bool {
        !self.operation_queue.is_empty()
    }

    /// Whether any pending operation is destructive.
    pub fn has_destructive_operations(&self) -> bool {
        self.operation_queue.iter().any(|op| op.is_destructive())
    }

    /// Count of pending operations.
    pub fn pending_count(&self) -> usize {
        self.operation_queue.len()
    }

    /// Add an operation to the queue.
    pub fn enqueue_operation(&mut self, op: PendingOperation) {
        self.status_message = format!("Queued: {}", op.describe());
        self.operation_queue.push(op);
    }

    /// Remove the last queued operation (undo).
    pub fn undo_last_operation(&mut self) -> Option<PendingOperation> {
        let op = self.operation_queue.pop();
        if op.is_some() {
            self.status_message = String::from("Last operation removed from queue");
        }
        op
    }

    /// Clear all pending operations.
    pub fn clear_operations(&mut self) {
        self.operation_queue.clear();
        self.status_message = String::from("Operation queue cleared");
    }

    /// Apply all pending operations (stub -- in a real system this calls syscalls).
    pub fn apply_operations(&mut self) -> usize {
        let count = self.operation_queue.len();
        self.operation_queue.clear();
        self.status_message = format!("Applied {count} operation(s) successfully");
        count
    }

    /// Select a disk by index.
    pub fn select_disk(&mut self, index: usize) {
        if index < self.disks.len() {
            self.selected_disk = index;
            self.selected_item = SelectedItem::None;
            self.partition_scroll = 0.0;
        }
    }

    /// Toolbar button labels.
    pub fn toolbar_buttons(&self) -> Vec<(&'static str, bool)> {
        let has_selection = matches!(self.selected_item, SelectedItem::Partition(_));
        let has_unalloc = matches!(self.selected_item, SelectedItem::Unallocated(_));
        let has_ops = self.has_pending_operations();

        vec![
            ("New Table", true),
            ("Create", has_unalloc),
            ("Delete", has_selection),
            ("Resize", has_selection),
            ("Format", has_selection),
            ("Label", has_selection),
            ("Flags", has_selection),
            ("Mount", has_selection),
            ("Undo", has_ops),
            ("Apply", has_ops),
        ]
    }
}

// ============================================================================
// Rendering -- title bar
// ============================================================================

fn render_title_bar(tree: &mut RenderTree, width: f32) {
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height: TITLE_BAR_HEIGHT,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    tree.push(RenderCommand::Text {
        x: 12.0,
        y: 10.0,
        text: String::from("Partition Manager"),
        color: COLOR_TEXT,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Divider line at bottom of title bar
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: TITLE_BAR_HEIGHT,
        x2: width,
        y2: TITLE_BAR_HEIGHT,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
}

// ============================================================================
// Rendering -- toolbar
// ============================================================================

fn render_toolbar(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let y = TITLE_BAR_HEIGHT;

    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: app.width,
        height: TOOLBAR_HEIGHT,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let buttons = app.toolbar_buttons();
    let mut bx = 8.0;
    let by = y + (TOOLBAR_HEIGHT - TOOLBAR_BTN_HEIGHT) / 2.0;

    for (i, (label, enabled)) in buttons.iter().enumerate() {
        let hovered = app.hovered_toolbar_btn == Some(i);
        let bg = if !enabled {
            COLOR_SURFACE0
        } else if hovered {
            COLOR_SURFACE2
        } else {
            COLOR_SURFACE1
        };
        let fg = if *enabled { COLOR_TEXT } else { COLOR_OVERLAY0 };

        tree.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: TOOLBAR_BTN_WIDTH,
            height: TOOLBAR_BTN_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        tree.push(RenderCommand::Text {
            x: bx + 8.0,
            y: by + 7.0,
            text: String::from(*label),
            color: fg,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(TOOLBAR_BTN_WIDTH - 16.0),
        });

        bx += TOOLBAR_BTN_WIDTH + 4.0;
    }

    // Bottom divider
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: y + TOOLBAR_HEIGHT,
        x2: app.width,
        y2: y + TOOLBAR_HEIGHT,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
}

// ============================================================================
// Rendering -- sidebar (disk list)
// ============================================================================

fn render_sidebar(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let bottom = app.height - STATUS_BAR_HEIGHT;
    let sidebar_height = bottom - top;

    // Sidebar background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: top,
        width: SIDEBAR_WIDTH,
        height: sidebar_height,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // "Disks" header
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: top + 8.0,
        text: String::from("Disks"),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(SIDEBAR_WIDTH - 24.0),
    });

    let list_top = top + 28.0;

    // Clip sidebar content
    tree.push(RenderCommand::PushClip {
        x: 0.0,
        y: list_top,
        width: SIDEBAR_WIDTH,
        height: bottom - list_top,
    });

    for (i, disk) in app.disks.iter().enumerate() {
        let ry = list_top + (i as f32) * SIDEBAR_DISK_ROW_HEIGHT;
        let selected = i == app.selected_disk;
        let hovered = app.hovered_sidebar_disk == Some(i);

        let bg = if selected {
            COLOR_SURFACE1
        } else if hovered {
            COLOR_SURFACE0
        } else {
            COLOR_MANTLE
        };

        tree.push(RenderCommand::FillRect {
            x: 4.0,
            y: ry,
            width: SIDEBAR_WIDTH - 8.0,
            height: SIDEBAR_DISK_ROW_HEIGHT - 2.0,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        // Disk name
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: ry + 6.0,
            text: disk.name.clone(),
            color: if selected { COLOR_TEXT } else { COLOR_SUBTEXT1 },
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 24.0),
        });

        // Disk model + size
        let info = format!("{} - {}", disk.model, format_size(disk.total_size_bytes));
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: ry + 22.0,
            text: info,
            color: COLOR_SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 24.0),
        });

        // Health indicator dot
        let health_color = disk.smart_health.color();
        tree.push(RenderCommand::FillRect {
            x: SIDEBAR_WIDTH - 20.0,
            y: ry + 18.0,
            width: 8.0,
            height: 8.0,
            color: health_color,
            corner_radii: CornerRadii::all(4.0),
        });
    }

    tree.push(RenderCommand::PopClip);

    // Right border of sidebar
    tree.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH,
        y1: top,
        x2: SIDEBAR_WIDTH,
        y2: bottom,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
}

// ============================================================================
// Rendering -- disk map (visual partition bar)
// ============================================================================

fn render_disk_map(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let disk = match app.current_disk() {
        Some(d) => d,
        None => return,
    };

    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let map_x = SIDEBAR_WIDTH + DISK_MAP_PADDING;
    let map_y = top + DISK_MAP_PADDING;
    let available_width = app.width - SIDEBAR_WIDTH - DETAIL_PANEL_WIDTH - DISK_MAP_PADDING * 2.0;
    let bar_y = map_y + 20.0;

    // Section header
    tree.push(RenderCommand::Text {
        x: map_x,
        y: map_y,
        text: format!("Disk Layout - {} ({})", disk.name, format_size(disk.total_size_bytes)),
        color: COLOR_TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(available_width),
    });

    // Bar background
    tree.push(RenderCommand::FillRect {
        x: map_x,
        y: bar_y,
        width: available_width,
        height: DISK_MAP_BAR_HEIGHT,
        color: COLOR_BASE,
        corner_radii: CornerRadii::all(4.0),
    });

    let regions = disk.regions();
    let total_sectors = disk.total_sectors as f64;
    if total_sectors <= 0.0 {
        return;
    }

    let mut rx = map_x;
    for (i, region) in regions.iter().enumerate() {
        let sector_span = (region.end_sector().saturating_sub(region.start_sector())) as f64;
        let fraction = sector_span / total_sectors;
        let region_width = (fraction * available_width as f64) as f32;
        let clamped_width = region_width.max(MIN_PARTITION_BAR_WIDTH);

        let is_selected = match (&app.selected_item, region) {
            (SelectedItem::Partition(idx), DiskRegion::Partition(p)) => p.index == *idx,
            (SelectedItem::Unallocated(ui), DiskRegion::Unallocated(_)) => *ui == i,
            _ => false,
        };
        let is_hovered = app.hovered_map_region == Some(i);

        let base_color = match region {
            DiskRegion::Partition(p) => p.filesystem.color(),
            DiskRegion::Unallocated(_) => COLOR_SURFACE0,
        };

        // Lighten on hover, highlight on select
        let color = if is_selected {
            Color::rgba(
                base_color.r.saturating_add(40),
                base_color.g.saturating_add(40),
                base_color.b.saturating_add(40),
                255,
            )
        } else if is_hovered {
            Color::rgba(
                base_color.r.saturating_add(20),
                base_color.g.saturating_add(20),
                base_color.b.saturating_add(20),
                255,
            )
        } else {
            base_color
        };

        tree.push(RenderCommand::FillRect {
            x: rx + 1.0,
            y: bar_y + 2.0,
            width: (clamped_width - 2.0).max(1.0),
            height: DISK_MAP_BAR_HEIGHT - 4.0,
            color,
            corner_radii: CornerRadii::all(2.0),
        });

        // Selection border
        if is_selected {
            tree.push(RenderCommand::StrokeRect {
                x: rx + 1.0,
                y: bar_y + 2.0,
                width: (clamped_width - 2.0).max(1.0),
                height: DISK_MAP_BAR_HEIGHT - 4.0,
                color: COLOR_TEXT,
                line_width: 2.0,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Label inside region (if wide enough)
        if clamped_width > 50.0 {
            let label_text = match region {
                DiskRegion::Partition(p) => p.label.clone(),
                DiskRegion::Unallocated(_) => String::from("Free"),
            };
            tree.push(RenderCommand::Text {
                x: rx + 4.0,
                y: bar_y + 6.0,
                text: label_text,
                color: COLOR_BASE,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(clamped_width - 8.0),
            });

            let size_text = format_size(region.size_bytes());
            tree.push(RenderCommand::Text {
                x: rx + 4.0,
                y: bar_y + 20.0,
                text: size_text,
                color: COLOR_BASE,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(clamped_width - 8.0),
            });
        }

        rx += clamped_width;
    }

    // Legend below bar
    let legend_y = bar_y + DISK_MAP_BAR_HEIGHT + 8.0;
    let mut lx = map_x;
    let legend_items: &[(FilesystemType, &str)] = &[
        (FilesystemType::Ext4, "ext4"),
        (FilesystemType::Fat32, "FAT32"),
        (FilesystemType::Ntfs, "NTFS"),
        (FilesystemType::Swap, "swap"),
        (FilesystemType::EfiSystem, "EFI"),
    ];

    for (fs, label) in legend_items {
        tree.push(RenderCommand::FillRect {
            x: lx,
            y: legend_y,
            width: 10.0,
            height: 10.0,
            color: fs.color(),
            corner_radii: CornerRadii::all(2.0),
        });
        tree.push(RenderCommand::Text {
            x: lx + 14.0,
            y: legend_y,
            text: String::from(*label),
            color: COLOR_SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        lx += 70.0;
    }

    // "Unallocated" in legend
    tree.push(RenderCommand::FillRect {
        x: lx,
        y: legend_y,
        width: 10.0,
        height: 10.0,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::all(2.0),
    });
    tree.push(RenderCommand::Text {
        x: lx + 14.0,
        y: legend_y,
        text: String::from("Free"),
        color: COLOR_SUBTEXT0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

// ============================================================================
// Rendering -- partition list
// ============================================================================

fn render_partition_list(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let disk = match app.current_disk() {
        Some(d) => d,
        None => return,
    };

    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + DISK_MAP_HEIGHT + DISK_MAP_PADDING * 2.0 + 30.0;
    let left = SIDEBAR_WIDTH + DISK_MAP_PADDING;
    let list_width = app.width - SIDEBAR_WIDTH - DETAIL_PANEL_WIDTH - DISK_MAP_PADDING * 2.0;
    let queue_h = if app.queue_expanded { QUEUE_PANEL_HEIGHT } else { 28.0 };
    let bottom = app.height - STATUS_BAR_HEIGHT - queue_h;
    let list_height = bottom - top;

    // Section header
    tree.push(RenderCommand::Text {
        x: left,
        y: top,
        text: String::from("Partitions"),
        color: COLOR_TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(list_width),
    });

    let header_y = top + 18.0;

    // Column headers
    let cols: &[(&str, f32)] = &[
        ("#", 30.0),
        ("Label", 120.0),
        ("Filesystem", 80.0),
        ("Size", 90.0),
        ("Used", 90.0),
        ("Flags", 80.0),
        ("Mount", 100.0),
    ];
    let mut cx = left;
    for (col_name, col_w) in cols {
        tree.push(RenderCommand::Text {
            x: cx + 4.0,
            y: header_y + 4.0,
            text: String::from(*col_name),
            color: COLOR_SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(*col_w),
        });
        cx += col_w;
    }

    // Divider under header
    let data_top = header_y + PARTITION_ROW_HEIGHT;
    tree.push(RenderCommand::Line {
        x1: left,
        y1: data_top,
        x2: left + list_width,
        y2: data_top,
        color: COLOR_SURFACE1,
        width: 1.0,
    });

    // Clip partition list
    tree.push(RenderCommand::PushClip {
        x: left,
        y: data_top,
        width: list_width,
        height: (list_height - 20.0).max(0.0),
    });

    let regions = disk.regions();
    for (i, region) in regions.iter().enumerate() {
        let ry = data_top + (i as f32) * PARTITION_ROW_HEIGHT - app.partition_scroll;

        let is_selected = match (&app.selected_item, region) {
            (SelectedItem::Partition(idx), DiskRegion::Partition(p)) => p.index == *idx,
            (SelectedItem::Unallocated(ui), DiskRegion::Unallocated(_)) => *ui == i,
            _ => false,
        };

        let bg = if is_selected {
            COLOR_SURFACE1
        } else if i % 2 == 0 {
            COLOR_BASE
        } else {
            COLOR_SURFACE0
        };

        tree.push(RenderCommand::FillRect {
            x: left,
            y: ry,
            width: list_width,
            height: PARTITION_ROW_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::ZERO,
        });

        let mut col_x = left;
        match region {
            DiskRegion::Partition(p) => {
                let values: Vec<String> = vec![
                    format!("{}", p.index),
                    p.label.clone(),
                    String::from(p.filesystem.label()),
                    format_size(p.size_bytes),
                    p.used_percent().map_or(String::from("-"), |pct| format!("{pct}%")),
                    p.flags_string(),
                    p.mount_point.clone().unwrap_or_else(|| String::from("-")),
                ];
                for (j, val) in values.iter().enumerate() {
                    let (_, col_w) = cols.get(j).copied().unwrap_or(("", 80.0));
                    let text_color = if p.is_system() && (j == 1 || j == 5) {
                        COLOR_YELLOW
                    } else {
                        COLOR_TEXT
                    };
                    tree.push(RenderCommand::Text {
                        x: col_x + 4.0,
                        y: ry + 5.0,
                        text: val.clone(),
                        color: text_color,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(col_w - 8.0),
                    });
                    col_x += col_w;
                }
            }
            DiskRegion::Unallocated(u) => {
                // Index column: dash
                let (_, w0) = cols[0];
                tree.push(RenderCommand::Text {
                    x: col_x + 4.0,
                    y: ry + 5.0,
                    text: String::from("-"),
                    color: COLOR_OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w0 - 8.0),
                });
                col_x += w0;

                // Label: "Unallocated"
                let (_, w1) = cols[1];
                tree.push(RenderCommand::Text {
                    x: col_x + 4.0,
                    y: ry + 5.0,
                    text: String::from("Unallocated"),
                    color: COLOR_OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w1 - 8.0),
                });
                col_x += w1;

                // Filesystem: dash
                let (_, w2) = cols[2];
                tree.push(RenderCommand::Text {
                    x: col_x + 4.0,
                    y: ry + 5.0,
                    text: String::from("-"),
                    color: COLOR_OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w2 - 8.0),
                });
                col_x += w2;

                // Size
                let (_, w3) = cols[3];
                tree.push(RenderCommand::Text {
                    x: col_x + 4.0,
                    y: ry + 5.0,
                    text: format_size(u.size_bytes),
                    color: COLOR_OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w3 - 8.0),
                });
            }
        }
    }

    tree.push(RenderCommand::PopClip);
}

// ============================================================================
// Rendering -- detail panel (disk info + partition details)
// ============================================================================

fn render_detail_panel(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let disk = match app.current_disk() {
        Some(d) => d,
        None => return,
    };

    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let right = app.width;
    let panel_x = right - DETAIL_PANEL_WIDTH;
    let bottom = app.height - STATUS_BAR_HEIGHT;
    let panel_height = bottom - top;

    // Panel background
    tree.push(RenderCommand::FillRect {
        x: panel_x,
        y: top,
        width: DETAIL_PANEL_WIDTH,
        height: panel_height,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Left border
    tree.push(RenderCommand::Line {
        x1: panel_x,
        y1: top,
        x2: panel_x,
        y2: bottom,
        color: COLOR_SURFACE1,
        width: 1.0,
    });

    let mut py = top + 8.0;
    let px = panel_x + 12.0;
    let val_x = panel_x + 120.0;
    let text_w = DETAIL_PANEL_WIDTH - 24.0;

    // -- Disk Info Section --
    tree.push(RenderCommand::Text {
        x: px,
        y: py,
        text: String::from("Disk Information"),
        color: COLOR_BLUE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(text_w),
    });
    py += SECTION_HEADER_HEIGHT;

    let disk_props: Vec<(&str, String)> = vec![
        ("Model", disk.model.clone()),
        ("Serial", disk.serial.clone()),
        ("Size", format_size(disk.total_size_bytes)),
        ("Sector Size", format!("{} bytes", disk.sector_size)),
        ("Table Type", String::from(disk.table_type.label())),
        ("Partitions", format!("{}", disk.partition_count())),
        ("SMART", String::from(disk.smart_health.label())),
        ("Temperature", disk.temperature_c.map_or(
            String::from("N/A"),
            |t| format!("{t} C"),
        )),
    ];

    for (label, value) in &disk_props {
        tree.push(RenderCommand::Text {
            x: px,
            y: py,
            text: String::from(*label),
            color: COLOR_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });
        let val_color = if *label == "SMART" {
            disk.smart_health.color()
        } else {
            COLOR_TEXT
        };
        tree.push(RenderCommand::Text {
            x: val_x,
            y: py,
            text: value.clone(),
            color: val_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(DETAIL_PANEL_WIDTH - 132.0),
        });
        py += PROPERTY_ROW_HEIGHT;
    }

    py += 8.0;

    // -- Partition Details Section (if a partition is selected) --
    if let Some(part) = app.selected_partition() {
        tree.push(RenderCommand::Line {
            x1: panel_x + 8.0,
            y1: py,
            x2: panel_x + DETAIL_PANEL_WIDTH - 8.0,
            y2: py,
            color: COLOR_SURFACE1,
            width: 1.0,
        });
        py += 8.0;

        tree.push(RenderCommand::Text {
            x: px,
            y: py,
            text: String::from("Partition Details"),
            color: COLOR_BLUE,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(text_w),
        });
        py += SECTION_HEADER_HEIGHT;

        // Warning banner for system partitions
        if part.is_system() {
            tree.push(RenderCommand::FillRect {
                x: panel_x + 8.0,
                y: py,
                width: DETAIL_PANEL_WIDTH - 16.0,
                height: 22.0,
                color: Color::rgba(COLOR_YELLOW.r, COLOR_YELLOW.g, COLOR_YELLOW.b, 40),
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: px,
                y: py + 4.0,
                text: String::from("! System partition - modify with caution"),
                color: COLOR_YELLOW,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(text_w),
            });
            py += 28.0;
        }

        let part_props: Vec<(&str, String)> = vec![
            ("Label", part.label.clone()),
            ("UUID", part.uuid.clone()),
            ("Filesystem", String::from(part.filesystem.label())),
            ("Size", format_size(part.size_bytes)),
            ("Used", part.used_bytes.map_or(
                String::from("N/A"),
                |u| format!("{} ({}%)", format_size(u), part.used_percent().unwrap_or(0)),
            )),
            ("Free", part.free_bytes.map_or(
                String::from("N/A"),
                |f| format_size(f),
            )),
            ("Flags", part.flags_string()),
            ("Mount", part.mount_point.clone().unwrap_or_else(|| String::from("Not mounted"))),
            ("Start", format!("sector {}", part.start_sector)),
            ("End", format!("sector {}", part.end_sector)),
        ];

        for (label, value) in &part_props {
            tree.push(RenderCommand::Text {
                x: px,
                y: py,
                text: String::from(*label),
                color: COLOR_SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
            tree.push(RenderCommand::Text {
                x: val_x,
                y: py,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(DETAIL_PANEL_WIDTH - 132.0),
            });
            py += PROPERTY_ROW_HEIGHT;
        }

        // Usage bar
        if let Some(pct) = part.used_percent() {
            py += 4.0;
            let bar_w = DETAIL_PANEL_WIDTH - 24.0;
            let bar_h = 12.0;

            tree.push(RenderCommand::FillRect {
                x: px,
                y: py,
                width: bar_w,
                height: bar_h,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });

            let fill_w = (bar_w * pct as f32 / 100.0).max(0.0);
            let fill_color = if pct > 90 {
                COLOR_RED
            } else if pct > 70 {
                COLOR_YELLOW
            } else {
                COLOR_GREEN
            };

            tree.push(RenderCommand::FillRect {
                x: px,
                y: py,
                width: fill_w,
                height: bar_h,
                color: fill_color,
                corner_radii: CornerRadii::all(3.0),
            });
        }
    }
}

// ============================================================================
// Rendering -- operation queue panel
// ============================================================================

fn render_queue_panel(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let queue_h = if app.queue_expanded { QUEUE_PANEL_HEIGHT } else { 28.0 };
    let top = app.height - STATUS_BAR_HEIGHT - queue_h;
    let left = SIDEBAR_WIDTH;
    let panel_width = app.width - SIDEBAR_WIDTH;

    // Background
    tree.push(RenderCommand::FillRect {
        x: left,
        y: top,
        width: panel_width,
        height: queue_h,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    // Top border
    tree.push(RenderCommand::Line {
        x1: left,
        y1: top,
        x2: left + panel_width,
        y2: top,
        color: COLOR_SURFACE1,
        width: 1.0,
    });

    // Header
    let header_text = if app.operation_queue.is_empty() {
        String::from("Pending Operations (none)")
    } else {
        format!("Pending Operations ({})", app.operation_queue.len())
    };

    let expand_icon = if app.queue_expanded { "v" } else { ">" };
    tree.push(RenderCommand::Text {
        x: left + 12.0,
        y: top + 7.0,
        text: format!("{expand_icon} {header_text}"),
        color: COLOR_TEXT,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(panel_width - 24.0),
    });

    if !app.queue_expanded {
        return;
    }

    let list_top = top + 28.0;
    let list_h = queue_h - 28.0;

    tree.push(RenderCommand::PushClip {
        x: left,
        y: list_top,
        width: panel_width,
        height: list_h,
    });

    for (i, op) in app.operation_queue.iter().enumerate() {
        let ry = list_top + (i as f32) * QUEUE_ROW_HEIGHT - app.queue_scroll;
        let hovered = app.hovered_queue_row == Some(i);

        let bg = if hovered { COLOR_SURFACE1 } else { COLOR_SURFACE0 };
        tree.push(RenderCommand::FillRect {
            x: left,
            y: ry,
            width: panel_width,
            height: QUEUE_ROW_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::ZERO,
        });

        // Operation number
        tree.push(RenderCommand::Text {
            x: left + 12.0,
            y: ry + 4.0,
            text: format!("{}.", i + 1),
            color: COLOR_SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Destructive indicator
        let desc_color = if op.is_destructive() { COLOR_RED } else { COLOR_TEXT };

        tree.push(RenderCommand::Text {
            x: left + 36.0,
            y: ry + 4.0,
            text: op.describe(),
            color: desc_color,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_width - 48.0),
        });
    }

    tree.push(RenderCommand::PopClip);
}

// ============================================================================
// Rendering -- status bar
// ============================================================================

fn render_status_bar(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let y = app.height - STATUS_BAR_HEIGHT;

    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: app.width,
        height: STATUS_BAR_HEIGHT,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Top border
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: y,
        x2: app.width,
        y2: y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });

    // Status message
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: y + 6.0,
        text: app.status_message.clone(),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(app.width * 0.6),
    });

    // Pending ops count on right
    if !app.operation_queue.is_empty() {
        let count_text = format!("{} pending operation(s)", app.operation_queue.len());
        tree.push(RenderCommand::Text {
            x: app.width - 200.0,
            y: y + 6.0,
            text: count_text,
            color: COLOR_YELLOW,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
    }
}

// ============================================================================
// Rendering -- confirmation dialog
// ============================================================================

fn render_confirm_dialog(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let dialog = match &app.dialog {
        ActiveDialog::Confirm(d) => d,
        _ => return,
    };

    // Dim overlay
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: app.width,
        height: app.height,
        color: Color::rgba(0, 0, 0, 160),
        corner_radii: CornerRadii::ZERO,
    });

    let dx = (app.width - DIALOG_WIDTH) / 2.0;
    let dy = (app.height - DIALOG_HEIGHT) / 2.0;

    // Shadow
    tree.push(RenderCommand::BoxShadow {
        x: dx,
        y: dy,
        width: DIALOG_WIDTH,
        height: DIALOG_HEIGHT,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 2.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(8.0),
    });

    // Dialog background
    tree.push(RenderCommand::FillRect {
        x: dx,
        y: dy,
        width: DIALOG_WIDTH,
        height: DIALOG_HEIGHT,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Border
    tree.push(RenderCommand::StrokeRect {
        x: dx,
        y: dy,
        width: DIALOG_WIDTH,
        height: DIALOG_HEIGHT,
        color: COLOR_SURFACE2,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 20.0,
        text: dialog.title.clone(),
        color: if dialog.destructive { COLOR_RED } else { COLOR_TEXT },
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(DIALOG_WIDTH - 40.0),
    });

    // Message
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 50.0,
        text: dialog.message.clone(),
        color: COLOR_SUBTEXT1,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(DIALOG_WIDTH - 40.0),
    });

    // Warning icon area for destructive operations
    if dialog.destructive {
        tree.push(RenderCommand::FillRect {
            x: dx + 20.0,
            y: dy + 120.0,
            width: DIALOG_WIDTH - 40.0,
            height: 36.0,
            color: Color::rgba(COLOR_RED.r, COLOR_RED.g, COLOR_RED.b, 30),
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: dx + 32.0,
            y: dy + 130.0,
            text: String::from("WARNING: This action cannot be undone!"),
            color: COLOR_RED,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(DIALOG_WIDTH - 64.0),
        });
    }

    // Buttons
    let btn_y = dy + DIALOG_HEIGHT - 50.0;
    let confirm_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH * 2.0 - 30.0;
    let cancel_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH - 20.0;

    // Confirm button
    let confirm_hovered = dialog.hovered_button == Some(0);
    let confirm_bg = if dialog.destructive {
        if confirm_hovered { COLOR_RED } else { Color::rgba(COLOR_RED.r, COLOR_RED.g, COLOR_RED.b, 180) }
    } else if confirm_hovered {
        COLOR_BLUE
    } else {
        Color::rgba(COLOR_BLUE.r, COLOR_BLUE.g, COLOR_BLUE.b, 180)
    };

    tree.push(RenderCommand::FillRect {
        x: confirm_x,
        y: btn_y,
        width: DIALOG_BTN_WIDTH,
        height: DIALOG_BTN_HEIGHT,
        color: confirm_bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: confirm_x + 10.0,
        y: btn_y + 8.0,
        text: dialog.confirm_text.clone(),
        color: COLOR_BASE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(DIALOG_BTN_WIDTH - 20.0),
    });

    // Cancel button
    let cancel_hovered = dialog.hovered_button == Some(1);
    let cancel_bg = if cancel_hovered { COLOR_SURFACE2 } else { COLOR_SURFACE1 };

    tree.push(RenderCommand::FillRect {
        x: cancel_x,
        y: btn_y,
        width: DIALOG_BTN_WIDTH,
        height: DIALOG_BTN_HEIGHT,
        color: cancel_bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: cancel_x + 20.0,
        y: btn_y + 8.0,
        text: String::from("Cancel"),
        color: COLOR_TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(DIALOG_BTN_WIDTH - 20.0),
    });
}

// ============================================================================
// Rendering -- create-partition dialog
// ============================================================================

fn render_create_partition_dialog(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let dialog = match &app.dialog {
        ActiveDialog::CreatePartition(d) => d,
        _ => return,
    };

    // Dim overlay
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: app.width,
        height: app.height,
        color: Color::rgba(0, 0, 0, 160),
        corner_radii: CornerRadii::ZERO,
    });

    let dw = DIALOG_WIDTH + 40.0;
    let dh = DIALOG_HEIGHT + 40.0;
    let dx = (app.width - dw) / 2.0;
    let dy = (app.height - dh) / 2.0;

    // Shadow
    tree.push(RenderCommand::BoxShadow {
        x: dx,
        y: dy,
        width: dw,
        height: dh,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 2.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(8.0),
    });

    // Background
    tree.push(RenderCommand::FillRect {
        x: dx,
        y: dy,
        width: dw,
        height: dh,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::all(8.0),
    });

    tree.push(RenderCommand::StrokeRect {
        x: dx,
        y: dy,
        width: dw,
        height: dh,
        color: COLOR_SURFACE2,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 20.0,
        text: String::from("Create Partition"),
        color: COLOR_TEXT,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(dw - 40.0),
    });

    // Available space info
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 45.0,
        text: format!("Available space: {}", format_size(dialog.available_bytes())),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(dw - 40.0),
    });

    // Filesystem selector
    let mut fy = dy + 75.0;
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: fy,
        text: String::from("Filesystem:"),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
    fy += 18.0;

    let formattable = FilesystemType::formattable();
    let mut fx = dx + 20.0;
    for (i, fs) in formattable.iter().enumerate() {
        let selected = i == dialog.filesystem_index;
        let bg = if selected { fs.color() } else { COLOR_SURFACE1 };
        let fg = if selected { COLOR_BASE } else { COLOR_TEXT };
        let btn_w = 70.0;

        tree.push(RenderCommand::FillRect {
            x: fx,
            y: fy,
            width: btn_w,
            height: 24.0,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: fx + 8.0,
            y: fy + 5.0,
            text: String::from(fs.label()),
            color: fg,
            font_size: 10.0,
            font_weight: if selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
            max_width: Some(btn_w - 16.0),
        });
        fx += btn_w + 4.0;
    }

    // Label field
    fy += 36.0;
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: fy,
        text: String::from("Label:"),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
    fy += 18.0;

    tree.push(RenderCommand::FillRect {
        x: dx + 20.0,
        y: fy,
        width: dw - 40.0,
        height: 26.0,
        color: COLOR_BASE,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x: dx + 20.0,
        y: fy,
        width: dw - 40.0,
        height: 26.0,
        color: COLOR_SURFACE2,
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    let label_display = if dialog.label.is_empty() {
        String::from("Enter label...")
    } else {
        dialog.label.clone()
    };
    let label_color = if dialog.label.is_empty() { COLOR_OVERLAY0 } else { COLOR_TEXT };
    tree.push(RenderCommand::Text {
        x: dx + 28.0,
        y: fy + 6.0,
        text: label_display,
        color: label_color,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(dw - 56.0),
    });

    // Size slider representation
    fy += 38.0;
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: fy,
        text: format!("Size: {} ({}%)", format_size(dialog.selected_size_bytes()), dialog.size_percent),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(dw - 40.0),
    });
    fy += 18.0;

    // Size bar
    let bar_w = dw - 40.0;
    tree.push(RenderCommand::FillRect {
        x: dx + 20.0,
        y: fy,
        width: bar_w,
        height: 10.0,
        color: COLOR_SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });
    let fill_w = bar_w * dialog.size_percent as f32 / 100.0;
    tree.push(RenderCommand::FillRect {
        x: dx + 20.0,
        y: fy,
        width: fill_w,
        height: 10.0,
        color: COLOR_BLUE,
        corner_radii: CornerRadii::all(3.0),
    });

    // Buttons
    let btn_y = dy + dh - 50.0;
    let create_x = dx + dw - DIALOG_BTN_WIDTH * 2.0 - 30.0;
    let cancel_x = dx + dw - DIALOG_BTN_WIDTH - 20.0;

    let create_hovered = dialog.hovered_button == Some(0);
    let create_bg = if create_hovered { COLOR_BLUE } else {
        Color::rgba(COLOR_BLUE.r, COLOR_BLUE.g, COLOR_BLUE.b, 180)
    };

    tree.push(RenderCommand::FillRect {
        x: create_x,
        y: btn_y,
        width: DIALOG_BTN_WIDTH,
        height: DIALOG_BTN_HEIGHT,
        color: create_bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: create_x + 18.0,
        y: btn_y + 8.0,
        text: String::from("Create"),
        color: COLOR_BASE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(DIALOG_BTN_WIDTH - 20.0),
    });

    let cancel_hovered = dialog.hovered_button == Some(1);
    let cancel_bg = if cancel_hovered { COLOR_SURFACE2 } else { COLOR_SURFACE1 };

    tree.push(RenderCommand::FillRect {
        x: cancel_x,
        y: btn_y,
        width: DIALOG_BTN_WIDTH,
        height: DIALOG_BTN_HEIGHT,
        color: cancel_bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: cancel_x + 20.0,
        y: btn_y + 8.0,
        text: String::from("Cancel"),
        color: COLOR_TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(DIALOG_BTN_WIDTH - 20.0),
    });
}

// ============================================================================
// Rendering -- format dialog
// ============================================================================

fn render_format_dialog(tree: &mut RenderTree, app: &PartitionManagerApp) {
    let dialog = match &app.dialog {
        ActiveDialog::Format(d) => d,
        _ => return,
    };

    // Dim overlay
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: app.width,
        height: app.height,
        color: Color::rgba(0, 0, 0, 160),
        corner_radii: CornerRadii::ZERO,
    });

    let dx = (app.width - DIALOG_WIDTH) / 2.0;
    let dy = (app.height - DIALOG_HEIGHT) / 2.0;

    // Shadow
    tree.push(RenderCommand::BoxShadow {
        x: dx,
        y: dy,
        width: DIALOG_WIDTH,
        height: DIALOG_HEIGHT,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 2.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(8.0),
    });

    // Background
    tree.push(RenderCommand::FillRect {
        x: dx,
        y: dy,
        width: DIALOG_WIDTH,
        height: DIALOG_HEIGHT,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::all(8.0),
    });

    tree.push(RenderCommand::StrokeRect {
        x: dx,
        y: dy,
        width: DIALOG_WIDTH,
        height: DIALOG_HEIGHT,
        color: COLOR_SURFACE2,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 20.0,
        text: format!("Format Partition {} (\"{}\")", dialog.partition_index, dialog.partition_label),
        color: COLOR_RED,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(DIALOG_WIDTH - 40.0),
    });

    // Warning
    tree.push(RenderCommand::FillRect {
        x: dx + 20.0,
        y: dy + 50.0,
        width: DIALOG_WIDTH - 40.0,
        height: 36.0,
        color: Color::rgba(COLOR_RED.r, COLOR_RED.g, COLOR_RED.b, 30),
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: dx + 32.0,
        y: dy + 60.0,
        text: String::from("All data on this partition will be erased!"),
        color: COLOR_RED,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(DIALOG_WIDTH - 64.0),
    });

    // Filesystem selector
    tree.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 100.0,
        text: String::from("New filesystem:"),
        color: COLOR_SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    let formattable = FilesystemType::formattable();
    let mut fx = dx + 20.0;
    let fs_y = dy + 120.0;
    for (i, fs) in formattable.iter().enumerate() {
        let selected = i == dialog.filesystem_index;
        let bg = if selected { fs.color() } else { COLOR_SURFACE1 };
        let fg = if selected { COLOR_BASE } else { COLOR_TEXT };
        let btn_w = 70.0;

        tree.push(RenderCommand::FillRect {
            x: fx,
            y: fs_y,
            width: btn_w,
            height: 24.0,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: fx + 8.0,
            y: fs_y + 5.0,
            text: String::from(fs.label()),
            color: fg,
            font_size: 10.0,
            font_weight: if selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
            max_width: Some(btn_w - 16.0),
        });
        fx += btn_w + 4.0;
    }

    // Buttons
    let btn_y = dy + DIALOG_HEIGHT - 50.0;
    let format_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH * 2.0 - 30.0;
    let cancel_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH - 20.0;

    let format_hovered = dialog.hovered_button == Some(0);
    let format_bg = if format_hovered { COLOR_RED } else {
        Color::rgba(COLOR_RED.r, COLOR_RED.g, COLOR_RED.b, 180)
    };

    tree.push(RenderCommand::FillRect {
        x: format_x,
        y: btn_y,
        width: DIALOG_BTN_WIDTH,
        height: DIALOG_BTN_HEIGHT,
        color: format_bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: format_x + 18.0,
        y: btn_y + 8.0,
        text: String::from("Format"),
        color: COLOR_BASE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(DIALOG_BTN_WIDTH - 20.0),
    });

    let cancel_hovered = dialog.hovered_button == Some(1);
    let cancel_bg = if cancel_hovered { COLOR_SURFACE2 } else { COLOR_SURFACE1 };

    tree.push(RenderCommand::FillRect {
        x: cancel_x,
        y: btn_y,
        width: DIALOG_BTN_WIDTH,
        height: DIALOG_BTN_HEIGHT,
        color: cancel_bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: cancel_x + 20.0,
        y: btn_y + 8.0,
        text: String::from("Cancel"),
        color: COLOR_TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(DIALOG_BTN_WIDTH - 20.0),
    });
}

// ============================================================================
// Full render
// ============================================================================

/// Render the entire application into a `RenderTree`.
pub fn render(app: &PartitionManagerApp) -> RenderTree {
    let mut tree = RenderTree::new();

    // Window background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: app.width,
        height: app.height,
        color: COLOR_BASE,
        corner_radii: CornerRadii::ZERO,
    });

    render_title_bar(&mut tree, app.width);
    render_toolbar(&mut tree, app);
    render_sidebar(&mut tree, app);
    render_disk_map(&mut tree, app);
    render_partition_list(&mut tree, app);
    render_detail_panel(&mut tree, app);
    render_queue_panel(&mut tree, app);
    render_status_bar(&mut tree, app);

    // Dialogs (overlay)
    match &app.dialog {
        ActiveDialog::Confirm(_) => render_confirm_dialog(&mut tree, app),
        ActiveDialog::CreatePartition(_) => render_create_partition_dialog(&mut tree, app),
        ActiveDialog::Format(_) => render_format_dialog(&mut tree, app),
        ActiveDialog::None => {}
    }

    tree
}

// ============================================================================
// Event handling
// ============================================================================

/// Handle an input event and return whether it was consumed.
pub fn handle_event(app: &mut PartitionManagerApp, event: &Event) -> EventResult {
    match event {
        Event::Resize { width, height } => {
            app.width = *width as f32;
            app.height = *height as f32;
            EventResult::Consumed
        }
        Event::Mouse(mouse) => handle_mouse(app, mouse),
        Event::Key(key_ev) => handle_key(app, key_ev),
        _ => EventResult::Ignored,
    }
}

fn handle_mouse(
    app: &mut PartitionManagerApp,
    mouse: &guitk::event::MouseEvent,
) -> EventResult {
    let x = mouse.x;
    let y = mouse.y;

    // If a dialog is open, route all mouse events to it
    if app.dialog.is_open() {
        return handle_dialog_mouse(app, mouse);
    }

    match &mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            handle_left_click(app, x, y)
        }
        MouseEventKind::Move => {
            handle_mouse_move(app, x, y)
        }
        MouseEventKind::Scroll { dy, .. } => {
            handle_scroll(app, x, y, *dy)
        }
        _ => EventResult::Ignored,
    }
}

fn handle_left_click(app: &mut PartitionManagerApp, x: f32, y: f32) -> EventResult {
    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let bottom = app.height - STATUS_BAR_HEIGHT;

    // Toolbar click
    if y >= TITLE_BAR_HEIGHT && y < top {
        return handle_toolbar_click(app, x);
    }

    // Sidebar click
    if x < SIDEBAR_WIDTH && y >= top && y < bottom {
        let list_top = top + 28.0;
        if y >= list_top {
            let row = ((y - list_top) / SIDEBAR_DISK_ROW_HEIGHT) as usize;
            if row < app.disks.len() {
                app.select_disk(row);
                return EventResult::Consumed;
            }
        }
        return EventResult::Consumed;
    }

    // Disk map click
    let map_y_start = top + DISK_MAP_PADDING + 20.0;
    let map_y_end = map_y_start + DISK_MAP_BAR_HEIGHT;
    let map_x_start = SIDEBAR_WIDTH + DISK_MAP_PADDING;
    let map_x_end = app.width - DETAIL_PANEL_WIDTH - DISK_MAP_PADDING;

    if y >= map_y_start && y < map_y_end && x >= map_x_start && x < map_x_end {
        return handle_map_click(app, x, map_x_start, map_x_end);
    }

    // Partition list click
    let list_top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + DISK_MAP_HEIGHT + DISK_MAP_PADDING * 2.0 + 30.0 + PARTITION_ROW_HEIGHT + 18.0;
    let queue_h = if app.queue_expanded { QUEUE_PANEL_HEIGHT } else { 28.0 };
    let list_bottom = app.height - STATUS_BAR_HEIGHT - queue_h;

    if y >= list_top && y < list_bottom && x >= SIDEBAR_WIDTH + DISK_MAP_PADDING && x < app.width - DETAIL_PANEL_WIDTH {
        return handle_partition_list_click(app, y, list_top);
    }

    // Queue panel toggle
    let queue_top = app.height - STATUS_BAR_HEIGHT - queue_h;
    if y >= queue_top && y < queue_top + 28.0 && x >= SIDEBAR_WIDTH {
        app.queue_expanded = !app.queue_expanded;
        return EventResult::Consumed;
    }

    EventResult::Ignored
}

fn handle_toolbar_click(app: &mut PartitionManagerApp, x: f32) -> EventResult {
    let buttons = app.toolbar_buttons();
    let mut bx = 8.0;

    for (i, (_, enabled)) in buttons.iter().enumerate() {
        if x >= bx && x < bx + TOOLBAR_BTN_WIDTH && *enabled {
            return execute_toolbar_action(app, i);
        }
        bx += TOOLBAR_BTN_WIDTH + 4.0;
    }
    EventResult::Ignored
}

fn execute_toolbar_action(app: &mut PartitionManagerApp, action: usize) -> EventResult {
    let disk_id = app.current_disk().map(|d| d.id).unwrap_or(0);

    match action {
        0 => {
            // New Table
            app.dialog = ActiveDialog::Confirm(ConfirmDialog::new(
                "Create New Partition Table",
                "This will destroy ALL data on the disk. Choose GPT (default) or MBR.",
                "Create GPT",
                true,
            ));
        }
        1 => {
            // Create partition (in unallocated space)
            if let SelectedItem::Unallocated(idx) = &app.selected_item {
                let regions = app.current_disk().map(|d| d.regions()).unwrap_or_default();
                if let Some(DiskRegion::Unallocated(u)) = regions.get(*idx) {
                    let sector_size = app.current_disk().map(|d| d.sector_size).unwrap_or(512);
                    app.dialog = ActiveDialog::CreatePartition(
                        CreatePartitionDialog::new(u.start_sector, u.end_sector, sector_size),
                    );
                }
            }
        }
        2 => {
            // Delete partition
            if let SelectedItem::Partition(idx) = &app.selected_item {
                let label = app.selected_partition()
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                let is_sys = app.selected_partition()
                    .map(|p| p.is_system())
                    .unwrap_or(false);
                let msg = if is_sys {
                    format!("Delete SYSTEM partition {} (\"{}\")? This is extremely dangerous!", idx, label)
                } else {
                    format!("Delete partition {} (\"{}\")?", idx, label)
                };
                app.dialog = ActiveDialog::Confirm(ConfirmDialog::new(
                    "Delete Partition",
                    &msg,
                    "Delete",
                    true,
                ));
            }
        }
        3 => {
            // Resize
            if let SelectedItem::Partition(idx) = &app.selected_item {
                app.status_message = format!("Resize partition {} (drag edges in disk map)", idx);
            }
        }
        4 => {
            // Format
            if let SelectedItem::Partition(idx) = &app.selected_item {
                let label = app.selected_partition()
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                app.dialog = ActiveDialog::Format(FormatDialog::new(*idx, &label));
            }
        }
        5 => {
            // Set Label
            if let SelectedItem::Partition(idx) = &app.selected_item {
                app.enqueue_operation(PendingOperation::SetLabel {
                    disk_id,
                    partition_index: *idx,
                    new_label: String::from("NewLabel"),
                });
            }
        }
        6 => {
            // Set Flags
            if let SelectedItem::Partition(idx) = &app.selected_item {
                app.enqueue_operation(PendingOperation::SetFlag {
                    disk_id,
                    partition_index: *idx,
                    flag: PartitionFlag::Boot,
                    enabled: true,
                });
            }
        }
        7 => {
            // Mount
            if let SelectedItem::Partition(idx) = &app.selected_item {
                app.enqueue_operation(PendingOperation::SetMountPoint {
                    disk_id,
                    partition_index: *idx,
                    mount_point: Some(String::from("/mnt/new")),
                });
            }
        }
        8 => {
            // Undo
            app.undo_last_operation();
        }
        9 => {
            // Apply
            if app.has_destructive_operations() {
                let count = app.pending_count();
                app.dialog = ActiveDialog::Confirm(ConfirmDialog::new(
                    "Apply Operations",
                    &format!("Apply {} pending operation(s)? Some are destructive.", count),
                    "Apply All",
                    true,
                ));
            } else {
                app.apply_operations();
            }
        }
        _ => {}
    }
    EventResult::Consumed
}

fn handle_map_click(
    app: &mut PartitionManagerApp,
    x: f32,
    map_start: f32,
    map_end: f32,
) -> EventResult {
    let disk = match app.current_disk() {
        Some(d) => d,
        None => return EventResult::Ignored,
    };

    let regions = disk.regions();
    let total_sectors = disk.total_sectors as f64;
    if total_sectors <= 0.0 {
        return EventResult::Ignored;
    }

    let available_width = (map_end - map_start) as f64;
    let mut rx = map_start as f64;

    for (i, region) in regions.iter().enumerate() {
        let sector_span = (region.end_sector().saturating_sub(region.start_sector())) as f64;
        let fraction = sector_span / total_sectors;
        let region_width = (fraction * available_width).max(MIN_PARTITION_BAR_WIDTH as f64);

        if (x as f64) >= rx && (x as f64) < rx + region_width {
            app.selected_item = match region {
                DiskRegion::Partition(p) => SelectedItem::Partition(p.index),
                DiskRegion::Unallocated(_) => SelectedItem::Unallocated(i),
            };
            return EventResult::Consumed;
        }
        rx += region_width;
    }
    EventResult::Ignored
}

fn handle_partition_list_click(
    app: &mut PartitionManagerApp,
    y: f32,
    list_top: f32,
) -> EventResult {
    let disk = match app.current_disk() {
        Some(d) => d,
        None => return EventResult::Ignored,
    };

    let regions = disk.regions();
    let row = ((y - list_top + app.partition_scroll) / PARTITION_ROW_HEIGHT) as usize;

    if let Some(region) = regions.get(row) {
        app.selected_item = match region {
            DiskRegion::Partition(p) => SelectedItem::Partition(p.index),
            DiskRegion::Unallocated(_) => SelectedItem::Unallocated(row),
        };
        return EventResult::Consumed;
    }
    EventResult::Ignored
}

fn handle_mouse_move(app: &mut PartitionManagerApp, x: f32, y: f32) -> EventResult {
    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let bottom = app.height - STATUS_BAR_HEIGHT;

    // Reset hovers
    app.hovered_toolbar_btn = None;
    app.hovered_sidebar_disk = None;
    app.hovered_map_region = None;
    app.hovered_queue_row = None;

    // Toolbar hover
    if y >= TITLE_BAR_HEIGHT && y < top {
        let mut bx = 8.0;
        let buttons = app.toolbar_buttons();
        for (i, _) in buttons.iter().enumerate() {
            if x >= bx && x < bx + TOOLBAR_BTN_WIDTH {
                app.hovered_toolbar_btn = Some(i);
                break;
            }
            bx += TOOLBAR_BTN_WIDTH + 4.0;
        }
        return EventResult::Consumed;
    }

    // Sidebar hover
    if x < SIDEBAR_WIDTH && y >= top && y < bottom {
        let list_top = top + 28.0;
        if y >= list_top {
            let row = ((y - list_top) / SIDEBAR_DISK_ROW_HEIGHT) as usize;
            if row < app.disks.len() {
                app.hovered_sidebar_disk = Some(row);
            }
        }
        return EventResult::Consumed;
    }

    // Map hover
    let map_y_start = top + DISK_MAP_PADDING + 20.0;
    let map_y_end = map_y_start + DISK_MAP_BAR_HEIGHT;
    if y >= map_y_start && y < map_y_end {
        let map_x_start = SIDEBAR_WIDTH + DISK_MAP_PADDING;
        let map_x_end = app.width - DETAIL_PANEL_WIDTH - DISK_MAP_PADDING;
        if x >= map_x_start && x < map_x_end {
            if let Some(disk) = app.current_disk() {
                let regions = disk.regions();
                let total_sectors = disk.total_sectors as f64;
                if total_sectors > 0.0 {
                    let available_width = (map_x_end - map_x_start) as f64;
                    let mut rx = map_x_start as f64;
                    for (i, region) in regions.iter().enumerate() {
                        let sector_span =
                            (region.end_sector().saturating_sub(region.start_sector())) as f64;
                        let fraction = sector_span / total_sectors;
                        let region_width =
                            (fraction * available_width).max(MIN_PARTITION_BAR_WIDTH as f64);
                        if (x as f64) >= rx && (x as f64) < rx + region_width {
                            app.hovered_map_region = Some(i);
                            break;
                        }
                        rx += region_width;
                    }
                }
            }
        }
        return EventResult::Consumed;
    }

    // Queue hover
    let queue_h = if app.queue_expanded { QUEUE_PANEL_HEIGHT } else { 28.0 };
    let queue_top = app.height - STATUS_BAR_HEIGHT - queue_h;
    if y >= queue_top + 28.0 && y < app.height - STATUS_BAR_HEIGHT && x >= SIDEBAR_WIDTH {
        if app.queue_expanded {
            let row = ((y - queue_top - 28.0 + app.queue_scroll) / QUEUE_ROW_HEIGHT) as usize;
            if row < app.operation_queue.len() {
                app.hovered_queue_row = Some(row);
            }
        }
        return EventResult::Consumed;
    }

    EventResult::Ignored
}

fn handle_scroll(app: &mut PartitionManagerApp, x: f32, y: f32, dy: f32) -> EventResult {
    let queue_h = if app.queue_expanded { QUEUE_PANEL_HEIGHT } else { 28.0 };
    let queue_top = app.height - STATUS_BAR_HEIGHT - queue_h;

    // Queue panel scroll
    if y >= queue_top && y < app.height - STATUS_BAR_HEIGHT && x >= SIDEBAR_WIDTH {
        let max_scroll = (app.operation_queue.len() as f32 * QUEUE_ROW_HEIGHT
            - (queue_h - 28.0))
            .max(0.0);
        app.queue_scroll = (app.queue_scroll - dy * 20.0).clamp(0.0, max_scroll);
        return EventResult::Consumed;
    }

    // Partition list scroll
    let list_top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + DISK_MAP_HEIGHT + DISK_MAP_PADDING * 2.0 + 30.0;
    if y >= list_top && y < queue_top && x >= SIDEBAR_WIDTH && x < app.width - DETAIL_PANEL_WIDTH {
        let region_count = app.current_disk().map(|d| d.regions().len()).unwrap_or(0);
        let max_scroll = (region_count as f32 * PARTITION_ROW_HEIGHT - (queue_top - list_top - 40.0))
            .max(0.0);
        app.partition_scroll = (app.partition_scroll - dy * 20.0).clamp(0.0, max_scroll);
        return EventResult::Consumed;
    }

    EventResult::Ignored
}

fn handle_dialog_mouse(
    app: &mut PartitionManagerApp,
    mouse: &guitk::event::MouseEvent,
) -> EventResult {
    let x = mouse.x;
    let y = mouse.y;

    match &mut app.dialog {
        ActiveDialog::Confirm(dialog) => {
            let dx = (app.width - DIALOG_WIDTH) / 2.0;
            let dy_base = (app.height - DIALOG_HEIGHT) / 2.0;
            let btn_y = dy_base + DIALOG_HEIGHT - 50.0;
            let confirm_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH * 2.0 - 30.0;
            let cancel_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH - 20.0;

            match &mouse.kind {
                MouseEventKind::Move => {
                    dialog.hovered_button = None;
                    if y >= btn_y && y < btn_y + DIALOG_BTN_HEIGHT {
                        if x >= confirm_x && x < confirm_x + DIALOG_BTN_WIDTH {
                            dialog.hovered_button = Some(0);
                        } else if x >= cancel_x && x < cancel_x + DIALOG_BTN_WIDTH {
                            dialog.hovered_button = Some(1);
                        }
                    }
                }
                MouseEventKind::Press(MouseButton::Left) => {
                    if y >= btn_y && y < btn_y + DIALOG_BTN_HEIGHT {
                        if x >= confirm_x && x < confirm_x + DIALOG_BTN_WIDTH {
                            // Confirmed -- perform the action
                            handle_confirm_accepted(app);
                            return EventResult::Consumed;
                        } else if x >= cancel_x && x < cancel_x + DIALOG_BTN_WIDTH {
                            app.dialog = ActiveDialog::None;
                            return EventResult::Consumed;
                        }
                    }
                }
                _ => {}
            }
        }
        ActiveDialog::CreatePartition(dialog) => {
            let dw = DIALOG_WIDTH + 40.0;
            let dh = DIALOG_HEIGHT + 40.0;
            let dx = (app.width - dw) / 2.0;
            let dy_base = (app.height - dh) / 2.0;
            let btn_y = dy_base + dh - 50.0;
            let create_x = dx + dw - DIALOG_BTN_WIDTH * 2.0 - 30.0;
            let cancel_x = dx + dw - DIALOG_BTN_WIDTH - 20.0;

            match &mouse.kind {
                MouseEventKind::Move => {
                    dialog.hovered_button = None;
                    if y >= btn_y && y < btn_y + DIALOG_BTN_HEIGHT {
                        if x >= create_x && x < create_x + DIALOG_BTN_WIDTH {
                            dialog.hovered_button = Some(0);
                        } else if x >= cancel_x && x < cancel_x + DIALOG_BTN_WIDTH {
                            dialog.hovered_button = Some(1);
                        }
                    }
                }
                MouseEventKind::Press(MouseButton::Left) => {
                    // Filesystem selector click
                    let fs_y = dy_base + 75.0 + 18.0;
                    if y >= fs_y && y < fs_y + 24.0 {
                        let formattable = FilesystemType::formattable();
                        let mut fx = dx + 20.0;
                        for (i, _) in formattable.iter().enumerate() {
                            if x >= fx && x < fx + 70.0 {
                                dialog.filesystem_index = i;
                                return EventResult::Consumed;
                            }
                            fx += 74.0;
                        }
                    }

                    // Buttons
                    if y >= btn_y && y < btn_y + DIALOG_BTN_HEIGHT {
                        if x >= create_x && x < create_x + DIALOG_BTN_WIDTH {
                            handle_create_partition_accepted(app);
                            return EventResult::Consumed;
                        } else if x >= cancel_x && x < cancel_x + DIALOG_BTN_WIDTH {
                            app.dialog = ActiveDialog::None;
                            return EventResult::Consumed;
                        }
                    }
                }
                _ => {}
            }
        }
        ActiveDialog::Format(dialog) => {
            let dx = (app.width - DIALOG_WIDTH) / 2.0;
            let dy_base = (app.height - DIALOG_HEIGHT) / 2.0;
            let btn_y = dy_base + DIALOG_HEIGHT - 50.0;
            let format_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH * 2.0 - 30.0;
            let cancel_x = dx + DIALOG_WIDTH - DIALOG_BTN_WIDTH - 20.0;

            match &mouse.kind {
                MouseEventKind::Move => {
                    dialog.hovered_button = None;
                    if y >= btn_y && y < btn_y + DIALOG_BTN_HEIGHT {
                        if x >= format_x && x < format_x + DIALOG_BTN_WIDTH {
                            dialog.hovered_button = Some(0);
                        } else if x >= cancel_x && x < cancel_x + DIALOG_BTN_WIDTH {
                            dialog.hovered_button = Some(1);
                        }
                    }
                }
                MouseEventKind::Press(MouseButton::Left) => {
                    // Filesystem selector click
                    let fs_y = dy_base + 120.0;
                    if y >= fs_y && y < fs_y + 24.0 {
                        let formattable = FilesystemType::formattable();
                        let mut fx = dx + 20.0;
                        for (i, _) in formattable.iter().enumerate() {
                            if x >= fx && x < fx + 70.0 {
                                dialog.filesystem_index = i;
                                return EventResult::Consumed;
                            }
                            fx += 74.0;
                        }
                    }

                    // Buttons
                    if y >= btn_y && y < btn_y + DIALOG_BTN_HEIGHT {
                        if x >= format_x && x < format_x + DIALOG_BTN_WIDTH {
                            handle_format_accepted(app);
                            return EventResult::Consumed;
                        } else if x >= cancel_x && x < cancel_x + DIALOG_BTN_WIDTH {
                            app.dialog = ActiveDialog::None;
                            return EventResult::Consumed;
                        }
                    }
                }
                _ => {}
            }
        }
        ActiveDialog::None => {}
    }
    EventResult::Consumed
}

fn handle_confirm_accepted(app: &mut PartitionManagerApp) {
    let disk_id = app.current_disk().map(|d| d.id).unwrap_or(0);

    // Determine what was being confirmed based on dialog title
    let title = match &app.dialog {
        ActiveDialog::Confirm(d) => d.title.clone(),
        _ => String::new(),
    };

    if title.contains("Partition Table") {
        app.enqueue_operation(PendingOperation::CreatePartitionTable {
            disk_id,
            table_type: PartitionTableType::Gpt,
        });
    } else if title.contains("Delete") {
        if let SelectedItem::Partition(idx) = &app.selected_item {
            let label = app.selected_partition()
                .map(|p| p.label.clone())
                .unwrap_or_default();
            app.enqueue_operation(PendingOperation::DeletePartition {
                disk_id,
                partition_index: *idx,
                partition_label: label,
            });
        }
    } else if title.contains("Apply") {
        app.apply_operations();
    }

    app.dialog = ActiveDialog::None;
}

fn handle_create_partition_accepted(app: &mut PartitionManagerApp) {
    let disk_id = app.current_disk().map(|d| d.id).unwrap_or(0);

    let (start, end, fs, label) = match &app.dialog {
        ActiveDialog::CreatePartition(d) => {
            (d.start_sector, d.computed_end_sector(), d.selected_filesystem(), d.label.clone())
        }
        _ => return,
    };

    let final_label = if label.is_empty() {
        String::from("New Partition")
    } else {
        label
    };

    app.enqueue_operation(PendingOperation::CreatePartition {
        disk_id,
        start_sector: start,
        end_sector: end,
        filesystem: fs,
        label: final_label,
    });
    app.dialog = ActiveDialog::None;
}

fn handle_format_accepted(app: &mut PartitionManagerApp) {
    let disk_id = app.current_disk().map(|d| d.id).unwrap_or(0);

    let (part_idx, fs) = match &app.dialog {
        ActiveDialog::Format(d) => (d.partition_index, d.selected_filesystem()),
        _ => return,
    };

    app.enqueue_operation(PendingOperation::FormatPartition {
        disk_id,
        partition_index: part_idx,
        new_filesystem: fs,
    });
    app.dialog = ActiveDialog::None;
}

fn handle_key(app: &mut PartitionManagerApp, key_ev: &KeyEvent) -> EventResult {
    if !key_ev.pressed {
        return EventResult::Ignored;
    }

    // Escape closes dialogs
    if key_ev.key == Key::Escape {
        if app.dialog.is_open() {
            app.dialog = ActiveDialog::None;
            return EventResult::Consumed;
        }
    }

    // If dialog open, handle text input for create-partition label
    if let ActiveDialog::CreatePartition(ref mut dialog) = app.dialog {
        match key_ev.key {
            Key::Backspace => {
                dialog.label.pop();
                return EventResult::Consumed;
            }
            Key::Left => {
                dialog.size_percent = dialog.size_percent.saturating_sub(5).max(5);
                return EventResult::Consumed;
            }
            Key::Right => {
                dialog.size_percent = (dialog.size_percent + 5).min(100);
                return EventResult::Consumed;
            }
            Key::Tab => {
                let max = FilesystemType::formattable().len();
                dialog.filesystem_index = (dialog.filesystem_index + 1) % max;
                return EventResult::Consumed;
            }
            _ => {
                if let Some(ch) = key_ev.text {
                    if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' {
                        dialog.label.push(ch);
                        return EventResult::Consumed;
                    }
                }
            }
        }
        return EventResult::Consumed;
    }

    // If format dialog, Tab cycles filesystem
    if let ActiveDialog::Format(ref mut dialog) = app.dialog {
        if key_ev.key == Key::Tab {
            let max = FilesystemType::formattable().len();
            dialog.filesystem_index = (dialog.filesystem_index + 1) % max;
            return EventResult::Consumed;
        }
        return EventResult::Consumed;
    }

    // Global shortcuts
    match key_ev.key {
        Key::Delete => {
            if matches!(app.selected_item, SelectedItem::Partition(_)) {
                let _ = execute_toolbar_action(app, 2); // Delete
                return EventResult::Consumed;
            }
        }
        Key::N if key_ev.modifiers.ctrl => {
            if matches!(app.selected_item, SelectedItem::Unallocated(_)) {
                let _ = execute_toolbar_action(app, 1); // Create
                return EventResult::Consumed;
            }
        }
        Key::Z if key_ev.modifiers.ctrl => {
            app.undo_last_operation();
            return EventResult::Consumed;
        }
        Key::Enter if key_ev.modifiers.ctrl => {
            if app.has_pending_operations() {
                let _ = execute_toolbar_action(app, 9); // Apply
                return EventResult::Consumed;
            }
        }
        Key::Up => {
            select_adjacent_region(app, true);
            return EventResult::Consumed;
        }
        Key::Down => {
            select_adjacent_region(app, false);
            return EventResult::Consumed;
        }
        _ => {}
    }

    EventResult::Ignored
}

/// Select the previous (up=true) or next (up=false) region.
fn select_adjacent_region(app: &mut PartitionManagerApp, up: bool) {
    let disk = match app.current_disk() {
        Some(d) => d,
        None => return,
    };

    let regions = disk.regions();
    if regions.is_empty() {
        return;
    }

    let current_index = match &app.selected_item {
        SelectedItem::Partition(idx) => {
            regions.iter().position(|r| {
                matches!(r, DiskRegion::Partition(p) if p.index == *idx)
            })
        }
        SelectedItem::Unallocated(ui) => Some(*ui),
        SelectedItem::None => None,
    };

    let new_index = match current_index {
        Some(ci) => {
            if up {
                ci.saturating_sub(1)
            } else {
                (ci + 1).min(regions.len().saturating_sub(1))
            }
        }
        None => 0,
    };

    if let Some(region) = regions.get(new_index) {
        app.selected_item = match region {
            DiskRegion::Partition(p) => SelectedItem::Partition(p.index),
            DiskRegion::Unallocated(_) => SelectedItem::Unallocated(new_index),
        };
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Size formatting tests --

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kib() {
        assert_eq!(format_size(1024), "1 KiB");
        assert_eq!(format_size(2048), "2 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
    }

    #[test]
    fn test_format_size_mib() {
        assert_eq!(format_size(1024 * 1024), "1 MiB");
        assert_eq!(format_size(10 * 1024 * 1024), "10 MiB");
    }

    #[test]
    fn test_format_size_gib() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1 GiB");
        assert_eq!(format_size(500_107_862_016), "465.76 GiB");
    }

    #[test]
    fn test_format_size_tib() {
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1 TiB");
        assert_eq!(format_size(2u64 * 1024 * 1024 * 1024 * 1024), "2 TiB");
    }

    #[test]
    fn test_sectors_to_bytes() {
        assert_eq!(sectors_to_bytes(1, 512), 512);
        assert_eq!(sectors_to_bytes(2048, 512), 1_048_576);
        assert_eq!(sectors_to_bytes(0, 512), 0);
        assert_eq!(sectors_to_bytes(1, 4096), 4096);
    }

    #[test]
    fn test_sectors_to_bytes_overflow_saturates() {
        assert_eq!(sectors_to_bytes(u64::MAX, 512), u64::MAX);
    }

    // -- PartitionTableType tests --

    #[test]
    fn test_partition_table_type_labels() {
        assert_eq!(PartitionTableType::Gpt.label(), "GPT");
        assert_eq!(PartitionTableType::Mbr.label(), "MBR");
    }

    // -- FilesystemType tests --

    #[test]
    fn test_filesystem_type_labels() {
        assert_eq!(FilesystemType::Ext4.label(), "ext4");
        assert_eq!(FilesystemType::Fat32.label(), "FAT32");
        assert_eq!(FilesystemType::Ntfs.label(), "NTFS");
        assert_eq!(FilesystemType::Swap.label(), "swap");
        assert_eq!(FilesystemType::EfiSystem.label(), "EFI System");
        assert_eq!(FilesystemType::Unformatted.label(), "Unformatted");
        assert_eq!(FilesystemType::Unknown.label(), "Unknown");
    }

    #[test]
    fn test_filesystem_type_colors_are_distinct() {
        let colors: Vec<Color> = FilesystemType::all().iter().map(|fs| fs.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "Filesystem colors must be distinct");
            }
        }
    }

    #[test]
    fn test_filesystem_formattable_excludes_unformatted_unknown() {
        let formattable = FilesystemType::formattable();
        assert!(!formattable.contains(&FilesystemType::Unformatted));
        assert!(!formattable.contains(&FilesystemType::Unknown));
        assert!(formattable.contains(&FilesystemType::Ext4));
    }

    #[test]
    fn test_filesystem_all_count() {
        assert_eq!(FilesystemType::all().len(), 7);
    }

    #[test]
    fn test_filesystem_formattable_count() {
        assert_eq!(FilesystemType::formattable().len(), 5);
    }

    // -- PartitionFlag tests --

    #[test]
    fn test_partition_flag_labels() {
        assert_eq!(PartitionFlag::Boot.label(), "boot");
        assert_eq!(PartitionFlag::Efi.label(), "efi");
        assert_eq!(PartitionFlag::Swap.label(), "swap");
        assert_eq!(PartitionFlag::Hidden.label(), "hidden");
    }

    #[test]
    fn test_partition_flag_all() {
        assert_eq!(PartitionFlag::all().len(), 4);
    }

    // -- SmartHealth tests --

    #[test]
    fn test_smart_health_labels() {
        assert_eq!(SmartHealth::Healthy.label(), "Healthy");
        assert_eq!(SmartHealth::Warning.label(), "Warning");
        assert_eq!(SmartHealth::Failing.label(), "Failing");
        assert_eq!(SmartHealth::Unknown.label(), "Unknown");
    }

    #[test]
    fn test_smart_health_colors_distinct() {
        let h = SmartHealth::Healthy.color();
        let w = SmartHealth::Warning.color();
        let f = SmartHealth::Failing.color();
        assert_ne!(h, w);
        assert_ne!(h, f);
        assert_ne!(w, f);
    }

    // -- Partition tests --

    fn make_test_partition() -> Partition {
        Partition {
            index: 1,
            label: String::from("Test"),
            filesystem: FilesystemType::Ext4,
            start_sector: 2048,
            end_sector: 1_000_000,
            size_bytes: 510_656_512,
            flags: vec![PartitionFlag::Boot],
            mount_point: Some(String::from("/")),
            uuid: String::from("test-uuid-1234"),
            used_bytes: Some(200_000_000),
            free_bytes: Some(310_656_512),
        }
    }

    #[test]
    fn test_partition_is_boot() {
        let p = make_test_partition();
        assert!(p.is_boot());
        assert!(!p.is_efi());
    }

    #[test]
    fn test_partition_is_system() {
        let p = make_test_partition();
        assert!(p.is_system()); // has boot flag
    }

    #[test]
    fn test_partition_not_system() {
        let p = Partition {
            flags: vec![],
            ..make_test_partition()
        };
        assert!(!p.is_system());
    }

    #[test]
    fn test_partition_flags_string() {
        let p = make_test_partition();
        assert_eq!(p.flags_string(), "boot");

        let p2 = Partition {
            flags: vec![PartitionFlag::Boot, PartitionFlag::Efi],
            ..make_test_partition()
        };
        assert_eq!(p2.flags_string(), "boot, efi");
    }

    #[test]
    fn test_partition_flags_string_empty() {
        let p = Partition {
            flags: vec![],
            ..make_test_partition()
        };
        assert_eq!(p.flags_string(), "none");
    }

    #[test]
    fn test_partition_used_percent() {
        let p = make_test_partition();
        let pct = p.used_percent().unwrap();
        assert!(pct > 0 && pct < 100);
    }

    #[test]
    fn test_partition_used_percent_none() {
        let p = Partition {
            used_bytes: None,
            ..make_test_partition()
        };
        assert!(p.used_percent().is_none());
    }

    #[test]
    fn test_partition_used_percent_zero_size() {
        let p = Partition {
            size_bytes: 0,
            used_bytes: Some(0),
            ..make_test_partition()
        };
        assert_eq!(p.used_percent(), Some(0));
    }

    #[test]
    fn test_partition_is_efi() {
        let p = Partition {
            flags: vec![PartitionFlag::Efi],
            ..make_test_partition()
        };
        assert!(p.is_efi());
        assert!(p.is_system());
    }

    // -- DiskRegion tests --

    #[test]
    fn test_disk_region_partition() {
        let r = DiskRegion::Partition(make_test_partition());
        assert!(r.is_partition());
        assert!(!r.is_unallocated());
        assert!(r.as_partition().is_some());
        assert_eq!(r.start_sector(), 2048);
    }

    #[test]
    fn test_disk_region_unallocated() {
        let r = DiskRegion::Unallocated(UnallocatedSpace {
            start_sector: 100,
            end_sector: 200,
            size_bytes: 51200,
        });
        assert!(r.is_unallocated());
        assert!(!r.is_partition());
        assert!(r.as_partition().is_none());
        assert_eq!(r.size_bytes(), 51200);
    }

    // -- Disk tests --

    fn make_test_disk() -> Disk {
        Disk {
            id: 0,
            name: String::from("/dev/sda"),
            model: String::from("TestDisk"),
            serial: String::from("SN123"),
            total_size_bytes: 1_000_000_000,
            sector_size: 512,
            total_sectors: 1_953_125,
            table_type: PartitionTableType::Gpt,
            partitions: vec![
                Partition {
                    index: 1,
                    label: String::from("Part1"),
                    filesystem: FilesystemType::Ext4,
                    start_sector: 2048,
                    end_sector: 500_000,
                    size_bytes: 254_976_000,
                    flags: vec![],
                    mount_point: Some(String::from("/")),
                    uuid: String::from("uuid-1"),
                    used_bytes: Some(100_000_000),
                    free_bytes: Some(154_976_000),
                },
                Partition {
                    index: 2,
                    label: String::from("Part2"),
                    filesystem: FilesystemType::Swap,
                    start_sector: 500_001,
                    end_sector: 600_000,
                    size_bytes: 51_199_488,
                    flags: vec![PartitionFlag::Swap],
                    mount_point: None,
                    uuid: String::from("uuid-2"),
                    used_bytes: None,
                    free_bytes: None,
                },
            ],
            smart_health: SmartHealth::Healthy,
            temperature_c: Some(35),
        }
    }

    #[test]
    fn test_disk_regions_include_partitions() {
        let disk = make_test_disk();
        let regions = disk.regions();
        let part_count = regions.iter().filter(|r| r.is_partition()).count();
        assert_eq!(part_count, 2);
    }

    #[test]
    fn test_disk_regions_include_unallocated() {
        let disk = make_test_disk();
        let regions = disk.regions();
        // There should be unallocated space after the last partition
        let unalloc_count = regions.iter().filter(|r| r.is_unallocated()).count();
        assert!(unalloc_count >= 1);
    }

    #[test]
    fn test_disk_regions_sorted_by_start() {
        let disk = make_test_disk();
        let regions = disk.regions();
        for i in 1..regions.len() {
            assert!(regions[i].start_sector() >= regions[i - 1].start_sector());
        }
    }

    #[test]
    fn test_disk_used_space() {
        let disk = make_test_disk();
        let used = disk.used_space();
        assert_eq!(used, 254_976_000 + 51_199_488);
    }

    #[test]
    fn test_disk_free_space() {
        let disk = make_test_disk();
        let free = disk.free_space();
        assert!(free > 0);
        assert_eq!(free, disk.total_size_bytes - disk.used_space());
    }

    #[test]
    fn test_disk_partition_count() {
        let disk = make_test_disk();
        assert_eq!(disk.partition_count(), 2);
    }

    #[test]
    fn test_disk_empty_partitions() {
        let disk = Disk {
            partitions: vec![],
            ..make_test_disk()
        };
        assert_eq!(disk.partition_count(), 0);
        let regions = disk.regions();
        assert_eq!(regions.len(), 1); // just one big unallocated
        assert!(regions[0].is_unallocated());
    }

    #[test]
    fn test_disk_regions_mbr() {
        let disk = Disk {
            table_type: PartitionTableType::Mbr,
            ..make_test_disk()
        };
        let regions = disk.regions();
        // MBR uses sector 1 as first usable (not 34)
        assert!(!regions.is_empty());
    }

    // -- PendingOperation tests --

    #[test]
    fn test_operation_describe_create() {
        let op = PendingOperation::CreatePartition {
            disk_id: 0,
            start_sector: 0,
            end_sector: 2_000_000,
            filesystem: FilesystemType::Ext4,
            label: String::from("Data"),
        };
        let desc = op.describe();
        assert!(desc.contains("ext4"));
        assert!(desc.contains("Data"));
    }

    #[test]
    fn test_operation_describe_delete() {
        let op = PendingOperation::DeletePartition {
            disk_id: 0,
            partition_index: 1,
            partition_label: String::from("Root"),
        };
        let desc = op.describe();
        assert!(desc.contains("Delete"));
        assert!(desc.contains("Root"));
    }

    #[test]
    fn test_operation_describe_format() {
        let op = PendingOperation::FormatPartition {
            disk_id: 0,
            partition_index: 2,
            new_filesystem: FilesystemType::Fat32,
        };
        let desc = op.describe();
        assert!(desc.contains("Format"));
        assert!(desc.contains("FAT32"));
    }

    #[test]
    fn test_operation_describe_resize() {
        let op = PendingOperation::ResizePartition {
            disk_id: 0,
            partition_index: 1,
            new_start_sector: 100,
            new_end_sector: 2_000_100,
        };
        let desc = op.describe();
        assert!(desc.contains("Resize"));
    }

    #[test]
    fn test_operation_describe_set_label() {
        let op = PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("MyDisk"),
        };
        assert!(op.describe().contains("MyDisk"));
    }

    #[test]
    fn test_operation_describe_set_flag() {
        let op = PendingOperation::SetFlag {
            disk_id: 0,
            partition_index: 1,
            flag: PartitionFlag::Boot,
            enabled: true,
        };
        assert!(op.describe().contains("boot"));
        assert!(op.describe().contains("Enable"));
    }

    #[test]
    fn test_operation_describe_disable_flag() {
        let op = PendingOperation::SetFlag {
            disk_id: 0,
            partition_index: 1,
            flag: PartitionFlag::Hidden,
            enabled: false,
        };
        assert!(op.describe().contains("Disable"));
    }

    #[test]
    fn test_operation_describe_mount() {
        let op = PendingOperation::SetMountPoint {
            disk_id: 0,
            partition_index: 1,
            mount_point: Some(String::from("/mnt/data")),
        };
        assert!(op.describe().contains("/mnt/data"));
    }

    #[test]
    fn test_operation_describe_unmount() {
        let op = PendingOperation::SetMountPoint {
            disk_id: 0,
            partition_index: 1,
            mount_point: None,
        };
        assert!(op.describe().contains("Unmount"));
    }

    #[test]
    fn test_operation_describe_create_table() {
        let op = PendingOperation::CreatePartitionTable {
            disk_id: 0,
            table_type: PartitionTableType::Gpt,
        };
        let desc = op.describe();
        assert!(desc.contains("GPT"));
        assert!(desc.contains("ALL DATA"));
    }

    #[test]
    fn test_operation_is_destructive() {
        assert!(PendingOperation::DeletePartition {
            disk_id: 0,
            partition_index: 1,
            partition_label: String::new(),
        }.is_destructive());
        assert!(PendingOperation::FormatPartition {
            disk_id: 0,
            partition_index: 1,
            new_filesystem: FilesystemType::Ext4,
        }.is_destructive());
        assert!(PendingOperation::CreatePartitionTable {
            disk_id: 0,
            table_type: PartitionTableType::Gpt,
        }.is_destructive());
    }

    #[test]
    fn test_operation_not_destructive() {
        assert!(!PendingOperation::CreatePartition {
            disk_id: 0,
            start_sector: 0,
            end_sector: 100,
            filesystem: FilesystemType::Ext4,
            label: String::new(),
        }.is_destructive());
        assert!(!PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::new(),
        }.is_destructive());
    }

    #[test]
    fn test_operation_disk_id() {
        let op = PendingOperation::CreatePartition {
            disk_id: 42,
            start_sector: 0,
            end_sector: 100,
            filesystem: FilesystemType::Ext4,
            label: String::new(),
        };
        assert_eq!(op.disk_id(), 42);
    }

    // -- Dialog tests --

    #[test]
    fn test_confirm_dialog_new() {
        let d = ConfirmDialog::new("Title", "Message", "OK", false);
        assert_eq!(d.title, "Title");
        assert_eq!(d.message, "Message");
        assert!(!d.destructive);
        assert!(d.hovered_button.is_none());
    }

    #[test]
    fn test_confirm_dialog_destructive() {
        let d = ConfirmDialog::new("Delete", "Sure?", "Delete", true);
        assert!(d.destructive);
    }

    #[test]
    fn test_create_partition_dialog() {
        let d = CreatePartitionDialog::new(100, 10_000, 512);
        assert_eq!(d.start_sector, 100);
        assert_eq!(d.end_sector, 10_000);
        assert_eq!(d.size_percent, 100);
        assert!(d.label.is_empty());
    }

    #[test]
    fn test_create_partition_dialog_available_bytes() {
        let d = CreatePartitionDialog::new(0, 2048, 512);
        assert_eq!(d.available_bytes(), 2048 * 512);
    }

    #[test]
    fn test_create_partition_dialog_selected_size() {
        let mut d = CreatePartitionDialog::new(0, 2048, 512);
        d.size_percent = 50;
        let expected = (2048u64 * 512 / 2) as u64;
        assert_eq!(d.selected_size_bytes(), expected);
    }

    #[test]
    fn test_create_partition_dialog_filesystem() {
        let mut d = CreatePartitionDialog::new(0, 100, 512);
        assert_eq!(d.selected_filesystem(), FilesystemType::Ext4);
        d.filesystem_index = 1;
        assert_eq!(d.selected_filesystem(), FilesystemType::Fat32);
    }

    #[test]
    fn test_create_partition_dialog_computed_end() {
        let d = CreatePartitionDialog::new(100, 1100, 512);
        assert_eq!(d.computed_end_sector(), 1100); // 100% of 1000 sectors + 100
    }

    #[test]
    fn test_format_dialog() {
        let d = FormatDialog::new(3, "Data");
        assert_eq!(d.partition_index, 3);
        assert_eq!(d.partition_label, "Data");
        assert_eq!(d.selected_filesystem(), FilesystemType::Ext4);
    }

    #[test]
    fn test_active_dialog_is_open() {
        assert!(!ActiveDialog::None.is_open());
        assert!(ActiveDialog::Confirm(ConfirmDialog::new("", "", "", false)).is_open());
        assert!(ActiveDialog::CreatePartition(CreatePartitionDialog::new(0, 0, 512)).is_open());
        assert!(ActiveDialog::Format(FormatDialog::new(1, "")).is_open());
    }

    // -- SelectedItem tests --

    #[test]
    fn test_selected_item_equality() {
        assert_eq!(SelectedItem::None, SelectedItem::None);
        assert_eq!(SelectedItem::Partition(1), SelectedItem::Partition(1));
        assert_ne!(SelectedItem::Partition(1), SelectedItem::Partition(2));
        assert_ne!(SelectedItem::Partition(1), SelectedItem::None);
    }

    // -- App state tests --

    #[test]
    fn test_app_new() {
        let app = PartitionManagerApp::new();
        assert!(!app.disks.is_empty());
        assert_eq!(app.selected_disk, 0);
        assert_eq!(app.selected_item, SelectedItem::None);
        assert!(app.operation_queue.is_empty());
        assert!(!app.dialog.is_open());
    }

    #[test]
    fn test_app_current_disk() {
        let app = PartitionManagerApp::new();
        assert!(app.current_disk().is_some());
        assert_eq!(app.current_disk().unwrap().id, 0);
    }

    #[test]
    fn test_app_select_disk() {
        let mut app = PartitionManagerApp::new();
        app.select_disk(1);
        assert_eq!(app.selected_disk, 1);
        assert_eq!(app.selected_item, SelectedItem::None);
    }

    #[test]
    fn test_app_select_disk_out_of_bounds() {
        let mut app = PartitionManagerApp::new();
        let count = app.disks.len();
        app.select_disk(count + 10);
        assert_eq!(app.selected_disk, 0); // unchanged
    }

    #[test]
    fn test_app_enqueue_operation() {
        let mut app = PartitionManagerApp::new();
        assert!(!app.has_pending_operations());
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("Test"),
        });
        assert!(app.has_pending_operations());
        assert_eq!(app.pending_count(), 1);
    }

    #[test]
    fn test_app_undo_operation() {
        let mut app = PartitionManagerApp::new();
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("Test"),
        });
        let undone = app.undo_last_operation();
        assert!(undone.is_some());
        assert!(!app.has_pending_operations());
    }

    #[test]
    fn test_app_undo_empty_queue() {
        let mut app = PartitionManagerApp::new();
        assert!(app.undo_last_operation().is_none());
    }

    #[test]
    fn test_app_clear_operations() {
        let mut app = PartitionManagerApp::new();
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("A"),
        });
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 2,
            new_label: String::from("B"),
        });
        app.clear_operations();
        assert_eq!(app.pending_count(), 0);
    }

    #[test]
    fn test_app_apply_operations() {
        let mut app = PartitionManagerApp::new();
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("X"),
        });
        let count = app.apply_operations();
        assert_eq!(count, 1);
        assert!(!app.has_pending_operations());
    }

    #[test]
    fn test_app_has_destructive_operations() {
        let mut app = PartitionManagerApp::new();
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("Safe"),
        });
        assert!(!app.has_destructive_operations());

        app.enqueue_operation(PendingOperation::DeletePartition {
            disk_id: 0,
            partition_index: 1,
            partition_label: String::from("Part"),
        });
        assert!(app.has_destructive_operations());
    }

    #[test]
    fn test_app_selected_partition() {
        let mut app = PartitionManagerApp::new();
        assert!(app.selected_partition().is_none());

        app.selected_item = SelectedItem::Partition(1);
        let p = app.selected_partition();
        assert!(p.is_some());
        assert_eq!(p.unwrap().index, 1);
    }

    #[test]
    fn test_app_selected_partition_wrong_index() {
        let mut app = PartitionManagerApp::new();
        app.selected_item = SelectedItem::Partition(999);
        assert!(app.selected_partition().is_none());
    }

    #[test]
    fn test_app_toolbar_buttons_nothing_selected() {
        let app = PartitionManagerApp::new();
        let buttons = app.toolbar_buttons();
        assert_eq!(buttons.len(), 10);
        // "New Table" always enabled
        assert!(buttons[0].1);
        // Create disabled (no unallocated selected)
        assert!(!buttons[1].1);
        // Delete disabled (no partition selected)
        assert!(!buttons[2].1);
    }

    #[test]
    fn test_app_toolbar_buttons_partition_selected() {
        let mut app = PartitionManagerApp::new();
        app.selected_item = SelectedItem::Partition(1);
        let buttons = app.toolbar_buttons();
        // Delete, Resize, Format, Label, Flags, Mount should be enabled
        assert!(buttons[2].1); // Delete
        assert!(buttons[3].1); // Resize
        assert!(buttons[4].1); // Format
        assert!(buttons[5].1); // Label
    }

    #[test]
    fn test_app_toolbar_buttons_unallocated_selected() {
        let mut app = PartitionManagerApp::new();
        app.selected_item = SelectedItem::Unallocated(0);
        let buttons = app.toolbar_buttons();
        assert!(buttons[1].1); // Create enabled
        assert!(!buttons[2].1); // Delete disabled
    }

    #[test]
    fn test_app_toolbar_buttons_undo_apply_with_ops() {
        let mut app = PartitionManagerApp::new();
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("X"),
        });
        let buttons = app.toolbar_buttons();
        assert!(buttons[8].1); // Undo
        assert!(buttons[9].1); // Apply
    }

    // -- Render tests --

    #[test]
    fn test_render_produces_commands() {
        let app = PartitionManagerApp::new();
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_selected_partition() {
        let mut app = PartitionManagerApp::new();
        app.selected_item = SelectedItem::Partition(1);
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_dialog() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::Confirm(ConfirmDialog::new("Test", "Body", "OK", false));
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_create_dialog() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::CreatePartition(CreatePartitionDialog::new(0, 1000, 512));
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_format_dialog() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::Format(FormatDialog::new(1, "Test"));
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_queue_expanded() {
        let mut app = PartitionManagerApp::new();
        app.queue_expanded = true;
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("X"),
        });
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_queue_collapsed() {
        let mut app = PartitionManagerApp::new();
        app.queue_expanded = false;
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_second_disk() {
        let mut app = PartitionManagerApp::new();
        app.select_disk(1);
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_destructive_confirm() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::Confirm(ConfirmDialog::new("Del", "Sure?", "Delete", true));
        let tree = render(&app);
        assert!(!tree.is_empty());
    }

    // -- Event handling tests --

    #[test]
    fn test_handle_resize() {
        let mut app = PartitionManagerApp::new();
        let result = handle_event(&mut app, &Event::Resize { width: 800, height: 600 });
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(app.width, 800.0);
        assert_eq!(app.height, 600.0);
    }

    #[test]
    fn test_handle_key_escape_closes_dialog() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::Confirm(ConfirmDialog::new("T", "M", "OK", false));
        let ev = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        handle_event(&mut app, &ev);
        assert!(!app.dialog.is_open());
    }

    #[test]
    fn test_handle_key_ctrl_z_undo() {
        let mut app = PartitionManagerApp::new();
        app.enqueue_operation(PendingOperation::SetLabel {
            disk_id: 0,
            partition_index: 1,
            new_label: String::from("X"),
        });
        let ev = Event::Key(KeyEvent {
            key: Key::Z,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        handle_event(&mut app, &ev);
        assert!(!app.has_pending_operations());
    }

    #[test]
    fn test_handle_key_arrow_navigation() {
        let mut app = PartitionManagerApp::new();
        // Down arrow selects first region
        let ev = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        handle_event(&mut app, &ev);
        assert_ne!(app.selected_item, SelectedItem::None);
    }

    #[test]
    fn test_handle_focus_event_ignored() {
        let mut app = PartitionManagerApp::new();
        let result = handle_event(&mut app, &Event::FocusIn);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn test_handle_key_release_ignored() {
        let mut app = PartitionManagerApp::new();
        let ev = Event::Key(KeyEvent {
            key: Key::A,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        let result = handle_event(&mut app, &ev);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn test_handle_create_dialog_text_input() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::CreatePartition(CreatePartitionDialog::new(0, 1000, 512));
        let ev = Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        handle_event(&mut app, &ev);
        if let ActiveDialog::CreatePartition(ref d) = app.dialog {
            assert_eq!(d.label, "a");
        } else {
            panic!("Dialog should still be CreatePartition");
        }
    }

    #[test]
    fn test_handle_create_dialog_backspace() {
        let mut app = PartitionManagerApp::new();
        let mut dlg = CreatePartitionDialog::new(0, 1000, 512);
        dlg.label = String::from("abc");
        app.dialog = ActiveDialog::CreatePartition(dlg);
        let ev = Event::Key(KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        handle_event(&mut app, &ev);
        if let ActiveDialog::CreatePartition(ref d) = app.dialog {
            assert_eq!(d.label, "ab");
        }
    }

    #[test]
    fn test_handle_create_dialog_size_arrows() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::CreatePartition(CreatePartitionDialog::new(0, 1000, 512));
        // Left arrow decreases
        let ev = Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        handle_event(&mut app, &ev);
        if let ActiveDialog::CreatePartition(ref d) = app.dialog {
            assert_eq!(d.size_percent, 95);
        }
    }

    #[test]
    fn test_handle_create_dialog_tab_cycles_fs() {
        let mut app = PartitionManagerApp::new();
        app.dialog = ActiveDialog::CreatePartition(CreatePartitionDialog::new(0, 1000, 512));
        let ev = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        handle_event(&mut app, &ev);
        if let ActiveDialog::CreatePartition(ref d) = app.dialog {
            assert_eq!(d.filesystem_index, 1);
        }
    }

    // -- Sample data tests --

    #[test]
    fn test_sample_disks_not_empty() {
        let disks = sample_disks();
        assert!(!disks.is_empty());
    }

    #[test]
    fn test_sample_disks_have_valid_structure() {
        let disks = sample_disks();
        for disk in &disks {
            assert!(!disk.name.is_empty());
            assert!(!disk.model.is_empty());
            assert!(disk.total_size_bytes > 0);
            assert!(disk.sector_size > 0);
            assert!(disk.total_sectors > 0);
        }
    }

    #[test]
    fn test_sample_disks_partitions_within_disk() {
        let disks = sample_disks();
        for disk in &disks {
            for part in &disk.partitions {
                assert!(part.end_sector <= disk.total_sectors,
                    "Partition {} on {} exceeds disk", part.index, disk.name);
            }
        }
    }

    #[test]
    fn test_sample_disks_unique_ids() {
        let disks = sample_disks();
        let ids: Vec<u32> = disks.iter().map(|d| d.id).collect();
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_sample_disks_has_gpt_and_mbr() {
        let disks = sample_disks();
        let has_gpt = disks.iter().any(|d| d.table_type == PartitionTableType::Gpt);
        let has_mbr = disks.iter().any(|d| d.table_type == PartitionTableType::Mbr);
        assert!(has_gpt);
        assert!(has_mbr);
    }

    #[test]
    fn test_select_adjacent_region_down() {
        let mut app = PartitionManagerApp::new();
        select_adjacent_region(&mut app, false);
        assert_ne!(app.selected_item, SelectedItem::None);
    }

    #[test]
    fn test_select_adjacent_region_up_at_start() {
        let mut app = PartitionManagerApp::new();
        // Select first region
        select_adjacent_region(&mut app, false);
        // Up should stay at 0
        select_adjacent_region(&mut app, true);
        assert_ne!(app.selected_item, SelectedItem::None);
    }
}
