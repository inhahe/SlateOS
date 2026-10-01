//! `topology.c` and its probers: a block device's I/O limits -- alignment
//! offset, minimum and optimal I/O size, sector sizes, DAX, disk sequence
//! number -- from sysfs, from `ioctl`s, or from the RAID and volume managers
//! that know them.
//!
//! Only block devices have a topology; for anything else the chain fails
//! with `EINVAL`, as upstream's does.

use std::fs::File;

use crate::blkdev;
use crate::probe::{ChainData, ChainDrv, ChainId, IdInfo, Probe};
use crate::{PROBE_NONE, PROBE_OK};

/// `struct blkid_struct_topology`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    /// `alignment_offset`.
    pub alignment_offset: u64,
    /// `minimum_io_size`.
    pub minimum_io_size: u64,
    /// `optimal_io_size`.
    pub optimal_io_size: u64,
    /// `logical_sector_size`.
    pub logical_sector_size: u64,
    /// `physical_sector_size`.
    pub physical_sector_size: u64,
    /// `dax`.
    pub dax: u64,
    /// `diskseq`.
    pub diskseq: u64,
}

/// Which field of [`Topology`] a value is.
#[derive(Clone, Copy)]
enum Field {
    AlignmentOffset,
    MinimumIoSize,
    OptimalIoSize,
    LogicalSectorSize,
    PhysicalSectorSize,
    Dax,
    Diskseq,
}

static SYSFS_TP: IdInfo = IdInfo {
    name: "sysfs",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_sysfs_tp),
    magics: &[],
};
static IOCTL_TP: IdInfo = IdInfo {
    name: "ioctl",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_ioctl_tp),
    magics: &[],
};
static MD_TP: IdInfo = IdInfo {
    name: "md",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_md_tp),
    magics: &[],
};
static DM_TP: IdInfo = IdInfo {
    name: "dm",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_dm_tp),
    magics: &[],
};
static LVM_TP: IdInfo = IdInfo {
    name: "lvm",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_lvm_tp),
    magics: &[],
};
static EVMS_TP: IdInfo = IdInfo {
    name: "evms",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_evms_tp),
    magics: &[],
};

/// `idinfos[]` (Linux's).
pub static IDINFOS: &[&IdInfo] = &[&SYSFS_TP, &IOCTL_TP, &MD_TP, &DM_TP, &LVM_TP, &EVMS_TP];

/// `topology_drv`.
pub(crate) static DRIVER: ChainDrv = ChainDrv {
    dflt_flags: 0,
    dflt_enabled: false,
    has_fltr: false,
    idinfos: IDINFOS,
    probe: topology_probe,
    safeprobe: topology_probe,
};

/// `topology_probe`.
fn topology_probe(pr: &mut Probe, chn: ChainId) -> i32 {
    if pr.chain(chn).idx < -1 {
        return -1;
    }
    if !pr.is_blk() {
        return -crate::EINVAL;
    }
    if pr.chain(chn).binary {
        pr.chain_mut(chn).data = ChainData::Topology(Topology::default());
    }
    pr.chain_reset_values(chn);
    let start = usize::try_from(pr.chain(chn).idx.saturating_add(1)).unwrap_or(0);
    for (i, &id) in IDINFOS.iter().enumerate().skip(start) {
        pr.chain_mut(chn).idx = i32::try_from(i).unwrap_or(i32::MAX);
        if let Some(f) = id.probefunc {
            pr.errno = 0;
            if f(pr, None) != 0 {
                continue;
            }
        }
        if !is_complete(pr) {
            continue;
        }
        set_logical_sector_size(pr);
        return PROBE_OK;
    }
    PROBE_NONE
}

/// `topology_set_value`: zeros are not values; in binary mode into the
/// struct, else a decimal value.
fn set_value(pr: &mut Probe, name: &'static str, field: Field, data: u64) -> i32 {
    let Some(chn) = pr.current_chain() else {
        return -1;
    };
    if data == 0 {
        return 0;
    }
    if pr.chain(chn).binary {
        if let ChainData::Topology(tp) = &mut pr.chain_mut(chn).data {
            let slot = match field {
                Field::AlignmentOffset => &mut tp.alignment_offset,
                Field::MinimumIoSize => &mut tp.minimum_io_size,
                Field::OptimalIoSize => &mut tp.optimal_io_size,
                Field::LogicalSectorSize => &mut tp.logical_sector_size,
                Field::PhysicalSectorSize => &mut tp.physical_sector_size,
                Field::Dax => &mut tp.dax,
                Field::Diskseq => &mut tp.diskseq,
            };
            *slot = data;
        }
        return 0;
    }
    pr.set_value_str(name, data.to_string().as_bytes())
}

/// `topology_is_complete`: MINIMUM_IO_SIZE is known.
fn is_complete(pr: &Probe) -> bool {
    let Some(chn) = pr.current_chain() else {
        return false;
    };
    if pr.chain(chn).binary
        && let ChainData::Topology(tp) = &pr.chain(chn).data
        && tp.minimum_io_size != 0
    {
        return true;
    }
    pr.has_value("MINIMUM_IO_SIZE")
}

/// `blkid_topology_set_alignment_offset`: the kernel's -1 (no alignment
/// works) is hidden as 0.
fn set_alignment_offset(pr: &mut Probe, val: i32) -> i32 {
    let x = u64::try_from(val).unwrap_or(0);
    set_value(pr, "ALIGNMENT_OFFSET", Field::AlignmentOffset, x)
}

fn set_minimum_io_size(pr: &mut Probe, val: u64) -> i32 {
    set_value(pr, "MINIMUM_IO_SIZE", Field::MinimumIoSize, val)
}

fn set_optimal_io_size(pr: &mut Probe, val: u64) -> i32 {
    set_value(pr, "OPTIMAL_IO_SIZE", Field::OptimalIoSize, val)
}

/// `topology_set_logical_sector_size`: BLKSSZGET, generic to every prober.
fn set_logical_sector_size(pr: &mut Probe) -> i32 {
    let val = pr.sectorsize();
    if val == 0 {
        return -1;
    }
    set_value(
        pr,
        "LOGICAL_SECTOR_SIZE",
        Field::LogicalSectorSize,
        u64::from(val),
    )
}

fn set_physical_sector_size(pr: &mut Probe, val: u64) -> i32 {
    set_value(pr, "PHYSICAL_SECTOR_SIZE", Field::PhysicalSectorSize, val)
}

fn set_dax(pr: &mut Probe, val: u64) -> i32 {
    set_value(pr, "DAX", Field::Dax, val)
}

fn set_diskseq(pr: &mut Probe, val: u64) -> i32 {
    set_value(pr, "DISKSEQ", Field::Diskseq, val)
}

/// How a sysfs attribute or `ioctl` result is set.
#[derive(Clone, Copy)]
enum Setter {
    Ulong(fn(&mut Probe, u64) -> i32),
    Int(fn(&mut Probe, i32) -> i32),
    U64(fn(&mut Probe, u64) -> i32),
}

/// `probe_sysfs_tp`: `/sys/dev/block/MAJ:MIN/...`, a partition's missing
/// attributes read from its disk.
fn probe_sysfs_tp(pr: &mut Probe, _mag: Option<&'static crate::IdMag>) -> i32 {
    let vals: [(&[u8], Setter); 6] = [
        (b"alignment_offset", Setter::Int(set_alignment_offset)),
        (b"queue/minimum_io_size", Setter::Ulong(set_minimum_io_size)),
        (b"queue/optimal_io_size", Setter::Ulong(set_optimal_io_size)),
        (
            b"queue/physical_block_size",
            Setter::Ulong(set_physical_sector_size),
        ),
        (b"queue/dax", Setter::Ulong(set_dax)),
        (b"diskseq", Setter::U64(set_diskseq)),
    ];
    let dev = pr.devno();
    if dev == 0 {
        return 1;
    }
    let Some(mut pc) = ulsysfs::new_sysfs_path(dev, None, None) else {
        return 1;
    };
    let mut rc = 1;
    let mut set_parent = true;
    let mut count = 0u32;
    for (attr, setter) in vals {
        let mut ok = pc.access(ulsysfs::F_OK, attr).is_ok();
        rc = 1;
        if !ok && set_parent {
            let disk = pr.wholedisk_devno();
            set_parent = false;
            if disk != 0 && disk != dev {
                let Some(parent) = ulsysfs::new_sysfs_path(disk, None, None) else {
                    break;
                };
                pc.blkdev_set_parent(Some(std::rc::Rc::new(parent)));
                ok = pc.access(ulsysfs::F_OK, attr).is_ok();
            }
        }
        if !ok {
            continue;
        }
        rc = match setter {
            Setter::Ulong(f) | Setter::U64(f) => match pc.read_u64(attr) {
                Some(d) => f(pr, d),
                None => continue,
            },
            Setter::Int(f) => match pc.read_s64(attr) {
                // `(int) data`.
                #[allow(clippy::cast_possible_truncation, reason = "C's (int) cast")]
                Some(d) => f(pr, d as i32),
                None => continue,
            },
        };
        if rc < 0 {
            break;
        }
        if rc == 0 {
            count = count.saturating_add(1);
        }
    }
    if count > 0 { 0 } else { rc }
}

/// `probe_ioctl_tp`: `BLKALIGNOFF`, `BLKIOMIN`, `BLKIOOPT`, `BLKPBSZGET`,
/// `BLKGETDISKSEQ` -- all of them, or the prober found nothing (though
/// what the earlier ones set stays set, as upstream's does).
fn probe_ioctl_tp(pr: &mut Probe, _mag: Option<&'static crate::IdMag>) -> i32 {
    let vals: [(u64, Setter); 5] = [
        (blkdev::BLKALIGNOFF, Setter::Int(set_alignment_offset)),
        (blkdev::BLKIOMIN, Setter::Ulong(set_minimum_io_size)),
        (blkdev::BLKIOOPT, Setter::Ulong(set_optimal_io_size)),
        (blkdev::BLKPBSZGET, Setter::Ulong(set_physical_sector_size)),
        (blkdev::BLKGETDISKSEQ, Setter::U64(set_diskseq)),
    ];
    let Some(file) = pr.file().cloned() else {
        return 1;
    };
    for (ioc, setter) in vals {
        // The kernel writes an int, an unsigned int or a u64 into a
        // zeroed eight-byte union. (Upstream's union is not zeroed; its
        // upper half after a four-byte answer is whatever the stack held.)
        let mut data: u64 = 0;
        // SAFETY: each request writes at most eight bytes: an int, an
        // unsigned int, or a u64.
        if unsafe { blkdev::ioctl_ptr(&file, ioc, &mut data) }.is_err() {
            return 1;
        }
        let low = u32::try_from(data & 0xffff_ffff).unwrap_or(0);
        let rc = match setter {
            // The int is the low four bytes.
            Setter::Int(f) => f(pr, i32::from_ne_bytes(low.to_ne_bytes())),
            Setter::Ulong(f) => f(pr, data),
            Setter::U64(f) => f(pr, data),
        };
        if rc != 0 {
            return -1;
        }
    }
    0
}

/// `blkid_driver_has_major(drvname, major)`: `/proc/devices` lists `major`
/// as `drvname`'s among the block devices.
fn driver_has_major(drvname: &[u8], drvmaj: u32) -> bool {
    let Ok(text) = std::fs::read("/proc/devices") else {
        return false;
    };
    let mut lines = text.split(|&b| b == b'\n');
    // Skip to the block devices.
    for l in lines.by_ref() {
        if l == b"Block devices:" {
            break;
        }
    }
    for l in lines {
        // `sscanf(buf, "%d %64[^\n ]", &maj, name)`.
        let mut pos = 0usize;
        let Some(sc) = ulstrutils::scan_integer(l, 10) else {
            continue;
        };
        pos = pos.saturating_add(sc.end);
        let rest = l.get(pos..).unwrap_or_default();
        let rest = rest
            .get(
                rest.iter()
                    .take_while(|&&b| crate::encode::c_isspace(b))
                    .count()..,
            )
            .unwrap_or_default();
        let name_len = rest.iter().take_while(|&&b| b != b' ').count().min(64);
        let name = rest.get(..name_len).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let Ok(maj) = u32::try_from(sc.magnitude) else {
            continue;
        };
        if !sc.negative && maj == drvmaj && name == drvname {
            return true;
        }
    }
    false
}

/// `probe_md_tp`: an MD RAID's chunk size and stripe width, from
/// `GET_ARRAY_INFO`.
fn probe_md_tp(pr: &mut Probe, _mag: Option<&'static crate::IdMag>) -> i32 {
    const MD_MAJOR: u32 = 9;
    // _IOR(MD_MAJOR, 0x11, struct md_array_info): 18 u32s.
    const GET_ARRAY_INFO: u64 = 0x8048_0911;
    let devno = pr.devno();
    if devno == 0 {
        return 1;
    }
    let maj = ulsysfs::major(devno);
    if maj != MD_MAJOR && !driver_has_major(b"md", maj) {
        return 1;
    }
    let Some((_, disk)) = crate::devno::devno_to_wholedisk(devno, 0) else {
        return 1;
    };
    let own;
    let file: &File = if disk == devno {
        match pr.file() {
            Some(f) => f,
            None => return 1,
        }
    } else {
        let Some(path) = crate::devno::devno_to_devname(disk) else {
            return 1;
        };
        match File::open(std::path::PathBuf::from(quoting::os_from_bytes(&path))) {
            Ok(f) => {
                own = f;
                &own
            }
            Err(_) => return 1,
        }
    };
    let mut md = [0u32; 18];
    // SAFETY: GET_ARRAY_INFO writes a struct md_array_info, 18 u32s.
    if unsafe { blkdev::ioctl_ptr(file, GET_ARRAY_INFO, &mut md) }.is_err() {
        return 1;
    }
    let level = md[4];
    let mut raid_disks = md[7];
    let chunk_size = md[17];
    match level {
        6 => raid_disks = raid_disks.wrapping_sub(2),
        4 | 5 => raid_disks = raid_disks.wrapping_sub(1),
        0 | 1 | 10 => {}
        _ => return 1,
    }
    set_minimum_io_size(pr, u64::from(chunk_size));
    set_optimal_io_size(
        pr,
        u64::from(chunk_size).wrapping_mul(u64::from(raid_disks)),
    );
    0
}

/// The first of `paths` that exists.
fn find_cmd(paths: &[&'static str]) -> Option<&'static str> {
    paths.iter().copied().find(|p| std::fs::metadata(p).is_ok())
}

/// Run `argv` with the real user and group IDs (`drop_permissions` in the
/// child), its standard output captured.
fn run_dropped(argv: &[&[u8]]) -> Option<Vec<u8>> {
    let (cmd, args) = argv.split_first()?;
    let mut c = std::process::Command::new(quoting::os_from_bytes(cmd));
    for a in args {
        c.arg(quoting::os_from_bytes(a));
    }
    c.stdin(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn getuid() -> u32;
            fn getgid() -> u32;
        }
        // SAFETY: getuid and getgid cannot fail and touch no memory.
        let (uid, gid) = unsafe { (getuid(), getgid()) };
        c.uid(uid).gid(gid);
    }
    let out = c.output().ok()?;
    Some(out.stdout)
}

/// `probe_dm_tp`: `dmsetup table` for a device-mapper stripe. Upstream
/// tests `fscanf(...) != 0` where it means `!= 4`, so a real stripe table is
/// "nothing" and only output that matches no number at all "succeeds" --
/// with zeros, which are not values. Kept.
fn probe_dm_tp(pr: &mut Probe, _mag: Option<&'static crate::IdMag>) -> i32 {
    let devno = pr.devno();
    if devno == 0 {
        return 1;
    }
    if !driver_has_major(b"device-mapper", ulsysfs::major(devno)) {
        return 1;
    }
    let Some(cmd) = find_cmd(&[
        "/usr/local/sbin/dmsetup",
        "/usr/sbin/dmsetup",
        "/sbin/dmsetup",
    ]) else {
        return 1;
    };
    #[allow(clippy::cast_possible_wrap, reason = "C prints the halves with %d")]
    let (maj, min) = (
        (ulsysfs::major(devno) as i32).to_string(),
        (ulsysfs::minor(devno) as i32).to_string(),
    );
    let Some(out) = run_dropped(&[
        cmd.as_bytes(),
        b"table",
        b"-j",
        maj.as_bytes(),
        b"-m",
        min.as_bytes(),
    ]) else {
        return 1;
    };
    // `fscanf(stream, "%lld %lld striped %d %d ", ...)`: how many matched.
    let matched = {
        let mut pos = 0usize;
        let skip = |t: &[u8], p: &mut usize| {
            while t.get(*p).copied().is_some_and(crate::encode::c_isspace) {
                *p = p.saturating_add(1);
            }
        };
        let num = |t: &[u8], p: &mut usize| -> bool {
            match ulstrutils::scan_integer(t.get(*p..).unwrap_or_default(), 10) {
                Some(sc) => {
                    *p = p.saturating_add(sc.end);
                    true
                }
                None => false,
            }
        };
        skip(&out, &mut pos);
        if pos >= out.len() {
            // EOF before the first conversion.
            -1
        } else if num(&out, &mut pos) {
            // One conversion or more; only whether it is 0 matters.
            1
        } else {
            0
        }
    };
    if matched != 0 {
        return 1;
    }
    set_minimum_io_size(pr, 0);
    set_optimal_io_size(pr, 0);
    0
}

/// `probe_lvm_tp`: `lvdisplay DEVICE`'s "Stripes" and "Stripe size".
fn probe_lvm_tp(pr: &mut Probe, _mag: Option<&'static crate::IdMag>) -> i32 {
    const LVM_BLK_MAJOR: u32 = 58;
    let devno = pr.devno();
    if devno == 0 {
        return 1;
    }
    let maj = ulsysfs::major(devno);
    if maj != LVM_BLK_MAJOR && !driver_has_major(b"lvm", maj) {
        return 1;
    }
    let Some(cmd) = find_cmd(&[
        "/usr/local/sbin/lvdisplay",
        "/usr/sbin/lvdisplay",
        "/sbin/lvdisplay",
    ]) else {
        return 1;
    };
    let Some(devname) = crate::devno::devno_to_devname(devno) else {
        return 1;
    };
    let Some(out) = run_dropped(&[cmd.as_bytes(), &devname]) else {
        return 1;
    };
    let mut stripes: i32 = 0;
    let mut stripesize: i32 = 0;
    // `fgets` into 1024 bytes: a longer line is read in pieces.
    for line in out.split_inclusive(|&b| b == b'\n') {
        for piece in line.chunks(1023) {
            if let Some(rest) = piece.strip_prefix(b"Stripes") {
                // `sscanf(buf, "Stripes %d", ...)`.
                if let Some(v) = scan_after_ws(rest) {
                    stripes = v;
                }
            }
            if let Some(rest) = piece.strip_prefix(b"Stripe size") {
                // `sscanf(buf, "Stripe size (KByte) %d", ...)`.
                let t = skip_ws(rest);
                if let Some(t) = t.strip_prefix(b"(KByte)")
                    && let Some(v) = scan_after_ws(t)
                {
                    stripesize = v;
                }
            }
        }
    }
    if stripes == 0 {
        return 1;
    }
    #[allow(
        clippy::cast_sign_loss,
        reason = "C passes the int to an unsigned long"
    )]
    {
        set_minimum_io_size(pr, (stripesize.wrapping_shl(10)) as u64);
        set_optimal_io_size(
            pr,
            (stripes.wrapping_mul(stripesize).wrapping_shl(10)) as u64,
        );
    }
    0
}

/// White space off the front, as a scanf directive skips it.
fn skip_ws(t: &[u8]) -> &[u8] {
    let n = t
        .iter()
        .take_while(|&&b| crate::encode::c_isspace(b))
        .count();
    t.get(n..).unwrap_or_default()
}

/// A `%d` after white space.
fn scan_after_ws(t: &[u8]) -> Option<i32> {
    let sc = ulstrutils::scan_integer(t, 10)?;
    let m = i128::try_from(sc.magnitude).ok()?;
    let v = if sc.negative { m.wrapping_neg() } else { m };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "scanf's %d stores a long in an int"
    )]
    Some(v as i32)
}

/// `probe_evms_tp`: `EVMS_GET_STRIPE_INFO`.
fn probe_evms_tp(pr: &mut Probe, _mag: Option<&'static crate::IdMag>) -> i32 {
    const EVMS_MAJOR: u32 = 117;
    // _IOR(EVMS_MAJOR, 0xF0, struct evms_stripe_info): two u32s.
    const EVMS_GET_STRIPE_INFO: u64 = 0x8008_75f0;
    let devno = pr.devno();
    if devno == 0 {
        return 1;
    }
    let maj = ulsysfs::major(devno);
    if maj != EVMS_MAJOR && !driver_has_major(b"evms", maj) {
        return 1;
    }
    let Some(file) = pr.file().cloned() else {
        return 1;
    };
    let mut evms = [0u32; 2];
    // SAFETY: EVMS_GET_STRIPE_INFO writes a struct evms_stripe_info, two u32s.
    if unsafe { blkdev::ioctl_ptr(&file, EVMS_GET_STRIPE_INFO, &mut evms) }.is_err() {
        return 1;
    }
    set_minimum_io_size(pr, u64::from(evms[0].wrapping_shl(9)));
    set_optimal_io_size(pr, u64::from(evms[0].wrapping_mul(evms[1]).wrapping_shl(9)));
    0
}

impl Probe {
    /// `blkid_probe_enable_topology`.
    pub fn enable_topology(&mut self, enable: bool) {
        self.chain_mut(ChainId::Toplgy).enabled = enable;
    }

    /// `blkid_probe_get_topology`: the binary interface.
    pub fn topology(&mut self) -> Option<Topology> {
        if !self.get_binary_data(ChainId::Toplgy) {
            return None;
        }
        match &self.chain(ChainId::Toplgy).data {
            ChainData::Topology(tp) => Some(tp.clone()),
            _ => None,
        }
    }
}
