//! `linux_raid.c`: Linux MD RAID members -- metadata 0.90 at the end of the
//! device, 1.0 at the end, 1.1 at the start, 1.2 at 4 KiB.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_RAID};

/// `MD_RESERVED_BYTES`.
const MD_RESERVED_BYTES: u64 = 0x10000;
/// `MD_SB_MAGIC`.
const MD_SB_MAGIC: u32 = 0xa92b_4efc;
/// `sizeof(struct mdp0_super_block)`.
const MDP0_SIZE: u64 = 64;
/// `sizeof(struct mdp1_super_block)`, without `dev_roles`.
const MDP1_SIZE: u64 = 256;
/// `offsetof(struct mdp1_super_block, sb_csum)`.
const MDP1_SB_CSUM: usize = 216;

/// `probe_raid0(pr, off)`: a 0.90 superblock at `off`. 0, 1, or `-errno`.
fn probe_raid0(pr: &mut Probe, off: u64) -> i32 {
    if pr.size < MD_RESERVED_BYTES {
        return 1;
    }
    let Some(mdp0) = pr.get_buffer(off, MDP0_SIZE) else {
        return pr.none_or_err();
    };
    let mut uuid = [0u8; 16];
    // A word's bytes in `uuid`: on this little-endian host, `swab32` of a
    // little-endian superblock's word is its bytes reversed; a big-endian
    // superblock's word is copied as it is.
    let put = |uuid: &mut [u8; 16], k: usize, at: usize, swap: bool| {
        let mut w = [0u8; 4];
        w.copy_from_slice(mdp0.span(at, 4));
        if swap {
            w.reverse();
        }
        if let Some(d) = uuid.get_mut(k.saturating_mul(4)..k.saturating_mul(4).saturating_add(4)) {
            d.copy_from_slice(&w);
        }
    };
    let (ma, mi, pa, size);
    if mdp0.le32(0) == MD_SB_MAGIC {
        put(&mut uuid, 0, 20, true);
        if mdp0.le32(8) >= 90 {
            put(&mut uuid, 1, 52, true);
            put(&mut uuid, 2, 56, true);
            put(&mut uuid, 3, 60, true);
        }
        ma = mdp0.le32(4);
        mi = mdp0.le32(8);
        pa = mdp0.le32(12);
        size = u64::from(mdp0.le32(32));
    } else if mdp0.be32(0) == MD_SB_MAGIC {
        put(&mut uuid, 0, 20, false);
        if mdp0.be32(8) >= 90 {
            put(&mut uuid, 1, 52, false);
            put(&mut uuid, 2, 56, false);
            put(&mut uuid, 3, 60, false);
        }
        ma = mdp0.be32(4);
        mi = mdp0.be32(8);
        pa = mdp0.be32(12);
        size = u64::from(mdp0.be32(32));
    } else {
        return 1;
    }
    // KiB to bytes.
    let size = size << 10;
    if pr.size < size.wrapping_add(MD_RESERVED_BYTES) {
        return 1;
    }
    if off < size {
        return 1;
    }
    // The superblock at the end of the last partition sits where one at the
    // end of the disk would: on a whole disk, one inside a partition is the
    // partition's.
    if (pr.is_reg() || pr.is_wholedisk())
        && crate::partitions::is_covered_by_pt(
            pr,
            off.wrapping_sub(size),
            size.wrapping_add(MD_RESERVED_BYTES),
        )
    {
        return 1;
    }
    if pr.sprintf_version(format!("{ma}.{mi}.{pa}")) != 0 {
        return 1;
    }
    if pr.set_uuid(&uuid) != 0 {
        return 1;
    }
    if pr.set_magic(off, mdp0.span(0, 4)) != 0 {
        return 1;
    }
    0
}

/// `raid1_verify_csum(pr, off, mdp1)`: the 1.x checksum -- 32-bit words
/// summed with the checksum field zeroed, folded to 32 bits. A header and
/// role table too large to read counts as verified, as upstream counts it.
fn raid1_verify_csum(pr: &mut Probe, off: u64) -> bool {
    let Some(mdp1) = pr.get_buffer(off, MDP1_SIZE) else {
        return true;
    };
    let max_dev = u64::from(mdp1.le32(220));
    let csummed_size = MDP1_SIZE.wrapping_add(max_dev.wrapping_mul(2));
    if pr.get_buffer(off, csummed_size).is_none() {
        return true;
    }
    // Upstream zeroes the checksum field in the buffer it was handed, which
    // may be the one `mdp1` points into.
    pr.zero_in_cache(off, csummed_size, off.wrapping_add(MDP1_SB_CSUM as u64), 4);
    let Some(csummed) = pr.get_buffer(off, csummed_size) else {
        return true;
    };
    let mut csum: u64 = 0;
    let mut rest: &[u8] = &csummed;
    while rest.len() >= 4 {
        csum = csum.wrapping_add(u64::from(rest.le32(0)));
        rest = rest.get(4..).unwrap_or_default();
    }
    if rest.len() == 2 {
        csum = csum.wrapping_add(u64::from(rest.le16(0)));
    }
    csum = (csum >> 32).wrapping_add(csum & 0xffff_ffff);
    // `mdp1->sb_csum` as it now reads.
    let expected = pr
        .get_buffer(off, MDP1_SIZE)
        .map_or(0, |b| u64::from(b.le32(MDP1_SB_CSUM)));
    pr.verify_csum(csum, expected)
}

/// `probe_raid1(pr, off)`: a 1.x superblock at `off`.
fn probe_raid1(pr: &mut Probe, off: u64) -> i32 {
    let Some(mdp1) = pr.get_buffer(off, MDP1_SIZE) else {
        return pr.none_or_err();
    };
    if mdp1.le32(0) != MD_SB_MAGIC {
        return 1;
    }
    if mdp1.le32(4) != 1 {
        return 1;
    }
    if mdp1.le64(144) != off >> 9 {
        return 1;
    }
    if !raid1_verify_csum(pr, off) {
        return 1;
    }
    if pr.set_uuid(mdp1.span(16, 16)) != 0 {
        return 1;
    }
    if pr.set_uuid_as(mdp1.span(168, 16), Some("UUID_SUB")) != 0 {
        return 1;
    }
    if pr.set_label(mdp1.span(32, 32)) != 0 {
        return 1;
    }
    if pr.set_magic(off, mdp1.span(0, 4)) != 0 {
        return 1;
    }
    0
}

/// `probe_raid`.
fn probe_raid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let mut ver: Option<&[u8]> = None;
    let mut ret = PROBE_NONE;
    if pr.size > MD_RESERVED_BYTES {
        // 0.90 at the end of the device.
        let sboff = (pr.size & !(MD_RESERVED_BYTES - 1)).wrapping_sub(MD_RESERVED_BYTES);
        ret = probe_raid0(pr, sboff);
        if ret < 1 {
            return ret;
        }
        // 1.0 at the end.
        let sboff = (pr.size & !(0x1000 - 1)).wrapping_sub(0x2000);
        ret = probe_raid1(pr, sboff);
        if ret < 0 {
            return ret;
        }
        if ret == 0 {
            ver = Some(b"1.0");
        }
    }
    if ver.is_none() {
        // 1.1 at the start; 1.2 at 4 KiB.
        ret = probe_raid1(pr, 0);
        if ret == 0 {
            ver = Some(b"1.1");
        } else if ret == PROBE_NONE {
            ret = probe_raid1(pr, 0x1000);
            if ret == 0 {
                ver = Some(b"1.2");
            }
        }
    }
    if let Some(v) = ver {
        pr.set_version(v);
        return PROBE_OK;
    }
    ret
}

/// `linuxraid_idinfo`.
pub static LINUXRAID: IdInfo = IdInfo {
    name: "linux_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_raid),
    magics: &[],
};
