//! `superblocks.c`: the chain that recognises filesystems, RAID members,
//! encrypted volumes and the rest by their superblocks, and the setters its
//! probers report through.
//!
//! [`IDINFOS`] is upstream's table in upstream's order, which matters twice
//! over: [`Probe::do_probe`] reports matches in it, and
//! [`Probe::do_safeprobe`] keeps the first of several tolerant matches.

use crate::encode::{encode_to_utf8, ltrim_whitespace, rtrim_whitespace};
use crate::probe::{ChainDrv, ChainId, IdInfo};
use crate::{
    EINVAL, Endianness, FLTR_NOTIN, FLTR_ONLYIN, IDINFO_TOLERANT, PROBE_AMBIGUOUS, PROBE_NONE,
    PROBE_OK, Probe, SUBLKS_DEFAULT, SUBLKS_FSINFO, SUBLKS_LABEL, SUBLKS_LABELRAW, SUBLKS_TYPE,
    SUBLKS_USAGE, SUBLKS_UUID, SUBLKS_UUIDRAW, SUBLKS_VERSION, USAGE_CRYPTO, USAGE_FILESYSTEM,
    USAGE_OTHER, USAGE_RAID, unparse_uuid, uuid_is_empty,
};

mod adaptec_raid;
mod apfs;
mod bcache;
mod befs;
mod bfs;
mod bitlocker;
mod bluestore;
mod btrfs;
mod cramfs;
mod cs_fvault2;
mod ddf_raid;
mod drbd;
mod drbdmanage;
mod drbdproxy_datalog;
mod erofs;
mod exfat;
mod exfs;
mod ext;
mod f2fs;
mod gfs;
mod hfs;
mod highpoint_raid;
mod hpfs;
mod iso9660;
mod isw_raid;
mod jfs;
mod jmicron_raid;
mod linux_raid;
mod lsi_raid;
mod luks;
mod lvm;
mod minix;
mod mpool;
mod netware;
mod nilfs;
mod ntfs;
mod nvidia_raid;
mod ocfs;
mod promise_raid;
mod refs;
mod reiserfs;
mod romfs;
mod silicon_raid;
mod squashfs;
mod stratis;
mod swap;
mod sysv;
mod ubi;
mod ubifs;
mod udf;
mod ufs;
mod vdo;
mod vfat;
mod via_raid;
mod vmfs;
mod vxfs;
mod xfs;
mod zfs;
mod zonefs;

// What the DOS partition-table prober asks before it believes an MBR: a FAT,
// exFAT or NTFS boot sector carries the same 55AA signature.
pub(crate) use exfat::is_exfat;
pub(crate) use ntfs::is_ntfs;
pub(crate) use vfat::is_vfat;

/// `idinfos[]`: every superblock prober, in the order they run.
pub static IDINFOS: &[&IdInfo] = &[
    // An OPAL-locked volume fails reads past the LUKS header, so LUKS is
    // tried first.
    &luks::LUKS,
    // RAIDs.
    &linux_raid::LINUXRAID,
    &ddf_raid::DDFRAID,
    &isw_raid::ISWRAID,
    &lsi_raid::LSIRAID,
    &via_raid::VIARAID,
    &silicon_raid::SILRAID,
    &nvidia_raid::NVRAID,
    &promise_raid::PDCRAID,
    &highpoint_raid::HIGHPOINT45X,
    &highpoint_raid::HIGHPOINT37X,
    &adaptec_raid::ADRAID,
    &jmicron_raid::JMRAID,
    &bcache::BCACHE,
    &bcache::BCACHEFS,
    &bluestore::BLUESTORE,
    &drbd::DRBD,
    &drbdmanage::DRBDMANAGE,
    &drbdproxy_datalog::DRBDPROXY_DATALOG,
    &lvm::LVM2,
    &lvm::LVM1,
    &lvm::SNAPCOW,
    &lvm::VERITY_HASH,
    &lvm::INTEGRITY,
    &vmfs::VMFS_VOLUME,
    &ubi::UBI,
    &vdo::VDO,
    &stratis::STRATIS,
    &bitlocker::BITLOCKER,
    &cs_fvault2::CS_FVAULT2,
    // Filesystems.
    &vfat::VFAT,
    &swap::SWSUSPEND,
    &swap::SWAP,
    &xfs::XFS,
    &xfs::XFS_LOG,
    &exfs::EXFS,
    &ext::EXT4DEV,
    &ext::EXT4,
    &ext::EXT3,
    &ext::EXT2,
    &ext::JBD,
    &reiserfs::REISER,
    &reiserfs::REISER4,
    &jfs::JFS,
    &udf::UDF,
    &iso9660::ISO9660,
    &zfs::ZFS,
    &hfs::HFSPLUS,
    &hfs::HFS,
    &ufs::UFS,
    &hpfs::HPFS,
    &sysv::SYSV,
    &sysv::XENIX,
    &ntfs::NTFS,
    &refs::REFS,
    &cramfs::CRAMFS,
    &romfs::ROMFS,
    &minix::MINIX,
    &gfs::GFS,
    &gfs::GFS2,
    &ocfs::OCFS,
    &ocfs::OCFS2,
    &ocfs::ORACLEASM,
    &vxfs::VXFS,
    &squashfs::SQUASHFS,
    &squashfs::SQUASHFS3,
    &netware::NETWARE,
    &btrfs::BTRFS,
    &ubifs::UBIFS,
    &bfs::BFS,
    &vmfs::VMFS_FS,
    &befs::BEFS,
    &nilfs::NILFS2,
    &exfat::EXFAT,
    &f2fs::F2FS,
    &mpool::MPOOL,
    &apfs::APFS,
    &zonefs::ZONEFS,
    &erofs::EROFS,
];

/// The table.
#[must_use]
pub fn idinfos() -> &'static [&'static IdInfo] {
    IDINFOS
}

/// `superblocks_drv`.
pub(crate) static DRIVER: ChainDrv = ChainDrv {
    dflt_flags: SUBLKS_DEFAULT,
    dflt_enabled: true,
    has_fltr: true,
    idinfos: IDINFOS,
    probe: superblocks_probe,
    safeprobe: superblocks_safeprobe,
};

/// `blkid_known_fstype`.
#[must_use]
pub fn known_fstype(fstype: &[u8]) -> bool {
    idinfos().iter().any(|id| id.name.as_bytes() == fstype)
}

/// `blkid_superblocks_get_name(idx, &name, &usage)`.
#[must_use]
pub fn get_name(idx: usize) -> Option<(&'static str, u32)> {
    idinfos().get(idx).map(|id| (id.name, id.usage))
}

/// `superblocks_probe`: the next matching prober from where the chain
/// stands.
fn superblocks_probe(pr: &mut Probe, chn: ChainId) -> i32 {
    let idx = pr.chain(chn).idx;
    if idx < -1 {
        return -EINVAL;
    }
    pr.chain_reset_values(chn);
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return PROBE_NONE;
    }
    if pr.size == 0 || (pr.size <= 1024 && !pr.is_chr()) {
        // Very small devices and files (an extended partition's stub) are
        // not probed; a UBI volume's size of 1 is exempt.
        return PROBE_NONE;
    }
    let table = idinfos();
    let mut i = usize::try_from(idx.saturating_add(1)).unwrap_or(0);
    let mut rc = PROBE_NONE;
    while i < table.len() {
        pr.chain_mut(chn).idx = i32::try_from(i).unwrap_or(i32::MAX);
        let Some(&id) = table.get(i) else {
            break;
        };
        i = i.saturating_add(1);
        if pr
            .chain(chn)
            .fltr
            .as_ref()
            .is_some_and(|f| f.get(i.saturating_sub(1)).copied().unwrap_or(false))
        {
            rc = PROBE_NONE;
            continue;
        }
        if id.minsz != 0 && id.minsz > pr.size {
            rc = PROBE_NONE;
            continue;
        }
        // No RAIDs, swap or journals on CD/DVDs; no RAIDs on floppies.
        if id.usage & (USAGE_RAID | USAGE_OTHER) != 0 && pr.is_cdrom() {
            rc = PROBE_NONE;
            continue;
        }
        if id.usage & USAGE_RAID != 0 && pr.is_tiny() {
            rc = PROBE_NONE;
            continue;
        }
        let (r, off, mag) = pr.get_idmag(id);
        rc = r;
        if rc < 0 {
            break;
        }
        if rc != PROBE_OK {
            continue;
        }
        if let Some(f) = id.probefunc {
            pr.errno = 0;
            rc = f(pr, mag);
            if rc != PROBE_OK {
                pr.chain_reset_values(chn);
                if rc < 0 {
                    break;
                }
                continue;
            }
        }
        if pr.chain(chn).flags & SUBLKS_TYPE != 0 {
            rc = pr.set_value_str("TYPE", id.name.as_bytes());
        }
        if rc == 0 {
            rc = set_usage(pr, id.usage);
        }
        if rc == 0
            && let Some(m) = mag
        {
            rc = pr.set_magic(off, m.magic);
        }
        if rc != 0 {
            pr.chain_reset_values(chn);
            continue;
        }
        return PROBE_OK;
    }
    rc
}

/// `superblocks_safeprobe`: one answer, and ambiguity refused -- unless the
/// device is tiny (the first answer is taken), or the answer is a RAID or
/// crypto signature (which a filesystem seen through it cannot contradict).
fn superblocks_safeprobe(pr: &mut Probe, chn: ChainId) -> i32 {
    if pr.flags & crate::probe::FL_NOSCAN_DEV != 0 {
        return PROBE_NONE;
    }
    let table = idinfos();
    let usage_at = |pr: &Probe| {
        usize::try_from(pr.chain(chn).idx)
            .ok()
            .and_then(|i| table.get(i))
            .map(|id| (id.usage, id.flags))
    };
    let mut vals = Vec::new();
    let mut idx = -1;
    let mut count = 0u32;
    let mut intol = 0u32;
    let mut rc;
    loop {
        rc = superblocks_probe(pr, chn);
        if rc != 0 {
            break;
        }
        if pr.is_tiny() && count == 0 {
            return PROBE_OK;
        }
        count = count.saturating_add(1);
        let info = usage_at(pr);
        if info.is_some_and(|(u, _)| u & (USAGE_RAID | USAGE_CRYPTO) != 0) {
            break;
        }
        if info.is_some_and(|(_, f)| f & IDINFO_TOLERANT == 0) {
            intol = intol.saturating_add(1);
        }
        if count == 1 {
            vals = pr.chain_save_values(chn);
            idx = pr.chain(chn).idx;
        }
    }
    if rc < 0 {
        return rc;
    }
    if count > 1 && intol > 0 {
        return PROBE_AMBIGUOUS;
    }
    if count == 0 {
        return PROBE_NONE;
    }
    if idx != -1 {
        pr.chain_reset_values(chn);
        pr.append_values(vals);
        pr.chain_mut(chn).idx = idx;
    }
    // A partition table seen through a RAID1 member belongs to the array.
    if usage_at(pr).is_some_and(|(u, _)| u & USAGE_RAID != 0) {
        pr.prob_flags |= crate::probe::PROBE_FL_IGNORE_PT;
    }
    PROBE_OK
}

/// `blkid_probe_set_usage`.
fn set_usage(pr: &mut Probe, usage: u32) -> i32 {
    if pr.chain_flags() & SUBLKS_USAGE == 0 {
        return 0;
    }
    let u: &[u8] = if usage & USAGE_FILESYSTEM != 0 {
        b"filesystem"
    } else if usage & USAGE_RAID != 0 {
        b"raid"
    } else if usage & USAGE_CRYPTO != 0 {
        b"crypto"
    } else if usage & USAGE_OTHER != 0 {
        b"other"
    } else {
        b"unknown"
    };
    pr.set_value_str("USAGE", u)
}

/// A value with its terminator, rtrimmed as `blkid_rtrim_whitespace` does,
/// or `None` if nothing is left.
fn rtrimmed(data: &[u8]) -> Option<Vec<u8>> {
    let t = rtrim_whitespace(data);
    if t.is_empty() {
        return None;
    }
    let mut v = t.to_vec();
    v.push(0);
    Some(v)
}

impl Probe {
    /// `blkid_probe_enable_superblocks`.
    pub fn enable_superblocks(&mut self, enable: bool) {
        self.chain_mut(ChainId::Sublks).enabled = enable;
    }

    /// `blkid_probe_set_superblocks_flags`.
    pub fn set_superblocks_flags(&mut self, flags: u32) {
        self.chain_mut(ChainId::Sublks).flags = flags;
    }

    /// `blkid_probe_reset_superblocks_filter`.
    pub fn reset_superblocks_filter(&mut self) -> i32 {
        self.reset_filter(ChainId::Sublks)
    }

    /// `blkid_probe_invert_superblocks_filter`.
    pub fn invert_superblocks_filter(&mut self) -> i32 {
        self.invert_filter(ChainId::Sublks)
    }

    /// `blkid_probe_filter_superblocks_type(pr, flag, names)`.
    pub fn filter_superblocks_type(&mut self, flag: u32, names: &[&[u8]]) -> i32 {
        self.filter_types(ChainId::Sublks, flag, names)
    }

    /// `blkid_probe_filter_superblocks_usage(pr, flag, usage)`.
    pub fn filter_superblocks_usage(&mut self, flag: u32, usage: u32) -> i32 {
        let Some(f) = self.get_filter(ChainId::Sublks, true) else {
            return -1;
        };
        for (i, id) in idinfos().iter().enumerate() {
            let set = if id.usage & usage != 0 {
                flag & FLTR_NOTIN != 0
            } else {
                flag & FLTR_ONLYIN != 0
            };
            if set && let Some(b) = f.get_mut(i) {
                *b = true;
            }
        }
        0
    }

    /// `blkid_probe_set_version`.
    pub fn set_version(&mut self, version: &[u8]) -> i32 {
        if self.chain_flags() & SUBLKS_VERSION != 0 {
            let mut v = crate::c_str(version).to_vec();
            v.push(0);
            self.push_value("VERSION", v);
        }
        0
    }

    /// `blkid_probe_sprintf_version`: VERSION from formatted text (the
    /// caller formats).
    pub fn sprintf_version(&mut self, s: impl AsRef<[u8]>) -> i32 {
        if self.chain_flags() & SUBLKS_VERSION != 0 {
            return self.set_value_str("VERSION", s.as_ref());
        }
        0
    }

    /// `blkid_probe_set_block_size`: BLOCK_SIZE, whatever the flags.
    pub fn set_block_size(&mut self, block_size: u32) -> i32 {
        self.set_value_str("BLOCK_SIZE", block_size.to_string().as_bytes())
    }

    /// `blkid_probe_set_fssize`.
    pub fn set_fssize(&mut self, size: u64) -> i32 {
        if self.chain_flags() & SUBLKS_FSINFO == 0 {
            return 0;
        }
        self.set_value_str("FSSIZE", size.to_string().as_bytes())
    }

    /// `blkid_probe_set_fslastblock`.
    pub fn set_fslastblock(&mut self, lastblock: u64) -> i32 {
        if self.chain_flags() & SUBLKS_FSINFO == 0 {
            return 0;
        }
        self.set_value_str("FSLASTBLOCK", lastblock.to_string().as_bytes())
    }

    /// `blkid_probe_set_fsblocksize`.
    pub fn set_fsblocksize(&mut self, block_size: u32) -> i32 {
        if self.chain_flags() & SUBLKS_FSINFO == 0 {
            return 0;
        }
        self.set_value_str("FSBLOCKSIZE", block_size.to_string().as_bytes())
    }

    /// `blkid_probe_set_fsendianness`.
    pub fn set_fsendianness(&mut self, e: Endianness) -> i32 {
        if self.chain_flags() & SUBLKS_FSINFO == 0 {
            return 0;
        }
        let v: &[u8] = match e {
            Endianness::Little => b"LITTLE",
            Endianness::Big => b"BIG",
        };
        self.set_value_str("ENDIANNESS", v)
    }

    /// `blkid_probe_set_id_label(pr, name, data, len)`: a label-like value
    /// under `name`, white space trimmed from both ends; nothing if that
    /// leaves nothing.
    pub fn set_id_label(&mut self, name: &'static str, data: &[u8]) -> i32 {
        if self.chain_flags() & SUBLKS_LABEL == 0 {
            return 0;
        }
        let r = rtrim_whitespace(data);
        if r.is_empty() {
            return 0;
        }
        let l = ltrim_whitespace(r);
        if l.is_empty() {
            return 0;
        }
        let mut v = l.to_vec();
        v.push(0);
        self.push_value(name, v);
        0
    }

    /// `blkid_probe_set_utf8_id_label(pr, name, data, len, enc)`.
    pub fn set_utf8_id_label(&mut self, name: &'static str, data: &[u8], enc: i32) -> i32 {
        if self.chain_flags() & SUBLKS_LABEL == 0 {
            return 0;
        }
        let u = encode_to_utf8(enc, data.len().saturating_mul(3).saturating_add(1), data);
        self.set_id_label_trimmed(name, &u)
    }

    /// Both trims of [`Probe::set_id_label`], on text already converted.
    fn set_id_label_trimmed(&mut self, name: &'static str, u: &[u8]) -> i32 {
        let r = rtrim_whitespace(u);
        if r.is_empty() {
            return 0;
        }
        let l = ltrim_whitespace(r);
        if l.is_empty() {
            return 0;
        }
        let mut v = l.to_vec();
        v.push(0);
        self.push_value(name, v);
        0
    }

    /// `blkid_probe_set_label(pr, label, len)`: LABEL_RAW if asked for;
    /// LABEL as a C string with trailing white space off, if anything is
    /// left.
    pub fn set_label(&mut self, label: &[u8]) -> i32 {
        let flags = self.chain_flags();
        if flags & SUBLKS_LABELRAW != 0 {
            self.set_value("LABEL_RAW", label);
        }
        if flags & SUBLKS_LABEL == 0 {
            return 0;
        }
        if let Some(v) = rtrimmed(label) {
            self.push_value("LABEL", v);
        }
        0
    }

    /// `blkid_probe_set_utf8label(pr, label, len, enc)`: LABEL_RAW as it is,
    /// LABEL converted to UTF-8 and trimmed at its end.
    pub fn set_utf8label(&mut self, label: &[u8], enc: i32) -> i32 {
        let flags = self.chain_flags();
        if flags & SUBLKS_LABELRAW != 0 {
            self.set_value("LABEL_RAW", label);
        }
        if flags & SUBLKS_LABEL == 0 {
            return 0;
        }
        let u = encode_to_utf8(enc, label.len().saturating_mul(3).saturating_add(1), label);
        if let Some(v) = rtrimmed(&u) {
            self.push_value("LABEL", v);
        }
        0
    }

    /// `blkid_probe_sprintf_uuid(pr, uuid, len, fmt, ...)`: UUID_RAW if
    /// asked for, UUID as `text` (formatted by the caller) -- neither for an
    /// all-zero UUID.
    pub fn sprintf_uuid(&mut self, uuid: &[u8], text: impl AsRef<[u8]>) -> i32 {
        if uuid_is_empty(uuid) {
            return 0;
        }
        let flags = self.chain_flags();
        if flags & SUBLKS_UUIDRAW != 0 {
            self.set_value("UUID_RAW", uuid);
        }
        if flags & SUBLKS_UUID == 0 {
            return 0;
        }
        self.set_value_str("UUID", text.as_ref())
    }

    /// `blkid_probe_strncpy_uuid(pr, str, len)`: a UUID stored as text,
    /// trailing white space off. `-EINVAL` for an empty one.
    pub fn strncpy_uuid(&mut self, s: &[u8]) -> i32 {
        if s.first().is_none_or(|&b| b == 0) {
            return -EINVAL;
        }
        let flags = self.chain_flags();
        if flags & SUBLKS_UUIDRAW != 0 {
            self.set_value("UUID_RAW", s);
        }
        if flags & SUBLKS_UUID == 0 {
            return 0;
        }
        if let Some(v) = rtrimmed(s) {
            self.push_value("UUID", v);
        }
        0
    }

    /// `blkid_probe_set_uuid_as(pr, uuid, name)`: a DCE UUID, unparsed, as
    /// `name` -- or as UUID (and UUID_RAW if asked for) when `name` is
    /// `None`. Nothing for an all-zero UUID.
    pub fn set_uuid_as(&mut self, uuid: &[u8], name: Option<&'static str>) -> i32 {
        let u = uuid.get(..16).unwrap_or(uuid);
        if uuid_is_empty(u) {
            return 0;
        }
        let name = match name {
            Some(n) => n,
            None => {
                let flags = self.chain_flags();
                if flags & SUBLKS_UUIDRAW != 0 {
                    self.set_value("UUID_RAW", u);
                }
                if flags & SUBLKS_UUID == 0 {
                    return 0;
                }
                "UUID"
            }
        };
        let mut v = unparse_uuid(u);
        v.push(0);
        self.push_value(name, v);
        0
    }

    /// `blkid_probe_set_uuid`.
    pub fn set_uuid(&mut self, uuid: &[u8]) -> i32 {
        self.set_uuid_as(uuid, None)
    }
}
