//! TZif (RFC 8536) binary timezone files — a zero-copy reader.
//!
//! A POSIX `TZ` *string* carries exactly one rule set, so it cannot say that
//! the United States moved the start of daylight saving in 2007, or that
//! Moscow abandoned DST in 2011.  The binary files under `/usr/share/zoneinfo`
//! can: they are a list of transition instants with the offset in force after
//! each.  This module reads those files, and hands anything past the last
//! recorded transition to the [`Tz`] engine in the parent module, because a
//! v2+ TZif file ends with a POSIX `TZ` string for exactly that purpose.  The
//! rules engine therefore becomes the *tail* of this reader rather than being
//! replaced by it.
//!
//! ## Zero-copy, by necessity
//!
//! [`TzFile`] borrows the file bytes and reads fields out of them on demand.
//! It never copies the transition table.  That is not an optimisation — this
//! crate has no allocator (it is linked into the libc, built `no_std`), and a
//! fixed-size inline table would either waste kilobytes per zone or impose an
//! arbitrary cap on transition count that some real zone would eventually
//! exceed.  The caller owns the bytes (a `read`-into-buffer, or an `mmap` of
//! the zoneinfo file) and the borrow checker makes sure the view cannot
//! outlive them.
//!
//! ## Everything is validated once, at parse time
//!
//! A zoneinfo file is attacker-shaped input: `TZ=/path/to/anything` is honoured
//! by every libc, so a user can point us at a file they wrote.  [`TzFile::parse`]
//! therefore checks every structural invariant up front — bounds, counts,
//! designation indices, and that the transition times really are sorted — and
//! returns `None` if any of them fails, so that the lookup path is total and
//! its binary search cannot be steered off the end of the array.  A rejected
//! file falls back to UTC, exactly as glibc does with a file it cannot read.
//!
//! ## What is deliberately ignored
//!
//! * **Leap-second records.** SlateOS's clock is UTC-with-Unix-epoch-seconds
//!   like every other mainstream system, i.e. leap seconds are smeared or
//!   stepped by NTP and never appear in `time_t`. A `right/` zone's leap table
//!   would make `localtime` disagree with `time` by the accumulated 27 s.  The
//!   records are skipped over (their size is still validated, since the block
//!   layout depends on it).
//! * **The standard/wall and UT/local indicator arrays.** They exist to let
//!   `zic` re-derive a POSIX `TZ` string's `/time` fields when recompiling a
//!   binary file back to source; a reader that already has the footer string
//!   has no use for them.
//! * **The v1 data block in a v2+ file**, which by design duplicates the v2
//!   block truncated to 32-bit times.  We read the v2 block, so we skip it.

use crate::{TZ_NAME_CAP, Tz, TzInfo, TzName};

/// Bytes in a TZif header.  v1 and v2+ headers have identical shape.
const HEADER_LEN: usize = 44;

/// Bytes in one local-time-type record: `utoff` (4) + `isdst` (1) + `desigidx` (1).
const TTINFO_LEN: usize = 6;

/// Widest UTC offset a zone may use, rounded up to whole hours.
///
/// RFC 8536 §3.2 caps `utoff` at −25:59:59‥+26:00:00, so every UTC instant that
/// can render as a given wall clock lies within this much of it.  Used to bound
/// the candidate search in [`TzFile::local_to_utc`].
const MAX_OFFSET: i64 = 26 * 3600;

/// How many distinct candidate offsets [`TzFile::local_to_utc`] will consider.
///
/// Two is the real answer (the offsets either side of one transition); the
/// slack absorbs pathological files with several transitions inside one
/// 52-hour window without letting a crafted file make the search unbounded.
const MAX_CANDIDATES: usize = 8;

/// A parsed TZif file: a borrowed view over its transition table.
///
/// See the module documentation for why this borrows rather than owns.
#[derive(Clone, Copy, Debug)]
pub struct TzFile<'a> {
    /// Transition instants, big-endian, [`Self::time_size`] bytes each.
    times: &'a [u8],
    /// One index into [`Self::types`] per transition, giving the state that
    /// takes effect *at* that instant.  Its length is the transition count.
    type_idx: &'a [u8],
    /// Local-time-type records, [`TTINFO_LEN`] bytes each.
    types: &'a [u8],
    /// Zone designations, NUL-separated; a type's `desigidx` points into this.
    desig: &'a [u8],
    /// 4 for a v1 file, 8 for the v2+ data block.
    time_size: usize,
    /// The type to use for instants before the first transition.
    default_type: u8,
    /// The footer's POSIX `TZ` rule, governing instants at or after the last
    /// transition.  Absent for a v1 file, and for a v2+ file whose footer is
    /// empty or unparseable.
    tail: Option<Tz>,
}

impl<'a> TzFile<'a> {
    /// Read a TZif file.
    ///
    /// Returns `None` for anything that is not a structurally valid TZif v1‥v4
    /// file; the caller should fall back to UTC, as glibc does.  For a v2+ file
    /// the 64-bit data block is used and the 32-bit one skipped, so timestamps
    /// outside 1901‥2038 are handled.
    #[must_use]
    pub fn parse(file: &'a [u8]) -> Option<Self> {
        let head = Header::parse(file)?;
        if head.version == 1 {
            // A v1 file is the whole file: one header, one 32-bit data block,
            // no footer and therefore no tail rule.
            let (view, _rest) = Self::data_block(file.get(HEADER_LEN..)?, &head, 4)?;
            return Some(view);
        }

        // v2+: skip the legacy block wholesale.  It is the same data truncated
        // to 32 bits, so reading it would only narrow the range we support.
        let skip = HEADER_LEN.checked_add(head.block_len(4)?)?;
        let head2 = Header::parse(file.get(skip..)?)?;
        // The two headers must agree on version; a file claiming v2 in the
        // first header and v1 in the second is malformed.
        if head2.version < 2 {
            return None;
        }
        let body = skip.checked_add(HEADER_LEN)?;
        let (mut view, rest) = Self::data_block(file.get(body..)?, &head2, 8)?;
        view.tail = Tz::parse(footer_body(rest)?);
        Some(view)
    }

    /// Slice one data block out of `body`, validating every field.
    ///
    /// Returns the view and whatever follows the block (the footer, for a v2+
    /// second block).
    fn data_block(body: &'a [u8], head: &Header, time_size: usize) -> Option<(Self, &'a [u8])> {
        let timecnt = usize::try_from(head.timecnt).ok()?;
        let typecnt = usize::try_from(head.typecnt).ok()?;
        let charcnt = usize::try_from(head.charcnt).ok()?;
        let leapcnt = usize::try_from(head.leapcnt).ok()?;

        let mut off = 0usize;
        let times = take(body, &mut off, timecnt.checked_mul(time_size)?)?;
        let type_idx = take(body, &mut off, timecnt)?;
        let types = take(body, &mut off, typecnt.checked_mul(TTINFO_LEN)?)?;
        let desig = take(body, &mut off, charcnt)?;
        // Leap seconds are deliberately not modelled (see the module docs), but
        // their size still has to be right or everything after them shifts.
        let _leap = take(body, &mut off, leapcnt.checked_mul(time_size.checked_add(4)?)?)?;
        let _isstd = take(body, &mut off, usize::try_from(head.isstdcnt).ok()?)?;
        let _isut = take(body, &mut off, usize::try_from(head.isutcnt).ok()?)?;

        let view = Self {
            times,
            type_idx,
            types,
            desig,
            time_size,
            default_type: 0,
            tail: None,
        };
        let view = Self {
            default_type: view.validate(typecnt)?,
            ..view
        };
        Some((view, body.get(off..)?))
    }

    /// Check every invariant the lookup path assumes, and pick the type to use
    /// before the first transition.
    ///
    /// Done once here so that [`Self::lookup`] can be infallible and total: it
    /// binary-searches a table whose sortedness is checked here, and indexes a
    /// type array whose indices are checked here.
    fn validate(&self, typecnt: usize) -> Option<u8> {
        // Every local-time-type record must be usable, because any transition
        // may name any of them.
        let mut default_type = None;
        for ty in 0..typecnt {
            let rec = self.type_record(ty)?;
            // RFC 8536 §3.2: `utoff` must not be −2^31, which the format
            // reserves; rejecting it also keeps every later negation in range.
            if rec.0 == i32::MIN {
                return None;
            }
            // `isdst` is specified as 0 or 1.  Anything else means the file was
            // not written by `zic`, and guessing at its intent is worse than
            // refusing it.
            if rec.1 > 1 {
                return None;
            }
            // The designation must be a NUL-terminated string inside the
            // designation block, and must fit a `TzName` — a truncated
            // abbreviation would print wrong output forever.
            let idx = usize::from(rec.2);
            let rest = self.desig.get(idx..)?;
            let end = rest.iter().position(|&b| b == 0)?;
            if end > TZ_NAME_CAP {
                return None;
            }
            if default_type.is_none() && rec.1 == 0 {
                default_type = Some(u8::try_from(ty).ok()?);
            }
        }

        // Every transition must name a type that exists, and the instants must
        // be strictly increasing — the binary search in `find` is only correct
        // on a sorted table, and `zic` always emits one.
        let mut prev: Option<i64> = None;
        for (i, &ty) in self.type_idx.iter().enumerate() {
            if usize::from(ty) >= typecnt {
                return None;
            }
            let t = self.transition_time(i)?;
            if prev.is_some_and(|p| t <= p) {
                return None;
            }
            prev = Some(t);
        }

        // With no standard-time type at all, type 0 is the only sane default —
        // and `typecnt >= 1` was checked in the header, so it exists.
        Some(default_type.unwrap_or(0))
    }

    /// The footer's POSIX `TZ` rule, if this file has a usable one.
    #[must_use]
    pub fn tail(&self) -> Option<Tz> {
        self.tail
    }

    /// How many transitions the file records.
    #[must_use]
    pub fn transition_count(&self) -> usize {
        self.type_idx.len()
    }

    /// Whether this zone ever observes daylight saving (the POSIX `daylight`
    /// global).
    ///
    /// True if any recorded state is a DST state, or if the tail rule has a DST
    /// half — a zone that abandoned DST decades ago still answers `true`, which
    /// is what glibc reports, because `daylight` describes the zone's history
    /// and not the current instant.
    #[must_use]
    pub fn has_dst(&self) -> bool {
        let any_dst = (0..self.types.len().saturating_div(TTINFO_LEN))
            .filter_map(|ty| self.type_record(ty))
            .any(|rec| rec.1 == 1);
        any_dst || self.tail.is_some_and(|t| t.has_dst())
    }

    /// The zone state at UTC instant `t` (seconds since the epoch).
    #[must_use]
    pub fn lookup(&self, t: i64) -> TzInfo {
        // Past the last recorded transition the footer rule governs: modern
        // `zic -b slim` stops emitting transitions once the POSIX string
        // describes them, so without this a zone would freeze on whichever
        // side of DST its last transition happened to land.
        if let Some(tail) = self.tail {
            match self.last_transition() {
                Some(last) if t >= last => return tail.lookup(t),
                // A file with no transitions at all is nothing *but* its tail.
                None => return tail.lookup(t),
                Some(_) => {}
            }
        }
        let ty = match self.find(t) {
            Some(i) => self.type_idx.get(i).copied().unwrap_or(self.default_type),
            // Before the first transition, or no transitions and no tail.
            None => self.default_type,
        };
        self.info_of_type(usize::from(ty))
    }

    /// Convert a local wall-clock time to UTC.
    ///
    /// `local` is seconds since the epoch *as if* the broken-down local time
    /// were UTC — what `timegm` returns for the same `struct tm`.  `isdst_hint`
    /// follows `tm_isdst`: negative means "work it out", zero means "standard
    /// time", positive means "daylight time".
    ///
    /// Local time is not a bijection: an hour vanishes at each spring-forward
    /// and repeats at each fall-back.  For the repeated hour we return the
    /// earlier of the two instants — the one still on the pre-transition offset
    /// — unless `isdst_hint` names the other; for the vanished hour, where no
    /// offset is self-consistent, we apply the offset in force *before* the
    /// jump, which lands just after it.  Both match glibc and the POSIX-string
    /// engine in [`Tz::local_to_utc`], so the two paths cannot disagree.
    #[must_use]
    pub fn local_to_utc(&self, local: i64, isdst_hint: i32) -> (i64, TzInfo) {
        let lo = local.saturating_sub(MAX_OFFSET);
        let hi = local.saturating_add(MAX_OFFSET);

        // Collect the offsets that are in force anywhere in the window that
        // could contain the answer.  Probing `lookup` rather than reading the
        // type table directly means the tail rule contributes its offsets too,
        // for a `local` past the last recorded transition.
        let mut cands = [0i32; MAX_CANDIDATES];
        let mut n = 0usize;
        push_offset(&mut cands, &mut n, self.lookup(lo).gmtoff);
        push_offset(&mut cands, &mut n, self.lookup(local).gmtoff);
        push_offset(&mut cands, &mut n, self.lookup(hi).gmtoff);
        // Plus the offsets either side of every recorded transition inside the
        // window, which is what catches a zone whose offset changed by more
        // than the window's own span.
        if let Some(mut i) = self.find(hi) {
            while n < MAX_CANDIDATES {
                let Some(tr) = self.transition_time(i) else { break };
                if tr <= lo {
                    break;
                }
                push_offset(&mut cands, &mut n, self.lookup(tr).gmtoff);
                push_offset(&mut cands, &mut n, self.lookup(tr.saturating_sub(1)).gmtoff);
                let Some(prev) = i.checked_sub(1) else { break };
                i = prev;
            }
        }

        // A candidate offset is a real answer only if the instant it produces
        // is itself governed by that offset.  Rank the ones that are: honour
        // `isdst_hint` first, then prefer the earlier instant.
        let mut best: Option<((u8, i64), (i64, TzInfo))> = None;
        let mut fallback: Option<(i64, TzInfo)> = None;
        for &off in cands.get(..n).unwrap_or(&[]) {
            let t = local.saturating_sub(i64::from(off));
            let info = self.lookup(t);
            if fallback.is_none() {
                // `cands[0]` is the offset at `lo`, i.e. the one in force
                // before any transition in the window — the vanished-hour
                // answer.
                fallback = Some((t, info));
            }
            if info.gmtoff != off {
                continue;
            }
            let mismatch = match isdst_hint.signum() {
                1 => u8::from(!info.is_dst),
                0 => u8::from(info.is_dst),
                _ => 0,
            };
            let key = (mismatch, t);
            let better = match best {
                None => true,
                Some((prev_key, _)) => key < prev_key,
            };
            if better {
                best = Some((key, (t, info)));
            }
        }

        if let Some((_, answer)) = best {
            return answer;
        }
        fallback.unwrap_or_else(|| {
            // Unreachable in practice: `n >= 1` always, since three offsets are
            // pushed unconditionally.  Answering UTC beats a panic in a libc.
            (local, TzInfo { gmtoff: 0, is_dst: false, name: TzName::UTC })
        })
    }

    /// The index of the last transition at or before `t`, or `None` if `t`
    /// precedes the first (or there are none).
    fn find(&self, t: i64) -> Option<usize> {
        let n = self.transition_count();
        let mut hi = n.checked_sub(1)?;
        if self.transition_time(0)? > t {
            return None;
        }
        // Invariant: `times[lo] <= t`, and `times[hi + 1] > t` or `hi + 1 == n`.
        // Validated-sorted in `validate`, so this converges on the right slot.
        let mut lo = 0usize;
        while lo < hi {
            let mid = lo.checked_add(hi.checked_sub(lo)?.checked_add(1)? / 2)?;
            if self.transition_time(mid)? <= t {
                lo = mid;
            } else {
                hi = mid.checked_sub(1)?;
            }
        }
        Some(lo)
    }

    /// The last recorded transition instant, if any.
    fn last_transition(&self) -> Option<i64> {
        self.transition_time(self.transition_count().checked_sub(1)?)
    }

    /// Transition `i`'s instant, widening a v1 file's 32-bit value.
    fn transition_time(&self, i: usize) -> Option<i64> {
        let off = i.checked_mul(self.time_size)?;
        let raw = self.times.get(off..off.checked_add(self.time_size)?)?;
        if self.time_size == 8 {
            Some(i64::from_be_bytes(raw.try_into().ok()?))
        } else {
            Some(i64::from(i32::from_be_bytes(raw.try_into().ok()?)))
        }
    }

    /// Local-time-type record `ty` as `(utoff, isdst, desigidx)`.
    fn type_record(&self, ty: usize) -> Option<(i32, u8, u8)> {
        let off = ty.checked_mul(TTINFO_LEN)?;
        let rec = self.types.get(off..off.checked_add(TTINFO_LEN)?)?;
        let utoff = i32::from_be_bytes(rec.get(0..4)?.try_into().ok()?);
        Some((utoff, *rec.get(4)?, *rec.get(5)?))
    }

    /// Render local-time-type `ty` as a [`TzInfo`].
    ///
    /// Total by construction: `validate` proved every type in the file decodes,
    /// and every caller passes an index that came out of the validated tables.
    /// The UTC fallbacks are unreachable, and are there so that a libc lookup
    /// cannot panic even if a future change breaks that reasoning.
    fn info_of_type(&self, ty: usize) -> TzInfo {
        let Some((gmtoff, isdst, idx)) = self.type_record(ty) else {
            return TzInfo { gmtoff: 0, is_dst: false, name: TzName::UTC };
        };
        TzInfo {
            gmtoff,
            is_dst: isdst != 0,
            name: self.name_at(usize::from(idx)).unwrap_or(TzName::UTC),
        }
    }

    /// The NUL-terminated designation starting at `idx` in the designation
    /// block.
    fn name_at(&self, idx: usize) -> Option<TzName> {
        let rest = self.desig.get(idx..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        TzName::new(rest.get(..end)?)
    }
}

/// Add `off` to the candidate set, ignoring duplicates and overflow.
fn push_offset(cands: &mut [i32; MAX_CANDIDATES], n: &mut usize, off: i32) {
    if cands.get(..*n).is_some_and(|seen| seen.contains(&off)) {
        return;
    }
    if let Some(slot) = cands.get_mut(*n) {
        *slot = off;
        *n = n.saturating_add(1);
    }
}

/// Advance `off` by `len` and return the bytes skipped over.
fn take<'a>(body: &'a [u8], off: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = off.checked_add(len)?;
    let out = body.get(*off..end)?;
    *off = end;
    Some(out)
}

/// The bytes between a v2+ footer's two newlines.
///
/// RFC 8536 §3.3 makes the footer mandatory for v2+, so its absence means the
/// file was truncated and [`TzFile::parse`] rejects the whole thing — that is
/// the only structural check that can catch a file cut off exactly at the end
/// of its data block, where every count still adds up.
///
/// The body itself may be empty (`"\n\n"`, which is legal and means the zone
/// has no rule past its last transition); an empty or unparseable body yields
/// no tail rule, leaving the last recorded transition to stand forever, which
/// is a better answer than refusing an otherwise complete file.
fn footer_body(rest: &[u8]) -> Option<&[u8]> {
    let body = rest.strip_prefix(b"\n")?;
    let end = body.iter().position(|&b| b == b'\n')?;
    body.get(..end)
}

/// A TZif header, v1 or v2+.
struct Header {
    /// 1 for a `'\0'` version byte, else the ASCII digit's value.
    version: u8,
    isutcnt: u32,
    isstdcnt: u32,
    leapcnt: u32,
    timecnt: u32,
    typecnt: u32,
    charcnt: u32,
}

impl Header {
    /// Parse the 44-byte header at the start of `b`.
    fn parse(b: &[u8]) -> Option<Self> {
        if b.get(0..4)? != b"TZif" {
            return None;
        }
        let version = match *b.get(4)? {
            0 => 1,
            v @ (b'2' | b'3' | b'4') => v.checked_sub(b'0')?,
            // A version we do not know might reorder the data block, and
            // guessing at its layout would silently produce wrong times.
            _ => return None,
        };
        let head = Self {
            version,
            isutcnt: be_u32(b, 20)?,
            isstdcnt: be_u32(b, 24)?,
            leapcnt: be_u32(b, 28)?,
            timecnt: be_u32(b, 32)?,
            typecnt: be_u32(b, 36)?,
            charcnt: be_u32(b, 40)?,
        };
        // RFC 8536 §3.1: `typecnt` and `charcnt` must be nonzero, and the two
        // indicator arrays are either absent or one entry per type.  A file
        // with zero types has no offset to report at all.
        if head.typecnt == 0 || head.charcnt == 0 {
            return None;
        }
        if head.isstdcnt != 0 && head.isstdcnt != head.typecnt {
            return None;
        }
        if head.isutcnt != 0 && head.isutcnt != head.typecnt {
            return None;
        }
        Some(head)
    }

    /// Bytes in a data block with `time_size`-byte instants.
    fn block_len(&self, time_size: usize) -> Option<usize> {
        let timecnt = usize::try_from(self.timecnt).ok()?;
        let mut len = timecnt.checked_mul(time_size.checked_add(1)?)?;
        len = len.checked_add(usize::try_from(self.typecnt).ok()?.checked_mul(TTINFO_LEN)?)?;
        len = len.checked_add(usize::try_from(self.charcnt).ok()?)?;
        len = len.checked_add(
            usize::try_from(self.leapcnt).ok()?.checked_mul(time_size.checked_add(4)?)?,
        )?;
        len = len.checked_add(usize::try_from(self.isstdcnt).ok()?)?;
        len.checked_add(usize::try_from(self.isutcnt).ok()?)
    }
}

/// Read a big-endian `u32` at `off`.
fn be_u32(b: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    Some(u32::from_be_bytes(b.get(off..end)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-capacity byte builder, so these tests can assemble real TZif
    /// files without `Vec` — this crate has no allocator even under test.
    struct Buf {
        bytes: [u8; 1024],
        len: usize,
    }

    impl Buf {
        fn new() -> Self {
            Self { bytes: [0; 1024], len: 0 }
        }
        fn put(&mut self, s: &[u8]) -> &mut Self {
            self.bytes[self.len..self.len + s.len()].copy_from_slice(s);
            self.len += s.len();
            self
        }
        fn u32(&mut self, v: u32) -> &mut Self {
            self.put(&v.to_be_bytes())
        }
        fn i64(&mut self, v: i64) -> &mut Self {
            self.put(&v.to_be_bytes())
        }
        fn ttinfo(&mut self, utoff: i32, isdst: u8, idx: u8) -> &mut Self {
            self.put(&utoff.to_be_bytes());
            self.put(&[isdst, idx])
        }
        fn as_slice(&self) -> &[u8] {
            &self.bytes[..self.len]
        }
    }

    /// A header with the counts spelled out.
    fn header(buf: &mut Buf, version: u8, timecnt: u32, typecnt: u32, charcnt: u32) {
        buf.put(b"TZif");
        buf.put(&[version]);
        buf.put(&[0; 15]);
        buf.u32(0); // isutcnt
        buf.u32(0); // isstdcnt
        buf.u32(0); // leapcnt
        buf.u32(timecnt);
        buf.u32(typecnt);
        buf.u32(charcnt);
    }

    /// The designation block used by the US-eastern fixtures: `"EST\0EDT\0"`.
    const EASTERN_DESIG: &[u8] = b"EST\0EDT\0";

    /// A v2 file for a US-eastern-like zone with two recorded transitions in
    /// 2020 and a `M3.2.0,M11.1.0` tail — the shape `zic -b slim` emits.
    ///
    /// The v1 block carries zero transitions, exactly as a slim file's does.
    fn eastern() -> Buf {
        let mut b = Buf::new();
        // --- v1 block: one type, no transitions.
        header(&mut b, b'2', 0, 2, 8);
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        // --- v2 block.
        header(&mut b, b'2', 2, 2, 8);
        b.i64(1_583_650_800); // 2020-03-08 07:00 UTC — EST -> EDT
        b.i64(1_604_210_400); // 2020-11-01 06:00 UTC — EDT -> EST
        b.put(&[1, 0]);
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        b.put(b"\nEST5EDT,M3.2.0,M11.1.0\n");
        b
    }

    fn name(s: &[u8]) -> TzName {
        TzName::new(s).expect("test name fits")
    }

    #[test]
    fn reads_the_v2_block_and_its_footer() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        assert_eq!(tz.transition_count(), 2);
        assert_eq!(tz.tail(), Tz::parse(b"EST5EDT,M3.2.0,M11.1.0"));
        assert!(tz.has_dst());
    }

    #[test]
    fn an_instant_between_two_transitions_uses_the_recorded_type() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        // 2020-07-01, squarely inside the recorded DST period.
        let info = tz.lookup(1_593_561_600);
        assert_eq!(info.gmtoff, -4 * 3600);
        assert!(info.is_dst);
        assert_eq!(info.name, name(b"EDT"));
    }

    #[test]
    fn a_transition_takes_effect_at_its_own_instant() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        assert!(!tz.lookup(1_583_650_799).is_dst);
        assert!(tz.lookup(1_583_650_800).is_dst);
    }

    #[test]
    fn an_instant_before_the_first_transition_uses_the_first_standard_type() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        // 1970 — long before anything the file records.
        let info = tz.lookup(0);
        assert_eq!(info.gmtoff, -5 * 3600);
        assert!(!info.is_dst);
        assert_eq!(info.name, name(b"EST"));
    }

    #[test]
    fn the_footer_rule_governs_instants_past_the_last_transition() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        // 2030-07-01 — a decade past the last recorded transition.  Without the
        // tail rule this would freeze on the last recorded state (EST) and
        // report standard time in the middle of summer.
        let info = tz.lookup(1_909_267_200);
        assert!(info.is_dst, "tail rule must resume DST");
        assert_eq!(info.gmtoff, -4 * 3600);
        // 2030-01-01, the other side of the tail rule's own transitions.
        assert!(!tz.lookup(1_893_456_000).is_dst);
    }

    #[test]
    fn a_repeated_local_hour_resolves_to_the_earlier_instant() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        // The clock goes back at 06:00 UTC, so 2020-11-01 01:30 local happens
        // twice: at 05:30 UTC still on EDT, and again at 06:30 UTC on EST.
        let local = 1_604_194_200; // 2020-11-01 01:30 as if UTC
        let (t, info) = tz.local_to_utc(local, -1);
        assert_eq!(t, 1_604_208_600, "the EDT reading is the earlier one");
        assert!(info.is_dst);
        // An explicit `tm_isdst == 0` asks for the later, standard-time one.
        let (t_std, info_std) = tz.local_to_utc(local, 0);
        assert_eq!(t_std, 1_604_212_200);
        assert!(!info_std.is_dst);
    }

    #[test]
    fn a_vanished_local_hour_lands_just_past_the_jump() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        // 2020-03-08 02:30 local never happens: the clock goes 01:59:59 EST to
        // 03:00:00 EDT.  Applying the pre-jump (EST) offset puts it at 07:30
        // UTC, i.e. 03:30 EDT — just past the jump, as glibc does.
        let local = 1_583_634_600; // 2020-03-08 02:30 as if UTC
        let (t, _) = tz.local_to_utc(local, -1);
        assert_eq!(t, local + 5 * 3600);
    }

    #[test]
    fn an_unambiguous_local_time_round_trips() {
        let f = eastern();
        let tz = TzFile::parse(f.as_slice()).expect("valid TZif");
        for utc in [0_i64, 1_593_561_600, 1_909_267_200, -1_000_000_000] {
            let info = tz.lookup(utc);
            let local = utc + i64::from(info.gmtoff);
            let (back, _) = tz.local_to_utc(local, -1);
            assert_eq!(back, utc, "round trip failed for {utc}");
        }
    }

    #[test]
    fn a_v1_file_is_read_with_32_bit_times_and_no_tail() {
        let mut b = Buf::new();
        header(&mut b, 0, 1, 2, 8);
        b.put(&1_583_650_800_i32.to_be_bytes());
        b.put(&[1]);
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        let tz = TzFile::parse(b.as_slice()).expect("valid v1 TZif");
        assert!(tz.tail().is_none());
        assert_eq!(tz.lookup(0).gmtoff, -5 * 3600);
        // With no tail, the last recorded state stands forever.
        assert!(tz.lookup(1_909_267_200).is_dst);
    }

    #[test]
    fn a_zone_with_no_dst_reports_none() {
        let mut b = Buf::new();
        header(&mut b, b'2', 0, 1, 4);
        b.ttinfo(9 * 3600, 0, 0);
        b.put(b"JST\0");
        header(&mut b, b'2', 0, 1, 4);
        b.ttinfo(9 * 3600, 0, 0);
        b.put(b"JST\0");
        b.put(b"\nJST-9\n");
        let tz = TzFile::parse(b.as_slice()).expect("valid TZif");
        assert!(!tz.has_dst());
        assert_eq!(tz.lookup(0).gmtoff, 9 * 3600);
        assert_eq!(tz.lookup(1_909_267_200).name, name(b"JST"));
        // A file with no transitions is nothing but its tail.
        assert_eq!(tz.local_to_utc(9 * 3600, -1).0, 0);
    }

    #[test]
    fn an_empty_footer_is_accepted_and_leaves_no_tail() {
        let mut b = Buf::new();
        header(&mut b, b'2', 0, 1, 4);
        b.ttinfo(0, 0, 0);
        b.put(b"UTC\0");
        header(&mut b, b'2', 0, 1, 4);
        b.ttinfo(0, 0, 0);
        b.put(b"UTC\0");
        b.put(b"\n\n");
        let tz = TzFile::parse(b.as_slice()).expect("valid TZif");
        assert!(tz.tail().is_none());
        assert_eq!(tz.lookup(123_456).gmtoff, 0);
    }

    #[test]
    fn a_file_that_is_not_tzif_is_rejected() {
        assert!(TzFile::parse(b"").is_none());
        assert!(TzFile::parse(b"TZif").is_none());
        assert!(TzFile::parse(&[0; HEADER_LEN]).is_none());
        let mut b = Buf::new();
        header(&mut b, b'9', 0, 1, 4);
        assert!(TzFile::parse(b.as_slice()).is_none(), "unknown version");
    }

    #[test]
    fn a_truncated_data_block_is_rejected() {
        let f = eastern();
        let full = f.as_slice();
        // Every prefix short of the whole file must be refused rather than
        // read past its end — this is the property that makes the lookup path
        // safe on a file the user pointed us at.
        // Every prefix short of the whole file must be refused rather than
        // read past its end — this is the property that makes the lookup path
        // safe on a file the user pointed us at.  A prefix cut exactly at the
        // end of the data block is caught only because the v2+ footer is
        // mandatory; every count still adds up at that point.
        for cut in 0..full.len() {
            let short = &full[..cut];
            assert!(TzFile::parse(short).is_none(), "prefix of {cut} bytes accepted");
        }
        assert!(TzFile::parse(full).is_some());
    }

    #[test]
    fn a_transition_naming_a_nonexistent_type_is_rejected() {
        let mut b = Buf::new();
        header(&mut b, b'2', 0, 2, 8);
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        header(&mut b, b'2', 1, 2, 8);
        b.i64(1_583_650_800);
        b.put(&[7]); // only types 0 and 1 exist
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        b.put(b"\nEST5EDT,M3.2.0,M11.1.0\n");
        assert!(TzFile::parse(b.as_slice()).is_none());
    }

    #[test]
    fn an_unsorted_transition_table_is_rejected() {
        let mut b = Buf::new();
        header(&mut b, b'2', 0, 2, 8);
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        header(&mut b, b'2', 2, 2, 8);
        b.i64(1_604_210_400); // out of order …
        b.i64(1_583_650_800); // … so the binary search would be wrong
        b.put(&[1, 0]);
        b.ttinfo(-5 * 3600, 0, 0);
        b.ttinfo(-4 * 3600, 1, 4);
        b.put(EASTERN_DESIG);
        b.put(b"\nEST5EDT,M3.2.0,M11.1.0\n");
        assert!(TzFile::parse(b.as_slice()).is_none());
    }

    #[test]
    fn a_designation_index_past_the_block_is_rejected() {
        let mut b = Buf::new();
        header(&mut b, b'2', 0, 1, 4);
        b.ttinfo(0, 0, 9); // block is only 4 bytes
        b.put(b"UTC\0");
        assert!(TzFile::parse(b.as_slice()).is_none());
    }

    #[test]
    fn an_unterminated_designation_is_rejected() {
        let mut b = Buf::new();
        header(&mut b, 0, 0, 1, 3);
        b.ttinfo(0, 0, 0);
        b.put(b"UTC"); // no NUL
        assert!(TzFile::parse(b.as_slice()).is_none());
    }

    #[test]
    fn a_type_with_a_nonsense_isdst_flag_is_rejected() {
        let mut b = Buf::new();
        header(&mut b, 0, 0, 1, 4);
        b.ttinfo(0, 2, 0);
        b.put(b"UTC\0");
        assert!(TzFile::parse(b.as_slice()).is_none());
    }

    #[test]
    fn a_type_count_of_zero_is_rejected() {
        let mut b = Buf::new();
        header(&mut b, 0, 0, 0, 4);
        b.put(b"UTC\0");
        assert!(TzFile::parse(b.as_slice()).is_none());
    }

    #[test]
    fn indicator_arrays_are_accounted_for_in_the_block_length() {
        // `zic` writes `isstdcnt == isutcnt == typecnt`; if we did not skip
        // them the v1 block length would be wrong and the second header would
        // be looked for in the middle of the first block.
        let mut b = Buf::new();
        b.put(b"TZif");
        b.put(&[b'2']);
        b.put(&[0; 15]);
        b.u32(1); // isutcnt
        b.u32(1); // isstdcnt
        b.u32(0);
        b.u32(0); // timecnt
        b.u32(1); // typecnt
        b.u32(4); // charcnt
        b.ttinfo(0, 0, 0);
        b.put(b"UTC\0");
        b.put(&[1, 1]);
        // Second header, same shape.
        b.put(b"TZif");
        b.put(&[b'2']);
        b.put(&[0; 15]);
        b.u32(1);
        b.u32(1);
        b.u32(0);
        b.u32(0);
        b.u32(1);
        b.u32(4);
        b.ttinfo(0, 0, 0);
        b.put(b"UTC\0");
        b.put(&[1, 1]);
        b.put(b"\nUTC0\n");
        let tz = TzFile::parse(b.as_slice()).expect("valid TZif with indicators");
        assert_eq!(tz.lookup(0).name, name(b"UTC"));
    }

    #[test]
    fn leap_second_records_are_skipped_not_applied() {
        // A `right/`-style file: the leap table must not shift `time_t`, but
        // its size still has to be accounted for or the designation block
        // would be misread.
        let mut b = Buf::new();
        b.put(b"TZif");
        b.put(&[0]);
        b.put(&[0; 15]);
        b.u32(0);
        b.u32(0);
        b.u32(1); // leapcnt
        b.u32(0);
        b.u32(1);
        b.u32(4);
        b.ttinfo(0, 0, 0);
        b.put(b"UTC\0");
        b.put(&78_796_800_i32.to_be_bytes()); // 1972-07-01
        b.u32(1); // correction
        let tz = TzFile::parse(b.as_slice()).expect("valid v1 TZif with a leap record");
        assert_eq!(tz.lookup(100_000_000).gmtoff, 0);
        assert_eq!(tz.lookup(100_000_000).name, name(b"UTC"));
    }
}
