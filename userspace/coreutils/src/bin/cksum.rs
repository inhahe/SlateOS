//! `cksum` — print or verify checksums, by default POSIX's 32-bit CRC.
//!
//! ```text
//! Usage: cksum [OPTION]... [FILE]...
//! ```
//!
//! A port of GNU coreutils 9.4's `cksum`, which upstream builds from
//! `src/digest.c` with `HASH_ALGO_CKSUM`, plus `src/cksum.c` (the CRC) and
//! `src/sum.c` (the BSD and System V checksums). Here those are
//! [`coreutils::digest`], [`coreutils::cksum`] and [`coreutils::sum`]; this
//! file is only the [`Build`].
//!
//! Since 9.0 `cksum` is the family's umbrella: `-a` selects any of `sysv`,
//! `bsd`, `crc` (the default), `md5`, `sha1`, `sha224`, `sha256`, `sha384`,
//! `sha512`, `blake2b` and `sm3`, and prints each in its own program's format —
//! the three legacy checksums as `sum` and the POSIX `cksum` print them, the
//! rest tagged (`SHA256 (f) = …`) unless `--untagged`. `--check` without `-a`
//! takes each line's algorithm from its tag, which is why the tagged form is
//! the default. `--base64` and `--raw` change how a digest is written.
//!
//! One difference, deliberate and documented in [`coreutils::cksum`]:
//! `--debug` prints nothing, as upstream built without its PCLMUL CRC does.
//!
//! This replaces the `cksum` personality of `userspace/getopt`, which no link
//! reached and which was the CRC alone (`design-decisions.md` §1005).
//!
//! Checked against GNU by `scripts/cksum-diff.sh`.

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Cksum)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::arithmetic_side_effects)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use coreutils::digest::{Algo, Build};

    #[test]
    fn the_default_is_the_crc() {
        assert_eq!(Build::Cksum.algo(), Algo::Crc);
        assert_eq!(Build::Cksum.name(), "cksum");
    }

    /// The CRC through the stream the program runs: the checksum and nothing
    /// else, big-endian. Values measured from GNU 9.4.
    #[test]
    fn the_crc_stream() {
        let crc = |data: &[u8]| {
            let mut s = Algo::Crc.stream(4);
            s.update(data);
            u32::from_be_bytes(s.finish().try_into().unwrap())
        };
        assert_eq!(crc(b""), 4_294_967_295);
        assert_eq!(crc(b"hello\n"), 3_015_617_425);
    }

    /// The two `sum` checksums through the stream the program runs.
    #[test]
    fn the_legacy_streams() {
        let sum = |algo: Algo| {
            let mut s = algo.stream(2);
            s.update(b"hello\n");
            u16::from_be_bytes(s.finish().try_into().unwrap())
        };
        assert_eq!(sum(Algo::Bsd), 36979);
        assert_eq!(sum(Algo::Sysv), 542);
    }
}
