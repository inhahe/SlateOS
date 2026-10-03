//! The `.xz` format's variable-length integers: liblzma's `vli_decoder.c`,
//! `vli_encoder.c` and `vli_size.c`.
//!
//! Seven bits a byte, least significant first, the top bit set on every byte
//! but the last; at most nine bytes, so at most 63 bits (`LZMA_VLI_MAX`).
//! liblzma refuses two things a plain decoder would accept: a tenth byte, and
//! a last byte of zero after the first (`0x80 0x00` for 0) -- every value has
//! exactly one encoding, which keeps sizes in a header from being padded.

#[cfg(test)]
use alloc::vec::Vec;

use crate::{Error, Result};

/// `LZMA_VLI_MAX`
pub(crate) const VLI_MAX: u64 = u64::MAX / 2;
/// `LZMA_VLI_BYTES_MAX`
const VLI_BYTES_MAX: usize = 9;

/// `lzma_vli_decode` in single-call mode: the integer at `input[*pos]`,
/// advancing `*pos` past it. Running out of input is
/// [`Error::UnexpectedEnd`] here (liblzma says `LZMA_DATA_ERROR`, since its
/// single-call input is a header already read whole -- the callers that can
/// see the difference say which they mean).
pub(crate) fn decode(input: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for i in 0..VLI_BYTES_MAX {
        let &b = input.get(*pos).ok_or(Error::UnexpectedEnd)?;
        *pos = pos.wrapping_add(1);
        value |= u64::from(b & 0x7f) << i.wrapping_mul(7);
        if b & 0x80 == 0 {
            if b == 0 && i > 0 {
                return Err(Error::InvalidData);
            }
            return Ok(value);
        }
    }
    Err(Error::InvalidData)
}

/// `lzma_vli_encode`.
#[cfg(test)]
pub(crate) fn encode(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// `lzma_vli_size`: how many bytes [`encode`] writes for `value`.
#[cfg(test)]
pub(crate) fn size(value: u64) -> u64 {
    let mut n = 1u64;
    let mut v = value >> 7;
    while v != 0 {
        n = n.wrapping_add(1);
        v >>= 7;
    }
    n
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_with_their_sizes() {
        for value in [0u64, 1, 127, 128, 300, 16_383, 16_384, 1 << 35, VLI_MAX] {
            let mut out = Vec::new();
            encode(value, &mut out);
            assert_eq!(out.len() as u64, size(value), "{value}");
            let mut pos = 0;
            assert_eq!(decode(&out, &mut pos).unwrap(), value);
            assert_eq!(pos, out.len());
        }
        let mut out = Vec::new();
        encode(VLI_MAX, &mut out);
        assert_eq!(out.len(), 9);
    }

    #[test]
    fn a_padded_or_overlong_integer_is_refused() {
        assert_eq!(decode(&[0x80, 0x00], &mut 0), Err(Error::InvalidData));
        assert_eq!(decode(&[0xff; 10], &mut 0), Err(Error::InvalidData));
        assert_eq!(decode(&[0x80, 0x80], &mut 0), Err(Error::UnexpectedEnd));
        assert_eq!(decode(&[0x00], &mut 0), Ok(0));
        let mut pos = 1;
        assert_eq!(decode(&[0xff, 0x05], &mut pos), Ok(5));
    }
}
