//! GNU gzip 1.12, as Ubuntu 24.04 ships it (`1.12-1ubuntu3.2`), ported.
//!
//! `gzip` compresses with gzip's own deflate and writes the `.gz` format;
//! it decompresses that, pkzip's first entry, `pack`, `compress` (`.Z`) and
//! SCO's `-lh5-` LZH. Upstream is C with every piece of state global, and
//! the compressed output depends on more of that state than its interfaces
//! show -- see "What the globals carry" below -- so this port keeps it as one
//! value, [`Gz`], whose fields are the C globals, and whose methods are the
//! C functions, file by file:
//!
//! | upstream | here |
//! |---|---|
//! | `util.c`, `gzip.h` | this file |
//! | `bits.c`, `trees.c`, `deflate.c` | [`bits`], [`trees`], [`deflate`] |
//! | `zip.c` | [`zip`] |
//! | `inflate.c`, `unzip.c` | [`inflate`], [`unzip`] |
//! | `unpack.c`, `unlzw.c`, `unlzh.c` | [`unpack`], [`unlzw`], [`unlzh`] |
//! | `gzip.c` | [`driver`] (Unix only: it is the part that touches files) |
//!
//! `scripts/gzip-diff.sh` holds the program to Ubuntu's `/usr/bin/gzip`,
//! byte for byte, compressed output included.
//!
//! # What the globals carry
//!
//! Three of upstream's buffers outlive the file they were filled for, and
//! what is left in them shapes the next file's output:
//!
//! * **The sliding window.** Near the end of its input, deflate's match
//!   finder reads past the last byte it was given -- up to a match's length
//!   -- and compares whatever the window holds there: zeros at first, then
//!   the previous file's bytes. A match that runs on into those bytes can
//!   beat another before both are cut to the input's end, and the one chosen
//!   decides the distance written. So `gzip a b` compresses `b` against what
//!   `a` left behind, and the window here is one buffer for the whole run.
//! * **`prev`**, deflate's hash chains, which `compress` decoding uses as its
//!   prefix table and LZH decoding as its tree links. A corrupt LZH file can
//!   reach links an earlier file left.
//! * **The static Huffman trees**, built once per process.
//!
//! None of this is visible in a correct file's decompressed bytes, all of it
//! in the compressed ones.
//!
//! # Deliberate differences
//!
//! 1. **A name in a diagnostic** that holds an unprintable byte is printed
//!    with that byte as an octal escape (`quoting::escape_unprintable`),
//!    where upstream prints it raw -- so a file name cannot write a line of
//!    its own into the error stream. Every printable name is printed as
//!    upstream prints it. The `-l` listing on standard output is data, and
//!    stays raw.
//! 2. **Where upstream indexes outside its tables** on corrupt input -- an
//!    LZH bit-length list longer than its array, a tree link past the end
//!    of `prev` -- which is undefined behaviour in C, this port reports the
//!    data corrupt (`Bad table`, as LZH's other table errors do). No correct
//!    file reaches those paths.
//! 3. **`gunzip`, `zcat` and `uncompress`** are this program under those
//!    names, doing what Ubuntu's scripts of those names do: their own
//!    `--help` and `--version`, and otherwise `gzip -d` (`gzip -cd` for
//!    `zcat`) with the same arguments. The scripts' other twins (`zgrep`,
//!    `zdiff`, `zless` and the rest) are shell scripts, not ported.

#![allow(
    // The C's integer arithmetic, transcribed: every place it wraps is
    // spelt `wrapping_*`, every shift and cast is the C's own, and the
    // bounds it relies on are argued where they are not obvious.
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    // Transcribed table code: each module says what bounds its indexes,
    // and every index a file's contents steer in the decompressors is
    // checked (`get`), so that no input panics the program -- which the
    // fuzz harness (`fuzz/`) holds it to.
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    // A port keeps upstream's function boundaries, so that each can be read
    // beside its original: some are long.
    clippy::too_many_lines,
    // Upstream's names (`ct_tally`, `bi_buf`, `insize`): kept, to be
    // searched for.
    clippy::similar_names,
    clippy::doc_markdown
)]

pub mod bits;
pub mod deflate;
#[cfg(unix)]
pub mod driver;
pub mod inflate;
pub mod member;
pub mod trees;
pub mod unlzh;
pub mod unlzw;
pub mod unpack;
pub mod unzip;
pub mod zip;

/// Return codes, and the exit statuses they become.
pub const OK: i32 = 0;
/// Something went wrong.
pub const ERROR: i32 = 1;
/// Something was skipped or not quite right; the work was done.
pub const WARNING: i32 = 2;

/// Compression methods.
pub const STORED: i32 = 0;
/// `compress`'s LZW.
pub const COMPRESSED: i32 = 1;
/// `pack`'s Huffman coding.
pub const PACKED: i32 = 2;
/// SCO's LZH.
pub const LZHED: i32 = 3;
/// Deflate, the method gzip writes.
pub const DEFLATED: i32 = 8;

/// The input buffer's size, and the slack `unlzw` reads past it.
pub const INBUFSIZ: usize = 0x40000;
/// `unlzw` keeps up to this many unread bytes below a fresh read.
pub const INBUF_EXTRA: usize = 64;
/// The output buffer's size, and the slack `unlzw` writes past it.
pub const OUTBUFSIZ: usize = 0x40000;
/// `unlzw`'s overrun past [`OUTBUFSIZ`].
pub const OUTBUF_EXTRA: usize = 2048;
/// The distance buffer's entries.
pub const DIST_BUFSIZE: usize = 0x8000;
/// Half the sliding window; matches reach at most this far back.
pub const WSIZE: usize = 0x8000;
/// `compress`'s largest code width, and so `prev`'s size: `1 << BITS`.
pub const BITS: i32 = 16;

/// Deflate's shortest and longest match.
pub const MIN_MATCH: u32 = 3;
/// The longest match.
pub const MAX_MATCH: u32 = 258;
/// The lookahead deflate keeps, except at the end of its input.
pub const MIN_LOOKAHEAD: u32 = MAX_MATCH + MIN_MATCH + 1;
/// The farthest back a match may reach.
pub const MAX_DIST: u32 = WSIZE as u32 - MIN_LOOKAHEAD;

/// Magic numbers: gzip, gzip 0.5, pack, `compress`, SCO LZH, pkzip.
pub const GZIP_MAGIC: [u8; 2] = [0o37, 0o213];
/// gzip 0.5, which was freeze 1.x.
pub const OLD_GZIP_MAGIC: [u8; 2] = [0o37, 0o236];
/// `pack`.
pub const PACK_MAGIC: [u8; 2] = [0o37, 0o36];
/// `compress`.
pub const LZW_MAGIC: [u8; 2] = [0o37, 0o235];
/// SCO's LZH.
pub const LZH_MAGIC: [u8; 2] = [0o37, 0o240];
/// pkzip's local header.
pub const PKZIP_MAGIC: [u8; 4] = [0o120, 0o113, 0o3, 0o4];

/// The gzip header's flag bits.
pub const ASCII_FLAG: u8 = 0x01;
/// A CRC-16 of the header follows it.
pub const HEADER_CRC: u8 = 0x02;
/// An extra field is present.
pub const EXTRA_FIELD: u8 = 0x04;
/// The original file name is present.
pub const ORIG_NAME: u8 = 0x08;
/// A comment is present.
pub const COMMENT: u8 = 0x10;
/// Encrypted.
pub const ENCRYPTED: u8 = 0x20;
/// Bits 6 and 7, reserved.
pub const RESERVED: u8 = 0xC0;

/// The operating system byte gzip writes: Unix.
pub const OS_CODE: u8 = 0x03;

/// `ifname` and `ofname`'s size in upstream, which its length checks are
/// written against.
pub const MAX_PATH_LEN: usize = 1024;

/// How a run stops early: upstream's `abort_gzip` (remove the output file
/// being written, exit 1) or `do_exit` with a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// `abort_gzip`: the diagnostic is printed; remove the output and exit 1.
    Abort,
    /// `do_exit (status)`: exit now, removing nothing.
    Exit(i32),
}

/// What a step returns: a value, or the run stopping.
pub type Res<T> = Result<T, Stop>;

/// The process's descriptors and streams, as upstream reaches them.
pub trait Io {
    /// `read_buffer`: `read (fd, buf)`, and once more with `O_NONBLOCK`
    /// cleared if that answered `EAGAIN` on a descriptor opened
    /// non-blocking. The byte count, or the `errno`.
    ///
    /// # Errors
    ///
    /// The `errno` the read failed with.
    fn read(&mut self, fd: i32, buf: &mut [u8]) -> Result<usize, i32>;
    /// `write (fd, buf)`: the byte count, or the `errno`.
    ///
    /// # Errors
    ///
    /// The `errno` the write failed with.
    fn write(&mut self, fd: i32, buf: &[u8]) -> Result<usize, i32>;
    /// Text for standard error, which is unbuffered.
    fn diag(&mut self, text: &[u8]);
    /// Text for standard output through stdio's buffer: `printf`.
    fn print(&mut self, text: &[u8]);
    /// `strerror (errno)`.
    fn strerror(&self, errno: i32) -> String;
}

/// The process-wide state upstream keeps in globals. See the module docs on
/// why it is one value and why it outlives a file.
pub struct Gz<I: Io> {
    /// The descriptors and streams.
    pub io: I,

    // --- gzip.h's buffers -------------------------------------------------
    /// Input, and `trees.c`'s literal buffer (`l_buf`).
    pub inbuf: Vec<u8>,
    /// Compressed output; `unpack`'s prefix table and `unlzh`'s `c_len`.
    pub outbuf: Vec<u8>,
    /// Deflate's distances; `unlzh`'s `c_table`.
    pub d_buf: Vec<u16>,
    /// The sliding window; decompressed output; `unlzw`'s suffix table.
    pub window: Vec<u8>,
    /// Deflate's hash links, with its hash heads at `WSIZE..`;
    /// `unlzw`'s prefix table; `unlzh`'s `left` and (at `WSIZE..`) `right`.
    pub prev: Vec<u16>,
    /// `unlzw`'s output stack, which upstream keeps in `d_buf`'s memory.
    pub lzw_stack: Vec<u8>,

    /// Valid bytes in `inbuf`.
    pub insize: u32,
    /// The next byte of `inbuf` to read.
    pub inptr: u32,
    /// Bytes in the output buffer (or the window, decompressing).
    pub outcnt: u32,

    /// Input and output byte counts for the current file.
    pub bytes_in: i64,
    /// Output bytes.
    pub bytes_out: i64,
    /// The gzip header's bytes, for the ratio.
    pub header_bytes: i64,
    /// The CRC shift register.
    pub crc: u32,

    /// The input and output descriptors.
    pub ifd: i32,
    /// The output descriptor.
    pub ofd: i32,
    /// The input's name (or `stdin`) and the output's (or `stdout`).
    pub ifname: Vec<u8>,
    /// The output's name.
    pub ofname: Vec<u8>,
    /// What diagnostics call the program: `argv[0]`'s last component.
    pub program_name: Vec<u8>,

    // --- options ----------------------------------------------------------
    /// `-c`, and implied by `-l`, `-t` and standard input.
    pub to_stdout: bool,
    /// `-d`.
    pub decompress: bool,
    /// `-f`, counted: `-ff` also passes a header with reserved flags.
    pub force: i32,
    /// `-t`, and `-l`: decompress without writing.
    pub test: bool,
    /// `-l`.
    pub list: bool,
    /// `-q`.
    pub quiet: bool,
    /// `-v`, counted.
    pub verbose: i32,
    /// `-n` and `-N` (and `-m`/`-M`): -1 until decided.
    pub no_name: i32,
    /// The timestamp half of `-n`/`-N`.
    pub no_time: i32,
    /// The compression level, 1 to 9.
    pub level: i32,
    /// `--rsyncable`.
    pub rsync: bool,
    /// `-b`'s value, and `.Z` headers' code width.
    pub maxbits: i32,

    // --- per-file state ---------------------------------------------------
    /// The method of the member being read or written.
    pub method: i32,
    /// The exit status so far.
    pub exit_code: i32,
    /// Whether to store the original name in the header.
    pub save_orig_name: bool,
    /// The input's size if it is a regular file, else -1.
    pub ifile_size: i64,
    /// The timestamp to store or restore: seconds and nanoseconds, the
    /// nanoseconds negative when unknown. Upstream's is a zeroed global and
    /// its seconds outlive the file, which `-lv` on standard input shows.
    pub time_stamp: (i64, i64),
    /// The member being read is the last one (zip, `.Z`, LZH, stored).
    pub last_member: bool,
    /// Members read so far in this file.
    pub part_nb: i32,
    /// What the next member is read or written with.
    pub work: Work,
    /// The CRC `unzip` found, for `-l`.
    pub unzip_crc: u32,

    /// Deflate's state.
    pub lm: deflate::Lm,
    /// The Huffman trees' state.
    pub ct: trees::Ct,
    /// The bit writer's state.
    pub bi: bits::Bi,
    /// Inflate's bit buffer.
    pub inf: inflate::Inf,
    /// `unpack`'s tables.
    pub pk: unpack::Pk,
    /// `unlzh`'s state.
    pub lzh: unlzh::Lzh,
    /// `unlzw`'s block mode, which upstream keeps from file to file.
    pub block_mode: bool,
    /// `unzip`'s pkzip flags.
    pub zp: unzip::Zp,
}

/// What a member is read or written with: upstream's `work` pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// Compress.
    Zip,
    /// gzip or pkzip, decompressed.
    Unzip,
    /// `pack`.
    Unpack,
    /// `compress`.
    Unlzw,
    /// LZH.
    Unlzh,
    /// Pass the input through: `zcat -f` of something not compressed.
    Copy,
}

/// The CRC-32 table: gzip's `crc_32_tab`, built as `makecrc.c` built it.
pub static CRC_32_TAB: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 0 {
                c >> 1
            } else {
                0xedb8_8320 ^ (c >> 1)
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// How a name is shown in a diagnostic: as itself, with any unprintable
/// byte as an octal escape. Deliberate difference 1 in the module docs.
#[must_use]
pub fn shown(name: &[u8]) -> String {
    quoting::escape_unprintable(name)
}

/// `last_component`: what follows the last `/`, ignoring slashes at the
/// end -- the byte offset where it starts.
#[must_use]
pub fn last_component(name: &[u8]) -> usize {
    let mut base = 0;
    let mut saw_slash = false;
    for (i, &b) in name.iter().enumerate() {
        if b == b'/' {
            saw_slash = true;
        } else if saw_slash {
            base = i;
            saw_slash = false;
        }
    }
    // A leading run of slashes is skipped as well.
    if base == 0 {
        name.iter().take_while(|&&b| b == b'/').count()
    } else {
        base
    }
}

impl<I: Io> Gz<I> {
    /// The state a process starts with: upstream's static initialisers.
    pub fn new(io: I, program_name: Vec<u8>) -> Self {
        Gz {
            io,
            inbuf: vec![0; INBUFSIZ + INBUF_EXTRA],
            outbuf: vec![0; OUTBUFSIZ + OUTBUF_EXTRA],
            d_buf: vec![0; DIST_BUFSIZE],
            window: vec![0; 2 * WSIZE],
            prev: vec![0; 1 << BITS],
            lzw_stack: vec![0; 2 * DIST_BUFSIZE - 2],
            insize: 0,
            inptr: 0,
            outcnt: 0,
            bytes_in: 0,
            bytes_out: 0,
            header_bytes: 0,
            crc: 0xffff_ffff,
            ifd: 0,
            ofd: 0,
            ifname: Vec::new(),
            ofname: Vec::new(),
            program_name,
            to_stdout: false,
            decompress: false,
            force: 0,
            test: false,
            list: false,
            quiet: false,
            verbose: 0,
            no_name: -1,
            no_time: -1,
            level: 6,
            rsync: false,
            maxbits: BITS,
            method: DEFLATED,
            exit_code: OK,
            save_orig_name: false,
            ifile_size: 0,
            time_stamp: (0, 0),
            last_member: false,
            part_nb: 0,
            work: Work::Zip,
            unzip_crc: 0,
            lm: deflate::Lm::default(),
            ct: trees::Ct::default(),
            bi: bits::Bi::default(),
            inf: inflate::Inf::default(),
            pk: unpack::Pk::default(),
            lzh: unlzh::Lzh::default(),
            block_mode: true,
            zp: unzip::Zp::default(),
        }
    }

    /// The program's name, as diagnostics show it.
    #[must_use]
    pub fn prog(&self) -> String {
        shown(&self.program_name)
    }

    /// `fprintf (stderr, ...)`.
    pub fn diag(&mut self, text: &str) {
        self.io.diag(text.as_bytes());
    }

    /// `perror (s)`, with the `errno` given: `s: message`, or just the
    /// message when `s` is empty.
    pub fn perror(&mut self, s: &[u8], errno: i32) {
        let msg = self.io.strerror(errno);
        let text = if s.is_empty() {
            format!("{msg}\n")
        } else {
            format!("{}: {msg}\n", shown(s))
        };
        self.diag(&text);
    }

    /// `WARN`: say it unless `-q`, and make the status a warning if it is
    /// still success.
    pub fn warn(&mut self, text: &str) {
        if !self.quiet {
            self.diag(text);
        }
        if self.exit_code == OK {
            self.exit_code = WARNING;
        }
    }

    /// `warning (m)`.
    pub fn warning(&mut self, m: &str) {
        let text = format!("{}: {}: warning: {m}\n", self.prog(), shown(&self.ifname));
        self.warn(&text);
    }

    /// `gzip_error (m)`: name the input and the fault, then abort.
    ///
    /// # Errors
    ///
    /// Always: [`Stop::Abort`].
    pub fn gzip_error<T>(&mut self, m: &str) -> Res<T> {
        let text = format!("\n{}: {}: {m}\n", self.prog(), shown(&self.ifname));
        self.diag(&text);
        Err(Stop::Abort)
    }

    /// `xalloc_die`.
    ///
    /// # Errors
    ///
    /// Always: [`Stop::Abort`].
    pub fn xalloc_die<T>(&mut self) -> Res<T> {
        let text = format!("\n{}: memory_exhausted\n", self.prog());
        self.diag(&text);
        Err(Stop::Abort)
    }

    /// `read_error`, for a read that failed with `errno` -- or, for 0,
    /// input that ended early.
    ///
    /// # Errors
    ///
    /// Always: [`Stop::Abort`].
    pub fn read_error<T>(&mut self, errno: i32) -> Res<T> {
        let text = format!("\n{}: ", self.prog());
        self.diag(&text);
        if errno == 0 {
            let text = format!("{}: unexpected end of file\n", shown(&self.ifname));
            self.diag(&text);
        } else {
            let name = self.ifname.clone();
            self.perror(&name, errno);
        }
        Err(Stop::Abort)
    }

    /// `write_error`, for a write that failed with `errno`.
    ///
    /// # Errors
    ///
    /// Always: [`Stop::Abort`].
    pub fn write_error<T>(&mut self, errno: i32) -> Res<T> {
        let text = format!("\n{}: ", self.prog());
        self.diag(&text);
        let name = self.ofname.clone();
        self.perror(&name, errno);
        Err(Stop::Abort)
    }

    /// `updcrc (s, n)`: run `s` through the CRC register and answer the
    /// CRC so far.
    pub fn updcrc(&mut self, s: &[u8]) -> u32 {
        let mut c = self.crc;
        for &b in s {
            c = CRC_32_TAB[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
        }
        self.crc = c;
        c ^ 0xffff_ffff
    }

    /// `updcrc (NULL, 0)`: start the register again.
    pub fn crc_reset(&mut self) -> u32 {
        self.crc = 0xffff_ffff;
        0
    }

    /// `getcrc`: the CRC so far.
    #[must_use]
    pub fn getcrc(&self) -> u32 {
        self.crc ^ 0xffff_ffff
    }

    /// `clear_bufs`.
    pub fn clear_bufs(&mut self) {
        self.outcnt = 0;
        self.insize = 0;
        self.inptr = 0;
        self.bytes_in = 0;
        self.bytes_out = 0;
    }

    /// `fill_inbuf (eof_ok)`: refill the input buffer, which is empty, and
    /// answer its first byte -- or `None` at the end of the input when
    /// `eof_ok`. At the end otherwise, the window is flushed and the run
    /// aborts.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`] for a read that failed, or input that ended where
    /// more was needed.
    pub fn fill_inbuf(&mut self, eof_ok: bool) -> Res<Option<u8>> {
        self.insize = 0;
        loop {
            let at = self.insize as usize;
            let len = match self.io.read(self.ifd, &mut self.inbuf[at..INBUFSIZ]) {
                Ok(n) => n,
                Err(e) => return self.read_error(e),
            };
            if len == 0 {
                break;
            }
            self.insize += len as u32;
            if self.insize as usize >= INBUFSIZ {
                break;
            }
        }
        if self.insize == 0 {
            if eof_ok {
                return Ok(None);
            }
            self.flush_window()?;
            return self.read_error(0);
        }
        self.bytes_in += i64::from(self.insize);
        self.inptr = 1;
        Ok(Some(self.inbuf[0]))
    }

    /// `get_byte`: the next input byte, refilling as needed; the end of the
    /// input aborts.
    ///
    /// # Errors
    ///
    /// As [`Gz::fill_inbuf`].
    pub fn get_byte(&mut self) -> Res<u8> {
        if self.inptr < self.insize {
            let b = self.inbuf[self.inptr as usize];
            self.inptr += 1;
            Ok(b)
        } else {
            // `fill_inbuf (0)` never answers the end: it aborts there.
            Ok(self.fill_inbuf(false)?.unwrap_or(0))
        }
    }

    /// `try_byte`: the next input byte, or `None` at the end.
    ///
    /// # Errors
    ///
    /// As [`Gz::fill_inbuf`], for a read that failed.
    pub fn try_byte(&mut self) -> Res<Option<u8>> {
        if self.inptr < self.insize {
            let b = self.inbuf[self.inptr as usize];
            self.inptr += 1;
            Ok(Some(b))
        } else {
            self.fill_inbuf(true)
        }
    }

    /// `write_buf (fd, buf)`: count the bytes as output and, unless
    /// testing, write them all.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`] for a write that failed.
    pub fn write_buf(&mut self, fd: i32, buf: &[u8]) -> Res<()> {
        self.bytes_out += buf.len() as i64;
        if self.test {
            return Ok(());
        }
        let mut rest = buf;
        while !rest.is_empty() {
            match self.io.write(fd, rest) {
                Ok(n) => rest = rest.get(n..).unwrap_or(&[]),
                Err(e) => return self.write_error(e),
            }
        }
        Ok(())
    }

    /// `write_buf` of `outbuf[from..to]`.
    fn write_outbuf(&mut self, fd: i32, from: usize, to: usize) -> Res<()> {
        let buf = std::mem::take(&mut self.outbuf);
        let r = self.write_buf(fd, &buf[from..to]);
        self.outbuf = buf;
        r
    }

    /// `write_buf` of `window[from..to]`.
    fn write_window(&mut self, fd: i32, from: usize, to: usize) -> Res<()> {
        let buf = std::mem::take(&mut self.window);
        let r = self.write_buf(fd, &buf[from..to]);
        self.window = buf;
        r
    }

    /// `flush_outbuf`: write the compressed output buffered so far.
    ///
    /// # Errors
    ///
    /// As [`Gz::write_buf`].
    pub fn flush_outbuf(&mut self) -> Res<()> {
        if self.outcnt == 0 {
            return Ok(());
        }
        let n = self.outcnt as usize;
        self.write_outbuf(self.ofd, 0, n)?;
        self.outcnt = 0;
        Ok(())
    }

    /// `flush_window`: run the decompressed bytes in the window through the
    /// CRC and write them.
    ///
    /// # Errors
    ///
    /// As [`Gz::write_buf`].
    pub fn flush_window(&mut self) -> Res<()> {
        if self.outcnt == 0 {
            return Ok(());
        }
        let n = self.outcnt as usize;
        let window = std::mem::take(&mut self.window);
        self.updcrc(&window[..n]);
        self.window = window;
        self.write_window(self.ofd, 0, n)?;
        self.outcnt = 0;
        Ok(())
    }

    /// `put_byte`: one byte of compressed output.
    ///
    /// # Errors
    ///
    /// As [`Gz::write_buf`], when the buffer fills.
    pub fn put_byte(&mut self, c: u8) -> Res<()> {
        self.outbuf[self.outcnt as usize] = c;
        self.outcnt += 1;
        if self.outcnt as usize == OUTBUFSIZ {
            self.flush_outbuf()?;
        }
        Ok(())
    }

    /// `put_short`: 16 bits, low byte first.
    ///
    /// # Errors
    ///
    /// As [`Gz::put_byte`].
    pub fn put_short(&mut self, w: u32) -> Res<()> {
        if (self.outcnt as usize) < OUTBUFSIZ - 2 {
            self.outbuf[self.outcnt as usize] = (w & 0xff) as u8;
            self.outbuf[self.outcnt as usize + 1] = ((w as u16) >> 8) as u8;
            self.outcnt += 2;
            Ok(())
        } else {
            self.put_byte((w & 0xff) as u8)?;
            self.put_byte(((w as u16) >> 8) as u8)
        }
    }

    /// `put_long`: 32 bits, low half first.
    ///
    /// # Errors
    ///
    /// As [`Gz::put_byte`].
    pub fn put_long(&mut self, n: u32) -> Res<()> {
        self.put_short(n & 0xffff)?;
        self.put_short(n >> 16)
    }

    /// `put_ubyte`: one byte of decompressed output, into the window.
    ///
    /// # Errors
    ///
    /// As [`Gz::flush_window`], when the window fills.
    pub fn put_ubyte(&mut self, c: u8) -> Res<()> {
        self.window[self.outcnt as usize] = c;
        self.outcnt += 1;
        if self.outcnt as usize == WSIZE {
            self.flush_window()?;
        }
        Ok(())
    }

    /// `copy (in, out)`: pass the input through unchanged.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`] for a read or a write that failed.
    pub fn copy(&mut self, ifd: i32, ofd: i32) -> Res<i32> {
        while self.insize > self.inptr {
            let (from, to) = (self.inptr as usize, self.insize as usize);
            let buf = std::mem::take(&mut self.inbuf);
            let r = self.write_buf(ofd, &buf[from..to]);
            self.inbuf = buf;
            r?;
            let got = match self.io.read(ifd, &mut self.inbuf[..INBUFSIZ]) {
                Ok(n) => n,
                Err(e) => return self.read_error(e),
            };
            self.bytes_in += got as i64;
            self.insize = got as u32;
            self.inptr = 0;
        }
        Ok(OK)
    }

    /// `display_ratio`: `num/den` as a percentage, six characters wide.
    #[must_use]
    pub fn ratio(num: i64, den: i64) -> String {
        let r = if den == 0 {
            0.0
        } else {
            100.0 * num as f64 / den as f64
        };
        format!("{r:5.1}%")
    }

    /// `fprint_off`: `offset`, right-aligned in `width`.
    #[must_use]
    pub fn off(offset: i64, width: usize) -> String {
        format!("{offset:>width$}")
    }
}

/// `gzip -c -n -LEVEL` of `input` read from a pipe: the `.gz` bytes it
/// writes, byte for byte, or how the run stopped.
///
/// # Errors
///
/// [`Stop::Abort`] for a level outside 1 to 9; nothing else can fail in
/// memory.
pub fn compress(input: &[u8], level: i32) -> Result<Vec<u8>, Stop> {
    let mut gz = Gz::new(
        memio::MemIo::with_input(input, usize::MAX),
        b"gzip".to_vec(),
    );
    gz.to_stdout = true;
    gz.no_name = 1;
    gz.no_time = 1;
    gz.level = level;
    gz.ifname = b"stdin".to_vec();
    gz.ofname = b"stdout".to_vec();
    // A pipe: no size to check, no time to store.
    gz.ifile_size = -1;
    gz.time_stamp = (0, -1);
    gz.clear_bufs();
    gz.members(0, 1)?;
    Ok(gz.io.out.remove(&1).unwrap_or_default())
}

/// `gzip -dc` of `input` read from a pipe -- `-dcf`, `-dcff` for `force`
/// 1 and 2: what it writes to standard output and to standard error, and
/// its exit status. `treat_stdin`, for decompression, in memory.
#[must_use]
pub fn zcat(input: &[u8], force: i32) -> (Vec<u8>, Vec<u8>, i32) {
    let mut gz = Gz::new(
        memio::MemIo::with_input(input, usize::MAX),
        b"gzip".to_vec(),
    );
    gz.decompress = true;
    gz.to_stdout = true;
    gz.force = force;
    gz.no_name = 1;
    gz.no_time = 1;
    gz.ifname = b"stdin".to_vec();
    gz.ofname = b"stdout".to_vec();
    gz.ifile_size = -1;
    gz.time_stamp.1 = -1;
    gz.clear_bufs();
    let run = |gz: &mut Gz<memio::MemIo>| -> Res<i32> {
        let m = gz.get_method(0)?;
        gz.method = m;
        if m >= 0 {
            gz.members(0, 1)?;
        }
        Ok(gz.exit_code)
    };
    let status = match run(&mut gz) {
        Ok(s) | Err(Stop::Exit(s)) => s,
        Err(Stop::Abort) => ERROR,
    };
    (
        gz.io.out.remove(&1).unwrap_or_default(),
        std::mem::take(&mut gz.io.err),
        status,
    )
}

pub mod memio {
    //! An [`Io`] over memory: for [`compress`](crate::compress),
    //! [`zcat`](crate::zcat), the tests and the fuzz harness.

    use super::Io;
    use std::collections::HashMap;

    /// Descriptor 0 reads `input` in chunks of `chunk`; descriptor 1 is
    /// collected; diagnostics and printed text are kept.
    #[derive(Default)]
    pub struct MemIo {
        pub input: Vec<u8>,
        pub at: usize,
        pub chunk: usize,
        pub out: HashMap<i32, Vec<u8>>,
        pub err: Vec<u8>,
        pub printed: Vec<u8>,
    }

    impl MemIo {
        pub fn with_input(input: &[u8], chunk: usize) -> Self {
            MemIo {
                input: input.to_vec(),
                chunk,
                ..MemIo::default()
            }
        }
    }

    impl Io for MemIo {
        fn read(&mut self, _fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
            let rest = self.input.get(self.at..).unwrap_or(&[]);
            let n = rest.len().min(buf.len()).min(self.chunk.max(1));
            buf[..n].copy_from_slice(&rest[..n]);
            self.at += n;
            Ok(n)
        }
        fn write(&mut self, fd: i32, buf: &[u8]) -> Result<usize, i32> {
            self.out.entry(fd).or_default().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn diag(&mut self, text: &[u8]) {
            self.err.extend_from_slice(text);
        }
        fn print(&mut self, text: &[u8]) {
            self.printed.extend_from_slice(text);
        }
        fn strerror(&self, errno: i32) -> String {
            format!("error {errno}")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    /// `yes 'hello, world' | head -3 | gzip -9 -n`, and `-1`, as Ubuntu's
    /// gzip 1.12 writes them.
    #[test]
    fn compressed_bytes_are_ubuntu_s() {
        let text = b"hello, world\n".repeat(3);
        let body = [
            0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0xd7, 0x51, 0x28, 0xcf, 0x2f, 0xca, 0x49, 0xe1, 0xca,
            0xc0, 0xc9, 0x01, 0x00, 0xf0, 0x4c, 0xf9, 0xe6, 0x27, 0x00, 0x00, 0x00,
        ];
        for (level, xfl) in [(9, 2), (1, 4)] {
            let mut want = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, xfl, 0x03];
            want.extend_from_slice(&body);
            assert_eq!(compress(&text, level).unwrap(), want, "level {level}");
        }
    }

    /// `seq 1 1000 | gzip -6 -n`: 1848 bytes, which decompress back.
    #[test]
    fn a_longer_input_round_trips() {
        let seq: Vec<u8> = (1..=1000)
            .flat_map(|i| format!("{i}\n").into_bytes())
            .collect();
        let gz = compress(&seq, 6).unwrap();
        assert_eq!(gz.len(), 1848);
        let (out, err, status) = zcat(&gz, 0);
        assert_eq!((out, err, status), (seq, Vec::new(), OK));
    }

    /// Every level, inputs of awkward sizes -- around the window's halves
    /// and the literal buffer -- and of every kind of redundancy.
    #[test]
    fn every_level_round_trips() {
        let mut seed: u32 = 12345;
        let mut next = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) as u8
        };
        let noise: Vec<u8> = (0..70_000).map(|_| next()).collect();
        let lumpy: Vec<u8> = (0..70_000)
            .map(|i| if (i / 97) % 3 == 0 { next() % 4 } else { b'a' })
            .collect();
        for data in [
            &noise[..],
            &lumpy[..],
            &noise[..32_768],
            &lumpy[..65_536],
            &lumpy[..65_537],
            &[][..],
        ] {
            for level in 1..=9 {
                let gz = compress(data, level).unwrap();
                let (out, err, status) = zcat(&gz, 0);
                assert_eq!(
                    status,
                    OK,
                    "level {level}: {}",
                    String::from_utf8_lossy(&err)
                );
                assert!(out == data, "level {level}, {} bytes", data.len());
            }
        }
    }

    /// Upstream's own `pack` sample.
    #[test]
    fn pack_is_read() {
        let banana = [
            0x1f, 0x1e, 0x00, 0x00, 0x00, 0x06, 0x03, 0x01, 0x01, 0x00, 0x61, 0x6e, 0x62, 0x16,
            0xc8,
        ];
        assert_eq!(zcat(&banana, 0), (b"banana".to_vec(), Vec::new(), OK));
    }

    /// Damage is reported as upstream reports it, and what was decoded
    /// before it is kept on standard output.
    #[test]
    fn damage_is_reported() {
        let mut gz = compress(b"hello, world\n", 6).unwrap();
        let n = gz.len();
        gz[n - 8] ^= 1;
        let (out, err, status) = zcat(&gz, 0);
        assert_eq!(out, b"hello, world\n");
        assert_eq!(err, b"\ngzip: stdin: invalid compressed data--crc error\n");
        assert_eq!(status, ERROR);

        let (out, err, status) = zcat(b"plain", 0);
        assert_eq!((out, status), (Vec::new(), ERROR));
        assert_eq!(err, b"\ngzip: stdin: not in gzip format\n");

        // `-dcf` passes what is not compressed through.
        assert_eq!(zcat(b"plain", 1), (b"plain".to_vec(), Vec::new(), OK));
    }

    #[test]
    fn the_crc_table_is_gzip_s() {
        assert_eq!(CRC_32_TAB[1], 0x7707_3096);
        assert_eq!(CRC_32_TAB[255], 0x2d02_ef8d);
        assert_eq!(CRC_32_TAB[128], 0xedb8_8320);
    }

    #[test]
    fn the_crc_of_a_known_text() {
        let mut gz = Gz::new(memio::MemIo::default(), b"gzip".to_vec());
        gz.crc_reset();
        assert_eq!(gz.updcrc(b"123456789"), 0xcbf4_3926);
        assert_eq!(gz.getcrc(), 0xcbf4_3926);
    }

    #[test]
    fn the_last_component_skips_trailing_slashes() {
        for (name, base) in [
            (&b"a/b/c"[..], 4),
            (b"c", 0),
            (b"/c", 1),
            (b"a/b/", 2),
            (b"///", 3),
            (b"", 0),
            (b"a//b", 3),
        ] {
            assert_eq!(
                last_component(name),
                base,
                "{:?}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn ratios_and_offsets_print_as_printf_does() {
        type G = Gz<memio::MemIo>;
        assert_eq!(G::ratio(0, 0), "  0.0%");
        assert_eq!(G::ratio(1, 3), " 33.3%");
        assert_eq!(G::ratio(-5, 10), "-50.0%");
        assert_eq!(G::ratio(1, 1), "100.0%");
        assert_eq!(G::ratio(-1, 1_000_000), " -0.0%");
        assert_eq!(G::off(42, 19), "                 42");
        assert_eq!(G::off(-7, 3), " -7");
    }
}
