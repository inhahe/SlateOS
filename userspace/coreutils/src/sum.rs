//! The BSD and System V checksums: upstream's `src/sum.c`.
//!
//! Upstream links this one file into two programs — `sum`, whose whole job it
//! is, and `cksum`, whose `-a bsd` and `-a sysv` print exactly what `sum -r`
//! and `sum -s` do. So it is a module here rather than private to either bin,
//! for the reason every shared module in this crate is: two copies of a
//! checksum are two chances for two programs to print different numbers for
//! the same file, which is the one thing a checksum may not do.
//!
//! Neither algorithm is a cryptographic hash. One is a 16-bit rotate-and-add,
//! the other a byte sum folded to 16 bits.
//!
//! * **BSD** (`sum -r`, `cksum -a bsd`): for each byte, rotate the 16-bit
//!   checksum right by one bit and add the byte. Printed `%05d %5s`, the size
//!   in 1024-byte blocks rounded up.
//! * **System V** (`sum -s`, `cksum -a sysv`): the sum of every byte modulo
//!   2^32, folded twice to 16 bits. Printed `%d %s`, in 512-byte blocks rounded
//!   up.
//!
//! The name, when there is one, is printed as its bytes — `printf (" %s",
//! file)` — because this is data, not a diagnostic, and quoting it would name a
//! different file.

/// `bsd_sum_stream`'s checksum: rotate right one bit within 16, then add.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bsd {
    checksum: u16,
}

impl Bsd {
    /// Absorb the next bytes.
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            // `(checksum >> 1) + ((checksum & 1) << 15)`, then `+= byte` and
            // `&= 0xffff`: a 16-bit rotation and a 16-bit wrapping add.
            self.checksum = self.checksum.rotate_right(1).wrapping_add(u16::from(byte));
        }
    }

    /// The checksum so far.
    #[must_use]
    pub const fn checksum(&self) -> u16 {
        self.checksum
    }
}

/// `sysv_sum_stream`'s checksum: every byte summed in an `unsigned int`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sysv {
    sum: u32,
}

impl Sysv {
    /// Absorb the next bytes.
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.sum = self.sum.wrapping_add(u32::from(byte));
        }
    }

    /// `r = (s & 0xffff) + (s >> 16); checksum = (r & 0xffff) + (r >> 16)`.
    ///
    /// Neither sum can overflow: `r` is at most `0x1fffe`, and the checksum at
    /// most `0xffff` — which is why the result is a `u16`.
    #[must_use]
    pub fn checksum(&self) -> u16 {
        let r = (self.sum & 0xffff).wrapping_add(self.sum >> 16);
        let folded = (r & 0xffff).wrapping_add(r >> 16);
        u16::try_from(folded).unwrap_or(u16::MAX)
    }
}

/// `output_bsd`: `%05d %5s`, the size in 1024-byte blocks rounded up, the name
/// if one was named, and `delim`.
///
/// `raw` is `cksum --raw`: the checksum alone, as two big-endian bytes, and
/// nothing else — no size, no name, no terminator.
#[must_use]
pub fn output_bsd(
    checksum: u16,
    length: u64,
    name: Option<&[u8]>,
    raw: bool,
    delim: u8,
) -> Vec<u8> {
    if raw {
        return checksum.to_be_bytes().to_vec();
    }
    line(
        format!("{checksum:05} {:>5}", length.div_ceil(1024)),
        name,
        delim,
    )
}

/// `output_sysv`: `%d %s`, the size in 512-byte blocks rounded up, the name
/// if one was named, and `delim`. `raw` as for [`output_bsd`].
#[must_use]
pub fn output_sysv(
    checksum: u16,
    length: u64,
    name: Option<&[u8]>,
    raw: bool,
    delim: u8,
) -> Vec<u8> {
    if raw {
        return checksum.to_be_bytes().to_vec();
    }
    line(format!("{checksum} {}", length.div_ceil(512)), name, delim)
}

/// The common tail of both: `if (args) printf (" %s", file); putchar (delim);`
fn line(head: String, name: Option<&[u8]>, delim: u8) -> Vec<u8> {
    let mut out = head.into_bytes();
    if let Some(name) = name {
        out.push(b' ');
        out.extend_from_slice(name);
    }
    out.push(delim);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn bsd(data: &[u8]) -> u16 {
        let mut b = Bsd::default();
        b.update(data);
        b.checksum()
    }

    fn sysv(data: &[u8]) -> u16 {
        let mut s = Sysv::default();
        s.update(data);
        s.checksum()
    }

    /// Values measured from GNU 9.4's `sum` and `sum -s`.
    #[test]
    fn checksums_match_upstream() {
        assert_eq!(bsd(b"hello\n"), 36979);
        assert_eq!(sysv(b"hello\n"), 542);
        assert_eq!(bsd(b""), 0);
        assert_eq!(sysv(b""), 0);
    }

    /// Feeding in pieces is feeding in one: the rotation carries across.
    #[test]
    fn chunking_does_not_change_the_answer() {
        let data: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
        let mut b = Bsd::default();
        let mut s = Sysv::default();
        for piece in data.chunks(333) {
            b.update(piece);
            s.update(piece);
        }
        assert_eq!(b.checksum(), bsd(&data));
        assert_eq!(s.checksum(), sysv(&data));
    }

    /// The fold: a sum past 16 bits comes back into range twice.
    #[test]
    fn the_sysv_fold_handles_carries() {
        assert_eq!(Sysv { sum: 0xffff_ffff }.checksum(), 0xffff);
        assert_eq!(Sysv { sum: 0x0001_ffff }.checksum(), 1);
    }

    #[test]
    fn lines_are_upstreams_two_formats() {
        assert_eq!(
            output_bsd(36979, 6, Some(b"s1"), false, b'\n'),
            b"36979     1 s1\n"
        );
        assert_eq!(output_bsd(7, 70_000, None, false, b'\n'), b"00007    69\n");
        assert_eq!(output_sysv(542, 6, Some(b"-"), false, b'\n'), b"542 1 -\n");
        assert_eq!(output_sysv(0, 0, None, false, b'\n'), b"0 0\n");
        // A name is data here, byte for byte.
        assert_eq!(
            output_sysv(1, 1, Some(b"a b\xff"), false, b'\n'),
            b"1 1 a b\xff\n"
        );
    }

    /// `cksum -a bsd -z` and `cksum -a sysv --raw`.
    #[test]
    fn the_cksum_variants() {
        assert_eq!(output_bsd(7, 1, None, false, 0), b"00007     1\0");
        assert_eq!(output_sysv(0x0102, 1, Some(b"f"), true, b'\n'), [1, 2]);
        assert_eq!(output_bsd(0xabcd, 1, Some(b"f"), true, b'\n'), [0xab, 0xcd]);
    }
}
