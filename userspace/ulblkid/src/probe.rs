//! `probe.c`: the probe -- a device, the window of it being probed, the
//! buffers read from it, the results, the hints -- and the loop that runs
//! the chains over it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;

use crate::blkdev;
use crate::partitions::Partlist;
use crate::topology::Topology;
use crate::{
    EINVAL, ENOMEM, ERANGE, PARTS_FORCE_GPT, PARTS_MAGIC, PROBE_ERROR, PROBE_NONE, PROBE_OK,
    S_IFBLK, S_IFCHR, S_IFMT, S_IFREG, SUBLKS_BADCSUM, SUBLKS_MAGIC, errno_of,
};

/// `BLKID_FL_PRIVATE_FD`: the probe opened the device and closes it.
pub(crate) const FL_PRIVATE_FD: u32 = 1 << 1;
/// `BLKID_FL_TINY_DEV`: at most 1.44 MiB, a floppy's size.
pub(crate) const FL_TINY_DEV: u32 = 1 << 2;
/// `BLKID_FL_CDROM_DEV`.
pub(crate) const FL_CDROM_DEV: u32 = 1 << 3;
/// `BLKID_FL_NOSCAN_DEV`: a private device-mapper device; never probed.
pub(crate) const FL_NOSCAN_DEV: u32 = 1 << 4;
/// `BLKID_FL_MODIF_BUFF`: [`Probe::hide_range`] changed the buffers.
pub(crate) const FL_MODIF_BUFF: u32 = 1 << 5;
/// `BLKID_FL_OPAL_LOCKED`: an OPAL-locked device; its I/O errors are
/// expected.
pub(crate) const FL_OPAL_LOCKED: u32 = 1 << 6;

/// `BLKID_PROBE_FL_IGNORE_PT`: a RAID was found, so a partition table seen
/// through it is its members', not its own.
pub(crate) const PROBE_FL_IGNORE_PT: u32 = 1 << 1;

/// The three chains, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainId {
    /// `BLKID_CHAIN_SUBLKS`: filesystem and RAID superblocks.
    Sublks = 0,
    /// `BLKID_CHAIN_TOPLGY`: block device topology.
    Toplgy = 1,
    /// `BLKID_CHAIN_PARTS`: partition tables.
    Parts = 2,
}

impl ChainId {
    /// All of them, in order.
    pub const ALL: [ChainId; 3] = [ChainId::Sublks, ChainId::Toplgy, ChainId::Parts];

    /// Index into [`Probe::chains`].
    #[must_use]
    pub fn index(self) -> usize {
        self as usize
    }
}

/// `struct blkid_idmag`: a magic string and where it sits.
#[derive(Debug)]
pub struct IdMag {
    /// The magic bytes (`magic`, `len` of them).
    pub magic: &'static [u8],
    /// `hoff`: a hint whose value is added to the offset.
    pub hoff: Option<&'static str>,
    /// `kboff`: the superblock's offset in KiB.
    pub kboff: i64,
    /// `sboff`: the magic's byte offset within the superblock.
    pub sboff: u32,
    /// `is_zoned`: the offset is within a zone.
    pub is_zoned: bool,
    /// `zonenum`.
    pub zonenum: i64,
    /// `kboff_inzone`.
    pub kboff_inzone: i64,
}

impl IdMag {
    /// A magic `magic` at `kboff` KiB plus `sboff` bytes.
    #[must_use]
    pub const fn new(magic: &'static [u8], kboff: i64, sboff: u32) -> Self {
        IdMag {
            magic,
            hoff: None,
            kboff,
            sboff,
            is_zoned: false,
            zonenum: 0,
            kboff_inzone: 0,
        }
    }

    /// The same, offset further by the value of the hint `hoff`.
    #[must_use]
    pub const fn hinted(magic: &'static [u8], hoff: &'static str, kboff: i64, sboff: u32) -> Self {
        IdMag {
            magic,
            hoff: Some(hoff),
            kboff,
            sboff,
            is_zoned: false,
            zonenum: 0,
            kboff_inzone: 0,
        }
    }

    /// A magic in zone `zonenum`, `kboff_inzone` KiB into it.
    #[must_use]
    pub const fn zoned(magic: &'static [u8], zonenum: i64, kboff_inzone: i64, sboff: u32) -> Self {
        IdMag {
            magic,
            hoff: None,
            kboff: 0,
            sboff,
            is_zoned: true,
            zonenum,
            kboff_inzone,
        }
    }
}

/// A prober: `BLKID_PROBE_OK`, `BLKID_PROBE_NONE`, or `-errno`.
pub type ProbeFn = fn(&mut Probe, Option<&'static IdMag>) -> i32;

/// `struct blkid_idinfo`: one prober.
#[derive(Debug)]
pub struct IdInfo {
    /// The type it reports (`vfat`, `gpt`).
    pub name: &'static str,
    /// `BLKID_USAGE_*`.
    pub usage: u32,
    /// `BLKID_IDINFO_*`.
    pub flags: u32,
    /// The smallest device it looks at.
    pub minsz: u64,
    /// The final check, after a magic matched.
    pub probefunc: Option<ProbeFn>,
    /// Its magic strings; empty for a prober that has none.
    pub magics: &'static [IdMag],
}

/// A chain's private data.
#[derive(Debug, Default)]
pub(crate) enum ChainData {
    /// None yet.
    #[default]
    None,
    /// The partitions chain's list.
    Parts(Box<Partlist>),
    /// The topology chain's binary result.
    Topology(Topology),
}

/// `struct blkid_chain`: a chain's state in a probe.
#[derive(Debug)]
pub(crate) struct Chain {
    /// `enabled`.
    pub(crate) enabled: bool,
    /// `flags`: `BLKID_SUBLKS_*` or `BLKID_PARTS_*`.
    pub(crate) flags: u32,
    /// `binary`: probing for the binary interface, not for values.
    pub(crate) binary: bool,
    /// `idx`: the prober reached, -1 before the first.
    pub(crate) idx: i32,
    /// `fltr`: a set bit is a prober filtered out.
    pub(crate) fltr: Option<Vec<bool>>,
    /// `data`.
    pub(crate) data: ChainData,
}

/// `struct blkid_chaindrv`: what a chain is. (Upstream's `id` and `name`
/// are left out: the port always knows which chain it is in, and the name
/// only appears in debugging output.)
pub(crate) struct ChainDrv {
    /// `dflt_flags`.
    pub(crate) dflt_flags: u32,
    /// `dflt_enabled`.
    pub(crate) dflt_enabled: bool,
    /// `has_fltr`.
    pub(crate) has_fltr: bool,
    /// `idinfos`.
    pub(crate) idinfos: &'static [&'static IdInfo],
    /// `probe`.
    pub(crate) probe: fn(&mut Probe, ChainId) -> i32,
    /// `safeprobe`.
    pub(crate) safeprobe: fn(&mut Probe, ChainId) -> i32,
}

/// `chains_drvs[]`.
pub(crate) fn driver(id: ChainId) -> &'static ChainDrv {
    match id {
        ChainId::Sublks => &crate::superblocks::DRIVER,
        ChainId::Toplgy => &crate::topology::DRIVER,
        ChainId::Parts => &crate::partitions::DRIVER,
    }
}

/// `struct blkid_prval`: one result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    /// The name (`TYPE`, `LABEL`).
    pub name: &'static str,
    /// The data, `len` bytes of it -- a string's count its NUL.
    data: Vec<u8>,
    /// The chain that set it.
    pub(crate) chain: ChainId,
}

impl Value {
    /// The value's bytes, as `blkid_probe_get_value` returns them with
    /// their length: a string ends in its NUL, binary data (`SBMAGIC`,
    /// `LABEL_RAW`, `UUID_RAW`) has none.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The value as the C string a caller printing it with `%s` sees.
    #[must_use]
    pub fn as_c_str(&self) -> &[u8] {
        crate::c_str(&self.data)
    }

    /// `len`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether `len` is 0.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// `struct blkid_bufinfo`: bytes read from the device, kept for reuse.
#[derive(Debug)]
struct BufInfo {
    /// `off`: where on the device (not in the probing window).
    off: u64,
    /// The bytes.
    data: Rc<[u8]>,
}

/// A window onto a cached buffer: what `blkid_probe_get_buffer` returns.
#[derive(Clone, Debug)]
pub struct Buf {
    data: Rc<[u8]>,
    start: usize,
    len: usize,
}

impl std::ops::Deref for Buf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.data
            .get(self.start..self.start.saturating_add(self.len))
            .unwrap_or_default()
    }
}

impl Buf {
    /// The cached buffer from this window's start to the buffer's end --
    /// what upstream sees when a prober reads past the length it asked for
    /// and its pointer runs on into the rest of the buffer. (A BSD label at
    /// byte 128 of sector 0 is 404 bytes long; upstream asks for the sector
    /// and reads the label's tail out of the 1 KiB buffer the magic search
    /// cached.) Past the buffer's end upstream reads whatever the heap holds;
    /// [`Bytes`](crate::Bytes) reads zeros there.
    #[must_use]
    pub fn through_end(&self) -> &[u8] {
        self.data.get(self.start..).unwrap_or_default()
    }
}

/// `struct blkid_hint`.
#[derive(Clone, Debug)]
struct Hint {
    name: Vec<u8>,
    value: u64,
}

/// `struct blkid_struct_probe`.
#[derive(Debug)]
pub struct Probe {
    /// `fd`.
    file: Option<Rc<File>>,
    /// `off`: where the probing window starts on the device.
    pub(crate) off: u64,
    /// `size`: the window's size.
    pub(crate) size: u64,
    /// `devno`: `st_rdev`, 0 for a regular file.
    pub(crate) devno: u64,
    /// `disk_devno`: the whole disk's, found lazily.
    pub(crate) disk_devno: u64,
    /// `blkssz`: the logical sector size, found lazily.
    pub(crate) blkssz: u32,
    /// `mode`: `st_mode`.
    pub(crate) mode: u32,
    /// `zone_size`.
    pub(crate) zone_size: u64,
    /// `flags`: `BLKID_FL_*`.
    pub(crate) flags: u32,
    /// `prob_flags`: `BLKID_PROBE_FL_*`, cleared by every `blkid_do_*`.
    pub(crate) prob_flags: u32,
    wipe_off: u64,
    wipe_size: u64,
    wipe_chain: Option<ChainId>,
    buffers: Vec<BufInfo>,
    hints: Vec<Hint>,
    /// `chains[]`.
    pub(crate) chains: [Chain; 3],
    /// `cur_chain`.
    pub(crate) cur_chain: Option<ChainId>,
    values: Vec<Value>,
    /// `parent`: for a clone, the probe it was cloned from -- moved in for
    /// the clone's lifetime, and back out after.
    parent: Option<Box<Probe>>,
    /// `disk_probe`: the whole disk, for a partition's `PART_ENTRY_*`.
    disk_probe: Option<Box<Probe>>,
    /// Upstream's `errno`, as libblkid's functions leave it.
    pub errno: i32,
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

/// `blkid_parse_tag_string(token, &type, &val)`: `NAME=value`, the value
/// optionally in matching quotes (the last such quote closes it). `None`
/// without an `=`, with an unclosed quote, or with an empty value.
pub(crate) fn parse_tag_string(token: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let eq = token.iter().position(|&b| b == b'=')?;
    let name = token.get(..eq)?.to_vec();
    let mut value = crate::c_str(token.get(eq.saturating_add(1)..)?);
    if let Some(&q) = value.first()
        && (q == b'"' || q == b'\'')
    {
        let rest = value.get(1..)?;
        let close = rest.iter().rposition(|&b| b == q)?;
        value = rest.get(..close)?;
    }
    if value.is_empty() {
        return None;
    }
    Some((name, value.to_vec()))
}

impl Probe {
    /// `blkid_new_probe`.
    #[must_use]
    pub fn new() -> Self {
        let chain = |id: ChainId| {
            let d = driver(id);
            Chain {
                enabled: d.dflt_enabled,
                flags: d.dflt_flags,
                binary: false,
                idx: -1,
                fltr: None,
                data: ChainData::None,
            }
        };
        Probe {
            file: None,
            off: 0,
            size: 0,
            devno: 0,
            disk_devno: 0,
            blkssz: 0,
            mode: 0,
            zone_size: 0,
            flags: 0,
            prob_flags: 0,
            wipe_off: 0,
            wipe_size: 0,
            wipe_chain: None,
            buffers: Vec::new(),
            hints: Vec::new(),
            chains: [
                chain(ChainId::Sublks),
                chain(ChainId::Toplgy),
                chain(ChainId::Parts),
            ],
            cur_chain: None,
            values: Vec::new(),
            parent: None,
            disk_probe: None,
            errno: 0,
        }
    }

    /// `blkid_clone_probe(parent)`: a probe sharing the parent's device,
    /// window, numbers and flags -- but not its results, and not its buffers
    /// unless the parent is attached with [`Probe::attach_parent`].
    #[must_use]
    pub(crate) fn clone_of(parent: &Probe) -> Probe {
        let mut pr = Probe::new();
        pr.file = parent.file.clone();
        pr.off = parent.off;
        pr.size = parent.size;
        pr.devno = parent.devno;
        pr.disk_devno = parent.disk_devno;
        pr.blkssz = parent.blkssz;
        pr.flags = parent.flags & !FL_PRIVATE_FD;
        pr.zone_size = parent.zone_size;
        pr
    }

    /// Move `parent` into this clone, so its buffers are shared as upstream's
    /// `pr->parent` shares them. [`Probe::detach_parent`] gives it back.
    pub(crate) fn attach_parent(&mut self, parent: Probe) {
        self.parent = Some(Box::new(parent));
    }

    /// Take the parent back out.
    pub(crate) fn detach_parent(&mut self) -> Option<Probe> {
        self.parent.take().map(|b| *b)
    }

    /// `blkid_new_probe_from_filename(filename)`: open it read-only (and
    /// non-blocking) and set it as the device. `Err(errno)` from the open or
    /// from [`Probe::set_device`].
    ///
    /// # Errors
    ///
    /// The `errno` of the open or of setting the device.
    pub fn from_filename(filename: &[u8]) -> Result<Probe, i32> {
        let file = open_nonblock(filename)?;
        let mut pr = Probe::new();
        if pr.set_device(Some(Rc::new(file)), 0, 0) != 0 {
            return Err(if pr.errno != 0 { pr.errno } else { EINVAL });
        }
        pr.flags |= FL_PRIVATE_FD;
        Ok(pr)
    }

    /// The device, as `blkid_probe_get_fd` gives it.
    #[must_use]
    pub fn file(&self) -> Option<&Rc<File>> {
        self.file.as_ref()
    }

    /// `blkid_probe_chain_reset_values`: drop the chain's results.
    pub(crate) fn chain_reset_values(&mut self, chn: ChainId) {
        self.values.retain(|v| v.chain != chn);
    }

    /// `blkid_probe_chain_reset_position`.
    fn chain_reset_position(&mut self, chn: ChainId) {
        self.chain_mut(chn).idx = -1;
    }

    /// `blkid_probe_chain_save_values`: move the chain's results out.
    pub(crate) fn chain_save_values(&mut self, chn: ChainId) -> Vec<Value> {
        let (mine, rest): (Vec<Value>, Vec<Value>) = std::mem::take(&mut self.values)
            .into_iter()
            .partition(|v| v.chain == chn);
        self.values = rest;
        mine
    }

    /// `blkid_probe_append_values_list`.
    pub(crate) fn append_values(&mut self, vals: Vec<Value>) {
        self.values.extend(vals);
    }

    /// A chain's state.
    pub(crate) fn chain(&self, id: ChainId) -> &Chain {
        // The array has one entry per `ChainId`.
        #[allow(clippy::indexing_slicing, reason = "ChainId indexes a 3-array")]
        &self.chains[id.index()]
    }

    /// A chain's state, to change.
    pub(crate) fn chain_mut(&mut self, id: ChainId) -> &mut Chain {
        #[allow(clippy::indexing_slicing, reason = "ChainId indexes a 3-array")]
        &mut self.chains[id.index()]
    }

    /// `blkid_probe_get_chain`: the chain now running.
    #[must_use]
    pub fn current_chain(&self) -> Option<ChainId> {
        self.cur_chain
    }

    /// The current chain's flags, 0 outside probing.
    pub(crate) fn chain_flags(&self) -> u32 {
        self.cur_chain.map_or(0, |c| self.chain(c).flags)
    }

    /// Whether the current chain is probing for its binary interface.
    pub(crate) fn chain_binary(&self) -> bool {
        self.cur_chain.is_some_and(|c| self.chain(c).binary)
    }

    /// `blkid_probe_get_probername`: the prober the current chain reached.
    #[must_use]
    pub fn probername(&self) -> Option<&'static str> {
        let c = self.cur_chain?;
        let idx = usize::try_from(self.chain(c).idx).ok()?;
        Some(driver(c).idinfos.get(idx)?.name)
    }

    /// `blkid_probe_get_binary_data(pr, chn)`: run the chain for its binary
    /// interface, independent of any probing in progress. `false` when it
    /// found nothing (or failed).
    pub(crate) fn get_binary_data(&mut self, chn: ChainId) -> bool {
        let org_chn = self.cur_chain;
        let org_prob_flags = self.prob_flags;
        self.cur_chain = Some(chn);
        self.prob_flags = 0;
        self.chain_mut(chn).binary = true;
        self.chain_reset_position(chn);
        let rc = (driver(chn).probe)(self, chn);
        self.chain_mut(chn).binary = false;
        self.chain_reset_position(chn);
        self.cur_chain = org_chn;
        self.prob_flags = org_prob_flags;
        rc == 0
    }

    /// `blkid_reset_probe`: results gone, every chain back to its start;
    /// device, filters and hints kept.
    pub fn reset_probe(&mut self) {
        self.values.clear();
        self.set_wiper(0, 0);
        self.cur_chain = None;
        for c in ChainId::ALL {
            self.chain_reset_position(c);
        }
    }

    /// `blkid_probe_get_filter(pr, chain, create)`: the chain's filter,
    /// cleared (made, with `create`, if there is none). Touching a filter
    /// restarts the chain.
    pub(crate) fn get_filter(&mut self, chain: ChainId, create: bool) -> Option<&mut Vec<bool>> {
        self.chain_reset_position(chain);
        self.cur_chain = None;
        let d = driver(chain);
        let n = d.idinfos.len();
        let chn = self.chain_mut(chain);
        if !d.has_fltr || (chn.fltr.is_none() && !create) {
            return None;
        }
        let f = chn.fltr.get_or_insert_with(Vec::new);
        f.clear();
        f.resize(n, false);
        Some(f)
    }

    /// `__blkid_probe_invert_filter`.
    pub(crate) fn invert_filter(&mut self, chain: ChainId) -> i32 {
        let d = driver(chain);
        let chn = self.chain_mut(chain);
        match chn.fltr.as_mut() {
            Some(f) if d.has_fltr => {
                for b in f.iter_mut() {
                    *b = !*b;
                }
                0
            }
            _ => -1,
        }
    }

    /// `__blkid_probe_reset_filter`.
    pub(crate) fn reset_filter(&mut self, chain: ChainId) -> i32 {
        if self.get_filter(chain, false).is_some() {
            0
        } else {
            -1
        }
    }

    /// `__blkid_probe_filter_types(pr, chain, flag, names)`.
    pub(crate) fn filter_types(&mut self, chain: ChainId, flag: u32, names: &[&[u8]]) -> i32 {
        let idinfos = driver(chain).idinfos;
        let Some(f) = self.get_filter(chain, true) else {
            return -1;
        };
        for (i, id) in idinfos.iter().enumerate() {
            let has = names.contains(&id.name.as_bytes());
            let set = if has {
                flag & crate::FLTR_NOTIN != 0
            } else {
                flag & crate::FLTR_ONLYIN != 0
            };
            if set && let Some(b) = f.get_mut(i) {
                *b = true;
            }
        }
        0
    }

    /// `read_buffer(pr, real_off, len)`: one `read` of `len` bytes at
    /// `real_off`. A short read is "nothing", `errno` 0; a failed one keeps
    /// the read's `errno` -- unless the device is a CD-ROM (audio tracks),
    /// or OPAL-locked, or turns out to be once asked.
    fn read_buffer(&mut self, real_off: u64, len: u64) -> Option<BufInfo> {
        let Some(file) = self.file.clone() else {
            self.errno = 0;
            return None;
        };
        let mut f: &File = &file;
        if f.seek(SeekFrom::Start(real_off)).is_err() {
            self.errno = 0;
            return None;
        }
        let Ok(n) = usize::try_from(len) else {
            self.errno = ENOMEM;
            return None;
        };
        let mut data = Vec::new();
        if data.try_reserve_exact(n).is_err() {
            self.errno = ENOMEM;
            return None;
        }
        data.resize(n, 0);
        let ret = f.read(&mut data);
        match ret {
            Ok(got) if got == n => {}
            Ok(_) => {
                self.errno = 0;
                return None;
            }
            Err(e) => {
                self.errno = errno_of(&e);
                if self.is_cdrom() || self.is_opal_locked() {
                    self.errno = 0;
                } else {
                    // An I/O error deep in a locked OPAL drive is expected; asking
                    // sets `errno` either way, as upstream's `ioctl` does.
                    let mut st = [0u32; 2];
                    // SAFETY: IOC_OPAL_GET_STATUS writes a struct opal_status,
                    // two u32s.
                    match unsafe { blkdev::ioctl_ptr(&file, blkdev::IOC_OPAL_GET_STATUS, &mut st) }
                    {
                        Ok(0) if st[0] & blkdev::OPAL_FL_LOCKED != 0 => {
                            self.flags |= FL_OPAL_LOCKED;
                            self.errno = 0;
                        }
                        Ok(_) => {}
                        Err(e) => self.errno = e,
                    }
                }
                return None;
            }
        }
        Some(BufInfo {
            off: real_off,
            data: Rc::from(data.into_boxed_slice()),
        })
    }

    /// `hide_buffer(pr, off, len)`: zero `len` bytes at `off` in every
    /// cached buffer that holds all of them.
    fn hide_buffer(&mut self, off: u64, len: u64) -> i32 {
        let real_off = self.off.wrapping_add(off);
        // `UINT64_MAX - len < off`.
        if off.checked_add(len).is_none() {
            return -EINVAL;
        }
        let mut hidden = false;
        for x in &mut self.buffers {
            let xlen = u64::try_from(x.data.len()).unwrap_or(u64::MAX);
            if real_off >= x.off && real_off.wrapping_add(len) <= x.off.wrapping_add(xlen) {
                let start = usize::try_from(real_off.wrapping_sub(x.off)).unwrap_or(usize::MAX);
                let n = usize::try_from(len).unwrap_or(usize::MAX);
                let mut copy = x.data.to_vec();
                if let Some(s) = copy.get_mut(start..start.saturating_add(n)) {
                    s.fill(0);
                }
                x.data = Rc::from(copy.into_boxed_slice());
                hidden = true;
            }
        }
        if hidden { 0 } else { -EINVAL }
    }

    /// `blkid_probe_get_buffer(pr, off, len)`: `len` bytes at `off` within
    /// the probing window, from a buffer already read if one holds them all,
    /// else read now and kept. `None` with [`Probe::errno`] 0 for a request
    /// outside the window (or a short read); with it set for a failed read
    /// or `EINVAL` for a window of size 0.
    pub fn get_buffer(&mut self, off: u64, len: u64) -> Option<Buf> {
        let real_off = self.off.wrapping_add(off);
        if self.size == 0 {
            self.errno = EINVAL;
            return None;
        }
        // `UINT64_MAX - len < off || UINT64_MAX - len < real_off`.
        if off.checked_add(len).is_none() || real_off.checked_add(len).is_none() {
            // Upstream returns here without touching errno.
            return None;
        }
        let chr = self.is_chr();
        if len == 0
            || (!chr && (self.size < off || self.size < len))
            || (!chr && self.off.wrapping_add(self.size) < real_off.wrapping_add(len))
        {
            self.errno = 0;
            return None;
        }
        if let Some(parent) = self.parent.as_mut()
            && parent.devno == self.devno
            && parent.off <= self.off
            && parent.off.wrapping_add(parent.size) >= self.off.wrapping_add(self.size)
        {
            // A clone looking at the parent's area: the parent's buffers.
            let poff = self.off.wrapping_add(off).wrapping_sub(parent.off);
            let r = parent.get_buffer(poff, len);
            self.errno = parent.errno;
            return r;
        }
        let found = self.buffers.iter().position(|x| {
            let xlen = u64::try_from(x.data.len()).unwrap_or(u64::MAX);
            real_off >= x.off && real_off.wrapping_add(len) <= x.off.wrapping_add(xlen)
        });
        let idx = match found {
            Some(i) => i,
            None => {
                let bf = self.read_buffer(real_off, len)?;
                self.buffers.push(bf);
                self.buffers.len().saturating_sub(1)
            }
        };
        let bf = self.buffers.get(idx)?;
        self.errno = 0;
        Some(Buf {
            data: bf.data.clone(),
            start: usize::try_from(real_off.wrapping_sub(bf.off)).unwrap_or(usize::MAX),
            len: usize::try_from(len).unwrap_or(usize::MAX),
        })
    }

    /// `errno ? -errno : BLKID_PROBE_NONE`: what a prober returns when a
    /// read came back empty.
    #[must_use]
    pub fn none_or_err(&self) -> i32 {
        if self.errno != 0 {
            self.errno.wrapping_neg()
        } else {
            PROBE_NONE
        }
    }

    /// Zero `zlen` bytes at `zoff` inside the cached buffer
    /// [`Probe::get_buffer`] would return for `off..off+len` -- what upstream
    /// does when a prober `memset`s through the pointer it was handed (MD
    /// RAID's checksum zeroes its own field before summing). Later reads
    /// served from that buffer see the zeros, as upstream's do. Nothing
    /// happens if no buffer holds the range.
    pub(crate) fn zero_in_cache(&mut self, off: u64, len: u64, zoff: u64, zlen: u64) {
        let real_off = self.off.wrapping_add(off);
        if let Some(parent) = self.parent.as_mut()
            && parent.devno == self.devno
            && parent.off <= self.off
            && parent.off.wrapping_add(parent.size) >= self.off.wrapping_add(self.size)
        {
            let base = self.off.wrapping_sub(parent.off);
            parent.zero_in_cache(base.wrapping_add(off), len, base.wrapping_add(zoff), zlen);
            return;
        }
        let Some(x) = self.buffers.iter_mut().find(|x| {
            let xlen = u64::try_from(x.data.len()).unwrap_or(u64::MAX);
            real_off >= x.off && real_off.wrapping_add(len) <= x.off.wrapping_add(xlen)
        }) else {
            return;
        };
        let zreal = self.off.wrapping_add(zoff);
        let Some(start) = zreal
            .checked_sub(x.off)
            .and_then(|s| usize::try_from(s).ok())
        else {
            return;
        };
        let n = usize::try_from(zlen).unwrap_or(usize::MAX);
        let mut copy = x.data.to_vec();
        if let Some(s) = copy.get_mut(start..start.saturating_add(n)) {
            s.fill(0);
        }
        x.data = Rc::from(copy.into_boxed_slice());
    }

    /// `blkid_probe_get_sector`: 512 bytes of sector `sector`.
    pub fn get_sector(&mut self, sector: u32) -> Option<Buf> {
        self.get_buffer(u64::from(sector) << 9, 0x200)
    }

    /// `blkid_probe_reset_buffers`: forget every buffer read.
    pub fn reset_buffers(&mut self) {
        self.flags &= !FL_MODIF_BUFF;
        self.buffers.clear();
    }

    /// `blkid_probe_hide_range(pr, off, len)`: zero bytes already read, so
    /// the next probe does not see them -- how `wipefs --no-act` walks past
    /// a signature it would have erased.
    pub fn hide_range(&mut self, off: u64, len: u64) -> i32 {
        let rc = self.hide_buffer(off, len);
        if rc == 0 {
            self.flags |= FL_MODIF_BUFF;
        }
        rc
    }

    /// `blkid_probe_is_tiny`.
    #[must_use]
    pub fn is_tiny(&self) -> bool {
        self.flags & FL_TINY_DEV != 0
    }

    /// `blkid_probe_is_cdrom`.
    #[must_use]
    pub fn is_cdrom(&self) -> bool {
        self.flags & FL_CDROM_DEV != 0
    }

    /// `blkdid_probe_is_opal_locked`.
    #[must_use]
    pub fn is_opal_locked(&self) -> bool {
        self.flags & FL_OPAL_LOCKED != 0
    }

    /// `S_ISCHR(pr->mode)`.
    pub(crate) fn is_chr(&self) -> bool {
        self.mode & S_IFMT == S_IFCHR
    }

    /// `S_ISBLK(pr->mode)`.
    pub(crate) fn is_blk(&self) -> bool {
        self.mode & S_IFMT == S_IFBLK
    }

    /// `S_ISREG(pr->mode)`.
    pub(crate) fn is_reg(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    /// `cdrom_size_correction(pr, last_written)`: shrink the window to the
    /// sectors that actually read -- a drive reports two or three unreadable
    /// blocks at the end as part of the disc.
    fn cdrom_size_correction(&mut self, last_written: u64) {
        let Some(file) = self.file.clone() else {
            return;
        };
        let mut nsectors = self.size >> 9;
        if last_written != 0 && nsectors > (last_written.saturating_add(1) << 2) {
            nsectors = last_written.saturating_add(1) << 2;
        }
        let mut n = nsectors.wrapping_sub(12);
        while n < nsectors {
            let mut f: &File = &file;
            let mut buf = [0u8; 512];
            let readable = f.seek(SeekFrom::Start(n.wrapping_mul(512))).is_ok()
                && matches!(f.read(&mut buf), Ok(512));
            if !readable {
                self.errno = 0;
                self.size = n << 9;
                return;
            }
            n = n.saturating_add(1);
        }
    }

    /// `blkid_probe_set_device(pr, fd, off, size)`: probe `file` from `off`
    /// for `size` bytes (0: to the end). Everything from before is reset;
    /// hints are kept. 0 on success, 1 for `None` (a reset only), -1 with
    /// [`Probe::errno`] set.
    pub fn set_device(&mut self, file: Option<Rc<File>>, off: i64, size: i64) -> i32 {
        self.reset_probe();
        self.reset_buffers();
        self.disk_probe = None;
        self.flags &= !(FL_PRIVATE_FD | FL_TINY_DEV | FL_CDROM_DEV);
        self.prob_flags = 0;
        self.file = file;
        // The C casts: an off_t offset and size taken as unsigned.
        #[allow(clippy::cast_sign_loss, reason = "C's (uint64_t) casts of off_t")]
        let (uoff, usize_) = (off as u64, size as u64);
        self.off = uoff;
        self.size = 0;
        self.devno = 0;
        self.disk_devno = 0;
        self.mode = 0;
        self.blkssz = 0;
        self.wipe_off = 0;
        self.wipe_size = 0;
        self.wipe_chain = None;
        self.zone_size = 0;
        let Some(file) = self.file.clone() else {
            return 1;
        };
        let sb = match file.metadata() {
            Ok(m) => m,
            Err(e) => {
                self.errno = errno_of(&e);
                return -1;
            }
        };
        let is_blk = blkdev::is_block(&sb);
        let is_chr = blkdev::is_char(&sb);
        if !is_blk && !is_chr && !sb.is_file() {
            self.errno = EINVAL;
            return -1;
        }
        self.mode = blkdev::mode(&sb);
        if is_blk || is_chr {
            self.devno = blkdev::rdev(&sb);
        }
        let devsiz = if is_blk {
            match blkdev::get_size(&file) {
                Ok(s) => s,
                Err(e) => {
                    self.errno = e;
                    return -1;
                }
            }
        } else if is_chr {
            let ubi = ulsysfs::chrdev_devno_to_devname(self.devno, ulsysfs::PATH_MAX)
                .is_some_and(|n| n.starts_with(b"ubi"));
            if !ubi {
                self.errno = EINVAL;
                return -1;
            }
            // UBI volumes are character devices, and probed as one byte.
            1
        } else {
            sb.len()
        };
        self.size = if size != 0 { usize_ } else { devsiz };
        if off != 0 && size == 0 {
            // An offset alone: to the end of the device.
            self.size = self.size.wrapping_sub(uoff);
        }
        if self.off.wrapping_add(self.size) > devsiz {
            self.errno = EINVAL;
            return -1;
        }
        if self.size <= 1440 * 1024 && !is_chr {
            self.flags |= FL_TINY_DEV;
        }
        let mut is_floppy = false;
        if is_blk {
            let mut fdc = [0u8; 40];
            // SAFETY: FDGETFDCSTAT writes a 40-byte struct floppy_fdc_state.
            if unsafe { blkdev::ioctl_ptr(&file, blkdev::FDGETFDCSTAT, &mut fdc) }.is_ok() {
                // A floppy works badly opened O_NONBLOCK: reopen without it.
                match reopen_blocking(&file) {
                    Ok(Some(f)) => {
                        self.file = Some(Rc::new(f));
                        self.flags |= FL_PRIVATE_FD;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.errno = e;
                        return -1;
                    }
                }
                is_floppy = true;
            }
            self.errno = 0;
        }
        let file = self.file.clone().unwrap_or(file);
        let mut dm_uuid = None;
        let mut dm_private = false;
        if is_blk && !is_floppy {
            let (private, uuid) = ulsysfs::devno_is_dm_private(self.devno);
            dm_uuid = uuid;
            dm_private = private;
        }
        // Upstream never clears BLKID_FL_NOSCAN_DEV once set: a probe reused
        // after a private device-mapper device stays unscannable. Kept.
        if dm_private {
            self.flags |= FL_NOSCAN_DEV;
        } else if is_blk
            && !self.is_tiny()
            && dm_uuid.is_none()
            && !is_floppy
            && self.is_wholedisk()
        {
            let mut last_written: i64 = 0;
            if blkdev::ioctl_val(&file, blkdev::CDROM_GET_CAPABILITY, 0).is_ok() {
                if let Ok(blkdev::CDS_TRAY_OPEN | blkdev::CDS_NO_DISC) =
                    blkdev::ioctl_val(&file, blkdev::CDROM_DRIVE_STATUS, blkdev::CDSL_CURRENT)
                {
                    self.errno = blkdev::ENOMEDIUM;
                    return -1;
                }
                self.flags |= FL_CDROM_DEV;
            }
            // SAFETY: CDROM_LAST_WRITTEN writes one long.
            match unsafe { blkdev::ioctl_ptr(&file, blkdev::CDROM_LAST_WRITTEN, &mut last_written) }
            {
                Ok(0) => self.flags |= FL_CDROM_DEV,
                Ok(_) => {}
                Err(e) => {
                    if e == blkdev::ENOMEDIUM {
                        self.errno = e;
                        return -1;
                    }
                }
            }
            if self.flags & FL_CDROM_DEV != 0 {
                #[allow(clippy::cast_sign_loss, reason = "C passes the long as uint64_t")]
                self.cdrom_size_correction(last_written as u64);
                if self.off == 0 && self.get_hint(b"session_offset").is_none() {
                    // struct cdrom_multisession: a 4-byte address, xa_flag,
                    // addr_format.
                    let mut ms = [0u8; 8];
                    if let Some(b) = ms.get_mut(5) {
                        *b = blkdev::CDROM_LBA;
                    }
                    // SAFETY: CDROMMULTISESSION reads and writes the 8-byte
                    // struct cdrom_multisession.
                    let rc =
                        unsafe { blkdev::ioctl_ptr(&file, blkdev::CDROMMULTISESSION, &mut ms) };
                    if rc == Ok(0) && ms.get(4).is_some_and(|&x| x != 0) {
                        let lba = i32::from_ne_bytes([ms[0], ms[1], ms[2], ms[3]]);
                        #[allow(clippy::cast_sign_loss, reason = "C's (uint64_t) of lba << 11")]
                        let v = (i64::from(lba) << 11) as u64;
                        self.set_hint(b"session_offset", v);
                    }
                }
            }
        }
        if is_blk && !is_floppy {
            let mut zone_sectors: u32 = 0;
            // SAFETY: BLKGETZONESZ writes one u32.
            if unsafe { blkdev::ioctl_ptr(&file, blkdev::BLKGETZONESZ, &mut zone_sectors) }.is_ok()
            {
                self.zone_size = u64::from(zone_sectors) << 9;
            }
        }
        0
    }

    /// `blkid_probe_get_dimension`.
    #[must_use]
    pub fn dimension(&self) -> (u64, u64) {
        (self.off, self.size)
    }

    /// `blkid_probe_set_dimension(pr, off, size)`: a new window; the buffers
    /// are dropped.
    pub fn set_dimension(&mut self, off: u64, size: u64) {
        self.off = off;
        self.size = size;
        self.flags &= !FL_TINY_DEV;
        if self.size <= 1440 * 1024 && !self.is_chr() {
            self.flags |= FL_TINY_DEV;
        }
        self.reset_buffers();
    }

    /// `blkid_probe_get_sb_buffer(pr, mag, size)`: `size` bytes at the
    /// magic's superblock, hint included.
    pub fn get_sb_buffer(&mut self, mag: Option<&IdMag>, size: u64) -> Option<Buf> {
        let (kboff, hoff) = mag.map_or((0, None), |m| (m.kboff, m.hoff));
        let hint_offset = hoff.and_then(|h| self.get_hint(h.as_bytes())).unwrap_or(0);
        // `mag->kboff << 10`: a long, shifted, added as unsigned.
        #[allow(clippy::cast_sign_loss, reason = "C adds the long to a uint64_t")]
        let base = (kboff << 10) as u64;
        self.get_buffer(hint_offset.wrapping_add(base), size)
    }

    /// `blkid_probe_get_idmag(pr, id, &offset, &res)`: the first of `id`'s
    /// magics found in place. `PROBE_OK` with the magic and the offset it
    /// sits at; `PROBE_OK` with none for a prober without magics;
    /// `PROBE_NONE` when none matched; `-errno` for a read error.
    pub fn get_idmag(&mut self, id: &'static IdInfo) -> (i32, u64, Option<&'static IdMag>) {
        for mag in id.magics {
            let hint_offset = mag
                .hoff
                .and_then(|h| self.get_hint(h.as_bytes()))
                .unwrap_or(0);
            if mag.is_zoned && self.zone_size == 0 {
                continue;
            }
            #[allow(clippy::cast_sign_loss, reason = "C's longs added to a uint64_t")]
            let kboff: u64 = if mag.is_zoned {
                ((mag.zonenum as u64).wrapping_mul(self.zone_size) >> 10)
                    .wrapping_add(mag.kboff_inzone as u64)
            } else {
                mag.kboff as u64
            };
            let off =
                hint_offset.wrapping_add(kboff.wrapping_add(u64::from(mag.sboff >> 10)) << 10);
            let buf = self.get_buffer(off, 1024);
            if buf.is_none() && self.errno != 0 {
                return (self.errno.wrapping_neg(), 0, None);
            }
            if let Some(buf) = buf {
                let at = usize::try_from(mag.sboff & 0x3ff).unwrap_or(0);
                if buf.get(at..at.saturating_add(mag.magic.len())) == Some(mag.magic) {
                    return (
                        PROBE_OK,
                        off.wrapping_add(u64::from(mag.sboff & 0x3ff)),
                        Some(mag),
                    );
                }
            }
        }
        if !id.magics.is_empty() {
            return (PROBE_NONE, 0, None);
        }
        (PROBE_OK, 0, None)
    }

    /// `blkid_probe_start`.
    fn start(&mut self) {
        self.cur_chain = None;
        self.prob_flags = 0;
        self.set_wiper(0, 0);
    }

    /// `blkid_probe_end`.
    fn end(&mut self) {
        self.cur_chain = None;
        self.prob_flags = 0;
        self.set_wiper(0, 0);
    }

    /// `blkid_do_probe`: the next result, from whichever chain has one --
    /// call it in a loop for every signature on the device. `PROBE_OK`,
    /// `PROBE_NONE` when every chain is done, `PROBE_ERROR`.
    pub fn do_probe(&mut self) -> i32 {
        if self.flags & FL_NOSCAN_DEV != 0 {
            return PROBE_NONE;
        }
        let mut rc = 1;
        loop {
            let chn = match self.cur_chain {
                None => {
                    self.start();
                    self.cur_chain = Some(ChainId::Sublks);
                    ChainId::Sublks
                }
                Some(c) => {
                    let st = self.chain(c);
                    let n = i32::try_from(driver(c).idinfos.len()).unwrap_or(i32::MAX);
                    if rc == 1 && (!st.enabled || st.idx.saturating_add(1) == n || st.idx == -1) {
                        let next = c.index().saturating_add(1);
                        match ChainId::ALL.get(next) {
                            Some(&nc) => {
                                self.cur_chain = Some(nc);
                                nc
                            }
                            None => {
                                self.end();
                                return PROBE_NONE;
                            }
                        }
                    } else {
                        c
                    }
                }
            };
            self.chain_mut(chn).binary = false;
            if !self.chain(chn).enabled {
                continue;
            }
            rc = (driver(chn).probe)(self, chn);
            if rc != PROBE_NONE {
                break;
            }
        }
        if rc < 0 {
            return PROBE_ERROR;
        }
        rc
    }

    /// `blkid_do_wipe(pr, dryrun)`: erase the signature just found -- its
    /// `SBMAGIC`/`PTMAGIC` bytes -- and step back so the next
    /// [`Probe::do_probe`] looks again. `dryrun` erases it in the buffers
    /// only. The device must be open for writing.
    pub fn do_wipe(&mut self, dryrun: bool) -> i32 {
        let Some(chn) = self.cur_chain else {
            return PROBE_ERROR;
        };
        let (off_name, mag_name) = match chn {
            ChainId::Sublks => ("SBMAGIC_OFFSET", "SBMAGIC"),
            ChainId::Parts => ("PTMAGIC_OFFSET", "PTMAGIC"),
            ChainId::Toplgy => return PROBE_OK,
        };
        let Some(off) = self.lookup_value(off_name).map(|v| v.as_c_str().to_vec()) else {
            return PROBE_OK;
        };
        let Some(mut len) = self.lookup_value(mag_name).map(Value::len) else {
            return PROBE_OK;
        };
        if len == 0 {
            return PROBE_OK;
        }
        let Some(sc) = ulstrutils::scan_integer(&off, 10) else {
            return PROBE_OK;
        };
        if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
            return PROBE_OK;
        }
        let m = u64::try_from(sc.magnitude).unwrap_or(0);
        let magoff = if sc.negative { m.wrapping_neg() } else { m };
        let offset = magoff.wrapping_add(self.off);
        let Some(file) = self.file.clone() else {
            return PROBE_ERROR;
        };
        // BUFSIZ.
        len = len.min(8192);
        let conventional = match self.is_conventional(offset) {
            Some(c) => c,
            None => return PROBE_ERROR,
        };
        let mut f: &File = &file;
        if f.seek(SeekFrom::Start(offset)).is_err() {
            return PROBE_ERROR;
        }
        if !dryrun {
            if conventional {
                use std::io::Write;
                let zeros = vec![0u8; len];
                if f.write_all(&zeros).is_err() || file.sync_all().is_err() {
                    return PROBE_ERROR;
                }
            } else if self.reset_zone(offset).is_err() {
                return PROBE_ERROR;
            }
            self.flags &= !FL_MODIF_BUFF;
            return self.step_back();
        }
        self.hide_range(magoff, u64::try_from(len).unwrap_or(0));
        self.step_back()
    }

    /// `is_conventional(pr, offset)`: whether the zone holding `offset` may
    /// be written in place. Every device without zones is. `None` when the
    /// zone report fails.
    fn is_conventional(&self, offset: u64) -> Option<bool> {
        if self.zone_size == 0 {
            return Some(true);
        }
        let file = self.file.as_ref()?;
        // struct blk_zone_report with one struct blk_zone.
        let mut rep = [0u8; 16 + 64];
        let zone_mask = !(self.zone_size.wrapping_sub(1));
        let sector = (offset & zone_mask) >> 9;
        rep.get_mut(..8)?.copy_from_slice(&sector.to_ne_bytes());
        rep.get_mut(8..12)?.copy_from_slice(&1u32.to_ne_bytes());
        const BLKREPORTZONE: u64 = 0xc010_1282;
        // SAFETY: BLKREPORTZONE reads the 16-byte header and writes at most
        // `nr_zones` (1) 64-byte zones after it; the buffer holds both.
        unsafe { blkdev::ioctl_ptr(file, BLKREPORTZONE, &mut rep) }.ok()?;
        // zones[0].type, BLK_ZONE_TYPE_CONVENTIONAL.
        Some(rep.get(16 + 24) == Some(&1))
    }

    /// `ioctl(BLKRESETZONE)` of the zone holding `offset`.
    fn reset_zone(&self, offset: u64) -> Result<(), i32> {
        let file = self.file.as_ref().ok_or(EINVAL)?;
        let zone_mask = !(self.zone_size.wrapping_sub(1));
        let mut range = [(offset & zone_mask) >> 9, self.zone_size >> 9];
        const BLKRESETZONE: u64 = 0x4010_1283;
        // SAFETY: BLKRESETZONE reads a struct blk_zone_range, two u64s.
        unsafe { blkdev::ioctl_ptr(file, BLKRESETZONE, &mut range) }.map(drop)
    }

    /// `blkid_probe_step_back`: make the next [`Probe::do_probe`] call the
    /// prober that just answered again -- after its signature was erased.
    pub fn step_back(&mut self) -> i32 {
        let Some(chn) = self.cur_chain else {
            return -1;
        };
        if self.flags & FL_MODIF_BUFF == 0 {
            self.reset_buffers();
        }
        if self.chain(chn).idx >= 0 {
            let c = self.chain_mut(chn);
            c.idx = c.idx.saturating_sub(1);
        }
        if self.chain(chn).idx == -1 {
            let idx = chn.index().saturating_sub(1);
            if idx > 0 {
                self.cur_chain = ChainId::ALL.get(idx).copied();
            } else {
                self.cur_chain = None;
            }
        }
        0
    }

    /// `blkid_do_safeprobe`: each enabled chain once, refusing an ambiguous
    /// superblock result. `PROBE_OK`, `PROBE_NONE`, `PROBE_ERROR`.
    pub fn do_safeprobe(&mut self) -> i32 {
        self.do_all(true)
    }

    /// `blkid_do_fullprobe`: each enabled chain once, with no ambiguity
    /// check.
    pub fn do_fullprobe(&mut self) -> i32 {
        self.do_all(false)
    }

    /// The loop both of the above are.
    fn do_all(&mut self, safe: bool) -> i32 {
        if self.flags & FL_NOSCAN_DEV != 0 {
            return PROBE_NONE;
        }
        self.start();
        let mut count = 0u32;
        let mut rc = 0;
        for chn in ChainId::ALL {
            self.cur_chain = Some(chn);
            self.chain_mut(chn).binary = false;
            if !self.chain(chn).enabled {
                continue;
            }
            self.chain_reset_position(chn);
            let d = driver(chn);
            rc = if safe {
                (d.safeprobe)(self, chn)
            } else {
                (d.probe)(self, chn)
            };
            self.chain_reset_position(chn);
            if rc < 0 {
                break;
            }
            if rc == 0 {
                count = count.saturating_add(1);
            }
        }
        self.end();
        if rc < 0 {
            return PROBE_ERROR;
        }
        if count == 0 { PROBE_NONE } else { PROBE_OK }
    }

    /// `blkid_probe_assign_value` with the data: a new result for the
    /// current chain.
    pub(crate) fn push_value(&mut self, name: &'static str, data: Vec<u8>) {
        let chain = self.cur_chain.unwrap_or(ChainId::Sublks);
        self.values.push(Value { name, data, chain });
    }

    /// `blkid_probe_set_value(pr, name, data, len)`.
    pub fn set_value(&mut self, name: &'static str, data: &[u8]) -> i32 {
        self.push_value(name, data.to_vec());
        0
    }

    /// `blkid_probe_sprintf_value(pr, name, ...)`: a string value, its NUL
    /// counted. An empty string is refused (`-EINVAL`) and not stored, as
    /// `vasprintf` returning 0 is refused upstream.
    pub fn set_value_str(&mut self, name: &'static str, s: &[u8]) -> i32 {
        if s.is_empty() {
            return -EINVAL;
        }
        let mut data = s.to_vec();
        data.push(0);
        self.push_value(name, data);
        0
    }

    /// `blkid_probe_set_magic(pr, offset, len, magic)`: SBMAGIC or PTMAGIC,
    /// and its offset, if the chain was asked for them.
    pub fn set_magic(&mut self, offset: u64, magic: &[u8]) -> i32 {
        let Some(chn) = self.cur_chain else {
            return 0;
        };
        if magic.is_empty() || self.chain(chn).binary {
            return 0;
        }
        let flags = self.chain(chn).flags;
        let (want, mname, oname) = match chn {
            ChainId::Sublks => (flags & SUBLKS_MAGIC != 0, "SBMAGIC", "SBMAGIC_OFFSET"),
            ChainId::Parts => (flags & PARTS_MAGIC != 0, "PTMAGIC", "PTMAGIC_OFFSET"),
            ChainId::Toplgy => return 0,
        };
        if !want {
            return 0;
        }
        let rc = self.set_value(mname, magic);
        if rc != 0 {
            return rc;
        }
        self.set_value_str(oname, offset.to_string().as_bytes())
    }

    /// `blkid_probe_verify_csum_buf(pr, n, csum, expected)`: whether they
    /// match -- or, if the superblocks chain was asked to accept bad ones,
    /// true anyway, with SBBADCSUM said.
    pub fn verify_csum_buf(&mut self, csum: &[u8], expected: &[u8]) -> bool {
        if csum == expected {
            return true;
        }
        if self.cur_chain == Some(ChainId::Sublks) && self.chain_flags() & SUBLKS_BADCSUM != 0 {
            self.set_value("SBBADCSUM", b"1\0");
            return true;
        }
        false
    }

    /// `blkid_probe_verify_csum(pr, csum, expected)`.
    pub fn verify_csum(&mut self, csum: u64, expected: u64) -> bool {
        self.verify_csum_buf(&csum.to_ne_bytes(), &expected.to_ne_bytes())
    }

    /// `blkid_probe_get_devno`.
    #[must_use]
    pub fn devno(&self) -> u64 {
        self.devno
    }

    /// `blkid_probe_get_wholedisk_devno`: the whole disk's number, 0 for a
    /// regular file or when it cannot be found.
    pub fn wholedisk_devno(&mut self) -> u64 {
        if self.disk_devno == 0 {
            if self.devno == 0 {
                return 0;
            }
            if let Some((_, disk)) = crate::devno::devno_to_wholedisk(self.devno, 0) {
                self.disk_devno = disk;
            }
        }
        self.disk_devno
    }

    /// `blkid_probe_is_wholedisk`.
    pub fn is_wholedisk(&mut self) -> bool {
        if self.devno == 0 {
            return false;
        }
        let disk = self.wholedisk_devno();
        disk != 0 && self.devno == disk
    }

    /// `blkid_probe_get_wholedisk_probe`: a probe of the whole disk this
    /// partition is on, kept for reuse; `None` for a whole disk (or a
    /// regular file), or when the disk cannot be opened.
    pub(crate) fn wholedisk_probe(&mut self) -> Option<&mut Probe> {
        if self.is_wholedisk() {
            return None;
        }
        if self.parent.is_some() {
            return self.parent.as_mut()?.wholedisk_probe();
        }
        let disk = self.wholedisk_devno();
        if self.disk_probe.as_ref().is_some_and(|d| d.devno != disk) {
            self.disk_probe = None;
        }
        if self.disk_probe.is_none() {
            let path = crate::devno::devno_to_devname(disk)?;
            let mut dp = Probe::from_filename(&path).ok()?;
            if self.chain(ChainId::Parts).flags & PARTS_FORCE_GPT != 0 {
                dp.chain_mut(ChainId::Parts).flags = PARTS_FORCE_GPT;
            }
            self.disk_probe = Some(Box::new(dp));
        }
        self.disk_probe.as_deref_mut()
    }

    /// `blkid_probe_get_size`.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// `blkid_probe_get_offset`.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.off
    }

    /// `blkid_probe_get_sectorsize`: `BLKSSZGET` for a block device, else
    /// 512; remembered.
    pub fn sectorsize(&mut self) -> u32 {
        if self.blkssz != 0 {
            return self.blkssz;
        }
        if self.is_blk()
            && let Some(f) = self.file.as_ref()
            && let Ok(sz) = blkdev::get_sector_size(f)
        {
            self.blkssz = sz;
            return sz;
        }
        self.blkssz = crate::DEFAULT_SECTOR_SIZE;
        self.blkssz
    }

    /// `blkid_probe_set_sectorsize`.
    pub fn set_sectorsize(&mut self, sz: u32) {
        self.blkssz = sz;
    }

    /// `blkid_probe_get_sectors`.
    #[must_use]
    pub fn sectors(&self) -> u64 {
        self.size >> 9
    }

    /// `blkid_probe_numof_values`.
    #[must_use]
    pub fn numof_values(&self) -> usize {
        self.values.len()
    }

    /// Every result, in the order set.
    #[must_use]
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// `blkid_probe_get_value(pr, num, ...)`.
    #[must_use]
    pub fn get_value(&self, num: usize) -> Option<&Value> {
        self.values.get(num)
    }

    /// `blkid_probe_lookup_value(pr, name, ...)`: the first result so named.
    #[must_use]
    pub fn lookup_value(&self, name: &str) -> Option<&Value> {
        self.values.iter().find(|v| v.name == name)
    }

    /// `blkid_probe_has_value`.
    #[must_use]
    pub fn has_value(&self, name: &str) -> bool {
        self.lookup_value(name).is_some()
    }

    /// `blkid_probe_set_wiper(pr, off, size)`: remember that the prober now
    /// running found a signature whose tool wipes `size` bytes at `off` --
    /// a later signature found inside that area was written after it, and
    /// wins. A size of 0 forgets.
    pub fn set_wiper(&mut self, off: u64, size: u64) {
        if size == 0 {
            self.wipe_size = 0;
            self.wipe_off = 0;
            self.wipe_chain = None;
            return;
        }
        let Some(chn) = self.cur_chain else {
            return;
        };
        let idx = self.chain(chn).idx;
        let n = driver(chn).idinfos.len();
        if idx < 0 || usize::try_from(idx).is_ok_and(|i| i >= n) {
            return;
        }
        self.wipe_size = size;
        self.wipe_off = off;
        self.wipe_chain = Some(chn);
    }

    /// `pr->wipe_size`: nonzero while a wiped area is remembered.
    pub(crate) fn wipe_size(&self) -> u64 {
        self.wipe_size
    }

    /// `blkid_probe_is_wiped`: the chain whose wiped area covers
    /// `off..off+size`, if one does and a chain is remembered.
    fn is_wiped(&self, off: u64, size: u64) -> Option<ChainId> {
        if size == 0 {
            return None;
        }
        if self.wipe_off <= off
            && off.wrapping_add(size) <= self.wipe_off.wrapping_add(self.wipe_size)
        {
            self.wipe_chain
        } else {
            None
        }
    }

    /// `blkid_probe_use_wiper(pr, off, size)`: a signature at `off..off+size`
    /// inside an area a previous result's tool wiped supersedes that result,
    /// which is dropped.
    pub fn use_wiper(&mut self, off: u64, size: u64) {
        if let Some(chn) = self.is_wiped(off, size) {
            self.set_wiper(0, 0);
            self.chain_reset_values(chn);
        }
    }

    /// `blkid_probe_set_hint(pr, name, value)`: a hint probers read -- a
    /// session's offset on a multi-session disc. `name` may be `NAME=value`,
    /// in which case `value` is ignored. `Err(errno)` for one that does not
    /// parse.
    ///
    /// # Errors
    ///
    /// `EINVAL` for a `NAME=value` whose value is not a whole number, or
    /// `ERANGE` for one too large.
    pub fn set_hint(&mut self, name: &[u8], value: u64) -> i32 {
        let (n, value) = if name.contains(&b'=') {
            let Some((n, v)) = parse_tag_string(name) else {
                return -EINVAL;
            };
            let Some(sc) = ulstrutils::scan_integer(&v, 10) else {
                return -EINVAL;
            };
            if sc.end != v.len() {
                return -EINVAL;
            }
            if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
                return -ERANGE;
            }
            let m = u64::try_from(sc.magnitude).unwrap_or(0);
            (n, if sc.negative { m.wrapping_neg() } else { m })
        } else {
            (crate::c_str(name).to_vec(), value)
        };
        if let Some(h) = self.hints.iter_mut().find(|h| h.name == n) {
            h.value = value;
        } else {
            self.hints.push(Hint { name: n, value });
        }
        0
    }

    /// `blkid_probe_get_hint`.
    #[must_use]
    pub fn get_hint(&self, name: &[u8]) -> Option<u64> {
        self.hints.iter().find(|h| h.name == name).map(|h| h.value)
    }

    /// `blkid_probe_reset_hints`.
    pub fn reset_hints(&mut self) {
        self.hints.clear();
    }

    /// The chain's partition list, for the partitions chain's code.
    pub(crate) fn partlist_mut(&mut self) -> Option<&mut Partlist> {
        match &mut self.chain_mut(ChainId::Parts).data {
            ChainData::Parts(ls) => Some(ls),
            _ => None,
        }
    }

    /// The chain's partition list.
    pub(crate) fn partlist(&self) -> Option<&Partlist> {
        match &self.chain(ChainId::Parts).data {
            ChainData::Parts(ls) => Some(ls),
            _ => None,
        }
    }
}

/// `open(filename, O_RDONLY|O_CLOEXEC|O_NONBLOCK)`.
///
/// # Errors
///
/// The open's `errno`.
pub fn open_nonblock(filename: &[u8]) -> Result<File, i32> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NONBLOCK: Linux's value, which SlateOS's C library shares.
        opts.custom_flags(0o4000);
    }
    opts.open(std::path::PathBuf::from(quoting::os_from_bytes(filename)))
        .map_err(|e| errno_of(&e))
}

/// `ul_reopen(fd, flags & ~O_NONBLOCK)` for a descriptor opened
/// non-blocking: the same file through `/proc/self/fd`, blocking. `None`
/// when it was not non-blocking to begin with.
#[cfg_attr(
    not(unix),
    allow(
        clippy::unnecessary_wraps,
        reason = "the unix half can fail; this half cannot"
    )
)]
fn reopen_blocking(file: &File) -> Result<Option<File>, i32> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        }
        const F_GETFL: i32 = 3;
        const O_NONBLOCK: i32 = 0o4000;
        const O_ACCMODE: i32 = 3;
        let fd = file.as_raw_fd();
        // SAFETY: F_GETFL takes no argument and only reads the descriptor's
        // flags.
        let flags = unsafe { fcntl(fd, F_GETFL) };
        if flags < 0 {
            return Err(errno_of(&std::io::Error::last_os_error()));
        }
        if flags & O_NONBLOCK == 0 {
            return Ok(None);
        }
        let mut opts = std::fs::OpenOptions::new();
        match flags & O_ACCMODE {
            1 => opts.write(true),
            2 => opts.read(true).write(true),
            _ => opts.read(true),
        };
        opts.open(format!("/proc/self/fd/{fd}"))
            .map(Some)
            .map_err(|e| errno_of(&e))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(None)
    }
}
