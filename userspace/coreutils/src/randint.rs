//! Random bytes and unbiased random integers: gnulib's `randread` and
//! `randint`, which `shred` uses (and GNU `shuf` and `sort -R` beside it).
//!
//! # Why a port and not "some random numbers"
//!
//! Because `--random-source=FILE` makes the randomness *reproducible*, and a
//! program that promises that must consume the file exactly as upstream does.
//! `shred -n 7 --random-source=FILE` chooses its seven overwrite patterns and
//! their order from FILE's bytes, and writes FILE's next bytes as the data of
//! every random pass; two implementations that read the bytes differently shred
//! the same file differently from the same "random" source. So this is
//! `randint_genmax` transcribed -- its byte-at-a-time widening of the range, its
//! rejection of the biased tail, and its keeping of the leftover randomness for
//! the next call -- rather than a modulo that would be unbiased "enough".
//!
//! # The two sources
//!
//! * **A named file** (`--random-source=FILE`): its bytes, in order. Running
//!   out is fatal -- `'FILE': end of file` -- exactly as gnulib's default
//!   handler makes it; there is no wrapping around to the start, which would
//!   repeat "random" data.
//! * **The system**: the kernel's CSPRNG through `randrange::fill_secret`.
//!   Upstream stretches a `getrandom` seed with ISAAC instead, a user-space
//!   generator gnulib carries because `getrandom` was once slow or absent. The
//!   bytes are unpredictable either way and no test can tell them apart, so
//!   this asks the kernel directly rather than porting a second generator whose
//!   only job would be to be less direct (`design-decisions.md` §539 would
//!   have it ported, not written, and here it need not exist at all).

use crate::quote::os_from_bytes;
use std::fs::File;
use std::io::{self, BufReader, Read};

/// Why random data could not be had. Upstream's handler prints this and exits
/// on the spot; here it is returned, and the caller does the same.
#[derive(Debug)]
pub enum RandError {
    /// The named source ran out: `'FILE': end of file`.
    EndOfFile(Vec<u8>),
    /// Reading the named source failed: `'FILE': read error: <strerror>`.
    Read(Vec<u8>, io::Error),
    /// The kernel's CSPRNG refused. Upstream cannot reach this after start-up
    /// (ISAAC never fails once seeded); it is kept distinct so that it is
    /// reported for what it is rather than as a file error.
    System(String),
}

/// gnulib's `struct randread_source`: where random bytes come from.
pub struct RandRead {
    source: Source,
}

enum Source {
    File {
        reader: BufReader<File>,
        name: Vec<u8>,
    },
    System,
}

/// gnulib's `RANDREAD_BUFFER_SIZE`, the stream buffer it gives a named source.
/// Buffering does not change which bytes are used, only how many are read ahead.
const RANDREAD_BUFFER_SIZE: usize = 2048;

impl RandRead {
    /// `randread_new (name, …)`: the named file, or the system when `None`.
    ///
    /// # Errors
    ///
    /// The file cannot be opened; the caller reports it against its name.
    pub fn open(name: Option<&[u8]>) -> io::Result<Self> {
        let source = match name {
            Some(name) => Source::File {
                reader: BufReader::with_capacity(
                    RANDREAD_BUFFER_SIZE,
                    File::open(os_from_bytes(name))?,
                ),
                name: name.to_vec(),
            },
            None => Source::System,
        };
        Ok(RandRead { source })
    }

    /// `randread`: fill `buf` completely.
    ///
    /// # Errors
    ///
    /// The named source ended or failed, or the system source refused.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<(), RandError> {
        match &mut self.source {
            Source::File { reader, name } => {
                let mut filled = 0usize;
                while filled < buf.len() {
                    match reader.read(buf.get_mut(filled..).unwrap_or(&mut [])) {
                        Ok(0) => return Err(RandError::EndOfFile(name.clone())),
                        Ok(n) => filled = filled.saturating_add(n),
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => return Err(RandError::Read(name.clone(), e)),
                    }
                }
                Ok(())
            }
            Source::System => {
                randrange::fill_secret(buf).map_err(|e| RandError::System(format!("{e:?}")))
            }
        }
    }
}

/// gnulib's `struct randint_source`: a byte source, plus the randomness a
/// previous draw did not use.
pub struct RandInt {
    source: RandRead,
    /// Uniform in `0..=randmax`, not yet delivered to anyone.
    randnum: u64,
    randmax: u64,
}

impl RandInt {
    /// `randint_new`.
    #[must_use]
    pub const fn new(source: RandRead) -> Self {
        RandInt {
            source,
            randnum: 0,
            randmax: 0,
        }
    }

    /// `randint_get_source`: the byte source, for callers that want raw bytes
    /// from the same stream the integers come from -- which is what makes
    /// `shred`'s choices and its data one reproducible sequence.
    pub fn source(&mut self) -> &mut RandRead {
        &mut self.source
    }

    /// `randint_choose`: uniform in `0..choices`. `choices` must be at least 1.
    ///
    /// # Errors
    ///
    /// As [`RandRead::read`].
    pub fn choose(&mut self, choices: u64) -> Result<u64, RandError> {
        self.genmax(choices.saturating_sub(1))
    }

    /// `randint_genmax`: uniform in `0..=genmax`, transcribed.
    ///
    /// The arithmetic is upstream's `uintmax_t` arithmetic, so it wraps where
    /// C's does -- only for a `genmax` within a byte of `u64::MAX`, which no
    /// caller here asks for.
    ///
    /// # Errors
    ///
    /// As [`RandRead::read`].
    pub fn genmax(&mut self, genmax: u64) -> Result<u64, RandError> {
        let mut randnum = self.randnum;
        let mut randmax = self.randmax;
        let choices = genmax.wrapping_add(1);

        loop {
            if randmax < genmax {
                // How many bytes widen the range past GENMAX, then read them.
                let mut rmax = randmax;
                let mut count = 0usize;
                loop {
                    rmax = shift_left(rmax).wrapping_add(u64::from(u8::MAX));
                    count = count.saturating_add(1);
                    if rmax >= genmax {
                        break;
                    }
                }
                let mut buf = [0u8; 8];
                self.source.read(buf.get_mut(..count).unwrap_or(&mut []))?;

                let mut used = 0usize;
                loop {
                    randnum = shift_left(randnum)
                        .wrapping_add(u64::from(buf.get(used).copied().unwrap_or(0)));
                    randmax = shift_left(randmax).wrapping_add(u64::from(u8::MAX));
                    used = used.saturating_add(1);
                    if randmax >= genmax {
                        break;
                    }
                }
            }

            if randmax == genmax {
                self.randnum = 0;
                self.randmax = 0;
                return Ok(randnum);
            }

            // GENMAX < RANDMAX: RANDNUM modulo CHOICES is fair while RANDNUM
            // lies in a whole multiple of CHOICES; past that, retry, keeping
            // what the overshoot says. CHOICES cannot be zero here, since
            // GENMAX < RANDMAX <= u64::MAX.
            let Some(nonzero) = std::num::NonZeroU64::new(choices) else {
                return Ok(0);
            };
            let excess_choices = randmax.wrapping_sub(genmax);
            let unusable_choices = excess_choices % nonzero;
            let last_usable_choice = randmax.wrapping_sub(unusable_choices);
            let reduced_randnum = randnum % nonzero;

            if randnum <= last_usable_choice {
                self.randnum = randnum / nonzero;
                self.randmax = excess_choices / nonzero;
                return Ok(reduced_randnum);
            }

            randnum = reduced_randnum;
            // `unusable_choices` is at least 1 on this path: had it been 0,
            // `last_usable_choice` would be RANDMAX, which RANDNUM cannot
            // exceed.
            randmax = unusable_choices.wrapping_sub(1);
        }
    }
}

/// `shift_left`: `x << CHAR_BIT`. Upstream's `HUGE_BYTES` guard is for a
/// `randint` no wider than a byte, which `u64` is not.
const fn shift_left(x: u64) -> u64 {
    x << 8
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A `RandInt` over the given bytes, through a real file, since a named
    /// source is a file.
    fn over(bytes: &[u8]) -> (RandInt, scratchdir::ScratchDir) {
        let dir = scratchdir::ScratchDir::new("randint");
        let path = dir.path("src");
        File::create(&path).unwrap().write_all(bytes).unwrap();
        let name = path.to_str().unwrap().as_bytes().to_vec();
        (RandInt::new(RandRead::open(Some(&name)).unwrap()), dir)
    }

    #[test]
    fn a_power_of_two_range_takes_one_byte_whole() {
        // genmax 255: one byte, delivered as is, nothing kept.
        let (mut r, _d) = over(&[7, 200]);
        assert_eq!(r.genmax(255).unwrap(), 7);
        assert_eq!(r.genmax(255).unwrap(), 200);
    }

    #[test]
    fn leftover_randomness_is_kept_for_the_next_draw() {
        // choose(2) from byte 0b1011_0101 (181): 181 % 2 = 1, keeping 90 in
        // 0..=127; the next choose(2) uses that and reads nothing.
        let (mut r, _d) = over(&[181]);
        assert_eq!(r.choose(2).unwrap(), 1);
        assert_eq!(r.choose(2).unwrap(), 0); // 90 % 2
        assert_eq!(r.choose(2).unwrap(), 1); // 45 % 2
    }

    #[test]
    fn a_biased_tail_is_rejected_and_retried() {
        // choose(3) from 255: 256 values, 255 is the one past the last whole
        // multiple of 3 (0..=254), so it is refused and the next byte read.
        let (mut r, _d) = over(&[255, 4]);
        // Retry keeps randnum = 255 % 3 = 0 in 0..=0 -- i.e. no information --
        // then widens with the next byte: randnum = 0 * 256 + 4 = 4 in 0..=255.
        assert_eq!(r.choose(3).unwrap(), 1);
    }

    #[test]
    fn running_out_of_a_named_source_is_end_of_file() {
        let (mut r, _d) = over(&[1]);
        assert_eq!(r.genmax(255).unwrap(), 1);
        assert!(matches!(r.genmax(255), Err(RandError::EndOfFile(_))));
    }

    #[test]
    fn wide_ranges_read_as_many_bytes_as_they_need() {
        // genmax 65535: two bytes, big-endian as the loop builds it.
        let (mut r, _d) = over(&[0x12, 0x34]);
        assert_eq!(r.genmax(65535).unwrap(), 0x1234);
    }

    /// Unix only: the host build of `randrange` has no kernel CSPRNG to ask,
    /// and says so with an error rather than invent bytes -- which is the
    /// right behaviour and would fail this test. (It did, under the
    /// pre-push hook's shuffled-order gate on 2026-09-25.)
    #[cfg(unix)]
    #[test]
    fn the_system_source_fills_a_buffer() {
        let mut s = RandRead::open(None).unwrap();
        let mut buf = [0u8; 64];
        s.read(&mut buf).unwrap();
        // Sixty-four zero bytes from a CSPRNG would be a one-in-2^512 event.
        assert!(buf.iter().any(|&b| b != 0));
    }
}
