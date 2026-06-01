//! OurOS memory information display utility.
//!
//! Multi-personality binary providing:
//! - **lsmem** — list the ranges of available memory with their online status
//!
//! Reads memory block information from /sys/devices/system/memory/ and
//! /proc/meminfo to display memory topology.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct MemoryBlock {
    /// Block index.
    index: u32,
    /// Start physical address.
    phys_start: u64,
    /// Size of the block.
    size: u64,
    /// Whether the block is online.
    online: bool,
    /// Whether the block is removable.
    removable: bool,
    /// NUMA node.
    node: Option<u32>,
    /// Memory zone (e.g., "Normal", "DMA32").
    zone: String,
    /// State string.
    state: String,
}

#[derive(Clone, Debug)]
struct MemoryRange {
    start: u64,
    end: u64,
    size: u64,
    state: String,
    removable: bool,
    block_count: u32,
    node: Option<u32>,
    zone: String,
}

struct LsmemOpts {
    json: bool,
    raw: bool,
    pairs: bool,
    noheadings: bool,
    bytes: bool,
    all: bool,
    summary: SummaryMode,
    columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum SummaryMode {
    Auto,
    Only,
    Never,
}

// ============================================================================
// Memory block enumeration
// ============================================================================

fn read_memory_blocks() -> Vec<MemoryBlock> {
    let mut blocks = Vec::new();
    let base = "/sys/devices/system/memory";

    let block_size = fs::read_to_string(format!("{base}/block_size_bytes"))
        .ok()
        .and_then(|s| u64::from_str_radix(s.trim(), 16).ok())
        .unwrap_or(128 * 1024 * 1024); // Default: 128 MiB.

    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("memory") || name == "memory" {
                continue;
            }
            let index_str = name.strip_prefix("memory").unwrap_or("");
            let index: u32 = match index_str.parse() {
                Ok(i) => i,
                Err(_) => continue,
            };

            let block_path = format!("{base}/{name}");

            let state = fs::read_to_string(format!("{block_path}/state"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "online".to_string());

            let online = state == "online";

            let removable = fs::read_to_string(format!("{block_path}/removable"))
                .map(|s| s.trim() == "1")
                .unwrap_or(false);

            let node = fs::read_dir(&block_path)
                .ok()
                .and_then(|entries| {
                    entries.flatten().find_map(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        n.strip_prefix("node")
                            .and_then(|s| s.parse::<u32>().ok())
                    })
                });

            let zone = fs::read_to_string(format!("{block_path}/valid_zones"))
                .map(|s| {
                    s.split_whitespace()
                        .next()
                        .unwrap_or("Normal")
                        .to_string()
                })
                .unwrap_or_else(|_| "Normal".to_string());

            blocks.push(MemoryBlock {
                index,
                phys_start: index as u64 * block_size,
                size: block_size,
                online,
                removable,
                node,
                zone,
                state,
            });
        }
    }

    blocks.sort_by_key(|b| b.index);
    blocks
}

/// Merge contiguous memory blocks with the same properties into ranges.
fn merge_blocks(blocks: &[MemoryBlock]) -> Vec<MemoryRange> {
    if blocks.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut current_start = blocks[0].phys_start;
    let mut current_size = blocks[0].size;
    let mut current_state = blocks[0].state.clone();
    let mut current_removable = blocks[0].removable;
    let mut current_node = blocks[0].node;
    let mut current_zone = blocks[0].zone.clone();
    let mut block_count = 1u32;

    for block in blocks.iter().skip(1) {
        // Merge if contiguous and same properties.
        if block.phys_start == current_start + current_size
            && block.state == current_state
            && block.removable == current_removable
            && block.node == current_node
            && block.zone == current_zone
        {
            current_size += block.size;
            block_count += 1;
        } else {
            ranges.push(MemoryRange {
                start: current_start,
                end: current_start + current_size - 1,
                size: current_size,
                state: current_state.clone(),
                removable: current_removable,
                block_count,
                node: current_node,
                zone: current_zone.clone(),
            });
            current_start = block.phys_start;
            current_size = block.size;
            current_state = block.state.clone();
            current_removable = block.removable;
            current_node = block.node;
            current_zone = block.zone.clone();
            block_count = 1;
        }
    }

    ranges.push(MemoryRange {
        start: current_start,
        end: current_start + current_size - 1,
        size: current_size,
        state: current_state,
        removable: current_removable,
        block_count,
        node: current_node,
        zone: current_zone,
    });

    ranges
}

/// Get total memory from /proc/meminfo as fallback.
fn get_meminfo_total() -> u64 {
    if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("MemTotal:") {
                let val = val.trim();
                if let Some(kb_str) = val.strip_suffix("kB").or_else(|| val.strip_suffix("KB"))
                    && let Ok(kb) = kb_str.trim().parse::<u64>() {
                        return kb * 1024;
                    }
            }
        }
    }
    0
}

// ============================================================================
// Formatting
// ============================================================================

fn format_size(bytes: u64, use_bytes: bool) -> String {
    if use_bytes {
        return bytes.to_string();
    }
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}G", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{}M", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{bytes}B")
    }
}

fn format_address(addr: u64) -> String {
    format!("0x{addr:016x}")
}

fn default_columns() -> Vec<String> {
    vec![
        "RANGE".to_string(),
        "SIZE".to_string(),
        "STATE".to_string(),
        "REMOVABLE".to_string(),
        "BLOCK".to_string(),
    ]
}

fn column_value(range: &MemoryRange, col: &str, use_bytes: bool) -> String {
    match col.to_uppercase().as_str() {
        "RANGE" => format!("{}-{}", format_address(range.start), format_address(range.end)),
        "SIZE" => format_size(range.size, use_bytes),
        "STATE" => range.state.clone(),
        "REMOVABLE" => if range.removable { "yes".to_string() } else { "no".to_string() },
        "BLOCK" => {
            if range.block_count == 1 {
                format!("{}", range.start / (128 * 1024 * 1024))
            } else {
                let first = range.start / (128 * 1024 * 1024);
                format!("{}-{}", first, first + range.block_count as u64 - 1)
            }
        }
        "NODE" => range.node.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string()),
        "ZONES" => range.zone.clone(),
        _ => String::new(),
    }
}

// ============================================================================
// Output
// ============================================================================

fn print_table(out: &mut io::StdoutLock<'_>, ranges: &[MemoryRange], opts: &LsmemOpts) {
    let cols = if opts.columns.is_empty() { default_columns() } else { opts.columns.clone() };

    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    for range in ranges {
        for (i, col) in cols.iter().enumerate() {
            let val = column_value(range, col, opts.bytes);
            if val.len() > widths[i] {
                widths[i] = val.len();
            }
        }
    }

    if !opts.noheadings {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, " ");
            }
            let _ = write!(out, "{:>width$}", col, width = widths[i]);
        }
        let _ = writeln!(out);
    }

    for range in ranges {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, " ");
            }
            let val = column_value(range, col, opts.bytes);
            let _ = write!(out, "{:>width$}", val, width = widths[i]);
        }
        let _ = writeln!(out);
    }
}

fn print_json(out: &mut io::StdoutLock<'_>, ranges: &[MemoryRange], opts: &LsmemOpts) {
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"memory\": [");
    for (i, range) in ranges.iter().enumerate() {
        let comma = if i + 1 < ranges.len() { "," } else { "" };
        let _ = writeln!(
            out,
            "    {{\"range\": \"{}-{}\", \"size\": {}, \"state\": \"{}\", \"removable\": {}, \"block\": {}}}{comma}",
            format_address(range.start),
            format_address(range.end),
            range.size,
            range.state,
            range.removable,
            range.block_count,
        );
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    let _ = opts;
}

fn print_summary(out: &mut io::StdoutLock<'_>, ranges: &[MemoryRange], blocks: &[MemoryBlock], opts: &LsmemOpts) {
    let total_online: u64 = blocks.iter().filter(|b| b.online).map(|b| b.size).sum();
    let total_offline: u64 = blocks.iter().filter(|b| !b.online).map(|b| b.size).sum();
    let total = total_online + total_offline;

    // If no sysfs data, use /proc/meminfo.
    let total = if total == 0 {
        let mem = get_meminfo_total();
        let _ = writeln!(out);
        let _ = writeln!(out, "Memory block size:       unknown");
        let _ = writeln!(out, "Total online memory:     {}", format_size(mem, opts.bytes));
        let _ = writeln!(out, "Total offline memory:    0B");
        let _ = writeln!(out, "Total memory:            {}", format_size(mem, opts.bytes));
        return;
    } else {
        total
    };

    let block_size = if !blocks.is_empty() { blocks[0].size } else { 128 * 1024 * 1024 };

    let _ = writeln!(out);
    let _ = writeln!(out, "Memory block size:       {}", format_size(block_size, opts.bytes));
    let _ = writeln!(out, "Total online memory:     {}", format_size(total_online, opts.bytes));
    let _ = writeln!(out, "Total offline memory:    {}", format_size(total_offline, opts.bytes));
    let _ = writeln!(out, "Total memory:            {}", format_size(total, opts.bytes));
    let _ = ranges;
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut opts = LsmemOpts {
        json: false,
        raw: false,
        pairs: false,
        noheadings: false,
        bytes: false,
        all: false,
        summary: SummaryMode::Auto,
        columns: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: lsmem [options]");
                println!();
                println!("List the ranges of available memory with their online status.");
                println!();
                println!("Options:");
                println!("  -J, --json           JSON output");
                println!("  -r, --raw            Raw output");
                println!("  -P, --pairs          Key=value output");
                println!("  -n, --noheadings     No headers");
                println!("  -b, --bytes          Show sizes in bytes");
                println!("  -a, --all            Show all memory ranges");
                println!("  -o, --output COLS    Columns (RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES)");
                println!("  -s, --summary[=WHEN] Summary (auto, only, never)");
                println!("  -h, --help           Show this help");
                println!("  -V, --version        Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lsmem {VERSION}");
                process::exit(0);
            }
            "-J" | "--json" => opts.json = true,
            "-r" | "--raw" => opts.raw = true,
            "-P" | "--pairs" => opts.pairs = true,
            "-n" | "--noheadings" => opts.noheadings = true,
            "-b" | "--bytes" => opts.bytes = true,
            "-a" | "--all" => opts.all = true,
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    opts.columns = args[i].split(',').map(|s| s.trim().to_uppercase()).collect();
                }
            }
            s if s.starts_with("--summary") => {
                if let Some(val) = s.strip_prefix("--summary=") {
                    opts.summary = match val {
                        "only" => SummaryMode::Only,
                        "never" => SummaryMode::Never,
                        _ => SummaryMode::Auto,
                    };
                } else {
                    opts.summary = SummaryMode::Only;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let blocks = read_memory_blocks();
    let ranges = merge_blocks(&blocks);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match opts.summary {
        SummaryMode::Only => {
            print_summary(&mut out, &ranges, &blocks, &opts);
        }
        SummaryMode::Never => {
            if opts.json {
                print_json(&mut out, &ranges, &opts);
            } else {
                print_table(&mut out, &ranges, &opts);
            }
        }
        SummaryMode::Auto => {
            if opts.json {
                print_json(&mut out, &ranges, &opts);
            } else {
                print_table(&mut out, &ranges, &opts);
            }
            print_summary(&mut out, &ranges, &blocks, &opts);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(1024, true), "1024");
        assert_eq!(format_size(1048576, true), "1048576");
    }

    #[test]
    fn test_format_size_human() {
        assert_eq!(format_size(1024, false), "1K");
        assert_eq!(format_size(1024 * 1024, false), "1M");
        assert_eq!(format_size(512, false), "512B");
    }

    #[test]
    fn test_format_size_gig() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(format_size(gb, false), "1.0G");
        assert_eq!(format_size(2 * gb, false), "2.0G");
    }

    #[test]
    fn test_format_address() {
        assert_eq!(format_address(0), "0x0000000000000000");
        assert_eq!(format_address(0x100000), "0x0000000000100000");
    }

    #[test]
    fn test_default_columns() {
        let cols = default_columns();
        assert_eq!(cols.len(), 5);
        assert!(cols.contains(&"RANGE".to_string()));
        assert!(cols.contains(&"SIZE".to_string()));
        assert!(cols.contains(&"STATE".to_string()));
    }

    #[test]
    fn test_merge_blocks_empty() {
        let ranges = merge_blocks(&[]);
        assert!(ranges.is_empty());
    }

    #[test]
    fn test_merge_blocks_single() {
        let blocks = vec![MemoryBlock {
            index: 0,
            phys_start: 0,
            size: 128 * 1024 * 1024,
            online: true,
            removable: false,
            node: Some(0),
            zone: "Normal".to_string(),
            state: "online".to_string(),
        }];
        let ranges = merge_blocks(&blocks);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].block_count, 1);
    }

    #[test]
    fn test_merge_blocks_contiguous() {
        let bs = 128 * 1024 * 1024;
        let blocks = vec![
            MemoryBlock {
                index: 0, phys_start: 0, size: bs, online: true,
                removable: false, node: Some(0), zone: "Normal".to_string(), state: "online".to_string(),
            },
            MemoryBlock {
                index: 1, phys_start: bs, size: bs, online: true,
                removable: false, node: Some(0), zone: "Normal".to_string(), state: "online".to_string(),
            },
        ];
        let ranges = merge_blocks(&blocks);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].size, 2 * bs);
        assert_eq!(ranges[0].block_count, 2);
    }

    #[test]
    fn test_merge_blocks_different_state() {
        let bs = 128 * 1024 * 1024;
        let blocks = vec![
            MemoryBlock {
                index: 0, phys_start: 0, size: bs, online: true,
                removable: false, node: Some(0), zone: "Normal".to_string(), state: "online".to_string(),
            },
            MemoryBlock {
                index: 1, phys_start: bs, size: bs, online: false,
                removable: false, node: Some(0), zone: "Normal".to_string(), state: "offline".to_string(),
            },
        ];
        let ranges = merge_blocks(&blocks);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn test_column_value_range() {
        let range = MemoryRange {
            start: 0,
            end: 0x7FFFFFF,
            size: 128 * 1024 * 1024,
            state: "online".to_string(),
            removable: false,
            block_count: 1,
            node: Some(0),
            zone: "Normal".to_string(),
        };
        let val = column_value(&range, "STATE", false);
        assert_eq!(val, "online");
    }

    #[test]
    fn test_column_value_removable() {
        let range = MemoryRange {
            start: 0, end: 0, size: 0,
            state: "online".to_string(), removable: true,
            block_count: 1, node: None, zone: "Normal".to_string(),
        };
        assert_eq!(column_value(&range, "REMOVABLE", false), "yes");

        let range2 = MemoryRange { removable: false, ..range.clone() };
        assert_eq!(column_value(&range2, "REMOVABLE", false), "no");
    }

    #[test]
    fn test_column_value_node() {
        let range = MemoryRange {
            start: 0, end: 0, size: 0,
            state: "online".to_string(), removable: false,
            block_count: 1, node: Some(0), zone: "Normal".to_string(),
        };
        assert_eq!(column_value(&range, "NODE", false), "0");

        let range_no_node = MemoryRange { node: None, ..range.clone() };
        assert_eq!(column_value(&range_no_node, "NODE", false), "-");
    }

    #[test]
    fn test_summary_mode() {
        assert_eq!(SummaryMode::Auto, SummaryMode::Auto);
        assert_ne!(SummaryMode::Only, SummaryMode::Never);
    }

    #[test]
    fn test_read_memory_blocks_no_crash() {
        let _ = read_memory_blocks();
    }

    #[test]
    fn test_get_meminfo_total_no_crash() {
        let total = get_meminfo_total();
        // On non-Linux, may be 0.
        let _ = total;
    }
}
