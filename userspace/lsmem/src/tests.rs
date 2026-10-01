//! The pieces of `lsmem.c`, each against what util-linux 2.39.3 does with
//! the same input. The program as a whole is compared with upstream's by
//! `scripts/lsmem-diff.sh`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use std::fs;

fn block(index: u64, state: MemState) -> MemoryBlock {
    MemoryBlock {
        index,
        count: 1,
        state,
        node: 0,
        zones: Vec::new(),
        removable: true,
    }
}

#[test]
fn the_help_is_upstreams() {
    let text = String::from_utf8(usage(b"lsmem")).unwrap();
    assert!(text.starts_with("\nUsage:\n lsmem [options]\n\nList the ranges"));
    assert!(text.contains("\n -J, --json           use JSON output format\n"));
    assert!(text.contains("\n      RANGE  start and end address of the memory range\n"));
    assert!(text.contains("\n  REMOVABLE  memory is removable\n"));
    assert!(text.ends_with("\nFor more details see lsmem(1).\n"));
}

#[test]
fn exclusive_options_are_refused_on_the_second_of_a_group() {
    let j = i32::from(b'J');
    let r = i32::from(b'r');
    let mut st = [0; 2];
    assert!(err_exclusive_options(j, &mut st, b"lsmem").is_ok());
    // The same option again is not a collision.
    assert!(err_exclusive_options(j, &mut st, b"lsmem").is_ok());
    assert!(err_exclusive_options(i32::from(b'a'), &mut st, b"lsmem").is_ok());
    assert!(err_exclusive_options(OPT_SUMMARY, &mut st, b"lsmem").is_ok());
    assert_eq!(err_exclusive_options(r, &mut st, b"lsmem"), Err(1));
    let mut st = [0; 2];
    assert!(err_exclusive_options(i32::from(b'a'), &mut st, b"lsmem").is_ok());
    assert_eq!(
        err_exclusive_options(i32::from(b'S'), &mut st, b"lsmem"),
        Err(1)
    );
}

#[test]
fn each_option_value_names_its_long_option() {
    assert_eq!(option_to_longopt(i32::from(b'J')), Some("json"));
    assert_eq!(option_to_longopt(i32::from(b'P')), Some("pairs"));
    assert_eq!(option_to_longopt(i32::from(b'S')), Some("split"));
    assert_eq!(option_to_longopt(OPT_SUMMARY), Some("summary"));
    assert_eq!(option_to_longopt(OPT_OUTPUT_ALL), Some("output-all"));
    assert_eq!(option_to_longopt(i32::from(b'x')), None);
}

#[test]
fn columns_and_zones_are_named_in_any_case() {
    assert_eq!(
        column_name_to_id(b"range", b"range", b"lsmem"),
        Some(Col::Range)
    );
    assert_eq!(
        column_name_to_id(b"ZoNeS", b"ZoNeS", b"lsmem"),
        Some(Col::Zones)
    );
    assert_eq!(column_name_to_id(b"RANG", b"RANG", b"lsmem"), None);
    assert_eq!(zone_name_to_id(b"normal"), 2);
    assert_eq!(zone_name_to_id(b"DMA32"), 1);
    assert_eq!(zone_name_to_id(b"Sideways"), ZONE_UNKNOWN);
}

#[test]
fn cells_are_formatted_as_upstream_formats_them() {
    let l = Lsmem {
        block_size: 128 << 20,
        have_nodes: true,
        have_zones: true,
        ..Lsmem::default()
    };
    let mut blk = MemoryBlock {
        index: 32,
        count: 225,
        state: MemState::Online,
        node: 0,
        zones: vec![2, 4],
        removable: true,
    };
    assert_eq!(
        cell_text(&l, &blk, Col::Range).unwrap(),
        "0x0000000100000000-0x0000000807ffffff"
    );
    assert_eq!(cell_text(&l, &blk, Col::Size).unwrap(), "28.1G");
    assert_eq!(cell_text(&l, &blk, Col::Block).unwrap(), "32-256");
    assert_eq!(cell_text(&l, &blk, Col::Removable).unwrap(), "yes");
    assert_eq!(cell_text(&l, &blk, Col::Node).unwrap(), "0");
    assert_eq!(cell_text(&l, &blk, Col::Zones).unwrap(), "Normal/Movable");
    blk.state = MemState::GoingOffline;
    assert_eq!(cell_text(&l, &blk, Col::State).unwrap(), "on->off");
    // Removability is shown for online blocks only.
    assert_eq!(cell_text(&l, &blk, Col::Removable), None);
    let bytes = Lsmem {
        bytes: true,
        ..Lsmem::default()
    };
    assert_eq!(cell_text(&bytes, &blk, Col::Node), None);
    assert_eq!(cell_text(&bytes, &blk, Col::Zones), None);
}

#[test]
fn a_block_size_of_zero_wraps_as_unsigned_arithmetic_does() {
    let l = Lsmem {
        bytes: true,
        ..Lsmem::default()
    };
    let blk = block(3, MemState::Unknown);
    assert_eq!(
        cell_text(&l, &blk, Col::Range).unwrap(),
        "0x0000000000000000-0xffffffffffffffff"
    );
    assert_eq!(cell_text(&l, &blk, Col::Size).unwrap(), "0");
    assert_eq!(cell_text(&l, &blk, Col::State).unwrap(), "?");
    assert_eq!(cell_text(&l, &blk, Col::Block).unwrap(), "3");
}

#[test]
fn values_past_2_to_the_63_print_negative_as_prid64_prints_them() {
    let l = Lsmem {
        bytes: true,
        block_size: 1 << 62,
        ..Lsmem::default()
    };
    let mut blk = block(u64::MAX, MemState::Online);
    blk.count = 2;
    assert_eq!(
        cell_text(&l, &blk, Col::Size).unwrap(),
        "-9223372036854775808"
    );
    assert_eq!(cell_text(&l, &blk, Col::Block).unwrap(), "-1-0");
}

#[test]
fn ranges_merge_only_while_every_split_column_agrees() {
    let mut l = Lsmem {
        split_by_state: true,
        split_by_removable: true,
        ..Lsmem::default()
    };
    l.blocks.push(block(0, MemState::Online));
    assert!(is_mergeable(&l, &block(1, MemState::Online)));
    // Not contiguous.
    assert!(!is_mergeable(&l, &block(2, MemState::Online)));
    assert!(!is_mergeable(&l, &block(1, MemState::Offline)));
    let mut not_removable = block(1, MemState::Online);
    not_removable.removable = false;
    assert!(!is_mergeable(&l, &not_removable));
    l.split_by_removable = false;
    assert!(is_mergeable(&l, &not_removable));
    l.list_all = true;
    assert!(!is_mergeable(&l, &block(1, MemState::Online)));
}

#[test]
fn zones_split_a_range_and_an_unknown_zone_always_does() {
    let mut l = Lsmem {
        split_by_zones: true,
        have_zones: true,
        ..Lsmem::default()
    };
    let mut first = block(0, MemState::Online);
    first.zones = vec![2];
    l.blocks.push(first);
    let mut same = block(1, MemState::Online);
    same.zones = vec![2];
    assert!(is_mergeable(&l, &same));
    let mut more = block(1, MemState::Online);
    more.zones = vec![2, 4];
    assert!(!is_mergeable(&l, &more));
    l.blocks[0].zones = vec![ZONE_UNKNOWN];
    let mut unknown = block(1, MemState::Online);
    unknown.zones = vec![ZONE_UNKNOWN];
    assert!(!is_mergeable(&l, &unknown));
    // Without `valid_zones` on the system, zones do not split.
    l.have_zones = false;
    assert!(is_mergeable(&l, &unknown));
}

#[test]
fn numbers_are_converted_as_strtoumax_and_strtol_convert_them() {
    assert_eq!(strtoumax(b"8000000", 16), (0x800_0000, false));
    assert_eq!(strtoumax(b"0x10", 16), (16, false));
    assert_eq!(strtoumax(b"zz", 16), (0, false));
    assert_eq!(strtoumax(b"-1", 16), (u64::MAX, false));
    assert_eq!(strtoumax(b"fffffffffffffffffff", 16), (u64::MAX, true));
    assert_eq!(strtoumax(b"99999999999999999999", 10), (u64::MAX, true));
    assert_eq!(strtol_digits(b"12"), (12, false));
    assert_eq!(strtol_digits(b"9223372036854775808"), (i64::MAX, true));
}

#[test]
fn a_terminal_size_variable_overflows_as_strtol_overflows() {
    assert!(!strtol_overflows(b"80"));
    assert!(!strtol_overflows(b"x"));
    assert!(!strtol_overflows(b"9223372036854775807"));
    assert!(strtol_overflows(b"9223372036854775808"));
    assert!(!strtol_overflows(b"-9223372036854775808"));
    assert!(strtol_overflows(b"-9223372036854775809"));
    assert!(strtol_overflows(b"99999999999999999999999999999999999"));
}

#[test]
fn memory_directories_are_memory_and_digits() {
    assert!(memory_block_filter(b"memory0"));
    assert!(memory_block_filter(b"memory007"));
    assert!(!memory_block_filter(b"memory"));
    assert!(!memory_block_filter(b"memory1a"));
    assert!(!memory_block_filter(b"block_size_bytes"));
}

#[test]
fn split_policy_follows_the_columns_given() {
    let mut l = Lsmem::default();
    set_split_policy(&mut l, &[Col::Range, Col::Node, Col::Zones]);
    assert!(l.split_by_node && l.split_by_zones);
    assert!(!l.split_by_state && !l.split_by_removable);
    set_split_policy(&mut l, &[]);
    assert!(!l.split_by_node && !l.split_by_zones);
}

/// A `/sys/devices/system/memory` under a scratch root.
struct Tree {
    dir: scratchdir::ScratchDir,
}

impl Tree {
    fn new(name: &str) -> Self {
        let dir = scratchdir::ScratchDir::new(name);
        fs::create_dir_all(dir.path("sys/devices/system/memory")).unwrap();
        Tree { dir }
    }

    fn put(&self, rel: &str, text: &str) {
        let path = self.dir.path(&format!("sys/devices/system/memory/{rel}"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn mkdir(&self, rel: &str) {
        fs::create_dir_all(self.dir.path(&format!("sys/devices/system/memory/{rel}"))).unwrap();
    }

    fn sys(&self) -> SysPath {
        let mut sys = SysPath::new(PATH_SYS_MEMORY);
        sys.set_prefix(&os_bytes(self.dir.dir().as_os_str()));
        sys
    }
}

#[test]
fn a_tree_is_read_merged_and_totalled_as_upstream_reads_it() {
    let t = Tree::new("lsmem_tree_read");
    t.put("block_size_bytes", "8000000\n");
    for (n, state) in [(0, "online"), (1, "online"), (2, "offline"), (10, "online")] {
        t.put(&format!("memory{n}/state"), &format!("{state}\n"));
        t.put(&format!("memory{n}/removable"), "1\n");
        t.put(&format!("memory{n}/valid_zones"), "Normal\n");
        t.mkdir(&format!("memory{n}/node0"));
    }
    t.put("not-a-block", "");
    t.mkdir("memoryX");

    let mut l = Lsmem {
        split_by_state: true,
        split_by_removable: true,
        ..Lsmem::default()
    };
    let mut sys = t.sys();
    let mut errno = Errno::Zero;
    read_basic_info(&mut l, &mut sys, &mut errno, b"lsmem").unwrap();
    assert_eq!(
        l.dirs,
        [&b"memory0"[..], b"memory1", b"memory2", b"memory10"]
    );
    assert!(l.have_nodes && l.have_zones);
    read_info(&mut l, &mut sys, &mut errno, b"lsmem").unwrap();
    assert_eq!(l.block_size, 128 << 20);
    assert_eq!(l.mem_online, 3 * (128 << 20));
    assert_eq!(l.mem_offline, 128 << 20);
    let ranges: Vec<(u64, u64, MemState)> = l
        .blocks
        .iter()
        .map(|b| (b.index, b.count, b.state))
        .collect();
    assert_eq!(
        ranges,
        [
            (0, 2, MemState::Online),
            (2, 1, MemState::Offline),
            (10, 1, MemState::Online)
        ]
    );
    assert_eq!(l.blocks[0].zones, [2]);
}

#[test]
fn a_block_size_without_its_newline_loses_its_last_digit() {
    let t = Tree::new("lsmem_tree_bare_size");
    t.put("block_size_bytes", "8000000");
    t.put("memory0/state", "online\n");
    let mut l = Lsmem::default();
    let mut sys = t.sys();
    let mut errno = Errno::Zero;
    read_basic_info(&mut l, &mut sys, &mut errno, b"lsmem").unwrap();
    // No `memory0/valid_zones`: the probe leaves ENOENT behind.
    assert!(matches!(errno, Errno::Os(_)));
    assert!(!l.have_nodes && !l.have_zones);
    read_info(&mut l, &mut sys, &mut errno, b"lsmem").unwrap();
    assert_eq!(l.block_size, 0x80_0000);
}

#[test]
fn a_tree_that_cannot_be_read_is_refused() {
    let t = Tree::new("lsmem_tree_refused");
    let mut sys = t.sys();
    let mut l = Lsmem::default();
    let mut errno = Errno::Zero;
    // No block_size_bytes.
    assert_eq!(
        read_basic_info(&mut l, &mut sys, &mut errno, b"lsmem"),
        Err(1)
    );
    // No blocks.
    t.put("block_size_bytes", "8000000\n");
    let mut sys = t.sys();
    assert_eq!(
        read_basic_info(&mut l, &mut sys, &mut errno, b"lsmem"),
        Err(1)
    );
    // An empty size.
    t.put("block_size_bytes", "");
    t.mkdir("memory0");
    let mut sys = t.sys();
    let mut l = Lsmem::default();
    read_basic_info(&mut l, &mut sys, &mut errno, b"lsmem").unwrap();
    assert_eq!(read_info(&mut l, &mut sys, &mut errno, b"lsmem"), Err(1));
    // A size too big for 64 bits.
    t.put("block_size_bytes", "fffffffffffffffffff\n");
    let mut sys = t.sys();
    let mut l = Lsmem::default();
    read_basic_info(&mut l, &mut sys, &mut errno, b"lsmem").unwrap();
    assert_eq!(read_info(&mut l, &mut sys, &mut errno, b"lsmem"), Err(1));
}

#[test]
fn the_node_is_the_first_node_entry_and_an_overflowing_one_is_minus_one() {
    let t = Tree::new("lsmem_tree_nodes");
    t.mkdir("memory0/node99999999999999999999");
    t.mkdir("memory1/nodeX");
    t.mkdir("memory2/node4294967298");
    let mut sys = t.sys();
    let mut errno = Errno::Zero;
    assert_eq!(
        memory_block_get_node(&mut sys, b"memory0", &mut errno).unwrap(),
        -1
    );
    assert!(matches!(errno, Errno::Range));
    assert_eq!(
        memory_block_get_node(&mut sys, b"memory1", &mut errno).unwrap(),
        -1
    );
    assert_eq!(
        memory_block_get_node(&mut sys, b"memory2", &mut errno).unwrap(),
        2
    );
    assert!(matches!(errno, Errno::Zero));
    assert!(memory_block_get_node(&mut sys, b"memory3", &mut errno).is_err());
}
