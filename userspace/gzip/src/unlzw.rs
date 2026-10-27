//! `unlzw.c`: `compress`'s LZW (`.Z`), decoded with the string table built
//! as the codes arrive. The table's prefixes are [`Gz::prev`] and its
//! suffixes [`Gz::window`], as upstream overlays them.

use crate::{ERROR, Gz, INBUF_EXTRA, INBUFSIZ, Io, OK, OUTBUFSIZ, Res, shown};

/// The first code of a fresh table's width.
const INIT_BITS: i32 = 9;
/// The header byte's width bits, block-mode bit and reserved bits.
const BIT_MASK: i32 = 0x1f;
const BLOCK_MODE: i32 = 0x80;
const LZW_RESERVED: i32 = 0x60;
/// Clear the table; the first free entry.
const CLEAR: i64 = 256;
const FIRST: i64 = CLEAR + 1;
/// The widest code this decodes.
const BITS: i32 = crate::BITS;

/// `MAXCODE (n)`.
fn maxcode(n: i32) -> i64 {
    1i64 << n
}

/// Round `posbits` up past the rest of the current group of `n_bits`
/// codes, as `compress` pads when the code width changes.
fn round_posbits(posbits: i64, n_bits: i32) -> i64 {
    let group = i64::from(n_bits) << 3;
    (posbits - 1) + (group - (posbits - 1 + group).rem_euclid(group))
}

impl<I: Io> Gz<I> {
    /// `unlzw (in, out)`.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for corrupt input or a read or a
    /// write that failed.
    pub fn unlzw(&mut self, ifd: i32, ofd: i32) -> Res<i32> {
        self.maxbits = i32::from(self.get_byte()?);
        self.block_mode = self.maxbits & BLOCK_MODE != 0;
        if self.maxbits & LZW_RESERVED != 0 {
            let text = format!(
                "\n{}: {}: warning, unknown flags 0x{:x}\n",
                self.prog(),
                shown(&self.ifname),
                self.maxbits & LZW_RESERVED
            );
            self.warn(&text);
        }
        self.maxbits &= BIT_MASK;
        let maxmaxcode = maxcode(self.maxbits);

        if self.maxbits > BITS {
            let text = format!(
                "\n{}: {}: compressed with {} bits, can only handle {} bits\n",
                self.prog(),
                shown(&self.ifname),
                self.maxbits,
                BITS
            );
            self.diag(&text);
            self.exit_code = ERROR;
            return Ok(ERROR);
        }
        let mut rsize = self.insize as i64;
        let mut n_bits = INIT_BITS;
        let mut maxcode_now = maxcode(n_bits) - 1;
        let mut bitmask: i64 = (1 << n_bits) - 1;
        let mut oldcode: i64 = -1;
        let mut finchar: u8 = 0;
        let mut outpos: usize = 0;
        let mut posbits: i64 = i64::from(self.inptr) << 3;

        let mut free_ent: i64 = if self.block_mode { FIRST } else { 256 };

        // `clear_tab_prefixof`: upstream zeroes 256 *bytes* of the table.
        for p in &mut self.prev[..128] {
            *p = 0;
        }
        for code in 0..256usize {
            self.window[code] = code as u8;
        }

        loop {
            // resetbuf:
            let o = (posbits >> 3) as usize;
            let e = (self.insize as usize).saturating_sub(o);
            // Rounding up for a new code width can leave `o` past the end,
            // where there is nothing to move.
            if e != 0 {
                self.inbuf.copy_within(o..o + e, 0);
            }
            self.insize = e as u32;
            posbits = 0;

            if (self.insize as usize) < INBUF_EXTRA {
                let at = self.insize as usize;
                rsize = match self.io.read(ifd, &mut self.inbuf[at..at + INBUFSIZ]) {
                    Ok(n) => n as i64,
                    Err(err) => return self.read_error(err),
                };
                self.insize += rsize as u32;
                self.bytes_in += rsize;
            }
            let insize = i64::from(self.insize);
            let inbits = if rsize != 0 {
                (insize - insize % i64::from(n_bits)) << 3
            } else {
                (insize << 3) - i64::from(n_bits - 1)
            };

            let mut restart = false;
            while inbits > posbits {
                if free_ent > maxcode_now {
                    posbits = round_posbits(posbits, n_bits);
                    n_bits += 1;
                    maxcode_now = if n_bits == self.maxbits {
                        maxmaxcode
                    } else {
                        maxcode(n_bits) - 1
                    };
                    bitmask = (1 << n_bits) - 1;
                    restart = true;
                    break;
                }

                // `input (inbuf, posbits, code, n_bits, bitmask)`.
                let p = (posbits >> 3) as usize;
                let byte = |i: usize| i64::from(self.inbuf.get(p + i).copied().unwrap_or(0));
                let mut code =
                    ((byte(0) | (byte(1) << 8) | (byte(2) << 16)) >> (posbits & 7)) & bitmask;
                posbits += i64::from(n_bits);

                if oldcode == -1 {
                    if 256 <= code {
                        return self.gzip_error("corrupt input.");
                    }
                    oldcode = code;
                    finchar = code as u8;
                    self.outbuf[outpos] = finchar;
                    outpos += 1;
                    continue;
                }
                if code == CLEAR && self.block_mode {
                    for p in &mut self.prev[..128] {
                        *p = 0;
                    }
                    free_ent = FIRST - 1;
                    posbits = round_posbits(posbits, n_bits);
                    n_bits = INIT_BITS;
                    maxcode_now = maxcode(n_bits) - 1;
                    bitmask = (1 << n_bits) - 1;
                    restart = true;
                    break;
                }
                let incode = code;
                let mut stackp = self.lzw_stack.len();

                if code >= free_ent {
                    if code > free_ent {
                        if outpos > 0 {
                            self.write_outbuf(ofd, 0, outpos)?;
                        }
                        return self.gzip_error(if self.to_stdout {
                            "corrupt input."
                        } else {
                            "corrupt input. Use zcat to recover some data."
                        });
                    }
                    stackp -= 1;
                    self.lzw_stack[stackp] = finchar;
                    code = oldcode;
                }

                while code >= 256 {
                    // A chain is at most as long as the table, which the
                    // stack holds; a link past either is corrupt.
                    let (Some(&suffix), Some(&prefix), Some(next)) = (
                        self.window.get(code as usize),
                        self.prev.get(code as usize),
                        stackp.checked_sub(1),
                    ) else {
                        return self.gzip_error("corrupt input.");
                    };
                    stackp = next;
                    self.lzw_stack[stackp] = suffix;
                    code = i64::from(prefix);
                }
                finchar = self.window[code as usize];
                let Some(next) = stackp.checked_sub(1) else {
                    return self.gzip_error("corrupt input.");
                };
                stackp = next;
                self.lzw_stack[stackp] = finchar;

                // Put them out in forward order.
                let mut i = self.lzw_stack.len() - stackp;
                if outpos + i >= OUTBUFSIZ {
                    loop {
                        if i > OUTBUFSIZ - outpos {
                            i = OUTBUFSIZ - outpos;
                        }
                        if i > 0 {
                            self.outbuf[outpos..outpos + i]
                                .copy_from_slice(&self.lzw_stack[stackp..stackp + i]);
                            outpos += i;
                        }
                        if outpos >= OUTBUFSIZ {
                            self.write_outbuf(ofd, 0, outpos)?;
                            outpos = 0;
                        }
                        stackp += i;
                        i = self.lzw_stack.len() - stackp;
                        if i == 0 {
                            break;
                        }
                    }
                } else {
                    self.outbuf[outpos..outpos + i]
                        .copy_from_slice(&self.lzw_stack[stackp..stackp + i]);
                    outpos += i;
                }

                let code = free_ent;
                if code < maxmaxcode {
                    self.prev[code as usize] = oldcode as u16;
                    self.window[code as usize] = finchar;
                    free_ent = code + 1;
                }
                oldcode = incode;
            }
            if restart {
                continue;
            }
            if rsize == 0 {
                break;
            }
        }

        if outpos > 0 {
            self.write_outbuf(ofd, 0, outpos)?;
        }
        Ok(OK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `compress` pads to the end of a group of eight codes.
    #[test]
    fn positions_round_to_a_group_of_codes() {
        assert_eq!(round_posbits(0, 9), 0);
        assert_eq!(round_posbits(9, 9), 72);
        assert_eq!(round_posbits(72, 9), 72);
        assert_eq!(round_posbits(73, 9), 144);
    }
}
