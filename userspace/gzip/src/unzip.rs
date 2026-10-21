//! `unzip.c`: one gzip member, or a pkzip file's first entry, inflated (or
//! copied, for a stored entry) and checked against its CRC and length.

use crate::{DEFLATED, ERROR, Gz, Io, OK, Res, STORED, Stop, shown};

/// pkzip's local header: its signature, and the offsets of its fields.
const LOCSIG: u32 = 0x0403_4b50;
const LOCFLG: usize = 6;
const CRPFLG: u8 = 1;
const EXTFLG: u8 = 8;
const LOCHOW: usize = 8;
const LOCCRC: usize = 14;
const LOCSIZ: usize = 18;
const LOCLEN: usize = 22;
const LOCFIL: usize = 26;
const LOCEXT: usize = 28;
const LOCHDR: u32 = 30;
/// The extended local header's size.
const EXTHDR: usize = 16;
/// The encryption header's length.
const RAND_HEAD_LEN: u32 = 12;

/// `unzip.c`'s statics.
#[derive(Debug, Default, Clone, Copy)]
pub struct Zp {
    /// The entry is encrypted.
    decrypt: bool,
    /// The input is a pkzip file.
    pkzip: bool,
    /// The entry's CRC and lengths follow its data.
    ext_header: bool,
}

/// `SH (p)`: 16 bits, low byte first.
fn sh(p: &[u8], at: usize) -> u32 {
    u32::from(p.get(at).copied().unwrap_or(0))
        | (u32::from(p.get(at + 1).copied().unwrap_or(0)) << 8)
}

/// `LG (p)`: 32 bits, low half first.
fn lg(p: &[u8], at: usize) -> u32 {
    sh(p, at) | (sh(p, at + 2) << 16)
}

impl<I: Io> Gz<I> {
    /// `check_zipfile (in)`: check a pkzip local header at `inptr` and skip
    /// to its data.
    ///
    /// # Errors
    ///
    /// Never; a bad header answers [`ERROR`], having said so.
    pub fn check_zipfile(&mut self, ifd: i32) -> Res<i32> {
        let h = self.inptr as usize;
        self.ifd = ifd;

        self.inptr = self
            .inptr
            .wrapping_add(LOCHDR + sh(&self.inbuf, h + LOCFIL) + sh(&self.inbuf, h + LOCEXT));

        if self.inptr > self.insize || lg(&self.inbuf, h) != LOCSIG {
            let text = format!(
                "\n{}: {}: not a valid zip file\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
            self.exit_code = ERROR;
            return Ok(ERROR);
        }
        self.method = i32::from(self.inbuf[h + LOCHOW]);
        if self.method != STORED && self.method != DEFLATED {
            let text = format!(
                "\n{}: {}: first entry not deflated or stored -- use unzip\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
            self.exit_code = ERROR;
            return Ok(ERROR);
        }

        self.zp.decrypt = self.inbuf[h + LOCFLG] & CRPFLG != 0;
        if self.zp.decrypt {
            let text = format!(
                "\n{}: {}: encrypted file -- use unzip\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
            self.exit_code = ERROR;
            return Ok(ERROR);
        }

        self.zp.ext_header = self.inbuf[h + LOCFLG] & EXTFLG != 0;
        self.zp.pkzip = true;
        Ok(OK)
    }

    /// `unzip (in, out)`: decompress one member and check it.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`] for corrupt data, a failed read or write, or -- when
    /// not testing -- a CRC or length that does not match.
    pub fn unzip(&mut self, ifd: i32, ofd: i32) -> Res<i32> {
        let mut orig_crc: u32 = 0;
        let mut orig_len: u32 = 0;
        let mut err = OK;

        self.ifd = ifd;
        self.ofd = ofd;

        self.crc_reset();

        if self.zp.pkzip && !self.zp.ext_header {
            orig_crc = lg(&self.inbuf, LOCCRC);
            orig_len = lg(&self.inbuf, LOCLEN);
        }

        if self.method == DEFLATED {
            let res = self.inflate()?;
            if res == 3 {
                return self.xalloc_die();
            } else if res != 0 {
                return self.gzip_error("invalid compressed data--format violated");
            }
        } else if self.zp.pkzip && self.method == STORED {
            let mut n = lg(&self.inbuf, LOCLEN);
            let siz = lg(&self.inbuf, LOCSIZ);
            if n != siz.wrapping_sub(if self.zp.decrypt { RAND_HEAD_LEN } else { 0 }) {
                let text = format!("len {n}, siz {siz}\n");
                self.diag(&text);
                return self.gzip_error("invalid compressed data--length mismatch");
            }
            while n != 0 {
                n -= 1;
                let c = self.get_byte()?;
                self.put_ubyte(c)?;
            }
            self.flush_window()?;
        } else {
            return self.gzip_error("internal error, invalid method");
        }

        if !self.zp.pkzip {
            let mut buf = [0u8; 8];
            for b in &mut buf {
                *b = self.get_byte()?;
            }
            orig_crc = lg(&buf, 0);
            orig_len = lg(&buf, 4);
        } else if self.zp.ext_header {
            let mut buf = [0u8; EXTHDR];
            for b in &mut buf {
                *b = self.get_byte()?;
            }
            orig_crc = lg(&buf, 4);
            orig_len = lg(&buf, 12);
        }

        if orig_crc != self.getcrc() {
            let text = format!(
                "\n{}: {}: invalid compressed data--crc error\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
            err = ERROR;
        }
        if orig_len != (self.bytes_out & 0xffff_ffff) as u32 {
            let text = format!(
                "\n{}: {}: invalid compressed data--length error\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
            err = ERROR;
        }

        if self.zp.pkzip
            && self.inptr.wrapping_add(4) < self.insize
            && lg(&self.inbuf, self.inptr as usize) == LOCSIG
        {
            if self.to_stdout {
                let text = format!(
                    "{}: {} has more than one entry--rest ignored\n",
                    self.prog(),
                    shown(&self.ifname)
                );
                self.warn(&text);
            } else {
                let text = format!(
                    "{}: {} has more than one entry -- unchanged\n",
                    self.prog(),
                    shown(&self.ifname)
                );
                self.diag(&text);
                err = ERROR;
            }
        }
        self.zp.ext_header = false;
        self.zp.pkzip = false;
        self.unzip_crc = orig_crc;
        if err == OK {
            return Ok(OK);
        }
        self.exit_code = ERROR;
        if !self.test {
            return Err(Stop::Abort);
        }
        Ok(err)
    }
}
