//! `zip.c`: one gzip member -- the header, deflate's output, and the CRC
//! and length that end it.

use crate::{DEFLATED, GZIP_MAGIC, Gz, Io, OK, ORIG_NAME, OS_CODE, Res, last_component, shown};

/// The extra-flags byte: the slowest and the fastest level.
const SLOW: u8 = 2;
const FAST: u8 = 4;

impl<I: Io> Gz<I> {
    /// `zip (in, out)`: compress `ifd` to `ofd` as one gzip member.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for a read or a write that failed.
    pub fn zip(&mut self, ifd: i32, ofd: i32) -> Res<i32> {
        let mut flags: u8 = 0;
        let mut deflate_flags: u8 = 0;

        self.ifd = ifd;
        self.ofd = ofd;
        self.outcnt = 0;

        self.method = DEFLATED;
        self.put_byte(GZIP_MAGIC[0])?;
        self.put_byte(GZIP_MAGIC[1])?;
        self.put_byte(DEFLATED as u8)?;

        if self.save_orig_name {
            flags |= ORIG_NAME;
        }
        self.put_byte(flags)?;
        let stamp = if self.time_stamp.1 < 0 {
            0
        } else if 0 < self.time_stamp.0 && self.time_stamp.0 <= 0xffff_ffff {
            self.time_stamp.0 as u32
        } else {
            // Timestamp 0 is meant to warn too: the format reserves it.
            self.warning("file timestamp out of range for gzip format");
            0
        };
        self.put_long(stamp)?;

        self.crc_reset();

        self.bi_init();
        self.ct_init();
        if self.level == 1 {
            deflate_flags |= FAST;
        } else if self.level == 9 {
            deflate_flags |= SLOW;
        }

        self.put_byte(deflate_flags)?;
        self.put_byte(OS_CODE)?;

        if self.save_orig_name {
            let base = self.ifname[last_component(&self.ifname)..].to_vec();
            for &b in &base {
                self.put_byte(b)?;
            }
            self.put_byte(0)?;
        }
        self.header_bytes = i64::from(self.outcnt);

        self.deflate(self.level)?;

        if self.ifile_size != -1 && self.bytes_in != self.ifile_size {
            let text = format!(
                "{}: {}: file size changed while zipping\n",
                self.prog(),
                shown(&self.ifname)
            );
            self.diag(&text);
        }

        self.put_long(self.getcrc())?;
        self.put_long(self.bytes_in as u32)?;
        self.header_bytes += 2 * 4;

        self.flush_outbuf()?;
        Ok(OK)
    }

    /// `file_read (buf, size)`: read input for deflate straight into `buf`,
    /// running it through the CRC and counting it. 0 at the end.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for a read that failed.
    pub fn file_read(&mut self, buf: &mut [u8]) -> Res<usize> {
        let len = match self.io.read(self.ifd, buf) {
            Ok(n) => n,
            Err(e) => return self.read_error(e),
        };
        if len == 0 {
            return Ok(0);
        }
        self.updcrc(buf.get(..len).unwrap_or(&[]));
        self.bytes_in += len as i64;
        Ok(len)
    }
}
