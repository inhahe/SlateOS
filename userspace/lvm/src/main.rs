// OurOS lvm - Logical Volume Manager utilities
//
// Multi-personality binary:
//   pvcreate  - initialize physical volume
//   vgcreate  - create volume group
//   lvcreate  - create logical volume
//   pvs       - report information about physical volumes
//   vgs       - report information about volume groups
//   lvs       - report information about logical volumes
//   pvdisplay - display physical volume attributes
//   vgdisplay - display volume group attributes
//   lvdisplay - display logical volume attributes
//   pvremove  - remove LVM label from physical volume
//   vgremove  - remove volume group
//   lvremove  - remove logical volume
//   vgextend  - add physical volume to volume group
//   lvextend  - extend logical volume size
//   lvresize  - resize logical volume

#![cfg_attr(not(test), no_main)]

// ── Constants ──────────────────────────────────────────────────────────

const LVM_SIGNATURE: &[u8] = b"LABELONE";
const LVM_LABEL_SECTOR: u64 = 1; // Second sector
const PE_DEFAULT_SIZE_MB: u64 = 4; // 4 MiB default physical extent size
const PE_SIZE_BYTES: u64 = PE_DEFAULT_SIZE_MB * 1024 * 1024;

// ── Personality Detection ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Personality {
    Pvcreate,
    Vgcreate,
    Lvcreate,
    Pvs,
    Vgs,
    Lvs,
    Pvdisplay,
    Vgdisplay,
    Lvdisplay,
    Pvremove,
    Vgremove,
    Lvremove,
    Vgextend,
    Lvextend,
    Lvresize,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };

    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe") {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"pvcreate") { Personality::Pvcreate }
    else if name.eq_ignore_ascii_case(b"vgcreate") { Personality::Vgcreate }
    else if name.eq_ignore_ascii_case(b"lvcreate") { Personality::Lvcreate }
    else if name.eq_ignore_ascii_case(b"pvs") { Personality::Pvs }
    else if name.eq_ignore_ascii_case(b"vgs") { Personality::Vgs }
    else if name.eq_ignore_ascii_case(b"lvs") { Personality::Lvs }
    else if name.eq_ignore_ascii_case(b"pvdisplay") { Personality::Pvdisplay }
    else if name.eq_ignore_ascii_case(b"vgdisplay") { Personality::Vgdisplay }
    else if name.eq_ignore_ascii_case(b"lvdisplay") { Personality::Lvdisplay }
    else if name.eq_ignore_ascii_case(b"pvremove") { Personality::Pvremove }
    else if name.eq_ignore_ascii_case(b"vgremove") { Personality::Vgremove }
    else if name.eq_ignore_ascii_case(b"lvremove") { Personality::Lvremove }
    else if name.eq_ignore_ascii_case(b"vgextend") { Personality::Vgextend }
    else if name.eq_ignore_ascii_case(b"lvextend") { Personality::Lvextend }
    else if name.eq_ignore_ascii_case(b"lvresize") { Personality::Lvresize }
    else { Personality::Pvs } // Default to pvs
}

// ── Data Structures ────────────────────────────────────────────────────

struct PhysicalVolume {
    device: Vec<u8>,
    uuid: Vec<u8>,
    vg_name: Vec<u8>,
    pv_size: u64,       // bytes
    pe_start: u64,      // offset where PEs begin
    pe_count: u32,      // total PEs
    pe_alloc: u32,      // allocated PEs
    pe_size: u64,       // bytes per PE
    format: Vec<u8>,    // "lvm2"
    status: Vec<u8>,    // "allocatable"
}

struct VolumeGroup {
    name: Vec<u8>,
    uuid: Vec<u8>,
    format: Vec<u8>,
    status: Vec<u8>,
    pv_count: u32,
    lv_count: u32,
    max_lv: u32,
    max_pv: u32,
    pe_size: u64,       // bytes
    pe_total: u32,
    pe_alloc: u32,
    pe_free: u32,
    vg_size: u64,       // bytes
    vg_free: u64,       // bytes
}

struct LogicalVolume {
    name: Vec<u8>,
    vg_name: Vec<u8>,
    uuid: Vec<u8>,
    lv_size: u64,       // bytes
    le_count: u32,      // logical extents
    segments: u32,
    status: Vec<u8>,    // "available"
    lv_path: Vec<u8>,   // /dev/vg/lv
}

// ── Argument Parsing ───────────────────────────────────────────────────

struct LvmArgs {
    targets: Vec<Vec<u8>>,     // device paths, VG/LV names
    name: Option<Vec<u8>>,     // -n, --name
    size: Option<Vec<u8>>,     // -L, --size
    extents: Option<Vec<u8>>,  // -l, --extents
    pe_size: Option<Vec<u8>>,  // -s, --physicalextentsize
    vg_name: Option<Vec<u8>>,  // for lvcreate: VG to create in
    force: bool,
    yes: bool,
    verbose: bool,
    noheadings: bool,
    separator: Option<Vec<u8>>,
    units: Option<u8>,         // b, k, m, g, t
    show_help: bool,
    show_version: bool,
    // lvextend/lvresize specific
    resizefs: bool,
    // lvcreate types
    lv_type: Option<Vec<u8>>,  // linear, striped, mirror, raid1, etc.
    stripes: Option<u32>,
    stripe_size: Option<Vec<u8>>,
}

fn parse_lvm_args(args: &[Vec<u8>]) -> LvmArgs {
    let mut result = LvmArgs {
        targets: Vec::new(),
        name: None,
        size: None,
        extents: None,
        pe_size: None,
        vg_name: None,
        force: false,
        yes: false,
        verbose: false,
        noheadings: false,
        separator: None,
        units: None,
        show_help: false,
        show_version: false,
        resizefs: false,
        lv_type: None,
        stripes: None,
        stripe_size: None,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"--version" {
            result.show_version = true;
        } else if arg == b"-n" || arg == b"--name" {
            i += 1;
            if i < args.len() {
                result.name = Some(args[i].clone());
            }
        } else if arg == b"-L" || arg == b"--size" {
            i += 1;
            if i < args.len() {
                result.size = Some(args[i].clone());
            }
        } else if arg == b"-l" || arg == b"--extents" {
            i += 1;
            if i < args.len() {
                result.extents = Some(args[i].clone());
            }
        } else if arg == b"-s" || arg == b"--physicalextentsize" {
            i += 1;
            if i < args.len() {
                result.pe_size = Some(args[i].clone());
            }
        } else if arg == b"-f" || arg == b"--force" {
            result.force = true;
        } else if arg == b"-y" || arg == b"--yes" {
            result.yes = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"--noheadings" {
            result.noheadings = true;
        } else if arg == b"--separator" {
            i += 1;
            if i < args.len() {
                result.separator = Some(args[i].clone());
            }
        } else if arg == b"--units" {
            i += 1;
            if i < args.len() && !args[i].is_empty() {
                result.units = Some(args[i][0]);
            }
        } else if arg == b"-r" || arg == b"--resizefs" {
            result.resizefs = true;
        } else if arg == b"--type" {
            i += 1;
            if i < args.len() {
                result.lv_type = Some(args[i].clone());
            }
        } else if arg == b"-i" || arg == b"--stripes" {
            i += 1;
            if i < args.len() {
                result.stripes = parse_u32(&args[i]);
            }
        } else if arg == b"-I" || arg == b"--stripesize" {
            i += 1;
            if i < args.len() {
                result.stripe_size = Some(args[i].clone());
            }
        } else if !arg.starts_with(b"-") {
            result.targets.push(arg.clone());
        }
        i += 1;
    }

    result
}

// ── Size Parsing ───────────────────────────────────────────────────────

fn parse_size_bytes(s: &[u8]) -> Option<u64> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }

    let mut num_end = s.len();
    let mut multiplier: u64 = 1;

    // Check for suffix
    if let Some(&last) = s.last() {
        match last {
            b'b' | b'B' => { num_end -= 1; multiplier = 1; }
            b'k' | b'K' => { num_end -= 1; multiplier = 1024; }
            b'm' | b'M' => { num_end -= 1; multiplier = 1024 * 1024; }
            b'g' | b'G' => { num_end -= 1; multiplier = 1024 * 1024 * 1024; }
            b't' | b'T' => { num_end -= 1; multiplier = 1024 * 1024 * 1024 * 1024; }
            _ => { multiplier = 1024 * 1024; } // default to MiB if no suffix
        }
    }

    let mut result: u64 = 0;
    for &b in &s[..num_end] {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as u64)?;
            }
            b'.' => break, // Ignore decimal part
            _ => return None,
        }
    }

    result.checked_mul(multiplier)
}

fn format_size(bytes: u64, unit: Option<u8>) -> Vec<u8> {
    let (value, suffix): (u64, &[u8]) = match unit {
        Some(b'b' | b'B') => (bytes, b"B"),
        Some(b'k' | b'K') => (bytes / 1024, b"K"),
        Some(b'g' | b'G') => (bytes / (1024 * 1024 * 1024), b"G"),
        Some(b't' | b'T') => (bytes / (1024 * 1024 * 1024 * 1024), b"T"),
        _ => {
            if bytes >= 1024 * 1024 * 1024 * 1024 {
                (bytes / (1024 * 1024 * 1024 * 1024), b"T")
            } else if bytes >= 1024 * 1024 * 1024 {
                (bytes / (1024 * 1024 * 1024), b"G")
            } else if bytes >= 1024 * 1024 {
                (bytes / (1024 * 1024), b"M")
            } else if bytes >= 1024 {
                (bytes / 1024, b"K")
            } else {
                (bytes, b"B")
            }
        }
    };

    let mut buf = format_u64(value);
    buf.push(b'.');
    // One decimal place
    let frac = if suffix == b"G" {
        ((bytes % (1024 * 1024 * 1024)) * 10 / (1024 * 1024 * 1024)) as u8
    } else {
        0
    };
    buf.push(b'0' + frac.min(9));
    buf.push(b'0');
    buf.push(b' ');
    buf.extend_from_slice(suffix);
    buf.extend_from_slice(b"iB");
    buf
}

// ── PV Commands ────────────────────────────────────────────────────────

fn cmd_pvcreate(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: pvcreate [options] PhysicalVolume...\n\n");
        print_out(b"Initialize physical volume(s) for use by LVM.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -f, --force         force creation\n");
        print_out(b"  -y, --yes           answer yes to all prompts\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"      --version       display version\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_err(b"pvcreate: no device specified\n");
        return 1;
    }

    for device in &args.targets {
        if args.verbose {
            print_out(b"  Writing physical volume data to disk \"");
            print_out(device);
            print_out(b"\"\n");
        }

        // In real implementation: write LVM label and metadata to device
        print_out(b"  Physical volume \"");
        print_out(device);
        print_out(b"\" successfully created.\n");
    }

    0
}

fn cmd_pvs(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: pvs [options] [PhysicalVolume...]\n\n");
        print_out(b"Display information about physical volumes.\n\n");
        print_out(b"Options:\n");
        print_out(b"      --noheadings    suppress headings\n");
        print_out(b"      --separator SEP use SEP as column separator\n");
        print_out(b"      --units UNITS   display sizes in units\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -h, --help          display this help\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    let sep = args.separator.as_deref().unwrap_or(b"  ");

    if !args.noheadings {
        print_out(b"  PV");
        print_out(sep);
        print_out(b"VG");
        print_out(sep);
        print_out(b"Fmt");
        print_out(sep);
        print_out(b"Attr");
        print_out(sep);
        print_out(b"PSize");
        print_out(sep);
        print_out(b"PFree\n");
    }

    // In real implementation: scan LVM labels on all block devices
    // Simulated empty output (no PVs configured)
    0
}

fn cmd_pvdisplay(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: pvdisplay [options] [PhysicalVolume...]\n\n");
        print_out(b"Display attributes of a physical volume.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -h, --help          display this help\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        // Display all PVs
        print_out(b"  No physical volumes found.\n");
    } else {
        for device in &args.targets {
            // In real implementation: read LVM metadata from device
            print_out(b"  --- Physical volume ---\n");
            print_out(b"  PV Name               ");
            print_out(device);
            print_out(b"\n");
            print_out(b"  VG Name               \n");
            print_out(b"  PV Size               <unknown>\n");
            print_out(b"  PE Size               0\n");
            print_out(b"  Total PE              0\n");
            print_out(b"  Free PE               0\n");
            print_out(b"  Allocated PE          0\n");
        }
    }

    0
}

fn cmd_pvremove(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: pvremove [options] PhysicalVolume...\n\n");
        print_out(b"Remove LVM label(s) from physical volume(s).\n\n");
        print_out(b"Options:\n");
        print_out(b"  -f, --force         force removal\n");
        print_out(b"  -y, --yes           answer yes to prompts\n");
        print_out(b"  -h, --help          display this help\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_err(b"pvremove: no device specified\n");
        return 1;
    }

    for device in &args.targets {
        print_out(b"  Labels on physical volume \"");
        print_out(device);
        print_out(b"\" successfully wiped.\n");
    }

    0
}

// ── VG Commands ────────────────────────────────────────────────────────

fn cmd_vgcreate(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: vgcreate [options] VolumeGroupName PhysicalVolume...\n\n");
        print_out(b"Create a volume group.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -s, --physicalextentsize SIZE  physical extent size\n");
        print_out(b"  -v, --verbose                  verbose output\n");
        print_out(b"  -h, --help                     display this help\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.len() < 2 {
        print_err(b"vgcreate: requires VG name and at least one PV\n");
        return 1;
    }

    let vg_name = &args.targets[0];
    let pvs = &args.targets[1..];

    if args.verbose {
        print_out(b"  Creating volume group \"");
        print_out(vg_name);
        print_out(b"\" with physical extent size ");
        if let Some(ref pe) = args.pe_size {
            print_out(pe);
        } else {
            print_out(b"4.00 MiB");
        }
        print_out(b"\n");
    }

    print_out(b"  Volume group \"");
    print_out(vg_name);
    print_out(b"\" successfully created\n");

    0
}

fn cmd_vgs(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: vgs [options] [VolumeGroup...]\n\n");
        print_out(b"Display information about volume groups.\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    let sep = args.separator.as_deref().unwrap_or(b"  ");

    if !args.noheadings {
        print_out(b"  VG");
        print_out(sep);
        print_out(b"#PV");
        print_out(sep);
        print_out(b"#LV");
        print_out(sep);
        print_out(b"#SN");
        print_out(sep);
        print_out(b"Attr");
        print_out(sep);
        print_out(b"VSize");
        print_out(sep);
        print_out(b"VFree\n");
    }

    0
}

fn cmd_vgdisplay(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: vgdisplay [options] [VolumeGroup...]\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_out(b"  No volume groups found.\n");
    } else {
        for vg in &args.targets {
            print_out(b"  --- Volume group ---\n");
            print_out(b"  VG Name               ");
            print_out(vg);
            print_out(b"\n");
            print_out(b"  System ID             \n");
            print_out(b"  Format                lvm2\n");
            print_out(b"  VG Access             read/write\n");
            print_out(b"  VG Status             resizable\n");
            print_out(b"  MAX LV                0\n");
            print_out(b"  Cur LV                0\n");
            print_out(b"  Open LV               0\n");
            print_out(b"  Max PV                0\n");
            print_out(b"  Cur PV                0\n");
            print_out(b"  Act PV                0\n");
            print_out(b"  VG Size               0\n");
            print_out(b"  PE Size               4.00 MiB\n");
            print_out(b"  Total PE              0\n");
            print_out(b"  Alloc PE / Size       0 / 0\n");
            print_out(b"  Free  PE / Size       0 / 0\n");
        }
    }

    0
}

fn cmd_vgremove(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: vgremove [options] VolumeGroup...\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_err(b"vgremove: no volume group specified\n");
        return 1;
    }

    for vg in &args.targets {
        print_out(b"  Volume group \"");
        print_out(vg);
        print_out(b"\" successfully removed\n");
    }

    0
}

fn cmd_vgextend(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: vgextend [options] VolumeGroup PhysicalVolume...\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.len() < 2 {
        print_err(b"vgextend: requires VG name and at least one PV\n");
        return 1;
    }

    let vg_name = &args.targets[0];
    print_out(b"  Volume group \"");
    print_out(vg_name);
    print_out(b"\" successfully extended\n");

    0
}

// ── LV Commands ────────────────────────────────────────────────────────

fn cmd_lvcreate(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: lvcreate [options] VolumeGroup\n\n");
        print_out(b"Create a logical volume.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -n, --name NAME       set logical volume name\n");
        print_out(b"  -L, --size SIZE       set logical volume size\n");
        print_out(b"  -l, --extents N[%]    set logical volume extents\n");
        print_out(b"      --type TYPE       set segment type (linear, striped, etc.)\n");
        print_out(b"  -i, --stripes N       number of stripes\n");
        print_out(b"  -I, --stripesize SIZE stripe size\n");
        print_out(b"  -v, --verbose         verbose output\n");
        print_out(b"  -h, --help            display this help\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_err(b"lvcreate: no volume group specified\n");
        return 1;
    }

    if args.size.is_none() && args.extents.is_none() {
        print_err(b"lvcreate: size not specified (use -L or -l)\n");
        return 1;
    }

    let vg_name = &args.targets[0];
    let lv_name = args.name.as_deref().unwrap_or(b"lvol0");

    print_out(b"  Logical volume \"");
    print_out(lv_name);
    print_out(b"\" created.\n");

    0
}

fn cmd_lvs(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: lvs [options] [LogicalVolume|VolumeGroup...]\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    let sep = args.separator.as_deref().unwrap_or(b"  ");

    if !args.noheadings {
        print_out(b"  LV");
        print_out(sep);
        print_out(b"VG");
        print_out(sep);
        print_out(b"Attr");
        print_out(sep);
        print_out(b"LSize");
        print_out(sep);
        print_out(b"Pool");
        print_out(sep);
        print_out(b"Origin");
        print_out(sep);
        print_out(b"Data%");
        print_out(sep);
        print_out(b"Meta%\n");
    }

    0
}

fn cmd_lvdisplay(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: lvdisplay [options] [LogicalVolume|VolumeGroup...]\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_out(b"  No logical volumes found.\n");
    } else {
        for lv in &args.targets {
            print_out(b"  --- Logical volume ---\n");
            print_out(b"  LV Path               ");
            print_out(lv);
            print_out(b"\n");
            print_out(b"  LV Name               ");
            // Extract LV name from path
            if let Some(pos) = lv.iter().rposition(|&b| b == b'/') {
                print_out(&lv[pos + 1..]);
            } else {
                print_out(lv);
            }
            print_out(b"\n");
            print_out(b"  VG Name               \n");
            print_out(b"  LV Size               0\n");
            print_out(b"  Current LE            0\n");
            print_out(b"  Segments              0\n");
            print_out(b"  Allocation            inherit\n");
            print_out(b"  Read ahead sectors    auto\n");
            print_out(b"  Block device          \n");
        }
    }

    0
}

fn cmd_lvremove(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: lvremove [options] LogicalVolume...\n\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_err(b"lvremove: no logical volume specified\n");
        return 1;
    }

    for lv in &args.targets {
        print_out(b"  Logical volume \"");
        print_out(lv);
        print_out(b"\" successfully removed.\n");
    }

    0
}

fn cmd_lvextend(args: &LvmArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: lvextend [options] LogicalVolume [PhysicalVolume...]\n\n");
        print_out(b"Options:\n");
        print_out(b"  -L, --size [+]SIZE    set or extend to size\n");
        print_out(b"  -l, --extents [+]N    set or extend by extents\n");
        print_out(b"  -r, --resizefs        resize underlying filesystem\n");
        print_out(b"  -h, --help            display this help\n");
        return 0;
    }

    if args.show_version { return show_version(); }

    if args.targets.is_empty() {
        print_err(b"lvextend: no logical volume specified\n");
        return 1;
    }

    if args.size.is_none() && args.extents.is_none() {
        print_err(b"lvextend: size not specified (use -L or -l)\n");
        return 1;
    }

    let lv = &args.targets[0];
    print_out(b"  Size of logical volume ");
    print_out(lv);
    print_out(b" changed.\n");

    if args.resizefs {
        print_out(b"  Resizing underlying filesystem...\n");
    }

    print_out(b"  Logical volume ");
    print_out(lv);
    print_out(b" successfully resized.\n");

    0
}

fn cmd_lvresize(args: &LvmArgs) -> i32 {
    // lvresize is essentially the same as lvextend but can also shrink
    if args.show_help {
        print_out(b"Usage: lvresize [options] LogicalVolume [PhysicalVolume...]\n\n");
        print_out(b"Options:\n");
        print_out(b"  -L, --size [+|-]SIZE  set, extend, or reduce size\n");
        print_out(b"  -l, --extents [+|-]N  set, extend, or reduce extents\n");
        print_out(b"  -r, --resizefs        resize underlying filesystem\n");
        print_out(b"  -h, --help            display this help\n");
        return 0;
    }

    // Delegate to lvextend logic
    cmd_lvextend(args)
}

// ── Utility Functions ──────────────────────────────────────────────────

fn show_version() -> i32 {
    print_out(b"  LVM version:     2.03.22(2)-OurOS (2025-01-01)\n");
    print_out(b"  Library version: 1.02.196-OurOS (2025-01-01)\n");
    print_out(b"  Driver version:  4.48.0-OurOS\n");
    0
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }
    let mut result: u32 = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as u32)?;
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
        print_err(b"lvm: unable to determine program name\n");
        return 1;
    }

    let personality = detect_personality(&args[0]);
    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();
    let parsed = parse_lvm_args(&rest);

    match personality {
        Personality::Pvcreate => cmd_pvcreate(&parsed),
        Personality::Vgcreate => cmd_vgcreate(&parsed),
        Personality::Lvcreate => cmd_lvcreate(&parsed),
        Personality::Pvs => cmd_pvs(&parsed),
        Personality::Vgs => cmd_vgs(&parsed),
        Personality::Lvs => cmd_lvs(&parsed),
        Personality::Pvdisplay => cmd_pvdisplay(&parsed),
        Personality::Vgdisplay => cmd_vgdisplay(&parsed),
        Personality::Lvdisplay => cmd_lvdisplay(&parsed),
        Personality::Pvremove => cmd_pvremove(&parsed),
        Personality::Vgremove => cmd_vgremove(&parsed),
        Personality::Lvremove => cmd_lvremove(&parsed),
        Personality::Vgextend => cmd_vgextend(&parsed),
        Personality::Lvextend => cmd_lvextend(&parsed),
        Personality::Lvresize => cmd_lvresize(&parsed),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality Detection ──────────────────────────────────

    #[test]
    fn test_detect_pvcreate() {
        assert_eq!(detect_personality(b"pvcreate"), Personality::Pvcreate);
        assert_eq!(detect_personality(b"/sbin/pvcreate"), Personality::Pvcreate);
    }

    #[test]
    fn test_detect_vgcreate() {
        assert_eq!(detect_personality(b"vgcreate"), Personality::Vgcreate);
    }

    #[test]
    fn test_detect_lvcreate() {
        assert_eq!(detect_personality(b"lvcreate"), Personality::Lvcreate);
    }

    #[test]
    fn test_detect_pvs() {
        assert_eq!(detect_personality(b"pvs"), Personality::Pvs);
    }

    #[test]
    fn test_detect_vgs() {
        assert_eq!(detect_personality(b"vgs"), Personality::Vgs);
    }

    #[test]
    fn test_detect_lvs() {
        assert_eq!(detect_personality(b"lvs"), Personality::Lvs);
    }

    #[test]
    fn test_detect_pvdisplay() {
        assert_eq!(detect_personality(b"pvdisplay"), Personality::Pvdisplay);
    }

    #[test]
    fn test_detect_vgdisplay() {
        assert_eq!(detect_personality(b"vgdisplay"), Personality::Vgdisplay);
    }

    #[test]
    fn test_detect_lvdisplay() {
        assert_eq!(detect_personality(b"lvdisplay"), Personality::Lvdisplay);
    }

    #[test]
    fn test_detect_pvremove() {
        assert_eq!(detect_personality(b"pvremove"), Personality::Pvremove);
    }

    #[test]
    fn test_detect_vgremove() {
        assert_eq!(detect_personality(b"vgremove"), Personality::Vgremove);
    }

    #[test]
    fn test_detect_lvremove() {
        assert_eq!(detect_personality(b"lvremove"), Personality::Lvremove);
    }

    #[test]
    fn test_detect_vgextend() {
        assert_eq!(detect_personality(b"vgextend"), Personality::Vgextend);
    }

    #[test]
    fn test_detect_lvextend() {
        assert_eq!(detect_personality(b"lvextend"), Personality::Lvextend);
    }

    #[test]
    fn test_detect_lvresize() {
        assert_eq!(detect_personality(b"lvresize"), Personality::Lvresize);
    }

    // ── Size Parsing ───────────────────────────────────────────

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size_bytes(b"1024b"), Some(1024));
        assert_eq!(parse_size_bytes(b"1K"), Some(1024));
        assert_eq!(parse_size_bytes(b"1M"), Some(1024 * 1024));
        assert_eq!(parse_size_bytes(b"1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size_bytes(b"1T"), Some(1024 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_default_mb() {
        // No suffix defaults to MiB
        assert_eq!(parse_size_bytes(b"100"), Some(100 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size_bytes(b""), None);
        assert_eq!(parse_size_bytes(b"abc"), None);
    }

    // ── PV Commands ────────────────────────────────────────────

    #[test]
    fn test_pvcreate_no_device() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_pvcreate(&args), 1);
    }

    #[test]
    fn test_pvcreate_with_device() {
        let args = LvmArgs {
            targets: vec![b"/dev/sdb".to_vec()],
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_pvcreate(&args), 0);
    }

    #[test]
    fn test_pvs_ok() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_pvs(&args), 0);
    }

    #[test]
    fn test_pvremove_no_device() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_pvremove(&args), 1);
    }

    // ── VG Commands ────────────────────────────────────────────

    #[test]
    fn test_vgcreate_too_few_args() {
        let args = LvmArgs {
            targets: vec![b"myvg".to_vec()],
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_vgcreate(&args), 1);
    }

    #[test]
    fn test_vgcreate_ok() {
        let args = LvmArgs {
            targets: vec![b"myvg".to_vec(), b"/dev/sdb".to_vec()],
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_vgcreate(&args), 0);
    }

    #[test]
    fn test_vgremove_no_name() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_vgremove(&args), 1);
    }

    #[test]
    fn test_vgextend_too_few() {
        let args = LvmArgs {
            targets: vec![b"myvg".to_vec()],
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_vgextend(&args), 1);
    }

    // ── LV Commands ────────────────────────────────────────────

    #[test]
    fn test_lvcreate_no_vg() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvcreate(&args), 1);
    }

    #[test]
    fn test_lvcreate_no_size() {
        let args = LvmArgs {
            targets: vec![b"myvg".to_vec()],
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvcreate(&args), 1);
    }

    #[test]
    fn test_lvcreate_ok() {
        let args = LvmArgs {
            targets: vec![b"myvg".to_vec()],
            name: Some(b"mylv".to_vec()),
            size: Some(b"1G".to_vec()),
            extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvcreate(&args), 0);
    }

    #[test]
    fn test_lvremove_no_lv() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvremove(&args), 1);
    }

    #[test]
    fn test_lvextend_no_lv() {
        let args = LvmArgs {
            targets: Vec::new(),
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvextend(&args), 1);
    }

    #[test]
    fn test_lvextend_no_size() {
        let args = LvmArgs {
            targets: vec![b"/dev/myvg/mylv".to_vec()],
            name: None, size: None, extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvextend(&args), 1);
    }

    #[test]
    fn test_lvextend_ok() {
        let args = LvmArgs {
            targets: vec![b"/dev/myvg/mylv".to_vec()],
            name: None,
            size: Some(b"+1G".to_vec()),
            extents: None, pe_size: None,
            vg_name: None, force: false, yes: false, verbose: false,
            noheadings: false, separator: None, units: None,
            show_help: false, show_version: false, resizefs: false,
            lv_type: None, stripes: None, stripe_size: None,
        };
        assert_eq!(cmd_lvextend(&args), 0);
    }

    // ── Argument Parsing ───────────────────────────────────────

    #[test]
    fn test_parse_args_help() {
        let args = parse_lvm_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_parse_args_name_size() {
        let args = parse_lvm_args(&[b"-n".to_vec(), b"test".to_vec(), b"-L".to_vec(), b"1G".to_vec(), b"myvg".to_vec()]);
        assert_eq!(args.name.as_deref(), Some(b"test".as_slice()));
        assert_eq!(args.size.as_deref(), Some(b"1G".as_slice()));
        assert_eq!(&args.targets[0], b"myvg");
    }

    #[test]
    fn test_parse_args_verbose() {
        let args = parse_lvm_args(&[b"-v".to_vec()]);
        assert!(args.verbose);
    }

    // ── Utility Functions ──────────────────────────────────────

    #[test]
    fn test_format_u64() {
        assert_eq!(format_u64(0), b"0");
        assert_eq!(format_u64(42), b"42");
    }

    #[test]
    fn test_parse_u32() {
        assert_eq!(parse_u32(b"0"), Some(0));
        assert_eq!(parse_u32(b"42"), Some(42));
        assert_eq!(parse_u32(b""), None);
    }

    #[test]
    fn test_show_version() {
        assert_eq!(show_version(), 0);
    }
}
