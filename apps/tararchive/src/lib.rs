//! TAR archives -- ustar, GNU and PAX -- listed: each member's name, kind,
//! size, time and where its bytes are.
//!
//! The archive manager refused every TAR ("this build has a ZIP back end
//! only"), and the file manager's archive columns were blank for one. There
//! were three TAR parsers in the tree already -- the kernel's, `tar` in
//! coreutils, and `undelete`'s -- and none a crate an application could use.
//!
//! # What a header can say, and where
//!
//! | Variant | Long names | Big numbers |
//! |---|---|---|
//! | ustar (POSIX.1-1988) | a 155-byte prefix before the 100-byte name | 11 octal digits: up to 8 GiB |
//! | GNU | an `L` entry before the member (`K` for a link target) | base-256: the first byte's high bit set |
//! | PAX (POSIX.1-2001) | an `x` entry of `path=`, `linkpath=` records | `size=`, `mtime=` records in decimal |
//!
//! All three are read. A header whose checksum does not add up -- the sum of
//! its bytes, the checksum field counted as spaces, signed or unsigned as
//! different tars wrote it -- is not a header: the listing stops there, and
//! says so. What came before it is still listed, because a damaged archive's
//! first members are still worth getting out.
//!
//! # Writing
//!
//! [`write_header`] writes one member's header -- ustar, which every reader
//! reads, with a PAX header before it only when a name, a link, a size or a
//! time does not fit ustar's fields -- and the caller writes the member's
//! bytes and [`write_padding`] after it; [`write_end`] closes the archive.
//! A long name goes in ustar's prefix where a `/` lets it, as GNU and POSIX
//! tars both read, and in a PAX `path` record where it does not.
//!
//! # A name is not a path
//!
//! A member's name is bytes someone else wrote, and may be `../../etc/passwd`.
//! It is given as it is; confining it under a destination is the extractor's
//! job, as it is for ZIP.

use std::io::{self, Read, Seek, SeekFrom, Write};

pub mod testing;

/// A TAR block: headers are one, and each member's bytes are padded to a
/// whole number of them.
pub const BLOCK: u64 = 512;

/// The longest name a GNU `L` or PAX `path` record may give. Real names are
/// a few hundred bytes; this is for the header that says a gigabyte.
const MAX_LONG_FIELD: u64 = 64 * 1024;

/// The most PAX extended header this reads for one member.
const MAX_PAX: u64 = 1024 * 1024;

/// The most members listed. An archive of more is listed this far and says so.
pub const MAX_MEMBERS: usize = 1_000_000;

/// What a member is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Symlink,
    /// Another name for a member earlier in the archive.
    HardLink,
    CharDevice,
    BlockDevice,
    Fifo,
    /// A type flag this does not know, as the header gives it.
    Other(u8),
}

impl Kind {
    /// The type flag a header writes for this kind.
    #[must_use]
    pub fn flag(self) -> u8 {
        match self {
            Self::File => b'0',
            Self::HardLink => b'1',
            Self::Symlink => b'2',
            Self::CharDevice => b'3',
            Self::BlockDevice => b'4',
            Self::Directory => b'5',
            Self::Fifo => b'6',
            Self::Other(flag) => flag,
        }
    }

    fn from_flag(flag: u8) -> Self {
        match flag {
            b'0' | 0 | b'7' => Self::File,
            b'1' => Self::HardLink,
            b'2' => Self::Symlink,
            b'3' => Self::CharDevice,
            b'4' => Self::BlockDevice,
            b'5' => Self::Directory,
            b'6' => Self::Fifo,
            other => Self::Other(other),
        }
    }
}

/// One member of an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The member's name as the archive gives it: bytes, not a path. A
    /// directory's usually ends in `/`.
    pub name: Vec<u8>,
    pub kind: Kind,
    /// Its bytes' length; 0 for what has none.
    pub size: u64,
    /// Permission bits, as the header gives them.
    pub mode: u32,
    /// Seconds since 1970, as the header gives them; negative before.
    pub mtime: i64,
    /// A link's target.
    pub link: Option<Vec<u8>>,
    /// Where its bytes start in the archive.
    pub offset: u64,
}

/// How a listing ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// At the block of zeros that closes an archive.
    Marker,
    /// At the end of the file, with no closing block: many writers leave it
    /// out, and nothing is missing.
    EndOfFile,
    /// At a block that is not a header, `at` bytes in: what follows it is
    /// not read.
    Damaged { at: u64, why: Damage },
    /// At [`MAX_MEMBERS`].
    TooManyMembers,
}

/// What was wrong where a listing stopped early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Damage {
    /// The header's checksum does not add up.
    Checksum,
    /// A number field is not a number.
    Number(&'static str),
    /// A member's bytes run past the end of the file.
    Truncated,
    /// A GNU long name or a PAX header larger than anyone writes.
    TooLong,
}

impl std::fmt::Display for Damage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checksum => f.write_str("a header's checksum does not add up"),
            Self::Number(field) => write!(f, "a header's {field} is not a number"),
            Self::Truncated => f.write_str("a member runs past the end of the file"),
            Self::TooLong => f.write_str("a long name or extended header is too large"),
        }
    }
}

/// An archive's members, and how the listing ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub end: End,
}

/// Whether `block` -- a file's first 512 bytes -- is a TAR header: its
/// checksum adds up, and it names something. The ustar magic is not
/// required: a pre-POSIX archive has none.
#[must_use]
pub fn is_header(block: &[u8]) -> bool {
    block.len() >= 512
        && block.iter().any(|&b| b != 0)
        && checksum_matches(block)
        && block.first().is_some_and(|&b| b != 0)
}

/// List the archive `r` holds.
///
/// # Errors
///
/// Only when a read or a seek fails; an archive that is damaged part-way is
/// listed as far as it reads, and [`Listing::end`] says where it stopped.
pub fn list<R: Read + Seek>(r: &mut R) -> io::Result<Listing> {
    let len = r.seek(SeekFrom::End(0))?;
    let mut entries = Vec::new();
    let mut at = 0_u64;
    // What a GNU `L`/`K` or a PAX `x` entry says about the member after it.
    let mut pending = Pending::default();
    let end = loop {
        if entries.len() >= MAX_MEMBERS {
            break End::TooManyMembers;
        }
        let block = read_block(r, at, len)?;
        let Some(block) = block else {
            break End::EndOfFile;
        };
        if block.iter().all(|&b| b == 0) {
            break End::Marker;
        }
        let header = match Header::parse(&block) {
            Ok(h) => h,
            Err(why) => break End::Damaged { at, why },
        };
        let data = at.saturating_add(BLOCK);
        let padded = header.size.div_ceil(BLOCK).saturating_mul(BLOCK);
        let next = data.saturating_add(padded);
        // A long name or extended header larger than anyone writes is said to
        // be that, before whether the file holds it.
        let cap = match header.flag {
            b'L' | b'K' => MAX_LONG_FIELD,
            b'x' | b'g' => MAX_PAX,
            _ => u64::MAX,
        };
        if header.size > cap {
            break End::Damaged {
                at,
                why: Damage::TooLong,
            };
        }
        if data.saturating_add(header.size) > len {
            break End::Damaged {
                at,
                why: Damage::Truncated,
            };
        }
        match header.flag {
            // GNU: the next member's long name, or its link's.
            b'L' | b'K' => {
                let value = until_nul(&read_exact_at(r, data, header.size)?).to_vec();
                if header.flag == b'L' {
                    pending.name = Some(value);
                } else {
                    pending.link = Some(value);
                }
            }
            // PAX: records for the next member; a global header's apply to
            // every member after it, and are rare enough that only the
            // fields a per-member one can hold are read from it too.
            b'x' | b'g' => {
                let records = read_exact_at(r, data, header.size)?;
                let target: &mut dyn TakePax = if header.flag == b'x' {
                    &mut pending
                } else {
                    &mut *pending.global
                };
                if let Err(why) = target.take_pax(&records) {
                    break End::Damaged { at, why };
                }
            }
            flag => {
                let taken = std::mem::take(&mut pending);
                pending.global = taken.global.clone();
                let name = taken
                    .name
                    .or_else(|| taken.global.name.clone())
                    .unwrap_or(header.name);
                let link = taken
                    .link
                    .or_else(|| taken.global.link.clone())
                    .or(header.link);
                let size = taken.size.or(taken.global.size).unwrap_or(header.size);
                // A PAX size larger than the header's moves where the next
                // header is; the file must still hold the bytes.
                let data_end = data.saturating_add(size);
                if data_end > len {
                    break End::Damaged {
                        at,
                        why: Damage::Truncated,
                    };
                }
                entries.push(Entry {
                    name,
                    kind: Kind::from_flag(flag),
                    size,
                    mode: header.mode,
                    mtime: taken.mtime.or(taken.global.mtime).unwrap_or(header.mtime),
                    link,
                    offset: data,
                });
                at = data.saturating_add(size.div_ceil(BLOCK).saturating_mul(BLOCK));
                continue;
            }
        }
        at = next;
    };
    Ok(Listing { entries, end })
}

/// [`list`] over bytes already in memory -- a `.tar.gz` inflated.
#[must_use]
pub fn list_bytes(bytes: &[u8]) -> Listing {
    // A cursor over a slice neither fails a read nor a seek.
    list(&mut io::Cursor::new(bytes)).unwrap_or(Listing {
        entries: Vec::new(),
        end: End::EndOfFile,
    })
}

/// What the entries before a member say about it.
#[derive(Clone, Debug, Default)]
struct Pending {
    name: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
    size: Option<u64>,
    mtime: Option<i64>,
    global: Box<PendingGlobal>,
}

/// What a PAX global header says about every member after it.
#[derive(Clone, Debug, Default)]
struct PendingGlobal {
    name: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
    size: Option<u64>,
    mtime: Option<i64>,
}

/// Where a PAX record's value goes.
trait TakePax {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Damage>;

    /// Read `records` -- `LENGTH KEY=VALUE\n`, the length counting itself.
    fn take_pax(&mut self, records: &[u8]) -> Result<(), Damage> {
        let mut rest = records;
        while !rest.is_empty() {
            // Padding after the last record is NULs.
            if rest.first() == Some(&0) {
                break;
            }
            let space = rest
                .iter()
                .position(|&b| b == b' ')
                .ok_or(Damage::Number("PAX record length"))?;
            let n: usize = std::str::from_utf8(rest.get(..space).unwrap_or(&[]))
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|&n| n > space)
                .ok_or(Damage::Number("PAX record length"))?;
            let record = rest.get(..n).ok_or(Damage::Truncated)?;
            let body = record
                .get(space.saturating_add(1)..)
                .and_then(|b| b.strip_suffix(b"\n"))
                .ok_or(Damage::Number("PAX record"))?;
            if let Some(eq) = body.iter().position(|&b| b == b'=') {
                let (key, value) = (
                    body.get(..eq).unwrap_or(&[]),
                    body.get(eq.saturating_add(1)..).unwrap_or(&[]),
                );
                self.put(key, value)?;
            }
            rest = rest.get(n..).unwrap_or(&[]);
        }
        Ok(())
    }
}

/// A PAX time: seconds, perhaps with a fraction, perhaps negative. The
/// fraction is dropped, toward the past.
fn pax_time(value: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(value).ok()?;
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let secs: i64 = whole.parse().ok()?;
    let has_fraction = fraction.bytes().any(|b| b != b'0');
    Some(if secs < 0 && has_fraction {
        secs.saturating_sub(1)
    } else {
        secs
    })
}

fn pax_size(value: &[u8]) -> Result<u64, Damage> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(Damage::Number("PAX size"))
}

impl TakePax for Pending {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Damage> {
        match key {
            b"path" => self.name = Some(value.to_vec()),
            b"linkpath" => self.link = Some(value.to_vec()),
            b"size" => self.size = Some(pax_size(value)?),
            b"mtime" => self.mtime = pax_time(value),
            _ => {}
        }
        Ok(())
    }
}

impl TakePax for PendingGlobal {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Damage> {
        match key {
            b"path" => self.name = Some(value.to_vec()),
            b"linkpath" => self.link = Some(value.to_vec()),
            b"size" => self.size = Some(pax_size(value)?),
            b"mtime" => self.mtime = pax_time(value),
            _ => {}
        }
        Ok(())
    }
}

/// A header's fields, as far as a listing needs them.
struct Header {
    name: Vec<u8>,
    mode: u32,
    size: u64,
    mtime: i64,
    flag: u8,
    link: Option<Vec<u8>>,
}

impl Header {
    fn parse(block: &[u8]) -> Result<Self, Damage> {
        if !checksum_matches(block) {
            return Err(Damage::Checksum);
        }
        let field =
            |from: usize, len: usize| block.get(from..from.saturating_add(len)).unwrap_or(&[]);
        let size = number(field(124, 12)).ok_or(Damage::Number("size"))?;
        let size = u64::try_from(size).map_err(|_| Damage::Number("size"))?;
        let mtime = number(field(136, 12)).ok_or(Damage::Number("time"))?;
        let mode = number(field(100, 8))
            .and_then(|m| u32::try_from(m & 0o7777).ok())
            .unwrap_or(0);
        let flag = block.get(156).copied().unwrap_or(0);
        let mut name = until_nul(field(0, 100)).to_vec();
        // POSIX ustar puts what does not fit in the name in a prefix. GNU's
        // magic ("ustar  ") uses those bytes for other things.
        if field(257, 6) == b"ustar\0" {
            let prefix = until_nul(field(345, 155));
            if !prefix.is_empty() {
                let mut joined = prefix.to_vec();
                joined.push(b'/');
                joined.extend_from_slice(&name);
                name = joined;
            }
        }
        let link = until_nul(field(157, 100));
        Ok(Self {
            name,
            mode,
            size,
            mtime,
            flag,
            link: (!link.is_empty()).then(|| link.to_vec()),
        })
    }
}

/// Whether a header's checksum field matches its bytes: their sum with the
/// field itself counted as eight spaces. Early tars summed signed bytes,
/// and either sum is taken.
fn checksum_matches(block: &[u8]) -> bool {
    let Some(stored) = block.get(148..156).and_then(number) else {
        return false;
    };
    let (mut unsigned, mut signed) = (0_i64, 0_i64);
    for (i, &b) in block.iter().take(512).enumerate() {
        let b = if (148..156).contains(&i) { b' ' } else { b };
        unsigned = unsigned.saturating_add(i64::from(b));
        signed = signed.saturating_add(i64::from(i8::from_ne_bytes([b])));
    }
    stored == unsigned || stored == signed
}

/// A number field: octal digits, perhaps padded with spaces and ended by a
/// NUL or a space -- or, with its first byte's high bit set, GNU's base-256,
/// big-endian, `0xFF` first for a negative number. `None` for anything else.
fn number(field: &[u8]) -> Option<i64> {
    let (&first, rest) = field.split_first()?;
    if first & 0x80 != 0 {
        // Base-256: the first byte's low bits and the rest, two's complement
        // when the first byte is 0xFF.
        let negative = first == 0xFF;
        let mut value: i64 = if negative {
            -1
        } else {
            i64::from(first & 0x7F)
        };
        for &b in rest {
            value = value.checked_mul(256)?.checked_add(i64::from(b))?;
        }
        return Some(value);
    }
    let text = field
        .iter()
        .skip_while(|&&b| b == b' ')
        .take_while(|&&b| b != 0 && b != b' ');
    // An empty field is 0, as tars that leave one blank mean it.
    let mut value = 0_i64;
    for &b in text {
        let digit = b.checked_sub(b'0').filter(|&d| d < 8)?;
        value = value.checked_mul(8)?.checked_add(i64::from(digit))?;
    }
    Some(value)
}

/// `b` up to its first NUL.
fn until_nul(b: &[u8]) -> &[u8] {
    b.iter()
        .position(|&x| x == 0)
        .map_or(b, |n| b.get(..n).unwrap_or(b))
}

/// The block at `at`, or `None` at the end of the file. A last block cut
/// short is read as far as it goes, padded with zeros.
fn read_block<R: Read + Seek>(r: &mut R, at: u64, len: u64) -> io::Result<Option<Vec<u8>>> {
    if at >= len {
        return Ok(None);
    }
    let room = usize::try_from(len.saturating_sub(at).min(BLOCK)).unwrap_or(0);
    let mut block = vec![0; 512];
    r.seek(SeekFrom::Start(at))?;
    r.read_exact(block.get_mut(..room).unwrap_or(&mut []))?;
    Ok(Some(block))
}

/// `n` bytes at `at`, which the caller has checked the file holds.
fn read_exact_at<R: Read + Seek>(r: &mut R, at: u64, n: u64) -> io::Result<Vec<u8>> {
    let mut buf = vec![0; usize::try_from(n).unwrap_or(0)];
    r.seek(SeekFrom::Start(at))?;
    r.read_exact(&mut buf)?;
    Ok(buf)
}

// ============================================================================
// Writing
// ============================================================================

/// The largest number ustar's twelve-byte fields hold: eleven octal digits.
const USTAR_MAX: u64 = 0o777_7777_7777;

/// A member to write: what its header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewMember<'a> {
    /// Its name in the archive, as bytes. A directory's should end in `/`.
    pub name: &'a [u8],
    pub kind: Kind,
    pub mode: u32,
    /// Seconds since 1970; negative before.
    pub mtime: i64,
    /// The bytes the caller writes after the header: 0 for what has none.
    pub size: u64,
    pub link: Option<&'a [u8]>,
}

/// Write `member`'s header, with a PAX header before it when ustar cannot
/// hold a field. The caller writes `member.size` bytes after it, then
/// [`write_padding`].
///
/// # Errors
///
/// When `w` does.
pub fn write_header<W: Write + ?Sized>(w: &mut W, member: &NewMember) -> io::Result<()> {
    let mut pax = Vec::new();
    let (prefix, name) = match split_name(member.name) {
        Some(split) => split,
        None => {
            pax_record(&mut pax, b"path", member.name);
            (&[][..], member.name.get(..100).unwrap_or(member.name))
        }
    };
    let link = member.link.unwrap_or(&[]);
    if link.len() > 100 {
        pax_record(&mut pax, b"linkpath", link);
    }
    if member.size > USTAR_MAX {
        pax_record(&mut pax, b"size", member.size.to_string().as_bytes());
    }
    let mtime = u64::try_from(member.mtime).ok().filter(|&t| t <= USTAR_MAX);
    if mtime.is_none() {
        pax_record(&mut pax, b"mtime", member.mtime.to_string().as_bytes());
    }
    if !pax.is_empty() {
        let base = member
            .name
            .rsplit(|&b| b == b'/')
            .find(|part| !part.is_empty())
            .unwrap_or(b"member");
        let mut pax_name = b"PaxHeader/".to_vec();
        pax_name.extend_from_slice(base);
        pax_name.truncate(100);
        let len = u64::try_from(pax.len()).unwrap_or(u64::MAX);
        w.write_all(&ustar_block(&pax_name, &[], b'x', 0o644, len, 0, &[]))?;
        w.write_all(&pax)?;
        write_padding(w, len)?;
    }
    let size = member.size.min(USTAR_MAX);
    let block = ustar_block(
        name,
        prefix,
        member.kind.flag(),
        member.mode,
        if member.size > USTAR_MAX { 0 } else { size },
        mtime.unwrap_or(0),
        link.get(..100).unwrap_or(link),
    );
    w.write_all(&block)
}

/// The zeros that pad `size` bytes of a member to a whole block.
///
/// # Errors
///
/// When `w` does.
pub fn write_padding<W: Write + ?Sized>(w: &mut W, size: u64) -> io::Result<()> {
    let rest = size % BLOCK;
    if rest == 0 {
        return Ok(());
    }
    let pad = usize::try_from(BLOCK.saturating_sub(rest)).unwrap_or(0);
    w.write_all(&vec![0; pad])
}

/// The two blocks of zeros that close an archive.
///
/// # Errors
///
/// When `w` does.
pub fn write_end<W: Write + ?Sized>(w: &mut W) -> io::Result<()> {
    w.write_all(&[0; 1024])
}

/// `name` split at a `/` into ustar's 155-byte prefix and 100-byte name, or
/// `None` when no `/` makes both fit. A name that fits the name field whole
/// has no prefix.
fn split_name(name: &[u8]) -> Option<(&[u8], &[u8])> {
    if name.len() <= 100 {
        return Some((&[], name));
    }
    // The leftmost `/` that leaves a name of 100 or less keeps the prefix
    // shortest; it must still be 155 or less, and the name not empty.
    name.iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'/')
        .map(|(i, _)| i)
        .find(|&i| name.len().saturating_sub(i.saturating_add(1)) <= 100)
        .filter(|&i| i <= 155 && i.saturating_add(1) < name.len())
        .and_then(|i| Some((name.get(..i)?, name.get(i.checked_add(1)?..)?)))
}

/// One PAX record onto `out`: `LENGTH KEY=VALUE\n`, the length counting its
/// own digits.
fn pax_record(out: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    // " KEY=VALUE\n" and the digits of a length that includes them.
    let body = key.len().saturating_add(value.len()).saturating_add(3);
    let mut n = body.saturating_add(1);
    while n.to_string().len().saturating_add(body) != n {
        n = n.saturating_add(1);
    }
    out.extend_from_slice(n.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(key);
    out.push(b'=');
    out.extend_from_slice(value);
    out.push(b'\n');
}

/// A ustar header block, its checksum filled in.
fn ustar_block(
    name: &[u8],
    prefix: &[u8],
    flag: u8,
    mode: u32,
    size: u64,
    mtime: u64,
    link: &[u8],
) -> [u8; 512] {
    let mut h = [0_u8; 512];
    let mut put = |at: usize, bytes: &[u8]| {
        for (i, &b) in bytes.iter().enumerate() {
            if let Some(slot) = at.checked_add(i).and_then(|j| h.get_mut(j)) {
                *slot = b;
            }
        }
    };
    put(0, name.get(..100).unwrap_or(name));
    put(100, format!("{:07o}\0", mode & 0o7777).as_bytes());
    put(108, b"0000000\0");
    put(116, b"0000000\0");
    put(124, format!("{size:011o}\0").as_bytes());
    put(136, format!("{mtime:011o}\0").as_bytes());
    put(148, b"        ");
    put(156, &[flag]);
    put(157, link.get(..100).unwrap_or(link));
    put(257, b"ustar\x0000");
    put(329, b"0000000\0");
    put(337, b"0000000\0");
    put(345, prefix.get(..155).unwrap_or(prefix));
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let mut put = |at: usize, bytes: &[u8]| {
        for (i, &b) in bytes.iter().enumerate() {
            if let Some(slot) = at.checked_add(i).and_then(|j| h.get_mut(j)) {
                *slot = b;
            }
        }
    };
    put(148, format!("{sum:06o}\0 ").as_bytes());
    h
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests;
