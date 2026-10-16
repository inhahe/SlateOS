//! `bits.c`: writing variable-length bit strings, low bit first, into the
//! compressed output.

use crate::{Gz, Io, Res};

/// The bit writer's state: bits waiting to be written, and how many.
#[derive(Debug, Default, Clone, Copy)]
pub struct Bi {
    /// Output is buffered here, low bits first.
    pub buf: u16,
    /// How many bits of `buf` are valid.
    pub valid: i32,
}

/// `Buf_size`: the bits `bi_buf` holds.
const BUF_SIZE: i32 = 16;

impl<I: Io> Gz<I> {
    /// `bi_init`: start a new output.
    pub fn bi_init(&mut self) {
        self.bi = Bi::default();
    }

    /// `send_bits (value, length)`: write `length` bits of `value`, which
    /// holds no more than that.
    ///
    /// # Errors
    ///
    /// As [`Gz::put_short`].
    pub fn send_bits(&mut self, value: i32, length: i32) -> Res<()> {
        let value = value as u32;
        if self.bi.valid > BUF_SIZE - length {
            self.bi.buf |= (value << self.bi.valid) as u16;
            self.put_short(u32::from(self.bi.buf))?;
            // `(ush)value >> (Buf_size - bi_valid)`, in `int` as C shifts.
            self.bi.buf = (u32::from(value as u16) >> (BUF_SIZE - self.bi.valid)) as u16;
            self.bi.valid += length - BUF_SIZE;
        } else {
            self.bi.buf |= (value << self.bi.valid) as u16;
            self.bi.valid += length;
        }
        Ok(())
    }

    /// `bi_windup`: write what is left, aligning to a byte.
    ///
    /// # Errors
    ///
    /// As [`Gz::put_short`].
    pub fn bi_windup(&mut self) -> Res<()> {
        if self.bi.valid > 8 {
            self.put_short(u32::from(self.bi.buf))?;
        } else if self.bi.valid > 0 {
            self.put_byte(self.bi.buf as u8)?;
        }
        self.bi = Bi::default();
        Ok(())
    }

    /// `copy_block (buf, len, header)`: a stored block -- `window[at..]`
    /// for `len` bytes, with its length and the length's complement first
    /// when `header`.
    ///
    /// # Errors
    ///
    /// As [`Gz::put_byte`].
    pub fn copy_block(&mut self, at: usize, len: u32, header: bool) -> Res<()> {
        self.bi_windup()?;
        if header {
            self.put_short(len & 0xffff)?;
            self.put_short(!len & 0xffff)?;
        }
        for i in 0..len as usize {
            let b = self.window.get(at + i).copied().unwrap_or(0);
            self.put_byte(b)?;
        }
        Ok(())
    }
}

/// `bi_reverse (code, len)`: the low `len` bits of `code`, reversed.
#[must_use]
pub fn bi_reverse(mut code: u32, mut len: i32) -> u32 {
    let mut res = 0;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len <= 0 {
            break;
        }
    }
    res >> 1
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::memio::MemIo;

    #[test]
    fn reversal() {
        assert_eq!(bi_reverse(0b1, 1), 0b1);
        assert_eq!(bi_reverse(0b110, 3), 0b011);
        assert_eq!(bi_reverse(0b10110, 5), 0b01101);
        assert_eq!(bi_reverse(0x8000, 16), 1);
    }

    #[test]
    fn bits_are_written_low_first_and_padded() {
        let mut gz = Gz::new(MemIo::default(), b"gzip".to_vec());
        gz.ofd = 1;
        gz.bi_init();
        gz.send_bits(0b101, 3).unwrap();
        gz.send_bits(0x1fff, 13).unwrap();
        gz.send_bits(0b1, 1).unwrap();
        gz.bi_windup().unwrap();
        gz.flush_outbuf().unwrap();
        assert_eq!(gz.io.out[&1], [0xfd, 0xff, 0x01]);
    }
}
