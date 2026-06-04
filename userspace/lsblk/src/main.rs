//! OurOS Block Device Lister
//!
//! Lists block devices by reading from `/sys/block/` and `/proc/partitions`.
//! Displays disk and partition information in tree, list, or JSON format.
//!
//! # Usage
//!
//! ```text
//! lsblk                      List all block devices (tree format)
//! lsblk -l                   List format (flat, no tree)
//! lsblk -f                   Show filesystem info (type, label, UUID, mount)
//! lsblk -b                   Show sizes in bytes
//! lsblk -o NAME,SIZE,TYPE    Custom output columns
//! lsblk --json               JSON output
//! lsblk --help               Show help
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

// ============================================================================
// Column definitions
// ============================================================================

/// All supported output columns.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Column {
    Name,
    Size,
    Type,
    FsType,
    MountPoint,
    Label,
    Uuid,
    Model,
    Serial,
    Ro,
    Rm,
}

impl Column {
    /// Parse a column name (case-insensitive).
    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "NAME" => Some(Self::Name),
            "SIZE" => Some(Self::Size),
            "TYPE" => Some(Self::Type),
            "FSTYPE" | "FS" => Some(Self::FsType),
            "MOUNTPOINT" | "MOUNT" => Some(Self::MountPoint),
            "LABEL" => Some(Self::Label),
            "UUID" => Some(Self::Uuid),
            "MODEL" => Some(Self::Model),
            "SERIAL" => Some(Self::Serial),
            "RO" => Some(Self::Ro),
            "RM" => Some(Self::Rm),
            _ => None,
        }
    }

    fn header(self) -> &'static str {
        match self {
            Self::Name => "NAME",
            Self::Size => "SIZE",
            Self::Type => "TYPE",
            Self::FsType => "FSTYPE",
            Self::MountPoint => "MOUNTPOINT",
            Self::Label => "LABEL",
            Self::Uuid => "UUID",
            Self::Model => "MODEL",
            Self::Serial => "SERIAL",
            Self::Ro => "RO",
            Self::Rm => "RM",
        }
    }

    fn default_width(self) -> usize {
        match self {
            Self::Name => 10,
            Self::Size => 6,
            Self::Type => 4,
            Self::FsType => 6,
            Self::MountPoint => 10,
            Self::Label => 8,
            Self::Uuid => 36,
            Self::Model => 16,
            Self::Serial => 16,
            Self::Ro => 2,
            Self::Rm => 2,
        }
    }
}

/// Default columns for normal output.
const DEFAULT_COLUMNS: &[Column] = &[
    Column::Name,
    Column::Size,
    Column::Type,
    Column::MountPoint,
];

/// Columns shown with -f (filesystem info).
const FS_COLUMNS: &[Column] = &[
    Column::Name,
    Column::FsType,
    Column::Label,
    Column::Uuid,
    Column::Size,
    Column::MountPoint,
];

// ============================================================================
// Block device structures
// ============================================================================

/// A single block device (disk or partition).
#[allow(dead_code)]
struct BlockDevice {
    /// Kernel name (e.g. "sda", "sda1", "vda", "nvme0n1").
    name: String,
    /// Size in 512-byte sectors.
    size_sectors: u64,
    /// "disk" or "part".
    dev_type: String,
    /// Filesystem type (e.g. "ext4", "fat32"), empty if unknown.
    fstype: String,
    /// Filesystem label, empty if unknown.
    label: String,
    /// Filesystem UUID, empty if unknown.
    uuid: String,
    /// Mount point from /proc/mounts, empty if not mounted.
    mountpoint: String,
    /// Device model string, empty if unavailable.
    model: String,
    /// Device serial number, empty if unavailable.
    serial: String,
    /// Read-only flag (true if device is read-only).
    read_only: bool,
    /// Removable flag (true if removable media).
    removable: bool,
    /// Child partitions (only populated for disks).
    children: Vec<BlockDevice>,
}

impl BlockDevice {
    /// Size in bytes (each sector is 512 bytes in /sys and /proc).
    fn size_bytes(&self) -> u64 {
        self.size_sectors.saturating_mul(512)
    }
}

// ============================================================================
// CLI configuration
// ============================================================================

struct Config {
    /// Use flat list output instead of tree.
    list_mode: bool,
    /// Show filesystem columns.
    fs_mode: bool,
    /// Show sizes in raw bytes.
    bytes_mode: bool,
    /// JSON output.
    json: bool,
    /// Which columns to display.
    columns: Vec<Column>,
}

// ============================================================================
// Sysfs and procfs reading helpers
// ============================================================================

/// Read a file and return its trimmed contents, or None on failure.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read a file and parse it as u64, or return 0.
fn read_u64(path: &str) -> u64 {
    read_file(path)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Read a file and parse it as a boolean flag (1 = true).
fn read_bool(path: &str) -> bool {
    read_file(path).is_some_and(|s| s == "1")
}

// ============================================================================
// Mount point lookup
// ============================================================================

/// Parse /proc/mounts and build a map of device path -> mount point.
/// Entries look like: `/dev/sda1 /mnt ext4 rw,relatime 0 0`
fn parse_mounts() -> HashMap<String, String> {
    let mut mounts = HashMap::new();

    let content = match read_file("/proc/mounts") {
        Some(c) => c,
        None => return mounts,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let dev = parts[0];
            let mount = parts[1];
            // Strip "/dev/" prefix to get the kernel name.
            let name = dev.strip_prefix("/dev/").unwrap_or(dev);
            mounts.insert(name.to_string(), mount.to_string());
        }
    }

    mounts
}

// ============================================================================
// Filesystem info lookup
// ============================================================================

/// Try to read filesystem info for a device from /sys/block/.../fstype etc.
/// Our kernel may expose these in sysfs or through /proc/partitions extended
/// info. We check both.
fn read_fsinfo(dev_name: &str, parent_name: Option<&str>) -> (String, String, String) {
    // Try partition-level path first, then disk-level.
    let paths_to_try: Vec<String> = if let Some(parent) = parent_name {
        vec![
            format!("/sys/block/{parent}/{dev_name}"),
            format!("/sys/block/{dev_name}"),
        ]
    } else {
        vec![format!("/sys/block/{dev_name}")]
    };

    let mut fstype = String::new();
    let mut label = String::new();
    let mut uuid = String::new();

    for base in &paths_to_try {
        if fstype.is_empty()
            && let Some(ft) = read_file(&format!("{base}/fstype"))
                && !ft.is_empty() {
                    fstype = ft;
                }
        if label.is_empty()
            && let Some(lb) = read_file(&format!("{base}/label"))
                && !lb.is_empty() {
                    label = lb;
                }
        if uuid.is_empty()
            && let Some(id) = read_file(&format!("{base}/uuid"))
                && !id.is_empty() {
                    uuid = id;
                }
    }

    // Also try /dev/disk/by-uuid/ and /dev/disk/by-label/ symlinks.
    if uuid.is_empty() || label.is_empty() {
        if let Ok(entries) = fs::read_dir("/dev/disk/by-uuid") {
            for entry in entries.flatten() {
                if let Ok(target) = fs::read_link(entry.path()) {
                    let target_name = target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if target_name == dev_name
                        && let Some(u) = entry.file_name().to_str() {
                            uuid = u.to_string();
                        }
                }
            }
        }
        if label.is_empty()
            && let Ok(entries) = fs::read_dir("/dev/disk/by-label") {
                for entry in entries.flatten() {
                    if let Ok(target) = fs::read_link(entry.path()) {
                        let target_name = target
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        if target_name == dev_name
                            && let Some(l) = entry.file_name().to_str() {
                                label = l.to_string();
                            }
                    }
                }
            }
    }

    (fstype, label, uuid)
}

// ============================================================================
// Block device scanning
// ============================================================================

/// Scan /sys/block/ for disks and their partitions.
fn scan_sysfs(mounts: &HashMap<String, String>) -> Vec<BlockDevice> {
    let mut devices = Vec::new();

    let block_dir = "/sys/block";
    let entries = match fs::read_dir(block_dir) {
        Ok(e) => e,
        Err(_) => return devices,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Skip loop and ram devices unless they have a non-zero size.
        if name.starts_with("loop") || name.starts_with("ram") {
            let sz = read_u64(&format!("{block_dir}/{name}/size"));
            if sz == 0 {
                continue;
            }
        }

        let dev_path = format!("{block_dir}/{name}");
        let size_sectors = read_u64(&format!("{dev_path}/size"));
        let read_only = read_bool(&format!("{dev_path}/ro"));
        let removable = read_bool(&format!("{dev_path}/removable"));
        let model = read_file(&format!("{dev_path}/device/model")).unwrap_or_default();
        let serial = read_file(&format!("{dev_path}/device/serial")).unwrap_or_default();
        let mountpoint = mounts.get(&name).cloned().unwrap_or_default();

        let (fstype, label, uuid) = read_fsinfo(&name, None);

        // Scan for partitions as subdirectories inside /sys/block/<disk>/.
        let children = scan_partitions(&dev_path, &name, mounts);

        devices.push(BlockDevice {
            name,
            size_sectors,
            dev_type: "disk".to_string(),
            fstype,
            label,
            uuid,
            mountpoint,
            model,
            serial,
            read_only,
            removable,
            children,
        });
    }

    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

/// Scan partitions of a disk from /sys/block/<disk>/<partition>/.
fn scan_partitions(
    disk_path: &str,
    disk_name: &str,
    mounts: &HashMap<String, String>,
) -> Vec<BlockDevice> {
    let mut parts = Vec::new();

    let entries = match fs::read_dir(disk_path) {
        Ok(e) => e,
        Err(_) => return parts,
    };

    for entry in entries.flatten() {
        let part_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Partitions are subdirectories that start with the parent disk name
        // (e.g. "sda1" under "sda", "nvme0n1p1" under "nvme0n1").
        if !part_name.starts_with(disk_name) {
            continue;
        }

        let part_path = format!("{disk_path}/{part_name}");

        // Verify this is actually a partition by checking for a "partition"
        // file or a "size" file.
        let is_partition = fs::metadata(format!("{part_path}/partition")).is_ok()
            || fs::metadata(format!("{part_path}/size")).is_ok();
        if !is_partition {
            continue;
        }

        let size_sectors = read_u64(&format!("{part_path}/size"));
        let read_only = read_bool(&format!("{part_path}/ro"));
        let mountpoint = mounts.get(&part_name).cloned().unwrap_or_default();

        let (fstype, label, uuid) = read_fsinfo(&part_name, Some(disk_name));

        parts.push(BlockDevice {
            name: part_name,
            size_sectors,
            dev_type: "part".to_string(),
            fstype,
            label,
            uuid,
            mountpoint,
            model: String::new(),
            serial: String::new(),
            read_only,
            removable: false,
            children: Vec::new(),
        });
    }

    parts.sort_by(|a, b| a.name.cmp(&b.name));
    parts
}

/// Fallback: scan /proc/partitions when /sys/block is unavailable.
/// Format:
/// ```text
/// major minor  #blocks  name
///    8        0  488386584 sda
///    8        1     512000 sda1
/// ```
fn scan_proc_partitions(mounts: &HashMap<String, String>) -> Vec<BlockDevice> {
    let content = match read_file("/proc/partitions") {
        Some(c) => c,
        None => return Vec::new(),
    };

    // First pass: collect all entries.
    let mut entries: Vec<(String, u64)> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip header and empty lines.
        if line.is_empty() || line.starts_with("major") || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let name = parts[3].to_string();
        // /proc/partitions gives size in 1024-byte blocks; convert to
        // 512-byte sectors for consistency with sysfs.
        let blocks: u64 = parts[2].parse().unwrap_or(0);
        let sectors = blocks.saturating_mul(2);

        entries.push((name, sectors));
    }

    // Classify: a name that is a prefix of another name is a disk; the rest
    // within that prefix are partitions.
    let mut disks: Vec<BlockDevice> = Vec::new();

    // Sort so that parent disks come before their partitions.
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut i = 0;
    while i < entries.len() {
        let (ref disk_name, disk_sectors) = entries[i];

        let (fstype, label, uuid) = read_fsinfo(disk_name, None);
        let mountpoint = mounts.get(disk_name).cloned().unwrap_or_default();

        let mut children = Vec::new();
        let mut j = i + 1;
        while j < entries.len() && entries[j].0.starts_with(disk_name.as_str()) {
            let (ref part_name, part_sectors) = entries[j];
            let part_mp = mounts.get(part_name).cloned().unwrap_or_default();
            let (pfs, plb, puu) = read_fsinfo(part_name, Some(disk_name));

            children.push(BlockDevice {
                name: part_name.clone(),
                size_sectors: part_sectors,
                dev_type: "part".to_string(),
                fstype: pfs,
                label: plb,
                uuid: puu,
                mountpoint: part_mp,
                model: String::new(),
                serial: String::new(),
                read_only: false,
                removable: false,
                children: Vec::new(),
            });
            j += 1;
        }

        disks.push(BlockDevice {
            name: disk_name.clone(),
            size_sectors: disk_sectors,
            dev_type: "disk".to_string(),
            fstype,
            label,
            uuid,
            mountpoint,
            model: String::new(),
            serial: String::new(),
            read_only: false,
            removable: false,
            children,
        });

        i = j;
    }

    disks
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count as a human-readable string.
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = ((bytes % TIB) * 10) / TIB;
        if frac > 0 {
            format!("{whole}.{frac}T")
        } else {
            format!("{whole}T")
        }
    } else if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = ((bytes % GIB) * 10) / GIB;
        if frac > 0 {
            format!("{whole}.{frac}G")
        } else {
            format!("{whole}G")
        }
    } else if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = ((bytes % MIB) * 10) / MIB;
        if frac > 0 {
            format!("{whole}.{frac}M")
        } else {
            format!("{whole}M")
        }
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = ((bytes % KIB) * 10) / KIB;
        if frac > 0 {
            format!("{whole}.{frac}K")
        } else {
            format!("{whole}K")
        }
    } else {
        format!("{bytes}B")
    }
}

// ============================================================================
// Column value extraction
// ============================================================================

/// Get the display value for a column from a device.
fn column_value(dev: &BlockDevice, col: Column, bytes_mode: bool, prefix: &str) -> String {
    match col {
        Column::Name => format!("{prefix}{}", dev.name),
        Column::Size => {
            if bytes_mode {
                dev.size_bytes().to_string()
            } else {
                format_size(dev.size_bytes())
            }
        }
        Column::Type => dev.dev_type.clone(),
        Column::FsType => dev.fstype.clone(),
        Column::MountPoint => dev.mountpoint.clone(),
        Column::Label => dev.label.clone(),
        Column::Uuid => dev.uuid.clone(),
        Column::Model => dev.model.clone(),
        Column::Serial => dev.serial.clone(),
        Column::Ro => {
            if dev.read_only { "1".to_string() } else { "0".to_string() }
        }
        Column::Rm => {
            if dev.removable { "1".to_string() } else { "0".to_string() }
        }
    }
}

// ============================================================================
// Table output (tree and list modes)
// ============================================================================

/// A single row in the output table.
struct Row {
    values: Vec<String>,
}

/// Build all output rows from the device list. In tree mode, partitions are
/// prefixed with tree-drawing characters. In list mode, everything is flat.
fn build_rows(
    devices: &[BlockDevice],
    columns: &[Column],
    bytes_mode: bool,
    list_mode: bool,
) -> Vec<Row> {
    let mut rows = Vec::new();

    for dev in devices {
        let values: Vec<String> = columns
            .iter()
            .map(|c| column_value(dev, *c, bytes_mode, ""))
            .collect();
        rows.push(Row { values });

        if list_mode {
            // Flat: just append children without tree prefixes.
            for child in &dev.children {
                let values: Vec<String> = columns
                    .iter()
                    .map(|c| column_value(child, *c, bytes_mode, ""))
                    .collect();
                rows.push(Row { values });
            }
        } else {
            // Tree mode: add tree-drawing prefixes to partitions.
            let child_count = dev.children.len();
            for (idx, child) in dev.children.iter().enumerate() {
                let is_last = idx == child_count - 1;
                let tree_prefix = if is_last {
                    "\u{2514}\u{2500}"  // "└─"
                } else {
                    "\u{251C}\u{2500}"  // "├─"
                };
                let values: Vec<String> = columns
                    .iter()
                    .map(|c| {
                        if *c == Column::Name {
                            column_value(child, *c, bytes_mode, tree_prefix)
                        } else {
                            column_value(child, *c, bytes_mode, "")
                        }
                    })
                    .collect();
                rows.push(Row { values });
            }
        }
    }

    rows
}

/// Compute column widths by taking the maximum of header and all row values.
fn compute_widths(columns: &[Column], rows: &[Row]) -> Vec<usize> {
    let mut widths: Vec<usize> = columns
        .iter()
        .map(|c| c.header().len().max(c.default_width()))
        .collect();

    for row in rows {
        for (i, val) in row.values.iter().enumerate() {
            if i < widths.len() {
                let display_len = display_width(val);
                if display_len > widths[i] {
                    widths[i] = display_len;
                }
            }
        }
    }

    widths
}

/// Compute the display width of a string, accounting for multi-byte
/// tree-drawing characters (each box-drawing char is one column wide).
fn display_width(s: &str) -> usize {
    // Each character counts as one column. Box-drawing characters
    // (U+2500..U+257F) used for tree lines are single-column wide.
    s.chars().count()
}

/// Pad a string to a target display width.
fn pad_to_width(s: &str, target: usize) -> String {
    let current = display_width(s);
    if current >= target {
        s.to_string()
    } else {
        let padding = target - current;
        format!("{s}{}", " ".repeat(padding))
    }
}

/// Print the table with headers and rows.
fn print_table(columns: &[Column], rows: &[Row]) {
    if columns.is_empty() {
        return;
    }

    let widths = compute_widths(columns, rows);

    // Print header.
    let header_parts: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| pad_to_width(c.header(), widths[i]))
        .collect();
    println!("{}", header_parts.join(" ").trim_end());

    // Print rows.
    for row in rows {
        let parts: Vec<String> = row
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if i < widths.len() {
                    pad_to_width(v, widths[i])
                } else {
                    v.clone()
                }
            })
            .collect();
        println!("{}", parts.join(" ").trim_end());
    }
}

// ============================================================================
// JSON output
// ============================================================================

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Print all devices as JSON.
fn print_json(devices: &[BlockDevice], columns: &[Column], bytes_mode: bool) {
    println!("{{");
    println!("  \"blockdevices\": [");

    for (di, dev) in devices.iter().enumerate() {
        print!("    {{");
        print_json_fields(dev, columns, bytes_mode);

        if dev.children.is_empty() {
            if di < devices.len() - 1 {
                println!("}},"  );
            } else {
                println!("}}"  );
            }
        } else {
            println!(",");
            println!("      \"children\": [");

            for (ci, child) in dev.children.iter().enumerate() {
                print!("        {{");
                print_json_fields(child, columns, bytes_mode);

                if ci < dev.children.len() - 1 {
                    println!("}},");
                } else {
                    println!("}}");
                }
            }

            print!("      ]");
            if di < devices.len() - 1 {
                println!("}},");
            } else {
                println!("}}");
            }
        }
    }

    println!("  ]");
    println!("}}");
}

/// Print the JSON key-value pairs for one device (no enclosing braces).
fn print_json_fields(dev: &BlockDevice, columns: &[Column], bytes_mode: bool) {
    let mut first = true;
    for col in columns {
        if !first {
            print!(", ");
        }
        first = false;

        let key = col.header().to_ascii_lowercase();
        let val = column_value(dev, *col, bytes_mode, "");

        // For SIZE in bytes mode, emit as a number; otherwise as a string.
        if *col == Column::Size && bytes_mode {
            print!("\"{}\":{}", json_escape(&key), dev.size_bytes());
        } else if *col == Column::Ro || *col == Column::Rm {
            // Boolean-like fields: emit as JSON booleans.
            let b = if val == "1" { "true" } else { "false" };
            print!("\"{}\": {b}", json_escape(&key));
        } else {
            print!("\"{}\": \"{}\"", json_escape(&key), json_escape(&val));
        }
    }
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    println!("OurOS Block Device Lister v0.1.0");
    println!();
    println!("List information about block devices.");
    println!();
    println!("USAGE:");
    println!("  lsblk [options]");
    println!();
    println!("OPTIONS:");
    println!("  -l              List format (flat, no tree lines)");
    println!("  -f              Show filesystem info (type, label, UUID, mount)");
    println!("  -b              Show sizes in bytes instead of human-readable");
    println!("  -o <columns>    Comma-separated list of output columns");
    println!("  --json          JSON output");
    println!("  --help, -h      Show this help");
    println!();
    println!("COLUMNS:");
    println!("  NAME       Device name");
    println!("  SIZE       Device size");
    println!("  TYPE       Device type (disk/part)");
    println!("  FSTYPE     Filesystem type");
    println!("  MOUNTPOINT Mount point");
    println!("  LABEL      Filesystem label");
    println!("  UUID       Filesystem UUID");
    println!("  MODEL      Device model");
    println!("  SERIAL     Device serial number");
    println!("  RO         Read-only flag");
    println!("  RM         Removable flag");
}

fn parse_columns(spec: &str) -> Result<Vec<Column>, String> {
    let mut cols = Vec::new();
    for name in spec.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        match Column::from_str(name) {
            Some(c) => cols.push(c),
            None => return Err(format!("unknown column: {name}")),
        }
    }
    if cols.is_empty() {
        return Err("no columns specified".to_string());
    }
    Ok(cols)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        list_mode: false,
        fs_mode: false,
        bytes_mode: false,
        json: false,
        columns: Vec::new(),
    };

    let mut custom_columns = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "--list" => {
                config.list_mode = true;
                i += 1;
            }
            "-f" | "--fs" => {
                config.fs_mode = true;
                i += 1;
            }
            "-b" | "--bytes" => {
                config.bytes_mode = true;
                i += 1;
            }
            "--json" => {
                config.json = true;
                i += 1;
            }
            "-o" | "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -o requires a comma-separated list of columns");
                    process::exit(1);
                }
                match parse_columns(&args[i + 1]) {
                    Ok(cols) => {
                        config.columns = cols;
                        custom_columns = true;
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                // Handle combined -o<columns> without space.
                if let Some(rest) = other.strip_prefix("-o") {
                    match parse_columns(rest) {
                        Ok(cols) => {
                            config.columns = cols;
                            custom_columns = true;
                        }
                        Err(e) => {
                            eprintln!("error: {e}");
                            process::exit(1);
                        }
                    }
                    i += 1;
                } else {
                    eprintln!("unknown option: {other}");
                    eprintln!("Try 'lsblk --help' for more information.");
                    process::exit(1);
                }
            }
        }
    }

    // Resolve column list: explicit > -f > default.
    if !custom_columns {
        if config.fs_mode {
            config.columns = FS_COLUMNS.to_vec();
        } else {
            config.columns = DEFAULT_COLUMNS.to_vec();
        }
    }

    // Gather mount information.
    let mounts = parse_mounts();

    // Scan block devices.
    let mut devices = scan_sysfs(&mounts);
    if devices.is_empty() {
        devices = scan_proc_partitions(&mounts);
    }

    if devices.is_empty() {
        eprintln!("No block devices found (is /sys/block or /proc/partitions available?)");
        process::exit(1);
    }

    // Output.
    if config.json {
        print_json(&devices, &config.columns, config.bytes_mode);
    } else {
        let rows = build_rows(
            &devices,
            &config.columns,
            config.bytes_mode,
            config.list_mode,
        );
        print_table(&config.columns, &rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Column::from_str --------------------------------------------------

    #[test]
    fn column_from_str_canonical_names() {
        assert_eq!(Column::from_str("NAME"), Some(Column::Name));
        assert_eq!(Column::from_str("SIZE"), Some(Column::Size));
        assert_eq!(Column::from_str("TYPE"), Some(Column::Type));
        assert_eq!(Column::from_str("MOUNTPOINT"), Some(Column::MountPoint));
        assert_eq!(Column::from_str("FSTYPE"), Some(Column::FsType));
        assert_eq!(Column::from_str("UUID"), Some(Column::Uuid));
    }

    #[test]
    fn column_from_str_case_insensitive() {
        assert_eq!(Column::from_str("name"), Some(Column::Name));
        assert_eq!(Column::from_str("Name"), Some(Column::Name));
        assert_eq!(Column::from_str("uuid"), Some(Column::Uuid));
    }

    #[test]
    fn column_from_str_aliases() {
        assert_eq!(Column::from_str("FS"), Some(Column::FsType));
        assert_eq!(Column::from_str("MOUNT"), Some(Column::MountPoint));
    }

    #[test]
    fn column_from_str_unknown_returns_none() {
        assert_eq!(Column::from_str("BANANA"), None);
        assert_eq!(Column::from_str(""), None);
    }

    // ---- Column::header / default_width ------------------------------------

    #[test]
    fn column_headers_match_names() {
        // The header text is what shows up at the top of the table; keep it
        // upper-case and matching the human label.
        assert_eq!(Column::Name.header(), "NAME");
        assert_eq!(Column::FsType.header(), "FSTYPE");
        assert_eq!(Column::MountPoint.header(), "MOUNTPOINT");
    }

    #[test]
    fn column_default_widths_are_positive() {
        for col in [
            Column::Name, Column::Size, Column::Type, Column::FsType,
            Column::MountPoint, Column::Label, Column::Uuid, Column::Model,
            Column::Serial, Column::Ro, Column::Rm,
        ] {
            assert!(col.default_width() > 0, "{:?} width is 0", col.header());
        }
    }

    // ---- parse_columns -----------------------------------------------------

    #[test]
    fn parse_columns_single() {
        assert_eq!(parse_columns("NAME"), Ok(vec![Column::Name]));
    }

    #[test]
    fn parse_columns_multiple_in_order() {
        let r = parse_columns("NAME,SIZE,UUID").expect("valid");
        assert_eq!(r, vec![Column::Name, Column::Size, Column::Uuid]);
    }

    #[test]
    fn parse_columns_ignores_whitespace_and_empty() {
        let r = parse_columns(" NAME , SIZE ,, ").expect("valid");
        assert_eq!(r, vec![Column::Name, Column::Size]);
    }

    #[test]
    fn parse_columns_unknown_is_error() {
        let err = parse_columns("NAME,FAKE").expect_err("must reject FAKE");
        assert!(err.contains("FAKE"), "got: {err}");
    }

    #[test]
    fn parse_columns_empty_is_error() {
        // Empty after trimming/skipping.
        assert!(parse_columns("").is_err());
        assert!(parse_columns(", ,").is_err());
    }

    // ---- format_size -------------------------------------------------------

    #[test]
    fn format_size_bytes_under_kib() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1023), "1023B");
    }

    #[test]
    fn format_size_kib_range() {
        assert_eq!(format_size(1024), "1K");
        assert_eq!(format_size(1024 + 512), "1.5K");
    }

    #[test]
    fn format_size_mib_range() {
        assert_eq!(format_size(1024 * 1024), "1M");
        assert_eq!(format_size(10 * 1024 * 1024), "10M");
    }

    #[test]
    fn format_size_gib_range() {
        assert_eq!(format_size(1024_u64.pow(3)), "1G");
        assert_eq!(format_size(8 * 1024_u64.pow(3)), "8G");
    }

    #[test]
    fn format_size_tib_range() {
        assert_eq!(format_size(1024_u64.pow(4)), "1T");
    }

    // ---- parse_mounts via inline parsing (test the data, not the file) -----

    #[test]
    fn parse_mounts_strips_dev_prefix() {
        // We can't easily mock /proc/mounts, but we can test the path
        // stripping logic by inserting expected entries. The logic
        // is `dev.strip_prefix("/dev/").unwrap_or(dev)`. Verify that
        // behavior against expected inputs.
        let stripped = "/dev/sda1".strip_prefix("/dev/").unwrap_or("/dev/sda1");
        assert_eq!(stripped, "sda1");
        let untouched = "tmpfs".strip_prefix("/dev/").unwrap_or("tmpfs");
        assert_eq!(untouched, "tmpfs");
    }

    // ---- BlockDevice::size_bytes -------------------------------------------

    fn make_dev(name: &str, sectors: u64) -> BlockDevice {
        BlockDevice {
            name: name.to_string(),
            size_sectors: sectors,
            dev_type: "disk".to_string(),
            fstype: String::new(),
            label: String::new(),
            uuid: String::new(),
            mountpoint: String::new(),
            model: String::new(),
            serial: String::new(),
            read_only: false,
            removable: false,
            children: Vec::new(),
        }
    }

    #[test]
    fn block_device_size_bytes_is_sectors_times_512() {
        let dev = make_dev("sda", 2_000);
        assert_eq!(dev.size_bytes(), 2_000 * 512);
    }

    #[test]
    fn block_device_size_bytes_saturates_on_overflow() {
        // A sector count near u64::MAX shouldn't panic.
        let dev = make_dev("huge", u64::MAX);
        assert_eq!(dev.size_bytes(), u64::MAX);
    }

    // ---- column_value ------------------------------------------------------

    #[test]
    fn column_value_name_uses_prefix() {
        let dev = make_dev("sda1", 0);
        let v = column_value(&dev, Column::Name, false, "├─");
        assert_eq!(v, "├─sda1");
    }

    #[test]
    fn column_value_size_human() {
        let dev = make_dev("sda", 2_097_152); // 2_097_152 * 512 = 1 GiB
        let v = column_value(&dev, Column::Size, false, "");
        assert_eq!(v, "1G");
    }

    #[test]
    fn column_value_size_bytes_mode() {
        let dev = make_dev("sda", 100);
        let v = column_value(&dev, Column::Size, true, "");
        assert_eq!(v, "51200");
    }

    #[test]
    fn column_value_ro_rm_are_one_or_zero() {
        let mut dev = make_dev("sda", 0);
        assert_eq!(column_value(&dev, Column::Ro, false, ""), "0");
        assert_eq!(column_value(&dev, Column::Rm, false, ""), "0");
        dev.read_only = true;
        dev.removable = true;
        assert_eq!(column_value(&dev, Column::Ro, false, ""), "1");
        assert_eq!(column_value(&dev, Column::Rm, false, ""), "1");
    }

    #[test]
    fn column_value_string_columns_pass_through() {
        let mut dev = make_dev("sda", 0);
        dev.fstype = "ext4".to_string();
        dev.uuid = "abc-123".to_string();
        dev.label = "boot".to_string();
        dev.mountpoint = "/".to_string();
        dev.model = "WD Blue".to_string();
        dev.serial = "SN12345".to_string();
        assert_eq!(column_value(&dev, Column::FsType, false, ""), "ext4");
        assert_eq!(column_value(&dev, Column::Uuid, false, ""), "abc-123");
        assert_eq!(column_value(&dev, Column::Label, false, ""), "boot");
        assert_eq!(column_value(&dev, Column::MountPoint, false, ""), "/");
        assert_eq!(column_value(&dev, Column::Model, false, ""), "WD Blue");
        assert_eq!(column_value(&dev, Column::Serial, false, ""), "SN12345");
        assert_eq!(column_value(&dev, Column::Type, false, ""), "disk");
    }

    // ---- display_width and pad_to_width ------------------------------------

    #[test]
    fn display_width_counts_unicode_box_chars_as_one() {
        // Each box-drawing glyph (├ ─ └) counts as one column wide.
        assert_eq!(display_width("├─sda1"), 6);
        assert_eq!(display_width("└─sda1"), 6);
        assert_eq!(display_width("plain"), 5);
    }

    #[test]
    fn pad_to_width_pads_with_spaces() {
        assert_eq!(pad_to_width("ab", 5), "ab   ");
    }

    #[test]
    fn pad_to_width_does_not_truncate() {
        // Longer than target -> returned unchanged.
        assert_eq!(pad_to_width("longer string", 4), "longer string");
    }

    #[test]
    fn pad_to_width_handles_unicode() {
        // "├─" has 2 display columns; padded to 5 needs 3 spaces.
        assert_eq!(pad_to_width("├─", 5), "├─   ");
    }

    // ---- build_rows --------------------------------------------------------

    #[test]
    fn build_rows_tree_mode_uses_box_chars_for_partitions() {
        let mut disk = make_dev("sda", 0);
        disk.children.push(make_dev("sda1", 0));
        disk.children.push(make_dev("sda2", 0));
        let cols = [Column::Name];
        let rows = build_rows(std::slice::from_ref(&disk), &cols, false, false);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].values[0], "sda");
        // First child uses ├─, last uses └─.
        assert_eq!(rows[1].values[0], "├─sda1");
        assert_eq!(rows[2].values[0], "└─sda2");
    }

    #[test]
    fn build_rows_list_mode_flattens_without_prefix() {
        let mut disk = make_dev("sda", 0);
        disk.children.push(make_dev("sda1", 0));
        let cols = [Column::Name];
        let rows = build_rows(std::slice::from_ref(&disk), &cols, false, true);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values[0], "sda");
        // No box-drawing prefix in list mode.
        assert_eq!(rows[1].values[0], "sda1");
    }

    #[test]
    fn build_rows_no_children_emits_one_row() {
        let disk = make_dev("sda", 0);
        let cols = [Column::Name, Column::Size];
        let rows = build_rows(std::slice::from_ref(&disk), &cols, false, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values, vec!["sda", "0B"]);
    }

    // ---- compute_widths ----------------------------------------------------

    #[test]
    fn compute_widths_uses_max_of_header_default_and_values() {
        let row = Row { values: vec!["a-long-name-here".to_string()] };
        let widths = compute_widths(&[Column::Name], std::slice::from_ref(&row));
        assert_eq!(widths.len(), 1);
        assert_eq!(widths[0], "a-long-name-here".len());
    }

    #[test]
    fn compute_widths_falls_back_to_default_when_values_are_short() {
        let row = Row { values: vec!["x".to_string()] };
        let widths = compute_widths(&[Column::Uuid], std::slice::from_ref(&row));
        // UUID default width is 36; "x" doesn't push it higher.
        assert_eq!(widths[0], 36);
    }

    // ---- json_escape -------------------------------------------------------

    #[test]
    fn json_escape_basic_passthrough() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn json_escape_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"he said "hi""#), r#"he said \"hi\""#);
        assert_eq!(json_escape(r"a\b"), r"a\\b");
    }

    #[test]
    fn json_escape_control_chars() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("\x01"), "\\u0001");
    }

    // ---- Default column sets ------------------------------------------------

    #[test]
    fn default_columns_contains_name_and_size() {
        assert!(DEFAULT_COLUMNS.contains(&Column::Name));
        assert!(DEFAULT_COLUMNS.contains(&Column::Size));
    }

    #[test]
    fn fs_columns_contains_filesystem_info() {
        assert!(FS_COLUMNS.contains(&Column::FsType));
        assert!(FS_COLUMNS.contains(&Column::Uuid));
        assert!(FS_COLUMNS.contains(&Column::Label));
    }
}
