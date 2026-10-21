//! `unpack.c`: `pack`'s Huffman-coded files (`.z`), decoded with a peek
//! table for the short codes and a walk down the tree for the long ones.

use crate::{Gz, Io, OK, Res};

/// The longest code; `pack` makes at most 24-bit ones.
const MAX_BITLEN: usize = 25;
/// Literals, the end-of-block code not counted.
const LITERALS: usize = 256;
/// Bits the peek table looks at, at most.
const MAX_PEEK: i32 = 12;

/// `unpack.c`'s statics. The prefix table is [`Gz::outbuf`], as upstream's.
#[derive(Debug, Clone)]
pub struct Pk {
    orig_len: u32,
    max_len: i32,
    literal: [u8; LITERALS],
    lit_base: [i32; MAX_BITLEN + 1],
    leaves: [i32; MAX_BITLEN + 1],
    parents: [i32; MAX_BITLEN + 1],
    peek_bits: i32,
    bitbuf: u64,
    valid: i32,
}

impl Default for Pk {
    fn default() -> Self {
        Pk {
            orig_len: 0,
            max_len: 0,
            literal: [0; LITERALS],
            lit_base: [0; MAX_BITLEN + 1],
            leaves: [0; MAX_BITLEN + 1],
            parents: [0; MAX_BITLEN + 1],
            peek_bits: 0,
            bitbuf: 0,
            valid: 0,
        }
    }
}

impl<I: Io> Gz<I> {
    /// `read_byte`: an input byte; the input ending aborts.
    fn pk_read_byte(&mut self) -> Res<u8> {
        self.get_byte()
    }

    /// `look_bits (code, bits, mask)`: the next `bits` input bits, without
    /// using them up.
    fn look_bits(&mut self, bits: i32, mask: u64) -> Res<u64> {
        while self.pk.valid < bits {
            self.pk.bitbuf = (self.pk.bitbuf << 8) | u64::from(self.pk_read_byte()?);
            self.pk.valid += 8;
        }
        Ok((self.pk.bitbuf >> (self.pk.valid - bits)) & mask)
    }

    /// `read_tree`: the original length, and the tree's shape and leaves.
    fn read_tree(&mut self) -> Res<()> {
        let mut max_leaves: i32 = 1;

        self.pk.orig_len = 0;
        for _ in 1..=4 {
            self.pk.orig_len = (self.pk.orig_len << 8) | u32::from(self.pk_read_byte()?);
        }

        self.pk.max_len = i32::from(self.pk_read_byte()?);
        if !(0 < self.pk.max_len && self.pk.max_len <= MAX_BITLEN as i32) {
            return self
                .gzip_error("invalid compressed data -- Huffman code bit length out of range");
        }

        let mut n = 0;
        for len in 1..=self.pk.max_len as usize {
            let leaves = i32::from(self.pk_read_byte()?);
            self.pk.leaves[len] = leaves;
            if max_leaves - i32::from(len == self.pk.max_len as usize) < leaves {
                return self.gzip_error("too many leaves in Huffman tree");
            }
            max_leaves = (max_leaves - leaves + 1) * 2 - 1;
            n += leaves;
        }
        if LITERALS as i32 <= n {
            return self.gzip_error("too many leaves in Huffman tree");
        }
        // The last count is offset by 2 to fit a byte; the end-of-block code
        // is not sent, so one here.
        self.pk.leaves[self.pk.max_len as usize] += 1;

        let mut base = 0usize;
        for len in 1..=self.pk.max_len as usize {
            self.pk.lit_base[len] = base as i32;
            for _ in 0..self.pk.leaves[len] {
                let b = self.pk_read_byte()?;
                // The counts were checked above: `n` leaves in all, under 256.
                if let Some(slot) = self.pk.literal.get_mut(base) {
                    *slot = b;
                }
                base += 1;
            }
        }
        self.pk.leaves[self.pk.max_len as usize] += 1;
        Ok(())
    }

    /// `build_tree`: the parents at each length, and the prefix table.
    fn pk_build_tree(&mut self) -> Res<()> {
        let mut nodes = 0;
        let mut len = self.pk.max_len as usize;
        while len >= 1 {
            nodes >>= 1;
            self.pk.parents[len] = nodes;
            self.pk.lit_base[len] -= nodes;
            nodes += self.pk.leaves[len];
            len -= 1;
        }
        if (nodes >> 1) != 1 {
            return self.gzip_error("too few leaves in Huffman tree");
        }

        self.pk.peek_bits = self.pk.max_len.min(MAX_PEEK);
        let mut prefixp = 1usize << self.pk.peek_bits;
        for len in 1..=self.pk.peek_bits {
            let mut prefixes = self.pk.leaves[len as usize] << (self.pk.peek_bits - len);
            while prefixes > 0 {
                prefixes -= 1;
                // The leaves' counts fit the table: `read_tree` checked them.
                prefixp = prefixp.saturating_sub(1);
                if let Some(slot) = self.outbuf.get_mut(prefixp) {
                    *slot = len as u8;
                }
            }
        }
        while prefixp > 0 {
            prefixp -= 1;
            self.outbuf[prefixp] = 0;
        }
        Ok(())
    }

    /// `unpack (in, out)`.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for corrupt data or a read or a
    /// write that failed.
    pub fn unpack(&mut self, ifd: i32, ofd: i32) -> Res<i32> {
        self.ifd = ifd;
        self.ofd = ofd;

        self.read_tree()?;
        self.pk_build_tree()?;
        self.pk.valid = 0;
        self.pk.bitbuf = 0;
        let peek_bits = self.pk.peek_bits;
        let peek_mask: u64 = (1 << peek_bits) - 1;

        let max_len = self.pk.max_len;
        let eob = (self.pk.leaves[max_len as usize] - 1) as u64;

        loop {
            let mut peek = self.look_bits(peek_bits, peek_mask)?;
            let mut len = i32::from(self.outbuf[peek as usize]);
            if len > 0 {
                peek >>= peek_bits - len;
            } else {
                let mut mask = peek_mask;
                len = peek_bits;
                while (peek as i64) < i64::from(self.pk.parents[len as usize]) {
                    len += 1;
                    mask = (mask << 1) + 1;
                    peek = self.look_bits(len, mask)?;
                }
            }
            if peek == eob && len == max_len {
                break;
            }
            let at = peek as i64 + i64::from(self.pk.lit_base[len as usize]);
            let Some(&lit) = usize::try_from(at)
                .ok()
                .and_then(|i| self.pk.literal.get(i))
            else {
                return self.gzip_error("invalid compressed data--format violated");
            };
            self.put_ubyte(lit)?;
            self.pk.valid -= len;
        }

        self.flush_window()?;
        if self.pk.orig_len != (self.bytes_out & 0xffff_ffff) as u32 {
            return self.gzip_error("invalid compressed data--length error");
        }
        Ok(OK)
    }
}
