//! libmagic's `is_tar.c`: a tar header, by its checksum.
//!
//! Tested before the magic rules, because a tar file whose first member's name
//! starts with a dot would otherwise read as troff input.

use crate::buffer::Buffer;
use crate::cstd::isspace;
use crate::funcs::Ms;
use crate::magic::{MAGIC_APPLE, MAGIC_EXTENSION, MAGIC_MIME, MAGIC_MIME_ENCODING};

/// `tartype`: what each kind of header is called -- the same words as the
/// rules in `Magdir/archive`.
const TARTYPE: [&[u8]; 3] = [b"tar archive", b"POSIX tar archive", b"POSIX tar archive (GNU)"];

/// `RECORDSIZE`.
const RECORDSIZE: usize = 512;
/// Where `struct header`'s fields are.
const NAME: usize = 0;
const NAMSIZ: usize = 100;
const CHKSUM: usize = 148;
const MAGIC_AT: usize = 257;

/// `file_is_tar`.
pub fn file_is_tar(ms: &mut Ms, b: &Buffer<'_>) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let tar = is_tar(b.fbuf);
    if !(1..=3).contains(&tar) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let what: &[u8] = if mime != 0 {
        b"application/x-tar"
    } else {
        TARTYPE[usize::try_from(tar - 1).unwrap_or(0)]
    };
    if ms.print_str(what) == -1 {
        return -1;
    }
    1
}

/// `is_tar`: 0 not a tar header (by its checksum), 1 old Unix tar, 2 POSIX,
/// 3 GNU.
fn is_tar(buf: &[u8]) -> i32 {
    if buf.len() < RECORDSIZE {
        return 0;
    }
    let h = &buf[..RECORDSIZE];
    // A Gentoo GLEP 78 package (a name ending `/gpkg-1`) is left to the rules.
    let name = &h[NAME..NAME + NAMSIZ];
    if let Some(nul) = name.iter().position(|&c| c == 0) {
        if nul >= 7 && &name[nul - 7..nul] == b"/gpkg-1" {
            return 0;
        }
    }
    let recsum = from_oct(buf, CHKSUM, 8);
    let mut sum: i32 = h.iter().map(|&c| i32::from(c)).sum();
    // The checksum field counts as blanks -- and is subtracted as the signed
    // `char`s it is declared as.
    for &c in &h[CHKSUM..CHKSUM + 8] {
        #[allow(clippy::cast_possible_wrap)]
        {
            sum -= i32::from(c as i8);
        }
    }
    sum += i32::from(b' ') * 8;
    if sum != recsum {
        return 0;
    }
    let magic = &h[MAGIC_AT..MAGIC_AT + 8];
    if strncmp8(magic, b"ustar  \0") {
        return 3;
    }
    if strncmp8(magic, b"ustar\0") {
        return 2;
    }
    1
}

/// `strncmp(a, b, 8) == 0`, both as C strings.
fn strncmp8(a: &[u8], b: &[u8]) -> bool {
    for i in 0..8 {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

/// `from_oct`: the octal number in the `digs`-byte field at `at`, or -1 for a
/// field that is all blank or ends on something other than a space or NUL.
/// Upstream's blank-skipping loop checks its count only after moving, so an
/// all-blank field reads the byte after it; this reads the same byte.
fn from_oct(buf: &[u8], at: usize, digs: usize) -> i32 {
    let c = |i: usize| buf.get(i).copied().unwrap_or(0);
    if digs == 0 {
        return -1;
    }
    let mut w = at;
    let mut digs = digs;
    while isspace(c(w)) {
        w += 1;
        if digs == 0 {
            return -1;
        }
        digs -= 1;
    }
    let mut value: i32 = 0;
    while digs > 0 && (b'0'..=b'7').contains(&c(w)) {
        value = (value << 3) | i32::from(c(w) - b'0');
        w += 1;
        digs -= 1;
    }
    if digs > 0 && c(w) != 0 && !isspace(c(w)) {
        return -1;
    }
    value
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn header(magic: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[..5].copy_from_slice(b"a.txt");
        h[MAGIC_AT..MAGIC_AT + magic.len()].copy_from_slice(magic);
        h[CHKSUM..CHKSUM + 8].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&c| u32::from(c)).sum();
        let s = format!("{sum:06o}\0 ");
        h[CHKSUM..CHKSUM + 8].copy_from_slice(s.as_bytes());
        h
    }

    #[test]
    fn the_three_kinds_by_their_magic() {
        assert_eq!(is_tar(&header(b"ustar\x0000")), 2);
        assert_eq!(is_tar(&header(b"ustar  \0")), 3);
        assert_eq!(is_tar(&header(b"")), 1);
        let mut bad = header(b"ustar\x0000");
        bad[0] = b'b';
        assert_eq!(is_tar(&bad), 0);
        assert_eq!(is_tar(&[0u8; 100]), 0);
    }

    #[test]
    fn octal_fields_read_as_upstream_reads_them() {
        assert_eq!(from_oct(b"  0644 \0x", 0, 8), 0o644);
        assert_eq!(from_oct(b"0644x   ", 0, 8), -1);
        // All blank, and a non-blank byte after: zero, not -1.
        assert_eq!(from_oct(b"        Z", 0, 8), 0);
        assert_eq!(from_oct(b"         ", 0, 8), -1);
    }
}
