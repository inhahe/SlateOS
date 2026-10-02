//! libmagic's `is_simh.c`: a SIMH tape image -- records framed by their
//! little-endian lengths, before and after, and tape marks of zero.

use crate::buffer::Buffer;
use crate::funcs::Ms;
use crate::magic::{MAGIC_APPLE, MAGIC_EXTENSION, MAGIC_MIME, MAGIC_MIME_ENCODING};

/// `SIMH_TAPEMARKS`: how many tape marks are looked at.
const SIMH_TAPEMARKS: usize = 10;

/// `getlen`: a record length -- even, 24 bits -- or `0xffffffff`, the end of
/// the medium.
fn getlen(s: &[u8], uc: &mut i64) -> u32 {
    let at = usize::try_from(*uc).unwrap_or(usize::MAX);
    let mut w = [0u8; 4];
    if let Some(src) = s.get(at..at.saturating_add(4)) {
        w.copy_from_slice(src);
    }
    *uc += 4;
    let mut n = u32::from_le_bytes(w);
    if n == 0xffff_ffff {
        return n;
    }
    n &= 0x00ff_ffff;
    if n & 1 == 1 {
        n += 1;
    }
    n
}

/// `simh_parse`.
fn simh_parse(s: &[u8]) -> bool {
    #[allow(clippy::cast_possible_wrap)]
    let ue = s.len() as i64;
    let mut uc: i64 = 0;
    let (mut nt, mut nr) = (0usize, 0usize);
    while ue - uc >= 4 {
        let nbytes = getlen(s, &mut uc);
        if (nt > 0 || nr > 0) && nbytes == 0xffff_ffff {
            // The end of the medium, after a record or a tape mark.
            break;
        }
        if nbytes == 0 {
            nt += 1;
            if nt == SIMH_TAPEMARKS {
                break;
            }
            continue;
        }
        // A data record.
        uc += i64::from(nbytes);
        if ue - uc < 4 {
            break;
        }
        let cbytes = getlen(s, &mut uc);
        if nbytes != cbytes {
            return false;
        }
        nr += 1;
    }
    // All examined data was tape marks.
    if (nt as u64).wrapping_mul(4) == uc as u64 {
        return false;
    }
    nr != 0
}

/// `file_is_simh`.
pub fn file_is_simh(ms: &mut Ms, b: &Buffer<'_>) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    if !simh_parse(b.fbuf) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let what: &[u8] = if mime != 0 { b"application/SIMH-tape-data" } else { b"SIMH tape data" };
    if ms.print_str(what) == -1 {
        return -1;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_framed_by_their_lengths() {
        let mut t = Vec::new();
        t.extend_from_slice(&4u32.to_le_bytes());
        t.extend_from_slice(b"abcd");
        t.extend_from_slice(&4u32.to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        assert!(simh_parse(&t));
        // Mismatched trailer.
        let mut bad = t.clone();
        bad[8] = 6;
        assert!(!simh_parse(&bad));
        // Tape marks alone.
        assert!(!simh_parse(&[0u8; 16]));
    }
}
