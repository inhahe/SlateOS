//! `befs.c`: the BeOS/Haiku filesystem. The volume ID is an attribute of the
//! root directory: in its inode's small-data area, or failing that in its
//! attribute directory's B+tree.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Buf, Bytes, Endianness, PROBE_NONE, PROBE_OK, USAGE_FILESYSTEM, copy_into};

/// `B_OS_NAME_LENGTH`.
const B_OS_NAME_LENGTH: u64 = 0x20;
/// `SUPER_BLOCK_MAGIC1`: "BFS1".
const SUPER_BLOCK_MAGIC1: u32 = 0x4246_5331;
/// `SUPER_BLOCK_MAGIC2`.
const SUPER_BLOCK_MAGIC2: u32 = 0xdd12_1031;
/// `SUPER_BLOCK_MAGIC3`.
const SUPER_BLOCK_MAGIC3: u32 = 0x15b6_830e;
/// `SUPER_BLOCK_FS_ENDIAN`: "BIGE".
const SUPER_BLOCK_FS_ENDIAN: u32 = 0x4249_4745;
/// `INODE_MAGIC1`.
const INODE_MAGIC1: u32 = 0x3bbe_0ad9;
/// `BPLUSTREE_MAGIC`.
const BPLUSTREE_MAGIC: i32 = 0x69f6_c2e8;
/// `BPLUSTREE_NULL`.
const BPLUSTREE_NULL: i64 = -1;
/// `B_UINT64_TYPE`: "ULLG".
const B_UINT64_TYPE: u32 = 0x554c_4c47;
/// `KEY_NAME`.
const KEY_NAME: &[u8] = b"be:volume_id";
/// `KEY_SIZE`.
const KEY_SIZE: u64 = 8;
/// `BAD_KEYS`.
const BAD_KEYS: i32 = -2;
/// `ENOENT`.
const ENOENT: i64 = 2;
/// `sizeof(struct befs_super_block)`.
const SUPER_BLOCK: u64 = 164;
/// `sizeof(struct befs_inode)`; its small data follows.
const INODE: u64 = 232;
/// `offsetof(struct befs_inode, data)`.
const INODE_DATA: usize = 72;
/// `sizeof(struct bplustree_node)`.
const BPT_NODE: u64 = 28;

/// The filesystem's byte order and the superblock fields every read needs.
struct Fs {
    le: bool,
    ag_shift: u32,
    block_shift: u32,
    block_size: u32,
}

impl Fs {
    fn u16(&self, b: &[u8], at: usize) -> u16 {
        if self.le { b.le16(at) } else { b.be16(at) }
    }
    fn u32(&self, b: &[u8], at: usize) -> u32 {
        if self.le { b.le32(at) } else { b.be32(at) }
    }
    fn i64(&self, b: &[u8], at: usize) -> i64 {
        let v = if self.le { b.le64(at) } else { b.be64(at) };
        i64::from_ne_bytes(v.to_ne_bytes())
    }
    /// A block run's length in bytes.
    fn run_len(&self, br: &[u8]) -> i64 {
        i64::from(self.u16(br, 6)).wrapping_shl(self.block_shift)
    }
    /// A block run's position in bytes.
    fn run_off(&self, br: &[u8]) -> u64 {
        (u64::from(self.u32(br, 0))
            .wrapping_shl(self.ag_shift)
            .wrapping_shl(self.block_shift))
        .wrapping_add(u64::from(self.u16(br, 4)).wrapping_shl(self.block_shift))
    }
}

/// `get_block_run`: the whole run.
fn get_block_run(pr: &mut Probe, fs: &Fs, br: &[u8]) -> Option<Buf> {
    let len = u64::from(fs.u16(br, 6)).wrapping_shl(fs.block_shift);
    pr.get_buffer(fs.run_off(br), len)
}

/// `get_custom_block_run`: `length` bytes at `offset` into the run.
fn get_custom_block_run(
    pr: &mut Probe,
    fs: &Fs,
    br: &[u8],
    offset: i64,
    length: u32,
) -> Option<Buf> {
    if offset.wrapping_add(i64::from(length)) > fs.run_len(br) {
        return None;
    }
    #[allow(
        clippy::cast_sign_loss,
        reason = "C adds the int64 offset to a uint64_t"
    )]
    let at = fs.run_off(br).wrapping_add(offset as u64);
    pr.get_buffer(at, u64::from(length))
}

/// `get_tree_node(pr, bs, ds, start, length)`: `length` bytes at `start` in
/// a data stream -- through its direct, indirect or double-indirect runs.
fn get_tree_node(pr: &mut Probe, fs: &Fs, ds: &[u8], start: i64, length: u32) -> Option<Buf> {
    let mut start = start;
    let max_direct = fs.i64(ds, 96);
    let max_indirect = fs.i64(ds, 112);
    let max_double = fs.i64(ds, 128);
    if start < max_direct {
        for i in 0..12usize {
            let br = ds.span(i.saturating_mul(8), 8);
            let br_len = fs.run_len(br);
            if start < br_len {
                return get_custom_block_run(pr, fs, br, start, length);
            }
            start = start.wrapping_sub(br_len);
        }
    } else if start < max_indirect {
        start = start.wrapping_sub(max_direct);
        let indirect = ds.span(104, 8);
        let max_br = fs.run_len(indirect) / 8;
        let runs = get_block_run(pr, fs, indirect)?;
        let mut i = 0i64;
        while i < max_br {
            let br = runs
                .span(
                    usize::try_from(i).unwrap_or(usize::MAX).saturating_mul(8),
                    8,
                )
                .to_vec();
            let br_len = fs.run_len(&br);
            if start < br_len {
                return get_custom_block_run(pr, fs, &br, start, length);
            }
            start = start.wrapping_sub(br_len);
            i = i.wrapping_add(1);
        }
    } else if start < max_double {
        start = start.wrapping_sub(max_indirect);
        let double = ds.span(120, 8);
        let di_br_size = fs.run_len(double);
        if di_br_size == 0 {
            return None;
        }
        let br_per_di_br = di_br_size / 8;
        if br_per_di_br == 0 {
            return None;
        }
        // Neither divisor is 0 here (both were checked), nor -1.
        let span = br_per_di_br.wrapping_mul(di_br_size);
        let within = start.checked_rem(span)?;
        let di_index = start.checked_div(span)?;
        let i_index = within.checked_div(di_br_size)?;
        start = within.checked_rem(di_br_size)?;
        if di_index >= br_per_di_br {
            return None;
        }
        let runs = get_block_run(pr, fs, double)?;
        let at = |k: i64| usize::try_from(k).unwrap_or(usize::MAX).saturating_mul(8);
        let di = runs.span(at(di_index), 8).to_vec();
        let max_br = fs.run_len(&di) / 8;
        if i_index >= max_br {
            return None;
        }
        let runs = get_block_run(pr, fs, &di)?;
        let br = runs.span(at(i_index), 8).to_vec();
        return get_custom_block_run(pr, fs, &br, start, length);
    }
    None
}

/// `compare_keys(keys1, keylengths1, index, key2, keylength2, all_key_length)`
/// over a node: key `index` against `key2`. The key-length array is at
/// `kl_off` in the node; an index of -1 (an empty node) reads before it, as
/// upstream's does.
fn compare_keys(
    fs: &Fs,
    bn: &[u8],
    kl_off: usize,
    index: i32,
    key2: &[u8],
    all_key_length: u16,
) -> i32 {
    let kl = |i: i32| -> u16 {
        let at = i64::try_from(kl_off)
            .unwrap_or(0)
            .wrapping_add(2i64.wrapping_mul(i64::from(i)));
        usize::try_from(at).map_or(0, |a| fs.u16(bn, a))
    };
    let keystart1 = if index == 0 {
        0
    } else {
        kl(index.wrapping_sub(1))
    };
    let keylength1 = kl(index).wrapping_sub(keystart1);
    if i32::from(keystart1).wrapping_add(i32::from(keylength1)) > i32::from(all_key_length) {
        return BAD_KEYS;
    }
    let key1 = bn
        .get(usize::from(keystart1).saturating_add(28)..)
        .unwrap_or_default();
    let keylength2 = u16::try_from(key2.len()).unwrap_or(u16::MAX);
    let n = usize::from(keylength1.min(keylength2));
    // strncmp.
    let mut result = 0i32;
    for i in 0..n {
        let a = key1.u8_at(i);
        let b = key2.get(i).copied().unwrap_or(0);
        if a != b {
            result = i32::from(a).wrapping_sub(i32::from(b));
            break;
        }
        if a == 0 {
            break;
        }
    }
    if result == 0 {
        return i32::from(keylength1).wrapping_sub(i32::from(keylength2));
    }
    if result < 0 {
        return -1;
    }
    result
}

/// `get_key_value(pr, bs, bi, key)`: the value stored under `key` in an
/// inode's B+tree; `-errno`, or `-ENOENT`, when it is not found.
fn get_key_value(pr: &mut Probe, fs: &Fs, bi: &[u8], key: &[u8]) -> i64 {
    let ds = bi.span(INODE_DATA, 144).to_vec();
    pr.errno = 0;
    let Some(bh) = get_tree_node(pr, fs, &ds, 0, 40) else {
        return if pr.errno != 0 {
            i64::from(pr.errno).wrapping_neg()
        } else {
            -ENOENT
        };
    };
    if i32::from_ne_bytes(fs.u32(&bh, 0).to_ne_bytes()) != BPLUSTREE_MAGIC {
        return -ENOENT;
    }
    let mut node_pointer = fs.i64(&bh, 16);
    let bn_size = fs.u32(&bh, 4);
    if u64::from(bn_size) < BPT_NODE {
        return -ENOENT;
    }
    let mut loop_detect = 0u32;
    loop {
        pr.errno = 0;
        let Some(bn) = get_tree_node(pr, fs, &ds, node_pointer, bn_size) else {
            return if pr.errno != 0 {
                i64::from(pr.errno).wrapping_neg()
            } else {
                -ENOENT
            };
        };
        let all_key_count = u32::from(fs.u16(&bn, 24));
        let all_key_length = fs.u16(&bn, 26);
        let kl_off = BPT_NODE
            .wrapping_add(u64::from(all_key_length))
            .wrapping_add(7)
            & !7;
        let values_off = kl_off.wrapping_add(u64::from(all_key_count).wrapping_mul(2));
        if values_off.wrapping_add(u64::from(all_key_count).wrapping_mul(8)) > u64::from(bn_size) {
            return -ENOENT;
        }
        let kl_off = usize::try_from(kl_off).unwrap_or(usize::MAX);
        let value = |i: i32| -> i64 {
            let at = i64::try_from(values_off)
                .unwrap_or(0)
                .wrapping_add(8i64.wrapping_mul(i64::from(i)));
            usize::try_from(at).map_or(0, |a| fs.i64(&bn, a))
        };
        let overflow = fs.i64(&bn, 16);
        let mut first = 0i32;
        let mut mid = 0i32;
        let mut last = i32::from_ne_bytes(all_key_count.wrapping_sub(1).to_ne_bytes());
        let mut cmp = compare_keys(fs, &bn, kl_off, last, key, all_key_length);
        if cmp == BAD_KEYS {
            return -ENOENT;
        }
        match cmp.cmp(&0) {
            std::cmp::Ordering::Equal => {
                if overflow == BPLUSTREE_NULL {
                    return value(last);
                }
                node_pointer = value(last);
            }
            std::cmp::Ordering::Less => node_pointer = overflow,
            std::cmp::Ordering::Greater => {
                while first <= last {
                    mid = first.wrapping_add(last) / 2;
                    cmp = compare_keys(fs, &bn, kl_off, mid, key, all_key_length);
                    if cmp == BAD_KEYS {
                        return -ENOENT;
                    }
                    if cmp == 0 {
                        if overflow == BPLUSTREE_NULL {
                            return value(mid);
                        }
                        break;
                    }
                    if cmp < 0 {
                        first = mid.wrapping_add(1);
                    } else {
                        last = mid.wrapping_sub(1);
                    }
                }
                node_pointer = if cmp < 0 {
                    value(mid.wrapping_add(1))
                } else {
                    value(mid)
                };
            }
        }
        loop_detect = loop_detect.saturating_add(1);
        if loop_detect >= 100 || overflow == BPLUSTREE_NULL {
            break;
        }
    }
    0
}

/// `get_uuid(pr, bs, &uuid)`: the volume ID's eight bytes, as stored.
fn get_uuid(pr: &mut Probe, fs: &Fs, bs: &[u8]) -> Result<[u8; 8], i32> {
    let err = Probe::none_or_err;
    let root_dir = bs.span(116, 8).to_vec();
    let Some(bi) = get_block_run(pr, fs, &root_dir) else {
        return Err(err(pr));
    };
    if fs.u32(&bi, 0) != INODE_MAGIC1 {
        return Err(PROBE_NONE);
    }
    let mut uuid = [0u8; 8];
    let bi_size = u64::from(fs.u16(&root_dir, 6)).wrapping_shl(fs.block_shift);
    let sd_total = bi_size.wrapping_sub(INODE).min(u64::from(fs.u32(&bi, 64)));
    let mut offset: u64 = 0;
    while offset.wrapping_add(8) <= sd_total {
        let sd = bi
            .get(usize::try_from(INODE.wrapping_add(offset)).unwrap_or(usize::MAX)..)
            .unwrap_or_default();
        let name_size = u64::from(fs.u16(sd, 4));
        let data_size = u64::from(fs.u16(sd, 6));
        // Header, name, three bytes of padding, data, NUL.
        let sd_size = name_size.wrapping_add(data_size).wrapping_add(12);
        if offset.wrapping_add(sd_size) > sd_total {
            break;
        }
        let ty = fs.u32(sd, 0);
        let mut key = KEY_NAME.to_vec();
        key.push(0);
        if ty == B_UINT64_TYPE
            && name_size == KEY_NAME.len() as u64
            && data_size == KEY_SIZE
            && sd.span(8, key.len()) == key.as_slice()
        {
            let at = usize::try_from(name_size.wrapping_add(11)).unwrap_or(usize::MAX);
            copy_into(&mut uuid, sd.span(at, 8));
            break;
        }
        if ty == 0 && name_size == 0 && data_size == 0 {
            break;
        }
        offset = offset.wrapping_add(sd_size);
    }
    let attributes = bi.span(52, 8).to_vec();
    if uuid == [0; 8] && attributes != [0; 8] {
        let Some(bi) = get_block_run(pr, fs, &attributes) else {
            return Err(err(pr));
        };
        if fs.u32(&bi, 0) != INODE_MAGIC1 {
            return Err(PROBE_NONE);
        }
        let value = get_key_value(pr, fs, &bi, KEY_NAME);
        if value < 0 {
            return Err(if value == -ENOENT {
                PROBE_NONE
            } else {
                i32::try_from(value).unwrap_or(i32::MIN)
            });
        }
        if value > 0 {
            #[allow(clippy::cast_sign_loss, reason = "C shifts the positive int64")]
            let at = (value as u64).wrapping_shl(fs.block_shift);
            let Some(bi) = pr.get_buffer(at, u64::from(fs.block_size)) else {
                return Err(err(pr));
            };
            if fs.u32(&bi, 0) != INODE_MAGIC1 {
                return Err(1);
            }
            let data = bi.span(INODE_DATA, 144).to_vec();
            if fs.u32(&bi, 60) == B_UINT64_TYPE && fs.i64(&data, 136) == 8 && fs.u16(&data, 6) == 1
            {
                let Some(attr) = get_block_run(pr, fs, data.span(0, 8)) else {
                    return Err(err(pr));
                };
                copy_into(&mut uuid, attr.span(0, 8));
            }
        }
    }
    Ok(uuid)
}

/// `probe_befs`.
fn probe_befs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let sboff = mag.map_or(B_OS_NAME_LENGTH, |m| u64::from(m.sboff));
    let Some(bs) = pr.get_buffer(sboff.wrapping_sub(B_OS_NAME_LENGTH), SUPER_BLOCK) else {
        return pr.none_or_err();
    };
    let matches = |r: fn(&[u8], usize) -> u32| {
        r(&bs, 32) == SUPER_BLOCK_MAGIC1
            && r(&bs, 68) == SUPER_BLOCK_MAGIC2
            && r(&bs, 112) == SUPER_BLOCK_MAGIC3
            && r(&bs, 36) == SUPER_BLOCK_FS_ENDIAN
    };
    let (le, version): (bool, &[u8]) = if matches(<[u8] as Bytes>::le32) {
        (true, b"little-endian")
    } else if matches(<[u8] as Bytes>::be32) {
        (false, b"big-endian")
    } else {
        return PROBE_NONE;
    };
    let rd = |at: usize| if le { bs.le32(at) } else { bs.be32(at) };
    let block_size = rd(40);
    let block_shift = rd(44);
    if !(10..=13).contains(&block_shift) || block_size != 1u32 << block_shift {
        return PROBE_NONE;
    }
    let ag_shift = rd(76);
    if ag_shift > 64 {
        return PROBE_NONE;
    }
    let fs = Fs {
        le,
        ag_shift,
        block_shift,
        block_size,
    };
    let volume_id = match get_uuid(pr, &fs, &bs) {
        Ok(u) => u,
        Err(rc) => return rc,
    };
    if bs.u8_at(0) != 0 {
        pr.set_label(bs.span(0, 32));
    }
    pr.set_version(version);
    if volume_id != [0; 8] {
        let v = if le {
            u64::from_le_bytes(volume_id)
        } else {
            u64::from_be_bytes(volume_id)
        };
        pr.sprintf_uuid(&volume_id, format!("{v:016x}"));
    }
    pr.set_fsblocksize(block_size);
    pr.set_block_size(block_size);
    pr.set_fsendianness(if le {
        Endianness::Little
    } else {
        Endianness::Big
    });
    PROBE_OK
}

/// `befs_idinfo`: "BFS1" in either byte order, in the first or second
/// sector.
pub static BEFS: IdInfo = IdInfo {
    name: "befs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 1024 * 1440,
    probefunc: Some(probe_befs),
    magics: &[
        IdMag::new(b"BFS1", 0, 0x20),
        IdMag::new(b"1SFB", 0, 0x20),
        IdMag::new(b"BFS1", 0, 0x220),
        IdMag::new(b"1SFB", 0, 0x220),
    ],
};
