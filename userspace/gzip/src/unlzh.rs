//! `unlzh.c`: SCO's `-lh5-` LZH, as gzip decodes it -- static Huffman
//! codes per block over an 8 KiB dictionary -- with Ubuntu's fix for
//! CVE-2026-41992 (`left` and `right` cleared when a block's code-length
//! code is a single value).
//!
//! The tables overlay upstream's other buffers: `c_len` is
//! [`Gz::outbuf`], `c_table` is [`Gz::d_buf`], `left` is [`Gz::prev`] and
//! `right` is `prev[WSIZE..]`. A tree link past the end of `prev`, or a
//! chain of links that never ends, is reported as a bad table; upstream
//! reads outside its memory or loops there.

use crate::{Gz, Io, OK, Res, WSIZE};

/// The dictionary's size.
const DICBIT: u32 = 13;
const DICSIZ: u32 = 1 << DICBIT;
/// Bits in the bit buffer.
const BITBUFSIZ: i32 = 16;
/// Matches are 3 to 256 bytes.
const MAXMATCH: u32 = 256;
const THRESHOLD: u32 = 3;
const UCHAR_MAX: u32 = 255;
/// The character-and-length alphabet.
const NC: u32 = UCHAR_MAX + MAXMATCH + 2 - THRESHOLD;
/// Bits for a count of codes in it.
const CBIT: i32 = 9;
const CODE_BIT: u32 = 16;
/// The position alphabet, and the code-length alphabet.
const NP: u32 = DICBIT + 1;
const NT: u32 = CODE_BIT + 3;
const PBIT: i32 = 4;
const TBIT: i32 = 5;
/// `pt_len`'s size. Upstream's is `1 << TBIT`; the slack takes the
/// up-to-two lengths a crafted block writes past it.
const NPT: usize = (1 << TBIT) + 8;
/// Where `right` starts in [`Gz::prev`].
const RIGHT: usize = WSIZE;
/// More links than `prev` holds: a chain that loops.
const MAX_CHAIN: usize = 1 << 16;

/// `unlzh.c`'s statics.
#[derive(Debug, Clone)]
pub struct Lzh {
    pt_len: [u8; NPT],
    blocksize: u32,
    pt_table: [u16; 256],
    bitbuf: u16,
    subbitbuf: u32,
    bitcount: i32,
    /// Bytes of a match still to copy; negative once used up.
    j: i32,
    done: bool,
    /// `decode`'s static copy position.
    i: u32,
}

impl Default for Lzh {
    fn default() -> Self {
        Lzh {
            pt_len: [0; NPT],
            blocksize: 0,
            pt_table: [0; 256],
            bitbuf: 0,
            subbitbuf: 0,
            bitcount: 0,
            j: 0,
            done: false,
            i: 0,
        }
    }
}

/// Which table `make_table` fills: `pt_table` or `c_table`.
#[derive(Clone, Copy)]
enum Table {
    Pt,
    C,
}

/// Where `make_table`'s pointer `p` points.
#[derive(Clone, Copy)]
enum Slot {
    Table(usize),
    Left(usize),
    Right(usize),
}

impl<I: Io> Gz<I> {
    /// The corrupt-table diagnostic, `Bad table` -- with upstream's own
    /// stray newline after it.
    fn bad_table<T>(&mut self) -> Res<T> {
        self.gzip_error("Bad table\n")
    }

    /// `fillbuf (n)`: shift `n` bits out of the bit buffer and `n` new
    /// ones in, reading zeros past the end.
    fn fillbuf(&mut self, mut n: i32) -> Res<()> {
        self.lzh.bitbuf = (u32::from(self.lzh.bitbuf) << n) as u16;
        while n > self.lzh.bitcount {
            n -= self.lzh.bitcount;
            self.lzh.bitbuf |= (self.lzh.subbitbuf << n) as u16;
            self.lzh.subbitbuf = u32::from(self.try_byte()?.unwrap_or(0));
            self.lzh.bitcount = 8;
        }
        self.lzh.bitcount -= n;
        self.lzh.bitbuf |= (self.lzh.subbitbuf >> self.lzh.bitcount) as u16;
        Ok(())
    }

    /// `getbits (n)`: the next `n` bits.
    fn getbits(&mut self, n: i32) -> Res<u32> {
        let x = u32::from(self.lzh.bitbuf) >> (BITBUFSIZ - n);
        self.fillbuf(n)?;
        Ok(x)
    }

    /// `left[i]` or `right[i]`.
    fn link(&mut self, right: bool, i: u32) -> Res<u32> {
        let at = if right {
            RIGHT + i as usize
        } else {
            i as usize
        };
        match self.prev.get(at) {
            Some(&v) => Ok(u32::from(v)),
            None => self.bad_table(),
        }
    }

    /// Store at `slot`.
    fn store(&mut self, which: Table, slot: Slot, value: u32) -> Res<()> {
        let cell = match slot {
            Slot::Table(i) => match which {
                Table::Pt => self.lzh.pt_table.get_mut(i),
                Table::C => self.d_buf.get_mut(i),
            },
            Slot::Left(i) => self.prev.get_mut(i),
            Slot::Right(i) => self.prev.get_mut(RIGHT + i),
        };
        match cell {
            Some(c) => {
                *c = value as u16;
                Ok(())
            }
            None => self.bad_table(),
        }
    }

    /// Load from `slot`.
    fn load(&mut self, which: Table, slot: Slot) -> Res<u32> {
        let v = match slot {
            Slot::Table(i) => match which {
                Table::Pt => self.lzh.pt_table.get(i).copied(),
                Table::C => self.d_buf.get(i).copied(),
            },
            Slot::Left(i) => self.prev.get(i).copied(),
            Slot::Right(i) => self.prev.get(RIGHT + i).copied(),
        };
        match v {
            Some(v) => Ok(u32::from(v)),
            None => self.bad_table(),
        }
    }

    /// `make_table (nchar, bitlen, tablebits, table)`.
    fn make_table(&mut self, nchar: u32, bitlen: &[u8], tablebits: u32, which: Table) -> Res<()> {
        let mut count = [0u32; 17];
        let mut weight = [0u32; 17];
        let mut start = [0u32; 18];

        for &len in bitlen.iter().take(nchar as usize) {
            // A length is at most 16: `read_pt_len` refuses longer, and
            // `read_c_len` takes them from a 19-symbol alphabet less 2.
            if let Some(c) = count.get_mut(len as usize) {
                *c += 1;
            }
        }
        for i in 1..=16usize {
            start[i + 1] = (start[i] + (count[i] << (16 - i))) & 0xffff;
        }
        if start[17] & 0xffff != 0 {
            return self.bad_table();
        }
        let jutbits = 16 - tablebits;
        let mut i = 1usize;
        while i <= tablebits as usize {
            start[i] >>= jutbits;
            weight[i] = 1 << (tablebits as usize - i);
            i += 1;
        }
        while i <= 16 {
            weight[i] = 1 << (16 - i);
            i += 1;
        }
        let mut i = start[tablebits as usize + 1] >> jutbits;
        if i != 0 {
            let k = 1u32 << tablebits;
            while i != k {
                self.store(which, Slot::Table(i as usize), 0)?;
                i += 1;
            }
        }

        let mut avail = nchar;
        let mask = 1u32 << (15 - tablebits);
        for ch in 0..nchar {
            let len = u32::from(bitlen.get(ch as usize).copied().unwrap_or(0));
            if len == 0 {
                continue;
            }
            let nextcode = start[len as usize] + weight[len as usize];
            if len <= tablebits {
                if (1u32 << tablebits) < nextcode {
                    return self.bad_table();
                }
                for i in start[len as usize]..nextcode {
                    self.store(which, Slot::Table(i as usize), ch)?;
                }
            } else {
                let mut k = start[len as usize];
                let mut p = Slot::Table((k >> jutbits) as usize);
                let mut i = len - tablebits;
                while i != 0 {
                    if self.load(which, p)? == 0 {
                        self.store(which, Slot::Right(avail as usize), 0)?;
                        self.store(which, Slot::Left(avail as usize), 0)?;
                        self.store(which, p, avail)?;
                        avail += 1;
                    }
                    let at = self.load(which, p)? as usize;
                    p = if k & mask != 0 {
                        Slot::Right(at)
                    } else {
                        Slot::Left(at)
                    };
                    k <<= 1;
                    i -= 1;
                }
                self.store(which, p, ch)?;
            }
            start[len as usize] = nextcode & 0xffff;
        }
        Ok(())
    }

    /// Follow `left`/`right` from `c` while it is `limit` or more, steered by
    /// the bit buffer from the bit `mask` down -- the walk `read_c_len`,
    /// `decode_c` and `decode_p` share.
    fn walk(&mut self, mut c: u32, limit: u32, mut mask: u32) -> Res<u32> {
        let mut steps = 0usize;
        while c >= limit {
            steps += 1;
            if steps > MAX_CHAIN {
                return self.bad_table();
            }
            let right = u32::from(self.lzh.bitbuf) & mask != 0;
            c = self.link(right, c)?;
            mask >>= 1;
        }
        Ok(c)
    }

    /// `read_pt_len (nn, nbit, i_special)`.
    fn read_pt_len(&mut self, nn: u32, nbit: i32, i_special: i32) -> Res<()> {
        let n = self.getbits(nbit)?;
        if n == 0 {
            let c = self.getbits(nbit)?;
            // `nn` is NT or NP, both within `pt_len`.
            self.lzh.pt_len[..nn as usize].fill(0);
            self.lzh.pt_table.fill(c as u16);
        } else {
            let mut i = 0usize;
            while i < n as usize {
                let mut c = u32::from(self.lzh.bitbuf) >> (BITBUFSIZ - 3);
                if c == 7 {
                    let mut mask = 1u32 << (BITBUFSIZ - 1 - 3);
                    while mask & u32::from(self.lzh.bitbuf) != 0 {
                        mask >>= 1;
                        c += 1;
                    }
                    if 16 < c {
                        return self.bad_table();
                    }
                }
                self.fillbuf(if c < 7 { 3 } else { c as i32 - 3 })?;
                if let Some(len) = self.lzh.pt_len.get_mut(i) {
                    *len = c as u8;
                }
                i += 1;
                if i as i32 == i_special {
                    let mut c = self.getbits(2)? as i32;
                    while {
                        c -= 1;
                        c >= 0
                    } {
                        if let Some(len) = self.lzh.pt_len.get_mut(i) {
                            *len = 0;
                        }
                        i += 1;
                    }
                }
            }
            while i < nn as usize {
                if let Some(len) = self.lzh.pt_len.get_mut(i) {
                    *len = 0;
                }
                i += 1;
            }
            let pt_len = self.lzh.pt_len;
            self.make_table(nn, &pt_len, 8, Table::Pt)?;
        }
        Ok(())
    }

    /// `read_c_len`.
    fn read_c_len(&mut self) -> Res<()> {
        let n = self.getbits(CBIT)?;
        if n == 0 {
            let c = self.getbits(CBIT)?;
            for len in &mut self.outbuf[..NC as usize] {
                *len = 0;
            }
            for t in &mut self.d_buf[..4096] {
                *t = c as u16;
            }
            // Ubuntu's CVE-2026-41992 fix: links an earlier `.Z` file left
            // in `prev` must not be followed.
            let span = 2 * NC as usize - 1;
            for l in &mut self.prev[..span] {
                *l = 0;
            }
            for r in &mut self.prev[RIGHT..RIGHT + span] {
                *r = 0;
            }
        } else {
            let mut i = 0usize;
            while i < n as usize {
                let first =
                    u32::from(self.lzh.pt_table[(self.lzh.bitbuf >> (BITBUFSIZ - 8)) as usize]);
                let c = self.walk(first, NT, 1 << (BITBUFSIZ - 1 - 8))?;
                let bits = self.lzh.pt_len.get(c as usize).copied().unwrap_or(0);
                self.fillbuf(i32::from(bits))?;
                if c <= 2 {
                    let mut c = match c {
                        0 => 1,
                        1 => self.getbits(4)? as i32 + 3,
                        _ => self.getbits(CBIT)? as i32 + 20,
                    };
                    while {
                        c -= 1;
                        c >= 0
                    } {
                        if let Some(len) = self.outbuf.get_mut(i) {
                            *len = 0;
                        }
                        i += 1;
                    }
                } else {
                    if let Some(len) = self.outbuf.get_mut(i) {
                        *len = (c - 2) as u8;
                    }
                    i += 1;
                }
            }
            while i < NC as usize {
                self.outbuf[i] = 0;
                i += 1;
            }
            let c_len = self.outbuf[..NC as usize].to_vec();
            self.make_table(NC, &c_len, 12, Table::C)?;
        }
        Ok(())
    }

    /// `decode_c`: the next character or length; [`NC`] at the end.
    fn decode_c(&mut self) -> Res<u32> {
        if self.lzh.blocksize == 0 {
            self.lzh.blocksize = self.getbits(16)?;
            if self.lzh.blocksize == 0 {
                return Ok(NC);
            }
            self.read_pt_len(NT, TBIT, 3)?;
            self.read_c_len()?;
            self.read_pt_len(NP, PBIT, -1)?;
        }
        self.lzh.blocksize -= 1;
        let first = u32::from(self.d_buf[(self.lzh.bitbuf >> (BITBUFSIZ - 12)) as usize]);
        let j = self.walk(first, NC, 1 << (BITBUFSIZ - 1 - 12))?;
        let bits = self.outbuf[j as usize];
        self.fillbuf(i32::from(bits))?;
        Ok(j)
    }

    /// `decode_p`: the next match position.
    fn decode_p(&mut self) -> Res<u32> {
        let first = u32::from(self.lzh.pt_table[(self.lzh.bitbuf >> (BITBUFSIZ - 8)) as usize]);
        let mut j = self.walk(first, NP, 1 << (BITBUFSIZ - 1 - 8))?;
        let bits = self.lzh.pt_len[j as usize];
        self.fillbuf(i32::from(bits))?;
        if j != 0 {
            j = (1u32 << (j - 1)).wrapping_add(self.getbits(j as i32 - 1)?);
        }
        Ok(j)
    }

    /// `decode (count, buffer)`, into the window: up to `count` bytes.
    fn decode(&mut self, count: u32) -> Res<u32> {
        let mut r: u32 = 0;
        loop {
            self.lzh.j -= 1;
            if self.lzh.j < 0 {
                break;
            }
            self.window[r as usize] = self.window[self.lzh.i as usize];
            self.lzh.i = (self.lzh.i + 1) & (DICSIZ - 1);
            r += 1;
            if r == count {
                return Ok(r);
            }
        }
        loop {
            let c = self.decode_c()?;
            if c == NC {
                self.lzh.done = true;
                return Ok(r);
            }
            if c <= UCHAR_MAX {
                self.window[r as usize] = c as u8;
                r += 1;
                if r == count {
                    return Ok(r);
                }
            } else {
                self.lzh.j = (c - (UCHAR_MAX + 1 - THRESHOLD)) as i32;
                let p = self.decode_p()?;
                self.lzh.i = r.wrapping_sub(p).wrapping_sub(1) & (DICSIZ - 1);
                loop {
                    self.lzh.j -= 1;
                    if self.lzh.j < 0 {
                        break;
                    }
                    self.window[r as usize] = self.window[self.lzh.i as usize];
                    self.lzh.i = (self.lzh.i + 1) & (DICSIZ - 1);
                    r += 1;
                    if r == count {
                        return Ok(r);
                    }
                }
            }
        }
    }

    /// `unlzh (in, out)`.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for a bad table or a read or a
    /// write that failed.
    pub fn unlzh(&mut self, ifd: i32, ofd: i32) -> Res<i32> {
        self.ifd = ifd;
        self.ofd = ofd;

        // decode_start: huf_decode_start, then the copy state.
        self.lzh.bitbuf = 0;
        self.lzh.subbitbuf = 0;
        self.lzh.bitcount = 0;
        self.fillbuf(BITBUFSIZ)?;
        self.lzh.blocksize = 0;
        self.lzh.j = 0;
        self.lzh.done = false;

        while !self.lzh.done {
            let n = self.decode(DICSIZ)?;
            if n > 0 {
                self.write_window(ofd, 0, n as usize)?;
            }
        }
        Ok(OK)
    }
}
