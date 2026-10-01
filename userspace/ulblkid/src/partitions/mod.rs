//! `partitions.c`: the chain that reads partition tables -- the list of
//! partitions and the tables they belong to (nested ones included), the
//! probe loop, and the `PART_ENTRY_*` values a partition device reports
//! about its own entry in its disk's table.
//!
//! Tables and partitions are addressed by index into the list ([`TabId`],
//! [`PartId`]). Upstream uses pointers into an array it `realloc`s, so a
//! nested table's parent pointer can dangle after the 33rd partition; an
//! index cannot.

use crate::encode::{encode_to_utf8, rtrim_whitespace};
use crate::probe::{ChainData, ChainDrv, ChainId, IdInfo, PROBE_FL_IGNORE_PT, Probe};
use crate::{
    EINVAL, PARTS_ENTRY_DETAILS, PROBE_NONE, PROBE_OK, UUID_STR_LEN, c_str, unparse_uuid,
    uuid_is_empty,
};

mod aix;
mod atari;
mod bsd;
mod dos;
mod gpt;
mod mac;
mod mbr;
mod minix;
mod sgi;
mod solaris_x86;
mod sun;
mod ultrix;
mod unixware;

/// `ENOSPC`, a nested table that would reach outside its disk.
const ENOSPC: i32 = 28;

/// `MBR_DOS_EXTENDED_PARTITION`.
pub(crate) const MBR_DOS_EXTENDED_PARTITION: i32 = 0x05;
/// `MBR_W95_EXTENDED_PARTITION`.
pub(crate) const MBR_W95_EXTENDED_PARTITION: i32 = 0x0f;
/// `MBR_LINUX_EXTENDED_PARTITION`.
pub(crate) const MBR_LINUX_EXTENDED_PARTITION: i32 = 0x85;

/// `idinfos[]`: every partition-table prober, in the order they run.
pub static IDINFOS: &[&IdInfo] = &[
    &aix::AIX,
    &sgi::SGI,
    &sun::SUN,
    &dos::DOS,
    &gpt::GPT,
    // Always after GPT.
    &gpt::PMBR,
    &mac::MAC,
    &ultrix::ULTRIX,
    &bsd::BSD,
    &unixware::UNIXWARE,
    &solaris_x86::SOLARIS_X86,
    &minix::MINIX,
    &atari::ATARI,
];

/// `partitions_drv`.
pub(crate) static DRIVER: ChainDrv = ChainDrv {
    dflt_flags: 0,
    dflt_enabled: false,
    has_fltr: true,
    idinfos: IDINFOS,
    probe: partitions_probe,
    safeprobe: partitions_probe,
};

/// A partition table's index in its list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabId(pub usize);

/// A partition's index in its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartId(pub usize);

/// `struct blkid_struct_parttable`.
#[derive(Clone, Debug, Default)]
pub struct Parttable {
    /// `type`: `dos`, `gpt`, `bsd`...
    pub ty: &'static str,
    /// `offset`: where the table is, in bytes.
    pub offset: u64,
    /// `nparts`: the partitions that refer to it.
    nparts: i32,
    /// `parent`: the partition a nested table is inside.
    pub parent: Option<PartId>,
    /// `id`: GPT's disk UUID, DOS's disk ID in hex -- at most 36 bytes.
    pub id: Vec<u8>,
}

impl Parttable {
    /// `blkid_parttable_get_id`: the ID, if it has one.
    #[must_use]
    pub fn id(&self) -> Option<&[u8]> {
        (!self.id.is_empty()).then_some(self.id.as_slice())
    }
}

/// `struct blkid_struct_partition`.
#[derive(Clone, Debug, Default)]
pub struct Partition {
    /// `start`, in 512-byte sectors.
    pub start: u64,
    /// `size`, in 512-byte sectors.
    pub size: u64,
    /// `type`: an MBR type byte, or 0 where the type is a string.
    pub ty: i32,
    /// `typestr`: GPT's type UUID, Mac's type name -- at most 36 bytes.
    pub typestr: Vec<u8>,
    /// `flags`: GPT's attributes, DOS's boot flag.
    pub flags: u64,
    /// `partno`: the number the partition would have (`sdaN`).
    pub partno: i32,
    /// `uuid`: GPT's partition UUID, DOS's `DISKID-NN` -- at most 36 bytes.
    pub uuid: Vec<u8>,
    /// `name`: GPT's and Mac's name, UTF-8 -- at most 127 bytes.
    pub name: Vec<u8>,
    /// `tab`.
    pub tab: TabId,
}

/// `struct blkid_struct_partlist`.
#[derive(Clone, Debug, Default)]
pub struct Partlist {
    /// `next_partno`: 0 until the list is first reset.
    next_partno: i32,
    /// `next_parent`: the parent for tables made during a subprobe.
    next_parent: Option<PartId>,
    /// `parts`.
    parts: Vec<Partition>,
    /// `l_tabs`, in the order made.
    tabs: Vec<Parttable>,
}

/// `set_string(item, max, data, len)`: at most `max - 1` bytes, as a C
/// string, trailing white space off.
fn set_string(max: usize, data: &[u8]) -> Vec<u8> {
    let n = data.len().min(max.saturating_sub(1));
    rtrim_whitespace(data.get(..n).unwrap_or_default()).to_vec()
}

impl Partlist {
    /// `reset_partlist`: empty, numbering from 1.
    pub(crate) fn reset(&mut self) {
        self.tabs.clear();
        self.parts.clear();
        self.next_parent = None;
        self.next_partno = 1;
    }

    /// `blkid_partlist_new_parttable(ls, type, offset)`.
    pub fn new_parttable(&mut self, ty: &'static str, offset: u64) -> TabId {
        self.tabs.push(Parttable {
            ty,
            offset,
            nparts: 0,
            parent: self.next_parent,
            id: Vec::new(),
        });
        TabId(self.tabs.len().saturating_sub(1))
    }

    /// `blkid_partlist_add_partition(ls, tab, start, size)`: the partition
    /// takes the next number.
    pub fn add_partition(&mut self, tab: TabId, start: u64, size: u64) -> PartId {
        if let Some(t) = self.tabs.get_mut(tab.0) {
            t.nparts = t.nparts.saturating_add(1);
        }
        let partno = self.increment_partno();
        self.parts.push(Partition {
            start,
            size,
            partno,
            tab,
            ..Partition::default()
        });
        PartId(self.parts.len().saturating_sub(1))
    }

    /// `blkid_partlist_set_partno`: number the next partition `partno`.
    pub fn set_partno(&mut self, partno: i32) {
        self.next_partno = partno;
    }

    /// `blkid_partlist_increment_partno`: the next number, and move on.
    pub fn increment_partno(&mut self) -> i32 {
        let n = self.next_partno;
        self.next_partno = self.next_partno.wrapping_add(1);
        n
    }

    /// `blkid_partlist_get_parent`.
    #[must_use]
    pub fn parent(&self) -> Option<PartId> {
        self.next_parent
    }

    /// `blkid_partlist_numof_partitions`.
    #[must_use]
    pub fn numof_partitions(&self) -> usize {
        self.parts.len()
    }

    /// `blkid_partlist_get_table`: the first table made -- the device's own.
    #[must_use]
    pub fn table(&self) -> Option<TabId> {
        (!self.tabs.is_empty()).then_some(TabId(0))
    }

    /// A table.
    #[must_use]
    pub fn tab(&self, id: TabId) -> Option<&Parttable> {
        self.tabs.get(id.0)
    }

    /// A table, to change.
    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Parttable> {
        self.tabs.get_mut(id.0)
    }

    /// `blkid_partlist_get_partition(ls, n)`.
    #[must_use]
    pub fn partition(&self, n: usize) -> Option<&Partition> {
        self.parts.get(n)
    }

    /// A partition, to change.
    pub fn part_mut(&mut self, id: PartId) -> Option<&mut Partition> {
        self.parts.get_mut(id.0)
    }

    /// Every partition, in the order found.
    #[must_use]
    pub fn partitions(&self) -> &[Partition] {
        &self.parts
    }

    /// `blkid_partlist_get_partition_by_start`.
    #[must_use]
    pub fn partition_by_start(&self, start: u64) -> Option<PartId> {
        self.parts.iter().position(|p| p.start == start).map(PartId)
    }

    /// `blkid_partlist_get_partition_by_partno`.
    #[must_use]
    pub fn partition_by_partno(&self, n: i32) -> Option<PartId> {
        self.parts.iter().position(|p| p.partno == n).map(PartId)
    }

    /// `partition_get_logical_type`: `P`, `E` or `L`.
    fn logical_type(&self, par: &Partition) -> Option<u8> {
        let tab = self.tabs.get(par.tab.0)?;
        if tab.ty.is_empty() {
            return None;
        }
        if tab.parent.is_some() {
            return Some(b'L');
        }
        if tab.ty == "dos" {
            if par.partno > 4 {
                return Some(b'L');
            }
            if matches!(
                par.ty,
                MBR_DOS_EXTENDED_PARTITION
                    | MBR_W95_EXTENDED_PARTITION
                    | MBR_LINUX_EXTENDED_PARTITION
            ) {
                return Some(b'E');
            }
        }
        Some(b'P')
    }

    /// `blkid_partition_is_primary`.
    #[must_use]
    pub fn is_primary(&self, par: &Partition) -> bool {
        self.logical_type(par) == Some(b'P')
    }

    /// `blkid_partition_is_extended`.
    #[must_use]
    pub fn is_extended(&self, par: &Partition) -> bool {
        self.logical_type(par) == Some(b'E')
    }

    /// `blkid_partition_is_logical`.
    #[must_use]
    pub fn is_logical(&self, par: &Partition) -> bool {
        self.logical_type(par) == Some(b'L')
    }

    /// `blkid_partition_gen_uuid(par)`: `TABLEID-NN` for a table with an
    /// ID. `false` when the table has none.
    pub fn gen_uuid(&mut self, par: PartId) -> bool {
        let Some(p) = self.parts.get(par.0) else {
            return false;
        };
        let Some(tab) = self.tabs.get(p.tab.0) else {
            return false;
        };
        if tab.id.is_empty() {
            return false;
        }
        let id = tab.id.get(..tab.id.len().min(33)).unwrap_or_default();
        let mut u = id.to_vec();
        u.extend_from_slice(format!("-{:02x}", p.partno).as_bytes());
        u.truncate(UUID_STR_LEN.saturating_sub(1));
        if let Some(p) = self.parts.get_mut(par.0) {
            p.uuid = u;
        }
        true
    }

    /// `blkid_partlist_devno_to_partition(ls, devno)`: the entry sysfs says
    /// the partition device `devno` is -- by its start and size, or (for a
    /// kpartx partition, which has no `start`) by the number in its DM
    /// UUID and its size.
    #[must_use]
    pub fn devno_to_partition(&self, devno: u64) -> Option<&Partition> {
        let pc = ulsysfs::new_sysfs_path(devno, None, None)?;
        let size = pc.read_u64(b"size")?;
        let mut partno = 0i32;
        let start = match pc.read_u64(b"start") {
            Some(s) => s,
            None => {
                // A partition mapped by kpartx has no `start`: its number is in
                // its DM UUID, "partN-...".
                let uuid = pc.read_string(b"dm/uuid").ok().flatten()?;
                let prefix = uuid.split(|&b| b == b'-').next().unwrap_or_default();
                if prefix.len() < 4
                    || !prefix
                        .get(..4)
                        .is_some_and(|h| h.eq_ignore_ascii_case(b"part"))
                {
                    return None;
                }
                let digits = prefix.get(4..).unwrap_or_default();
                // `strtol(prefix + 4, &end, 10)`, then `prefix == end` -- which
                // compares with the wrong pointer, so a bare "part" converts
                // nothing, passes, and means partition 0: a search by start 0.
                if !digits.is_empty() {
                    let sc = ulstrutils::scan_integer(digits, 10)?;
                    if sc.end != digits.len() {
                        return None;
                    }
                    let max = u128::from(i64::MAX.unsigned_abs());
                    if sc.saturated || sc.magnitude > max.saturating_add(u128::from(sc.negative)) {
                        // ERANGE.
                        return None;
                    }
                    let m = i128::try_from(sc.magnitude).ok()?;
                    let v = if sc.negative { m.wrapping_neg() } else { m };
                    // A long stored in an int: truncated, as C truncates it.
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "C's long-to-int assignment"
                    )]
                    {
                        partno = v as i32;
                    }
                }
                0
            }
        };
        if partno != 0 {
            return self.parts.iter().find(|par| {
                par.partno == partno
                    && (size == par.size || (self.is_extended(par) && size <= 1024))
            });
        }
        self.parts.iter().find(|par| {
            (par.start == start && par.size == size)
                || (par.start == start && self.is_extended(par) && size <= 1024)
        })
    }
}

impl Partition {
    /// `blkid_partition_set_type`.
    pub fn set_type(&mut self, ty: i32) {
        self.ty = ty;
    }

    /// `blkid_partition_set_name(par, name, len)`.
    pub fn set_name(&mut self, name: &[u8]) {
        self.name = set_string(128, name);
    }

    /// `blkid_partition_set_utf8name(par, name, len, enc)`.
    pub fn set_utf8name(&mut self, name: &[u8], enc: i32) {
        let u = encode_to_utf8(enc, 128, name);
        self.name = rtrim_whitespace(&u).to_vec();
    }

    /// `blkid_partition_set_uuid(par, uuid)`.
    pub fn set_uuid(&mut self, uuid: &[u8]) {
        self.uuid = unparse_uuid(uuid);
    }

    /// `blkid_partition_set_type_string(par, type, len)`.
    pub fn set_type_string(&mut self, ty: &[u8]) {
        self.typestr = set_string(UUID_STR_LEN, ty);
    }

    /// `blkid_partition_set_type_uuid(par, uuid)`.
    pub fn set_type_uuid(&mut self, uuid: &[u8]) {
        self.typestr = unparse_uuid(uuid);
    }

    /// `blkid_partition_set_flags`.
    pub fn set_flags(&mut self, flags: u64) {
        self.flags = flags;
    }

    /// `blkid_partition_get_name`.
    #[must_use]
    pub fn name(&self) -> Option<&[u8]> {
        let n = c_str(&self.name);
        (!n.is_empty()).then_some(n)
    }

    /// `blkid_partition_get_uuid`.
    #[must_use]
    pub fn uuid(&self) -> Option<&[u8]> {
        let u = c_str(&self.uuid);
        (!u.is_empty()).then_some(u)
    }

    /// `blkid_partition_get_type_string`.
    #[must_use]
    pub fn type_string(&self) -> Option<&[u8]> {
        let t = c_str(&self.typestr);
        (!t.is_empty()).then_some(t)
    }
}

/// `blkid_known_pttype`.
#[must_use]
pub fn known_pttype(pttype: &[u8]) -> bool {
    IDINFOS.iter().any(|id| id.name.as_bytes() == pttype)
}

/// `blkid_partitions_get_name(idx, &name)`.
#[must_use]
pub fn get_name(idx: usize) -> Option<&'static str> {
    IDINFOS.get(idx).map(|id| id.name)
}

/// `blkid_is_nested_dimension(par, start, size)`: `start..start+size` lies
/// inside the partition.
#[must_use]
pub fn is_nested_dimension(par: &Partition, start: u64, size: u64) -> bool {
    !(start < par.start || start.wrapping_add(size) > par.start.wrapping_add(par.size))
}

/// `partitions_init_data`: the chain's list, made or reset.
fn init_data(pr: &mut Probe) {
    let chn = pr.chain_mut(ChainId::Parts);
    match &mut chn.data {
        ChainData::Parts(ls) => ls.reset(),
        other => {
            let mut ls = Partlist::default();
            ls.reset();
            *other = ChainData::Parts(Box::new(ls));
        }
    }
}

/// `idinfo_probe(pr, id, chn)`: one prober -- size, magic, function -- and
/// PTMAGIC if it matched.
fn idinfo_probe(pr: &mut Probe, id: &'static IdInfo, chn: ChainId) -> i32 {
    if pr.size == 0 || (id.minsz != 0 && id.minsz > pr.size) {
        return PROBE_NONE;
    }
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return PROBE_NONE;
    }
    let (rc, off, mag) = pr.get_idmag(id);
    if rc != PROBE_OK {
        return PROBE_NONE;
    }
    let Some(f) = id.probefunc else {
        return rc;
    };
    pr.errno = 0;
    let mut rc = f(pr, mag);
    if rc < 0 {
        if let Some(ls) = pr.partlist_mut() {
            ls.reset();
        }
        if !pr.chain(chn).binary {
            pr.chain_reset_values(chn);
        }
    }
    if rc == PROBE_OK
        && let Some(m) = mag
        && !pr.chain(chn).binary
    {
        rc = pr.set_magic(off, m.magic);
    }
    rc
}

/// `partitions_probe`: the next table from where the chain stands, and then
/// -- for a partition device -- its own entry's details.
fn partitions_probe(pr: &mut Probe, chn: ChainId) -> i32 {
    if pr.chain(chn).idx < -1 {
        return -EINVAL;
    }
    pr.chain_reset_values(chn);
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return PROBE_NONE;
    }
    if pr.chain(chn).binary {
        init_data(pr);
    }
    let mut rc = PROBE_NONE;
    let skip_tables = pr.wipe_size() == 0 && pr.prob_flags & PROBE_FL_IGNORE_PT != 0;
    if !skip_tables {
        let start = usize::try_from(pr.chain(chn).idx.saturating_add(1)).unwrap_or(0);
        for (i, &id) in IDINFOS.iter().enumerate().skip(start) {
            pr.chain_mut(chn).idx = i32::try_from(i).unwrap_or(i32::MAX);
            if pr
                .chain(chn)
                .fltr
                .as_ref()
                .is_some_and(|f| f.get(i).copied().unwrap_or(false))
            {
                continue;
            }
            rc = idinfo_probe(pr, id, chn);
            if rc < 0 {
                break;
            }
            if rc != PROBE_OK {
                continue;
            }
            if !pr.chain(chn).binary {
                pr.set_value_str("PTTYPE", id.name.as_bytes());
            }
            rc = PROBE_OK;
            break;
        }
    }
    if (rc == PROBE_OK || rc == PROBE_NONE)
        && !pr.chain(chn).binary
        && pr.chain(ChainId::Parts).flags & PARTS_ENTRY_DETAILS != 0
    {
        let xrc = probe_partition(pr);
        if xrc < 0 || rc == PROBE_NONE {
            rc = xrc;
        }
    }
    rc
}

/// `blkid_partitions_do_subprobe(pr, parent, id)`: probe for a table nested
/// in partition `parent` (a BSD label inside a DOS slice) with a clone of
/// `pr` whose window is the partition; what it finds is added to `pr`'s
/// list, its tables parented on `parent`.
pub(crate) fn do_subprobe(pr: &mut Probe, parent: PartId, id: &'static IdInfo) -> i32 {
    let Some(par) = pr.partlist().and_then(|ls| ls.parts.get(parent.0)).cloned() else {
        return -EINVAL;
    };
    if par.size == 0 {
        return -EINVAL;
    }
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return PROBE_NONE;
    }
    let sz = par.size << 9;
    let off = par.start << 9;
    if off < pr.off || pr.off.wrapping_add(pr.size) < off.wrapping_add(sz) {
        return -ENOSPC;
    }
    let mut prc = Probe::clone_of(pr);
    prc.set_dimension(off, sz);
    // The clone runs as the parent's chain: its flags, binary or not.
    let chn = pr.cur_chain.unwrap_or(ChainId::Parts);
    {
        let (flags, binary, idx, enabled) = {
            let c = pr.chain(chn);
            (c.flags, c.binary, c.idx, c.enabled)
        };
        let cc = prc.chain_mut(chn);
        cc.flags = flags;
        cc.binary = binary;
        cc.idx = idx;
        cc.enabled = enabled;
    }
    prc.cur_chain = Some(chn);
    // The parent's list, extended rather than replaced.
    let mut data = std::mem::take(&mut pr.chain_mut(ChainId::Parts).data);
    if let ChainData::Parts(ls) = &mut data {
        ls.next_parent = Some(parent);
    }
    prc.chain_mut(ChainId::Parts).data = data;
    let parent_probe = std::mem::take(pr);
    prc.attach_parent(parent_probe);
    let rc = idinfo_probe(&mut prc, id, chn);
    let mut data = std::mem::take(&mut prc.chain_mut(ChainId::Parts).data);
    if let ChainData::Parts(ls) = &mut data {
        ls.next_parent = None;
    }
    if let Some(back) = prc.detach_parent() {
        *pr = back;
    }
    pr.errno = prc.errno;
    pr.chain_mut(ChainId::Parts).data = data;
    rc
}

/// `blkid_partitions_need_typeonly`: only the table's type is wanted, not
/// its partitions -- anything but the binary interface.
#[must_use]
pub(crate) fn need_typeonly(pr: &Probe) -> bool {
    let Some(c) = pr.cur_chain else {
        return true;
    };
    let chn = pr.chain(c);
    !(matches!(chn.data, ChainData::Parts(_)) && chn.binary)
}

/// `blkid_partitions_get_flags`.
#[must_use]
pub(crate) fn get_flags(pr: &Probe) -> u32 {
    pr.chain_flags()
}

/// `blkid_partitions_set_ptuuid(pr, uuid)`: PTUUID from a binary UUID, for
/// the values interface only.
pub(crate) fn set_ptuuid(pr: &mut Probe, uuid: &[u8]) -> i32 {
    if pr.chain_binary() || uuid_is_empty(uuid.get(..16).unwrap_or(uuid)) {
        return 0;
    }
    let mut v = unparse_uuid(uuid);
    v.push(0);
    pr.push_value("PTUUID", v);
    0
}

/// `blkid_partitions_strcpy_ptuuid(pr, str)`: PTUUID from text.
pub(crate) fn strcpy_ptuuid(pr: &mut Probe, s: &[u8]) -> i32 {
    let s = c_str(s);
    if pr.chain_binary() || s.is_empty() {
        return 0;
    }
    let mut v = s.to_vec();
    v.push(0);
    pr.push_value("PTUUID", v);
    0
}

/// `blkid_parttable_set_uuid(tab, id)`.
pub(crate) fn parttable_set_uuid(tab: &mut Parttable, id: &[u8]) {
    tab.id = unparse_uuid(id);
}

/// `blkid_parttable_set_id(tab, id)`: text, at most 36 bytes.
pub(crate) fn parttable_set_id(tab: &mut Parttable, id: &[u8]) {
    let id = c_str(id);
    tab.id = id
        .get(..id.len().min(UUID_STR_LEN - 1))
        .unwrap_or_default()
        .to_vec();
}

/// `blkid_partitions_probe_partition`: for a partition device, its entry in
/// the whole disk's table, as `PART_ENTRY_*` values.
fn probe_partition(pr: &mut Probe) -> i32 {
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return PROBE_NONE;
    }
    let devno = pr.devno();
    if devno == 0 {
        return PROBE_NONE;
    }
    let Some(disk) = pr.wholedisk_probe() else {
        return PROBE_NONE;
    };
    if !disk.get_binary_data(ChainId::Parts) {
        return PROBE_NONE;
    }
    let disk_devno = disk.devno();
    let Some(ls) = disk.partlist() else {
        return PROBE_NONE;
    };
    let Some(par) = ls.devno_to_partition(devno) else {
        return PROBE_NONE;
    };
    let scheme = ls.tab(par.tab).map(|t| t.ty);
    let par = par.clone();
    if let Some(v) = scheme {
        pr.set_value_str("PART_ENTRY_SCHEME", v.as_bytes());
    }
    if let Some(v) = par.name() {
        let v = v.to_vec();
        pr.set_value_str("PART_ENTRY_NAME", &v);
    }
    if let Some(v) = par.uuid() {
        let v = v.to_vec();
        pr.set_value_str("PART_ENTRY_UUID", &v);
    }
    match par.type_string() {
        Some(v) => {
            let v = v.to_vec();
            pr.set_value_str("PART_ENTRY_TYPE", &v);
        }
        None => {
            pr.set_value_str("PART_ENTRY_TYPE", format!("0x{:x}", par.ty).as_bytes());
        }
    }
    if par.flags != 0 {
        pr.set_value_str("PART_ENTRY_FLAGS", format!("0x{:x}", par.flags).as_bytes());
    }
    pr.set_value_str("PART_ENTRY_NUMBER", par.partno.to_string().as_bytes());
    // `%jd` of the unsigned values, as upstream casts them.
    #[allow(clippy::cast_possible_wrap, reason = "C's (intmax_t) casts")]
    {
        pr.set_value_str(
            "PART_ENTRY_OFFSET",
            (par.start as i64).to_string().as_bytes(),
        );
        pr.set_value_str("PART_ENTRY_SIZE", (par.size as i64).to_string().as_bytes());
    }
    pr.set_value_str(
        "PART_ENTRY_DISK",
        format!(
            "{}:{}",
            ulsysfs::major(disk_devno),
            ulsysfs::minor(disk_devno)
        )
        .as_bytes(),
    );
    PROBE_OK
}

/// `blkid_probe_is_covered_by_pt(pr, offset, size)`: whether the device's
/// partition table -- read by a clone, leaving `pr`'s probing where it was
/// -- has a partition holding `offset..offset+size`. A table with a
/// partition past the end of the device covers nothing.
pub(crate) fn is_covered_by_pt(pr: &mut Probe, offset: u64, size: u64) -> bool {
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return false;
    }
    let mut prc = Probe::clone_of(pr);
    let dev_sectors = pr.size >> 9;
    let parent = std::mem::take(pr);
    prc.attach_parent(parent);
    let found = prc.get_binary_data(ChainId::Parts);
    let parts = prc
        .partlist()
        .map(|ls| ls.parts.clone())
        .unwrap_or_default();
    if let Some(back) = prc.detach_parent() {
        *pr = back;
    }
    if !found || parts.is_empty() {
        return false;
    }
    let end = offset.wrapping_add(size) >> 9;
    let start = offset >> 9;
    if parts
        .iter()
        .any(|p| p.start.wrapping_add(p.size) > dev_sectors)
    {
        return false;
    }
    parts
        .iter()
        .any(|p| start >= p.start && end <= p.start.wrapping_add(p.size))
}

impl Probe {
    /// `blkid_probe_enable_partitions`.
    pub fn enable_partitions(&mut self, enable: bool) {
        self.chain_mut(ChainId::Parts).enabled = enable;
    }

    /// `blkid_probe_set_partitions_flags`.
    pub fn set_partitions_flags(&mut self, flags: u32) {
        self.chain_mut(ChainId::Parts).flags = flags;
    }

    /// `blkid_probe_get_partitions_flags`.
    #[must_use]
    pub fn partitions_flags(&self) -> u32 {
        self.chain(ChainId::Parts).flags
    }

    /// `blkid_probe_reset_partitions_filter`.
    pub fn reset_partitions_filter(&mut self) -> i32 {
        self.reset_filter(ChainId::Parts)
    }

    /// `blkid_probe_invert_partitions_filter`.
    pub fn invert_partitions_filter(&mut self) -> i32 {
        self.invert_filter(ChainId::Parts)
    }

    /// `blkid_probe_filter_partitions_type(pr, flag, names)`.
    pub fn filter_partitions_type(&mut self, flag: u32, names: &[&[u8]]) -> i32 {
        self.filter_types(ChainId::Parts, flag, names)
    }

    /// `blkid_probe_get_partitions`: the binary interface -- the device's
    /// tables and partitions, independent of any values probing. `None` when
    /// there is no table.
    pub fn partitions(&mut self) -> Option<&Partlist> {
        if !self.get_binary_data(ChainId::Parts) {
            return None;
        }
        self.partlist()
    }
}
