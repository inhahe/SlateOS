//! The LZMA2 decoder: liblzma's `lzma2_decoder.c`.
//!
//! LZMA2 is LZMA cut into chunks of at most 2 MiB of output, each either
//! LZMA-compressed or stored, with a control byte that says whether the
//! dictionary, the properties or the coder's state start afresh. The rules
//! that make a stream invalid are liblzma's, in its order: the first chunk
//! must reset the dictionary; after a reset the next LZMA chunk must bring
//! properties; a control byte from 3 to 0x7f is invalid; and an LZMA chunk
//! must use exactly the compressed bytes its header declares -- the decoder
//! is handed the whole rest of the input, as liblzma's is, and its
//! consumption is checked afterwards.

use alloc::vec::Vec;

use crate::lzma::{Decoder, Dict, End, Props, Rc};
use crate::{Error, Result};

/// `lzma_lzma2_props_decode`: the dictionary size a filter's one properties
/// byte stands for, or `None` for an unsupported byte (reserved bits set, or
/// above 40).
pub(crate) fn dict_size_from_prop(prop: u8) -> Option<u32> {
    if prop & 0xc0 != 0 || prop > 40 {
        return None;
    }
    if prop == 40 {
        return Some(u32::MAX);
    }
    Some((2 | u32::from(prop & 1)) << u32::from(prop / 2).wrapping_add(11))
}

/// Reads one byte of a chunk header.
fn byte(input: &[u8], pos: &mut usize) -> Result<u8> {
    let &b = input.get(*pos).ok_or(Error::UnexpectedEnd)?;
    *pos = pos.wrapping_add(1);
    Ok(b)
}

/// Decodes the LZMA2 data starting at `input[start]` onto `out`, with the
/// dictionary size `dict_size`, refusing to let `out` grow past `cap`.
/// Returns the position just after the end marker.
pub(crate) fn decode(
    input: &[u8],
    start: usize,
    out: &mut Vec<u8>,
    dict_size: u32,
    cap: usize,
) -> Result<usize> {
    let window = Dict::window_for(dict_size);
    let mut pos = start;
    let mut need_properties = true;
    let mut need_dictionary_reset = true;
    let mut dict = Dict {
        start: out.len(),
        window,
    };
    let mut lzma: Option<Decoder> = None;

    loop {
        let control = byte(input, &mut pos)?;
        if control == 0x00 {
            return Ok(pos);
        }

        if control >= 0xe0 || control == 1 {
            need_properties = true;
            need_dictionary_reset = true;
        } else if need_dictionary_reset {
            return Err(Error::InvalidData);
        }

        if control >= 0x80 {
            // An LZMA chunk: the top five bits of its size are in the
            // control byte.
            let mut uncompressed = usize::from(control & 0x1f) << 16;
            let mut new_properties = false;
            if control >= 0xc0 {
                need_properties = false;
                new_properties = true;
            } else if need_properties {
                return Err(Error::InvalidData);
            } else if control >= 0xa0 {
                // A state reset with the properties already in use.
                if let Some(d) = lzma.as_mut() {
                    let props = d.props();
                    d.reset(props);
                }
            }
            if need_dictionary_reset {
                need_dictionary_reset = false;
                dict.start = out.len();
            }
            // Sizes are at most 21 bits: none of this can wrap.
            uncompressed |= usize::from(byte(input, &mut pos)?) << 8;
            uncompressed =
                uncompressed.wrapping_add(usize::from(byte(input, &mut pos)?).wrapping_add(1));
            let mut compressed = usize::from(byte(input, &mut pos)?) << 8;
            compressed =
                compressed.wrapping_add(usize::from(byte(input, &mut pos)?).wrapping_add(1));
            if new_properties {
                let props = Props::from_byte(byte(input, &mut pos)?).ok_or(Error::InvalidData)?;
                match lzma.as_mut() {
                    Some(d) => d.reset(props),
                    None => lzma = Some(Decoder::new(props)),
                }
            }
            let decoder = lzma.as_mut().ok_or(Error::InvalidData)?;

            // liblzma checks "used more than the chunk declares" after every
            // call into the LZMA decoder, so running out of input once past
            // the chunk's end is corrupt data, not a stream cut short.
            let chunk_start = pos;
            let overran = |e: Error| {
                if e == Error::UnexpectedEnd && input.len().saturating_sub(chunk_start) > compressed
                {
                    Error::InvalidData
                } else {
                    e
                }
            };
            let mut rc = Rc::new(input, pos).map_err(overran)?;
            let end = decoder
                .decode(&mut rc, out, dict, Some(uncompressed), cap)
                .map_err(overran)?;
            if end != End::Size {
                return Err(Error::InvalidData);
            }
            // The chunk must have used exactly the bytes it declared: more
            // is liblzma's "in_used > compressed_size", fewer its
            // "compressed_size != 0".
            let used = rc.pos.wrapping_sub(pos);
            if used != compressed {
                return Err(Error::InvalidData);
            }
            pos = rc.pos;
        } else {
            // Control bytes 3 to 0x7f mean nothing.
            if control > 2 {
                return Err(Error::InvalidData);
            }
            // A stored chunk, copied into the dictionary as it is.
            if need_dictionary_reset {
                need_dictionary_reset = false;
                dict.start = out.len();
            }
            let mut size = usize::from(byte(input, &mut pos)?) << 8;
            size = size.wrapping_add(usize::from(byte(input, &mut pos)?).wrapping_add(1));
            let end = pos.checked_add(size).ok_or(Error::UnexpectedEnd)?;
            let data = input.get(pos..end).ok_or(Error::UnexpectedEnd)?;
            if size > cap.saturating_sub(out.len()) {
                return Err(Error::OutputTooLarge);
            }
            out.extend_from_slice(data);
            pos = end;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn dictionary_sizes_are_two_or_three_times_a_power_of_two() {
        assert_eq!(dict_size_from_prop(0), Some(4096));
        assert_eq!(dict_size_from_prop(1), Some(6144));
        assert_eq!(dict_size_from_prop(18), Some(2 << 20));
        assert_eq!(dict_size_from_prop(39), Some(3 << 30));
        assert_eq!(dict_size_from_prop(40), Some(u32::MAX));
        assert_eq!(dict_size_from_prop(41), None);
        assert_eq!(dict_size_from_prop(0x40), None);
    }

    /// Stored chunks need no LZMA at all: a dictionary-resetting one, a
    /// continuing one, and the end marker.
    #[test]
    fn stored_chunks_are_copied() {
        let data = [
            1, 0x00, 0x02, b'a', b'b', b'c', 2, 0x00, 0x01, b'd', b'e', 0x00,
        ];
        let mut out = Vec::new();
        assert_eq!(decode(&data, 0, &mut out, 4096, 100).unwrap(), data.len());
        assert_eq!(out, b"abcde");
    }

    #[test]
    fn the_first_chunk_must_reset_the_dictionary() {
        let mut out = Vec::new();
        assert_eq!(
            decode(&[2, 0, 0, b'a', 0], 0, &mut out, 4096, 100),
            Err(Error::InvalidData)
        );
        // Control bytes 3 to 0x7f are invalid even after a reset.
        let data = [1, 0, 0, b'a', 3];
        assert_eq!(
            decode(&data, 0, &mut out, 4096, 100),
            Err(Error::InvalidData)
        );
        // An LZMA chunk after a stored reset must bring properties.
        let data = [1, 0, 0, b'a', 0x80, 0, 0, 0, 0];
        assert_eq!(
            decode(&data, 0, &mut vec![], 4096, 100),
            Err(Error::InvalidData)
        );
    }

    #[test]
    fn a_stored_chunk_obeys_the_cap() {
        let data = [1, 0x00, 0x02, b'a', b'b', b'c', 0x00];
        assert_eq!(
            decode(&data, 0, &mut Vec::new(), 4096, 2),
            Err(Error::OutputTooLarge)
        );
        assert_eq!(
            decode(&data[..5], 0, &mut Vec::new(), 4096, 9),
            Err(Error::UnexpectedEnd)
        );
    }
}
