//! The `.lzma` format ("LZMA_Alone", from the LZMA SDK and LZMA Utils):
//! liblzma's `alone_decoder.c`, with what `xz -d` adds around it.
//!
//! A 13-byte header -- the properties byte, the dictionary size and the
//! uncompressed size, both little-endian, the size all ones when unknown --
//! then raw LZMA. With the size unknown the data must end with the
//! end-of-payload marker; with it known, liblzma 5.2.5 refuses the marker.
//! `xz -d` refuses bytes after the stream (`coder.c`: "Check that there is no
//! trailing garbage"), and so does this.
//!
//! liblzma's `lzma_alone_decoder` is not "picky" -- it is `lzma_auto_decoder`
//! that also demands a dictionary size of 2^n or 2^n + 2^(n-1) and a known
//! size under 256 GiB, to tell `.lzma` from noise. Being given a file named
//! `.lzma` is the caller's evidence, so this decodes as the non-picky decoder
//! does; [`looks_like_lzma`](crate::looks_like_lzma) is the picky test.

use alloc::vec::Vec;

use crate::lzma::{Decoder, Dict, Props, Rc};
use crate::{Error, Result};

/// The header's length.
pub(crate) const HEADER_SIZE: usize = 13;

/// The parsed header.
pub(crate) struct Header {
    pub(crate) props: Props,
    pub(crate) dict_size: u32,
    /// `None` for "unknown" (all ones).
    pub(crate) size: Option<u64>,
}

/// Reads the header, or says why it is not one.
pub(crate) fn header(input: &[u8]) -> Result<Header> {
    let &p = input.first().ok_or(Error::UnexpectedEnd)?;
    let props = Props::from_byte(p).ok_or(Error::NotLzma)?;
    let raw = input.get(..HEADER_SIZE).ok_or(Error::UnexpectedEnd)?;
    let mut d = [0u8; 4];
    d.copy_from_slice(raw.get(1..5).unwrap_or(&[0; 4]));
    let mut s = [0u8; 8];
    s.copy_from_slice(raw.get(5..13).unwrap_or(&[0; 8]));
    let size = u64::from_le_bytes(s);
    Ok(Header {
        props,
        dict_size: u32::from_le_bytes(d),
        size: (size != u64::MAX).then_some(size),
    })
}

/// `lzma_auto_decoder`'s picky test: a dictionary size of 2^n or
/// 2^n + 2^(n-1) (or all ones), and a known size under 256 GiB.
pub(crate) fn plausible(h: &Header) -> bool {
    if h.dict_size != u32::MAX {
        let mut d = h.dict_size.wrapping_sub(1);
        d |= d >> 2;
        d |= d >> 3;
        d |= d >> 4;
        d |= d >> 8;
        d |= d >> 16;
        if d.wrapping_add(1) != h.dict_size {
            return false;
        }
    }
    h.size.is_none_or(|s| s < 1 << 38)
}

/// Decodes a whole `.lzma` file onto `out`.
pub(crate) fn decode(input: &[u8], out: &mut Vec<u8>, cap: usize) -> Result<()> {
    let h = header(input)?;
    // A size no `usize` holds can never be reached, so it bounds nothing.
    let size = h.size.map(|s| usize::try_from(s).unwrap_or(usize::MAX));
    let mut rc = Rc::new(input, HEADER_SIZE)?;
    let mut decoder = Decoder::new(h.props);
    let dict = Dict {
        start: out.len(),
        window: Dict::window_for(h.dict_size),
    };
    decoder.decode(&mut rc, out, dict, size, cap)?;
    if rc.pos != input.len() {
        return Err(Error::TrailingData);
    }
    Ok(())
}
