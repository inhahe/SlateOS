//! libmagic's `cdf.c`: reading a Composite Document File -- the OLE2
//! container of Microsoft Office's pre-2007 documents, MSI installers,
//! Outlook messages and Thumbs.db -- far enough to find its summary
//! information and its stream names.
//!
//! A CDF is a little filesystem: a header, a sector allocation table (SAT)
//! chaining sectors into streams, a short-sector table for small streams kept
//! inside one container stream, and a directory naming the streams. Upstream
//! walks it with pointers and `memcpy`; this walks the same bytes with
//! offsets, keeping every bound upstream checks and failing where it fails.
//!
//! Upstream also leaves `errno` behind -- `EFTYPE` (which is `EINVAL` on
//! Linux) on most failures, `ESRCH` for a stream that is not there -- and
//! both its own caller (`readcdf.c` asks whether a missing summary was
//! `ESRCH`) and later, unrelated messages read it. So the reader carries
//! `errno` as upstream's would hold it.

use std::fs::File;
use std::io::ErrorKind;

use crate::buffer::pread;
use crate::funcs::Errno;

/// `EFTYPE`, which is `EINVAL` where the system has no `EFTYPE`.
pub const EFTYPE: Option<Errno> = Some(Errno::Kind(ErrorKind::InvalidInput));
/// `ESRCH`: "No such process" -- upstream's "no such stream".
pub const ESRCH: Option<Errno> = Some(Errno::Srch);

pub const CDF_LOOP_LIMIT: usize = 10000;
pub const CDF_ELEMENT_LIMIT: usize = 100_000;

pub const CDF_SECID_FREE: i32 = -1;
pub const CDF_SECID_END_OF_CHAIN: i32 = -2;

pub const CDF_MAGIC: u64 = 0xE11A_B1A1_E011_CFD0;
pub const CDF_DIRECTORY_SIZE: usize = 128;

pub const CDF_DIR_TYPE_USER_STORAGE: u8 = 1;
pub const CDF_DIR_TYPE_USER_STREAM: u8 = 2;
pub const CDF_DIR_TYPE_ROOT_STORAGE: u8 = 5;

pub const CDF_TIME_PREC: i64 = 10_000_000;

// Variant types.
pub const CDF_EMPTY: u32 = 0x00;
pub const CDF_NULL: u32 = 0x01;
pub const CDF_SIGNED16: u32 = 0x02;
pub const CDF_SIGNED32: u32 = 0x03;
pub const CDF_FLOAT: u32 = 0x04;
pub const CDF_DOUBLE: u32 = 0x05;
pub const CDF_BOOL: u32 = 0x0b;
pub const CDF_SIGNED64: u32 = 0x14;
pub const CDF_UNSIGNED64: u32 = 0x15;
pub const CDF_UNSIGNED32: u32 = 0x13;
pub const CDF_LENGTH32_STRING: u32 = 0x1e;
pub const CDF_LENGTH32_WSTRING: u32 = 0x1f;
pub const CDF_FILETIME: u32 = 0x40;
pub const CDF_CLIPBOARD: u32 = 0x47;
pub const CDF_VECTOR: u32 = 0x1000;
pub const CDF_ARRAY: u32 = 0x2000;
pub const CDF_BYREF: u32 = 0x4000;
pub const CDF_RESERVED: u32 = 0x8000;
pub const CDF_TYPEMASK: u32 = 0x0fff;

pub const CDF_PROPERTY_NAME_OF_APPLICATION: u32 = 0x12;

/// `cdf_info_t`: the file, and the bytes of it already read.
pub struct Info<'a> {
    pub fd: Option<&'a File>,
    pub buf: &'a [u8],
}

/// The reader: the file, and `errno`.
pub struct Cdf<'a> {
    pub info: Info<'a>,
    pub errno: Option<Errno>,
}

/// `cdf_header_t`, the fields that are read.
#[derive(Clone)]
pub struct Header {
    pub magic: u64,
    pub sec_size_p2: u16,
    pub short_sec_size_p2: u16,
    pub secid_first_directory: u32,
    pub min_size_standard_stream: u32,
    pub secid_first_sector_in_short_sat: i32,
    pub secid_first_sector_in_master_sat: i32,
    pub num_sectors_in_master_sat: u32,
    pub master_sat: [i32; 109],
}

/// `CDF_SEC_SIZE`.
pub fn sec_size(h: &Header) -> usize {
    1usize << h.sec_size_p2
}

/// `CDF_SHORT_SEC_SIZE`.
pub fn short_sec_size(h: &Header) -> usize {
    1usize << h.short_sec_size_p2
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([byte(b, at), byte(b, at + 1)])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([byte(b, at), byte(b, at + 1), byte(b, at + 2), byte(b, at + 3)])
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from(le32(b, at)) | u64::from(le32(b, at + 4)) << 32
}

fn byte(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}

/// `cdf_sat_t`: a sector allocation table, as bytes.
pub struct Sat {
    pub tab: Vec<u8>,
    pub len: usize,
}

impl Sat {
    /// `sat_tab[sid]`.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    fn next(&self, sid: i32) -> i32 {
        le32(&self.tab, (sid as usize).wrapping_mul(4)) as i32
    }
}

/// `cdf_directory_t`.
#[derive(Clone, Default)]
pub struct Dir {
    pub name: [u16; 32],
    pub typ: u8,
    pub storage_uuid: [u64; 2],
    pub stream_first_sector: i32,
    pub size: u32,
}

/// `cdf_stream_t`: `tab` is `None` for a stream that was zeroed.
#[derive(Default)]
pub struct Stream {
    pub tab: Option<Vec<u8>>,
    /// Sectors.
    pub len: usize,
    pub dirlen: usize,
    /// Sector size.
    pub ss: usize,
}

impl Stream {
    /// `cdf_zero_stream`: -1, always.
    pub fn zero(&mut self) -> i32 {
        *self = Stream::default();
        -1
    }

    pub fn bytes(&self) -> &[u8] {
        self.tab.as_deref().unwrap_or_default()
    }
}

impl Cdf<'_> {
    /// `cdf_read`: `len` bytes at `off`, from what was read if it reaches
    /// that far, else from the file. -1 or `len`.
    fn read(&mut self, off: usize, out: &mut [u8]) -> isize {
        let len = out.len();
        let siz = off.wrapping_add(len);
        if siz <= self.info.buf.len() && off <= siz {
            if let Some(src) = self.info.buf.get(off..siz) {
                out.copy_from_slice(src);
                return isize::try_from(len).unwrap_or(isize::MAX);
            }
        }
        let Some(fd) = self.info.fd else {
            self.errno = Some(Errno::Kind(ErrorKind::InvalidInput));
            return -1;
        };
        match pread(fd, out, off as u64) {
            Ok(n) if n == len => isize::try_from(len).unwrap_or(isize::MAX),
            // A short read leaves `errno` as it was.
            Ok(_) => -1,
            Err(e) => {
                self.errno = Some(Errno::of(&e));
                -1
            }
        }
    }

    /// `cdf_read_header`: the header, or `None` (`errno` `EFTYPE` for one
    /// that is not a CDF's).
    pub fn read_header(&mut self) -> Option<Header> {
        let mut buf = [0u8; 512];
        if self.read(0, &mut buf) == -1 {
            return None;
        }
        let mut master_sat = [0i32; 109];
        for (i, m) in master_sat.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_wrap)]
            {
                *m = le32(&buf, 76 + i * 4) as i32;
            }
        }
        #[allow(clippy::cast_possible_wrap)]
        let h = Header {
            magic: le64(&buf, 0),
            sec_size_p2: le16(&buf, 30),
            short_sec_size_p2: le16(&buf, 32),
            secid_first_directory: le32(&buf, 48),
            min_size_standard_stream: le32(&buf, 56),
            secid_first_sector_in_short_sat: le32(&buf, 60) as i32,
            secid_first_sector_in_master_sat: le32(&buf, 68) as i32,
            num_sectors_in_master_sat: le32(&buf, 72),
            master_sat,
        };
        if h.magic != CDF_MAGIC || h.sec_size_p2 > 20 || h.short_sec_size_p2 > 20 {
            self.errno = EFTYPE;
            return None;
        }
        Some(h)
    }

    /// `cdf_read_sector`: sector `id` into `buf[offs..offs + len]`.
    fn read_sector(&mut self, buf: &mut [u8], offs: usize, len: usize, h: &Header, id: i32) -> isize {
        let ss = sec_size(h);
        #[allow(clippy::cast_sign_loss)]
        let uid = id as isize as usize;
        if usize::MAX / ss < uid {
            return -1;
        }
        let pos = ss.wrapping_add(uid.wrapping_mul(ss));
        let Some(dst) = buf.get_mut(offs..offs + len) else {
            return -1;
        };
        self.read(pos, dst)
    }

    /// `cdf_read_sat`.
    pub fn read_sat(&mut self, h: &Header) -> Option<Sat> {
        let ss = sec_size(h);
        let nsatpersec = (ss / 4).wrapping_sub(1);
        let mut i = h.master_sat.iter().position(|&m| m == CDF_SECID_FREE).unwrap_or(109);
        let sec_limit = (u32::MAX as usize) / (64 * ss);
        if (nsatpersec > 0 && h.num_sectors_in_master_sat as usize > sec_limit / nsatpersec) || i > sec_limit {
            self.errno = EFTYPE;
            return None;
        }
        let sat_len = h.num_sectors_in_master_sat as usize * nsatpersec + i;
        // `calloc` of nothing allocates one.
        let mut tab = vec![0u8; sat_len.max(1) * ss];
        i = 0;
        while i < 109 {
            let m = h.master_sat[i];
            if m < 0 {
                break;
            }
            if self.read_sector(&mut tab, ss * i, ss, h, m) != ss as isize {
                return None;
            }
            i += 1;
        }
        let mut msa = vec![0u8; ss];
        let mut mid = h.secid_first_sector_in_master_sat;
        let mut j = 0usize;
        'master: while j < h.num_sectors_in_master_sat as usize {
            if mid < 0 {
                break 'master;
            }
            if j >= CDF_LOOP_LIMIT {
                self.errno = EFTYPE;
                return None;
            }
            if self.read_sector(&mut msa, 0, ss, h, mid) != ss as isize {
                return None;
            }
            for k in 0..nsatpersec {
                #[allow(clippy::cast_possible_wrap)]
                let sec = le32(&msa, k * 4) as i32;
                if sec < 0 {
                    break 'master;
                }
                if i >= sat_len {
                    self.errno = EFTYPE;
                    return None;
                }
                if self.read_sector(&mut tab, ss * i, ss, h, sec) != ss as isize {
                    return None;
                }
                i += 1;
            }
            #[allow(clippy::cast_possible_wrap)]
            {
                mid = le32(&msa, nsatpersec * 4) as i32;
            }
            j += 1;
        }
        Some(Sat { tab, len: i })
    }

    /// `cdf_count_chain`: the sectors of the chain from `sid`, or `None`
    /// (`(size_t)-1`).
    pub fn count_chain(&mut self, sat: &Sat, sid: i32, size: usize) -> Option<usize> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let maxsector = (sat.len.wrapping_mul(size) / 4) as i32;
        if sid == CDF_SECID_END_OF_CHAIN {
            return Some(0);
        }
        let mut sid = sid;
        let mut i = 0usize;
        while sid >= 0 {
            if i >= CDF_LOOP_LIMIT || sid >= maxsector {
                self.errno = EFTYPE;
                return None;
            }
            sid = sat.next(sid);
            i += 1;
        }
        if i == 0 {
            self.errno = EFTYPE;
            return None;
        }
        Some(i)
    }

    /// `cdf_read_long_sector_chain`: 0, or -1 with the stream zeroed.
    pub fn read_long_sector_chain(&mut self, h: &Header, sat: &Sat, sid: i32, len: usize, scn: &mut Stream) -> i32 {
        let ss = sec_size(h);
        let count = self.count_chain(sat, sid, ss);
        scn.tab = None;
        scn.len = count.unwrap_or(usize::MAX);
        scn.dirlen = (h.min_size_standard_stream as usize).max(len);
        scn.ss = ss;
        if sid == CDF_SECID_END_OF_CHAIN || len == 0 {
            return scn.zero();
        }
        let Some(n) = count else {
            self.errno = EFTYPE;
            return scn.zero();
        };
        let mut tab = vec![0u8; n.max(1) * ss];
        let mut sid = sid;
        let mut i = 0usize;
        while sid >= 0 {
            if i >= CDF_LOOP_LIMIT || i >= n {
                self.errno = EFTYPE;
                return scn.zero();
            }
            // "Last sector might be truncated" -- but `cdf_read` never
            // returns a part, so a short sector fails like any other.
            if self.read_sector(&mut tab, i * ss, ss, h, sid) != ss as isize {
                self.errno = EFTYPE;
                return scn.zero();
            }
            sid = sat.next(sid);
            i += 1;
        }
        scn.tab = Some(tab);
        0
    }

    /// `cdf_read_short_sector_chain`.
    fn read_short_sector_chain(&mut self, h: &Header, ssat: &Sat, sst: &Stream, sid: i32, len: usize, scn: &mut Stream) -> i32 {
        let ss = short_sec_size(h);
        let count = self.count_chain(ssat, sid, sec_size(h));
        scn.tab = None;
        scn.len = count.unwrap_or(usize::MAX);
        scn.dirlen = len;
        scn.ss = ss;
        let Some(n) = count else {
            self.errno = EFTYPE;
            return scn.zero();
        };
        let mut tab = vec![0u8; n.max(1) * ss];
        let mut sid = sid;
        let mut i = 0usize;
        while sid >= 0 {
            if i >= CDF_LOOP_LIMIT || i >= n {
                self.errno = EFTYPE;
                return scn.zero();
            }
            // `cdf_read_short_sector`.
            #[allow(clippy::cast_sign_loss)]
            let uid = sid as usize;
            let pos = uid.wrapping_mul(ss);
            if usize::MAX / ss < uid || pos.wrapping_add(ss) > sec_size(h).wrapping_mul(sst.len) {
                self.errno = EFTYPE;
                return scn.zero();
            }
            let src = sst.bytes().get(pos..pos + ss);
            match (src, tab.get_mut(i * ss..(i + 1) * ss)) {
                (Some(s), Some(d)) => d.copy_from_slice(s),
                _ => {
                    self.errno = EFTYPE;
                    return scn.zero();
                }
            }
            sid = ssat.next(sid);
            i += 1;
        }
        scn.tab = Some(tab);
        0
    }

    /// `cdf_read_sector_chain`: a short stream when it is short and there is
    /// a container for it.
    pub fn read_sector_chain(&mut self, h: &Header, sat: &Sat, ssat: &Sat, sst: &Stream, sid: i32, len: usize, scn: &mut Stream) -> i32 {
        if len < h.min_size_standard_stream as usize && sst.tab.is_some() {
            self.read_short_sector_chain(h, ssat, sst, sid, len, scn)
        } else {
            self.read_long_sector_chain(h, sat, sid, len, scn)
        }
    }

    /// `cdf_read_dir`.
    pub fn read_dir(&mut self, h: &Header, sat: &Sat) -> Option<Vec<Dir>> {
        let ss = sec_size(h);
        #[allow(clippy::cast_possible_wrap)]
        let mut sid = h.secid_first_directory as i32;
        let ns = self.count_chain(sat, sid, ss)?;
        let nd = ss / CDF_DIRECTORY_SIZE;
        let mut dir = vec![Dir::default(); ns * nd];
        let mut buf = vec![0u8; ss];
        // Upstream's loop counter doubles as the inner loop's, so its limit
        // check sees `nd + 1` from the second sector on.
        let mut j = 0usize;
        for i in 0..ns {
            if j >= CDF_LOOP_LIMIT || self.read_sector(&mut buf, 0, ss, h, sid) != ss as isize {
                self.errno = EFTYPE;
                return None;
            }
            for jj in 0..nd {
                if let Some(d) = dir.get_mut(i * nd + jj) {
                    *d = unpack_dir(&buf, jj * CDF_DIRECTORY_SIZE);
                }
            }
            j = nd + 1;
            sid = sat.next(sid);
        }
        Some(dir)
    }

    /// `cdf_read_ssat`.
    pub fn read_ssat(&mut self, h: &Header, sat: &Sat) -> Option<Sat> {
        let ss = sec_size(h);
        let mut sid = h.secid_first_sector_in_short_sat;
        let Some(len) = self.count_chain(sat, sid, ss) else {
            self.errno = EFTYPE;
            return None;
        };
        let mut tab = vec![0u8; len.max(1) * ss];
        let mut i = 0usize;
        while sid >= 0 {
            if i >= CDF_LOOP_LIMIT || i >= len {
                self.errno = EFTYPE;
                return None;
            }
            if self.read_sector(&mut tab, i * ss, ss, h, sid) != ss as isize {
                return None;
            }
            sid = sat.next(sid);
            i += 1;
        }
        Some(Sat { tab, len })
    }

    /// `cdf_read_short_stream`: the root storage's stream, which holds the
    /// short streams; and which directory entry the root is.
    pub fn read_short_stream(&mut self, h: &Header, sat: &Sat, dir: &[Dir], scn: &mut Stream) -> (i32, Option<usize>) {
        let Some(i) = dir.iter().position(|d| d.typ == CDF_DIR_TYPE_ROOT_STORAGE) else {
            // "If the it is not there, just fake it; some docs don't have it".
            scn.zero();
            return (0, None);
        };
        let d = &dir[i];
        if d.stream_first_sector < 0 {
            scn.zero();
            return (0, Some(i));
        }
        let (first, size) = (d.stream_first_sector, d.size as usize);
        (self.read_long_sector_chain(h, sat, first, size, scn), Some(i))
    }

    /// `cdf_find_stream`: the 1-based index of the last entry of `typ` named
    /// `name`, or 0 with `errno` `ESRCH`.
    pub fn find_stream(&mut self, dir: &[Dir], name: &[u8], typ: u8) -> usize {
        for i in (1..=dir.len()).rev() {
            let d = &dir[i - 1];
            if d.typ == typ && namecmp(name, &d.name) {
                return i;
            }
        }
        self.errno = ESRCH;
        0
    }

    /// `cdf_read_user_stream`.
    pub fn read_user_stream(&mut self, h: &Header, sat: &Sat, ssat: &Sat, sst: &Stream, dir: &[Dir], name: &[u8], scn: &mut Stream) -> i32 {
        let i = self.find_stream(dir, name, CDF_DIR_TYPE_USER_STREAM);
        if i == 0 {
            *scn = Stream::default();
            return -1;
        }
        let d = &dir[i - 1];
        let (first, size) = (d.stream_first_sector, d.size as usize);
        self.read_sector_chain(h, sat, ssat, sst, first, size, scn)
    }
}

/// `cdf_unpack_dir`.
fn unpack_dir(b: &[u8], at: usize) -> Dir {
    let mut name = [0u16; 32];
    for (k, n) in name.iter_mut().enumerate() {
        *n = le16(b, at + k * 2);
    }
    #[allow(clippy::cast_possible_wrap)]
    Dir {
        name,
        typ: byte(b, at + 66),
        storage_uuid: [le64(b, at + 80), le64(b, at + 88)],
        stream_first_sector: le32(b, at + 116) as i32,
        size: le32(b, at + 120),
    }
}

/// `cdf_namecmp(name, d_name, strlen(name) + 1) == 0`: the name and its
/// terminator.
fn namecmp(name: &[u8], d: &[u16; 32]) -> bool {
    name.iter()
        .chain(std::iter::once(&0u8))
        .enumerate()
        .all(|(k, &c)| d.get(k).copied().unwrap_or(0) == u16::from(c))
}

/// A property: `cdf_property_info_t`. `val` holds the little-endian bytes
/// `cdf_copy_info` copied (zero-extended); a string is where it starts in the
/// stream and its length.
#[derive(Clone, Copy, Default)]
pub struct Prop {
    pub id: u32,
    pub typ: u32,
    pub val: u64,
    pub str_off: usize,
    pub str_len: u32,
}

/// An entry a vector of strings ran over: upstream leaves its id and type as
/// `realloc` left them. Its consumer stops at the vector before reaching it.
const UNSET: u32 = u32::MAX;

/// `cdf_check_stream`: the stream's sector size.
fn check_stream(sst: &Stream) -> usize {
    sst.ss
}

/// `cdf_check_stream_offset`: whether `[0, end)` lies in the stream.
fn check_offset(cdf: &mut Cdf<'_>, sst: &Stream, end: usize) -> bool {
    let ss = check_stream(sst);
    if end <= ss.wrapping_mul(sst.len) {
        return true;
    }
    cdf.errno = EFTYPE;
    false
}

/// `CDF_SHLEN_LIMIT` and `CDF_PROP_LIMIT` (`cdf_property_info_t` is 24
/// bytes).
const CDF_SHLEN_LIMIT: usize = u32::MAX as usize / 64;
const CDF_PROP_LIMIT: usize = u32::MAX as usize / (64 * 24);

/// `cdf_read_property_info`: the properties of the section at `offs`.
#[allow(clippy::too_many_lines)]
pub fn read_property_info(cdf: &mut Cdf<'_>, sst: &Stream, offs: u32) -> Option<Vec<Prop>> {
    let fail = |cdf: &mut Cdf<'_>| {
        cdf.errno = EFTYPE;
        None
    };
    if offs > u32::MAX / 4 {
        return fail(cdf);
    }
    let tab = sst.bytes();
    let shp = offs as usize;
    if !check_offset(cdf, sst, shp + 8) {
        return fail(cdf);
    }
    let sh_len = le32(tab, shp) as usize;
    if sh_len > CDF_SHLEN_LIMIT {
        return fail(cdf);
    }
    if !check_offset(cdf, sst, shp + sh_len) {
        return fail(cdf);
    }
    let sh_properties = le32(tab, shp + 4) as usize;
    if sh_properties > CDF_PROP_LIMIT {
        return fail(cdf);
    }
    let mut info = vec![
        Prop {
            id: UNSET,
            typ: UNSET,
            ..Prop::default()
        };
        sh_properties
    ];
    let p = shp + 8;
    let e = shp + sh_len;
    if p >= e || !check_offset(cdf, sst, e) {
        return fail(cdf);
    }
    let mut i = 0usize;
    while i < sh_properties {
        // `cdf_get_property_info_pos`.
        let tail = (i << 1) + 1;
        if p >= e || !check_offset(cdf, sst, p + (tail + 1) * 4) {
            return fail(cdf);
        }
        let ofs = le32(tab, p + tail * 4) as usize;
        if ofs < 8 || ofs - 8 > e - p {
            return fail(cdf);
        }
        let q = p + (ofs - 8);
        info[i].id = le32(tab, p + (i << 1) * 4);
        let left = e - q;
        if left < 4 {
            return fail(cdf);
        }
        let typ = le32(tab, q);
        info[i].typ = typ;
        let (nelements, mut slen) = if typ & CDF_VECTOR != 0 {
            if left < 8 {
                return fail(cdf);
            }
            let n = le32(tab, q + 4) as usize;
            if n > CDF_ELEMENT_LIMIT || n == 0 {
                return fail(cdf);
            }
            (n, 2usize)
        } else {
            (1, 1)
        };
        let mut o4 = slen * 4;
        let mut unknown = typ & (CDF_ARRAY | CDF_BYREF | CDF_RESERVED) != 0;
        if !unknown {
            // `cdf_copy_info`: a scalar, unless it is a vector or does not fit.
            let copy = |info: &mut Prop, len: usize| -> bool {
                if info.typ & CDF_VECTOR != 0 || e - (q + o4) < len {
                    return false;
                }
                info.val = match len {
                    2 => u64::from(le16(tab, q + o4)),
                    4 => u64::from(le32(tab, q + o4)),
                    _ => le64(tab, q + o4),
                };
                true
            };
            match typ & CDF_TYPEMASK {
                CDF_NULL | CDF_EMPTY => {}
                CDF_SIGNED16 => unknown = !copy(&mut info[i], 2),
                CDF_SIGNED32 | CDF_BOOL | CDF_UNSIGNED32 | CDF_FLOAT => unknown = !copy(&mut info[i], 4),
                CDF_SIGNED64 | CDF_UNSIGNED64 | CDF_DOUBLE | CDF_FILETIME => {
                    unknown = !copy(&mut info[i], 8);
                }
                CDF_LENGTH32_STRING | CDF_LENGTH32_WSTRING => {
                    if nelements > 1 {
                        // `cdf_grow_info`: room for the elements, past the end.
                        if info.len() + nelements > CDF_PROP_LIMIT {
                            return fail(cdf);
                        }
                        info.resize(
                            info.len() + nelements,
                            Prop {
                                id: UNSET,
                                typ: UNSET,
                                ..Prop::default()
                            },
                        );
                    }
                    let mut j = 0usize;
                    while j < nelements && i < sh_properties {
                        if o4 + 4 > left {
                            return fail(cdf);
                        }
                        let l = le32(tab, q + slen * 4);
                        o4 += 4;
                        if o4 + l as usize > left {
                            return fail(cdf);
                        }
                        info[i].str_len = l;
                        info[i].str_off = q + o4;
                        let l = if l & 1 != 0 { l.wrapping_add(1) } else { l };
                        slen += (l >> 1) as usize;
                        o4 = slen * 4;
                        j += 1;
                        i += 1;
                    }
                    i -= 1;
                }
                CDF_CLIPBOARD => unknown = typ & CDF_VECTOR != 0,
                _ => unknown = true,
            }
        }
        if unknown {
            info[i].val = 0;
            info[i].str_off = 0;
            info[i].str_len = 0;
        }
        i += 1;
    }
    info.truncate(sh_properties);
    Some(info)
}

/// `cdf_summary_info_header_t`, the fields that are read.
pub struct SummaryInfo {
    pub byte_order: u16,
    pub os_version: u16,
    pub os: u16,
}

/// `cdf_unpack_summary_info`.
pub fn unpack_summary_info(cdf: &mut Cdf<'_>, sst: &Stream) -> Option<(SummaryInfo, Vec<Prop>)> {
    if !check_offset(cdf, sst, 28) || !check_offset(cdf, sst, 0x1c + 20) {
        return None;
    }
    let tab = sst.bytes();
    let si = SummaryInfo {
        byte_order: le16(tab, 0),
        os_version: le16(tab, 4),
        os: le16(tab, 6),
    };
    let props = read_property_info(cdf, sst, le32(tab, 0x1c + 16))?;
    Some((si, props))
}

/// `cdf_print_property_name`.
pub fn print_property_name(p: u32) -> Vec<u8> {
    const VN: &[(u32, &str)] = &[
        (0x01, "Code page"),
        (0x02, "Title"),
        (0x03, "Subject"),
        (0x04, "Author"),
        (0x05, "Keywords"),
        (0x06, "Comments"),
        (0x07, "Template"),
        (0x08, "Last Saved By"),
        (0x09, "Revision Number"),
        (0x0a, "Total Editing Time"),
        (0x0b, "Last Printed"),
        (0x0c, "Create Time/Date"),
        (0x0d, "Last Saved Time/Date"),
        (0x0e, "Number of Pages"),
        (0x0f, "Number of Words"),
        (0x10, "Number of Characters"),
        (0x11, "Thumbnail"),
        (0x12, "Name of Creating Application"),
        (0x13, "Security"),
        (0x8000_0000, "Locale ID"),
    ];
    let s = match VN.iter().find(|&&(v, _)| v == p) {
        Some(&(_, n)) => n.as_bytes().to_vec(),
        None => crate::printf::format(b"%#x", &[crate::printf::Arg::I32(p)]).unwrap_or_default(),
    };
    // Into a 64-byte buffer.
    s.into_iter().take(63).collect()
}

/// `cdf_print_elapsed_time`.
#[allow(clippy::cast_possible_truncation)]
pub fn print_elapsed_time(ts: i64) -> Vec<u8> {
    use crate::printf::{Arg, format};
    let mut ts = ts / CDF_TIME_PREC;
    let secs = (ts % 60) as i32;
    ts /= 60;
    let mins = (ts % 60) as i32;
    ts /= 60;
    let hours = (ts % 24) as i32;
    ts /= 24;
    let days = ts as i32;
    let a = |v: i32| Arg::I32(v as u32);
    let mut out = Vec::new();
    if days != 0 {
        out.extend(format(b"%dd+", &[a(days)]).unwrap_or_default());
    }
    if days != 0 || hours != 0 {
        out.extend(format(b"%.2d:", &[a(hours)]).unwrap_or_default());
    }
    out.extend(format(b"%.2d:", &[a(mins)]).unwrap_or_default());
    out.extend(format(b"%.2d", &[a(secs)]).unwrap_or_default());
    out.truncate(63);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_is_upstreams() {
        assert_eq!(print_elapsed_time(0), b"00:00");
        assert_eq!(print_elapsed_time(61 * CDF_TIME_PREC), b"01:01");
        assert_eq!(print_elapsed_time((3600 * 25 + 5) * CDF_TIME_PREC), b"1d+01:00:05");
        assert_eq!(print_elapsed_time(3600 * 2 * CDF_TIME_PREC), b"02:00:00");
    }

    #[test]
    fn property_names() {
        assert_eq!(print_property_name(2), b"Title");
        assert_eq!(print_property_name(0x99), b"0x99");
    }

    #[test]
    fn names_compare_with_their_terminator() {
        let mut d = [0u16; 32];
        for (k, c) in b"Book".iter().enumerate() {
            d[k] = u16::from(*c);
        }
        assert!(namecmp(b"Book", &d));
        assert!(!namecmp(b"Boo", &d));
        assert!(!namecmp(b"Books", &d));
    }

    #[test]
    fn a_header_that_is_not_a_cdf_leaves_eftype() {
        let buf = [0u8; 600];
        let mut c = Cdf {
            info: Info { fd: None, buf: &buf },
            errno: None,
        };
        assert!(c.read_header().is_none());
        assert_eq!(c.errno, EFTYPE);
        // Too short, and no file to read the rest from: EINVAL.
        let mut c = Cdf {
            info: Info { fd: None, buf: &buf[..100] },
            errno: None,
        };
        assert!(c.read_header().is_none());
        assert_eq!(c.errno, Some(Errno::Kind(ErrorKind::InvalidInput)));
    }
}
