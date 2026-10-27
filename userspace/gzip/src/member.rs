//! The members of a compressed input, one after another: `gzip.c`'s
//! `get_method`, which reads a member's magic number and header, and the
//! loop `treat_file` and `treat_stdin` run over them. Here rather than in
//! [`driver`](crate::driver) because the header is input a stranger wrote,
//! and the fuzz harness must reach it without a file system.

use crate::{
    COMMENT, COMPRESSED, DEFLATED, ENCRYPTED, ERROR, EXTRA_FIELD, GZIP_MAGIC, Gz, HEADER_CRC,
    INBUFSIZ, Io, LZH_MAGIC, LZHED, LZW_MAGIC, MAX_PATH_LEN, OK, OLD_GZIP_MAGIC, ORIG_NAME,
    PACK_MAGIC, PACKED, PKZIP_MAGIC, RESERVED, Res, STORED, Work, last_component, shown,
};

/// How a file's members ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    /// The input ended after a member.
    Eof,
    /// `work` answered other than `OK`.
    WorkFailed,
    /// The next member's header was an error, trailing garbage or zeros:
    /// `method` says which.
    Header,
}

impl<I: Io> Gz<I> {
    /// `input_eof`: whether the input has no more members.
    pub fn input_eof(&mut self) -> Res<bool> {
        if !self.decompress || self.last_member {
            return Ok(true);
        }
        if self.inptr == self.insize {
            if self.insize as usize != INBUFSIZ || self.fill_inbuf(true)?.is_none() {
                return Ok(true);
            }
            self.inptr = 0;
        }
        Ok(false)
    }

    /// Run the member loop for the input on `ifd`: `work` until the input
    /// has no more members, `work` fails, or a later member's header is
    /// not one.
    pub fn members(&mut self, ifd: i32, ofd: i32) -> Res<End> {
        loop {
            let r = match self.work {
                Work::Zip => self.zip(ifd, ofd)?,
                Work::Unzip => self.unzip(ifd, ofd)?,
                Work::Unpack => self.unpack(ifd, ofd)?,
                Work::Unlzw => self.unlzw(ifd, ofd)?,
                Work::Unlzh => self.unlzh(ifd, ofd)?,
                Work::Copy => self.copy(ifd, ofd)?,
            };
            if r != OK {
                return Ok(End::WorkFailed);
            }
            if self.input_eof()? {
                return Ok(End::Eof);
            }
            let m = self.get_method(ifd)?;
            self.method = m;
            if m < 0 {
                return Ok(End::Header);
            }
            self.bytes_out = 0;
        }
    }

    /// `discard_input_bytes (nbytes, flags)`: skip `n` bytes, or (for
    /// `None`) through the next zero byte, through the header CRC if the
    /// flags ask for one.
    pub fn discard_input_bytes(&mut self, n: Option<u32>, flags: u8) -> Res<()> {
        let mut left = n;
        loop {
            if left == Some(0) {
                break;
            }
            let c = self.get_byte()?;
            if flags & HEADER_CRC != 0 {
                self.updcrc(&[c]);
            }
            match &mut left {
                Some(k) => *k -= 1,
                None if c == 0 => break,
                None => {}
            }
        }
        Ok(())
    }

    /// `get_method (in)`: read the next member's magic number and header.
    /// The method; -1 for an error (said), -2 for trailing garbage, -3 for
    /// trailing zeros.
    pub fn get_method(&mut self, ifd: i32) -> Res<i32> {
        const EOF: i32 = -1;
        let mut magic = [0u8; 10];
        let imagic0: i32;
        let imagic1: i32;

        if self.force != 0 && self.to_stdout {
            imagic0 = self.try_byte()?.map_or(EOF, i32::from);
            magic[0] = imagic0 as u8;
            imagic1 = self.try_byte()?.map_or(EOF, i32::from);
            magic[1] = imagic1 as u8;
        } else {
            magic[0] = self.get_byte()?;
            imagic0 = 0;
            if magic[0] == 0 {
                imagic1 = self.try_byte()?.map_or(EOF, i32::from);
                magic[1] = imagic1 as u8;
            } else {
                magic[1] = self.get_byte()?;
                imagic1 = 0;
            }
        }
        self.method = -1;
        self.part_nb += 1;
        self.header_bytes = 0;
        self.last_member = false;

        let m2 = [magic[0], magic[1]];
        if m2 == GZIP_MAGIC || m2 == OLD_GZIP_MAGIC {
            self.method = i32::from(self.get_byte()?);
            if self.method != DEFLATED {
                let text = format!(
                    "{}: {}: unknown method {} -- not supported\n",
                    self.prog(),
                    shown(&self.ifname),
                    self.method
                );
                self.diag(&text);
                self.exit_code = ERROR;
                return Ok(-1);
            }
            self.work = Work::Unzip;
            let flags = self.get_byte()?;

            if flags & ENCRYPTED != 0 {
                let text = format!(
                    "{}: {} is encrypted -- not supported\n",
                    self.prog(),
                    shown(&self.ifname)
                );
                self.diag(&text);
                self.exit_code = ERROR;
                return Ok(-1);
            }
            if flags & RESERVED != 0 {
                let text = format!(
                    "{}: {} has flags 0x{:x} -- not supported\n",
                    self.prog(),
                    shown(&self.ifname),
                    flags
                );
                self.diag(&text);
                self.exit_code = ERROR;
                if self.force <= 1 {
                    return Ok(-1);
                }
            }
            let mut stamp = u32::from(self.get_byte()?);
            stamp |= u32::from(self.get_byte()?) << 8;
            stamp |= u32::from(self.get_byte()?) << 16;
            stamp |= u32::from(self.get_byte()?) << 24;
            if stamp != 0 && self.no_time == 0 {
                self.time_stamp = (i64::from(stamp), 0);
            }

            magic[8] = self.get_byte()?;
            magic[9] = self.get_byte()?;

            if flags & HEADER_CRC != 0 {
                magic[2] = DEFLATED as u8;
                magic[3] = flags;
                magic[4..8].copy_from_slice(&stamp.to_le_bytes());
                self.crc_reset();
                self.updcrc(&magic);
            }

            if flags & EXTRA_FIELD != 0 {
                let lenbuf = [self.get_byte()?, self.get_byte()?];
                let len = u32::from(lenbuf[0]) | (u32::from(lenbuf[1]) << 8);
                if self.verbose != 0 {
                    let text = format!(
                        "{}: {}: extra field of {len} bytes ignored\n",
                        self.prog(),
                        shown(&self.ifname)
                    );
                    self.diag(&text);
                }
                if flags & HEADER_CRC != 0 {
                    self.updcrc(&lenbuf);
                }
                self.discard_input_bytes(Some(len), flags)?;
            }

            if flags & ORIG_NAME != 0 {
                if self.no_name != 0 || (self.to_stdout && !self.list) || self.part_nb > 1 {
                    self.discard_input_bytes(None, flags)?;
                } else {
                    // Copy the stored name over the output's base name,
                    // keeping its directory.
                    let base = last_component(&self.ofname);
                    self.ofname.truncate(base);
                    loop {
                        let c = self.get_byte()?;
                        self.ofname.push(c);
                        if c == 0 {
                            break;
                        }
                        if self.ofname.len() >= MAX_PATH_LEN {
                            return self.gzip_error("corrupted input -- file name too large");
                        }
                    }
                    if flags & HEADER_CRC != 0 {
                        let stored = self.ofname[base..].to_vec();
                        self.updcrc(&stored);
                    }
                    self.ofname.pop();
                    let stored = self.ofname[base..].to_vec();
                    let p = last_component(&stored);
                    self.ofname.truncate(base);
                    self.ofname.extend_from_slice(&stored[p..]);
                }
            }

            if flags & COMMENT != 0 {
                self.discard_input_bytes(None, flags)?;
            }

            if flags & HEADER_CRC != 0 {
                let crc16 = self.getcrc() & 0xffff;
                let mut header16 = u32::from(self.get_byte()?);
                header16 |= u32::from(self.get_byte()?) << 8;
                if header16 != crc16 {
                    let text = format!(
                        "{}: {}: header checksum 0x{header16:04x} != computed checksum 0x{crc16:04x}\n",
                        self.prog(),
                        shown(&self.ifname)
                    );
                    self.diag(&text);
                    self.exit_code = ERROR;
                    if self.force <= 1 {
                        return Ok(-1);
                    }
                }
            }

            if self.part_nb == 1 {
                self.header_bytes = i64::from(self.inptr) + 2 * 4;
            }
        } else if m2 == [PKZIP_MAGIC[0], PKZIP_MAGIC[1]]
            && self.inptr == 2
            && self.inbuf[..4] == PKZIP_MAGIC
        {
            self.inptr = 0;
            self.work = Work::Unzip;
            if self.check_zipfile(ifd)? != OK {
                return Ok(-1);
            }
            self.last_member = true;
        } else if m2 == PACK_MAGIC {
            self.work = Work::Unpack;
            self.method = PACKED;
        } else if m2 == LZW_MAGIC {
            self.work = Work::Unlzw;
            self.method = COMPRESSED;
            self.last_member = true;
        } else if m2 == LZH_MAGIC {
            self.work = Work::Unlzh;
            self.method = LZHED;
            self.last_member = true;
        } else if self.force != 0 && self.to_stdout && !self.list {
            self.method = STORED;
            self.work = Work::Copy;
            if imagic1 != EOF {
                self.inptr = self.inptr.wrapping_sub(1);
            }
            self.last_member = true;
            if imagic0 != EOF {
                self.write_buf(1, &magic[..1])?;
            }
        }
        if self.method >= 0 {
            return Ok(self.method);
        }

        if self.part_nb == 1 {
            let text = format!(
                "\n{}: {}: not in gzip format\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
            self.exit_code = ERROR;
            return Ok(-1);
        }
        if magic[0] == 0 {
            let mut inbyte = imagic1;
            while inbyte == 0 {
                inbyte = self.try_byte()?.map_or(EOF, i32::from);
            }
            if inbyte == EOF {
                if self.verbose != 0 {
                    let text = format!(
                        "\n{}: {}: decompression OK, trailing zero bytes ignored\n",
                        self.prog(),
                        shown(&self.ifname)
                    );
                    self.warn(&text);
                }
                return Ok(-3);
            }
        }
        let text = format!(
            "\n{}: {}: decompression OK, trailing garbage ignored\n",
            self.prog(),
            shown(&self.ifname)
        );
        self.warn(&text);
        Ok(-2)
    }
}
