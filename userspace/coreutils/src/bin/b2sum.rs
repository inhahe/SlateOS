//! `b2sum` — print or check BLAKE2b checksums, 512 bits wide unless `-l` says
//! otherwise.
//!
//! Everything lives in [`coreutils::digest`], upstream's `src/digest.c`
//! compiled with `HASH_ALGO_BLAKE2`; this file is the [`Build`] and the vectors
//! that check the wiring. What that build adds over `md5sum` and the SHA
//! programs is a digest width that varies: `-l BITS` picks one (a multiple of
//! 8, at most 512, `-l 0` meaning the default), a narrower digest is tagged
//! `BLAKE2b-BITS`, and `--check` reads a line's width from its tag or, untagged,
//! from how many hex digits it has.
//!
//! The hash is the workspace's `blake2` crate: the BLAKE2 reference
//! implementation — the very `blake2b-ref.c` upstream builds `b2sum` from —
//! ported rather than rewritten (`design-decisions.md` §539).

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::B2sum)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::arithmetic_side_effects)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use coreutils::digest::{Algo, Build};

    fn hex(digest: &[u8]) -> String {
        use std::fmt::Write as _;
        digest.iter().fold(String::new(), |mut out, b| {
            // Formatting into a `String` cannot fail.
            let _ = write!(out, "{b:02x}");
            out
        })
    }

    fn b2_hex(out_len: usize, data: &[u8]) -> String {
        let mut h = Algo::Blake2b.stream(out_len);
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn the_build_hashes_with_blake2b() {
        assert_eq!(Build::B2sum.algo(), Algo::Blake2b);
        assert_eq!(Algo::Blake2b.bits(), 512);
    }

    /// RFC 7693 Appendix A, and the widths `-l` produces; cross-checked against
    /// Python's `hashlib.blake2b`.
    #[test]
    fn the_known_answers() {
        for (out_len, msg, want) in [
            (
                64,
                &b"abc"[..],
                "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923",
            ),
            (
                64,
                &b""[..],
                "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce",
            ),
            (
                32,
                &b""[..],
                "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8",
            ),
            (1, &b""[..], "2e"),
            (20, &b"abc"[..], "384264f676f39536840523f284921cdc68b6846b"),
            (
                64,
                &[b'x'; 128][..],
                "082b91ea2e15d1556d2ceefdd5af5d64d31b4e01aff1959724578876293825b236ee8079173a0a38160d7d6685d6bca0bfb62c177b3599b8727d9173e2115b91",
            ),
            (
                64,
                &[b'x'; 129][..],
                "362a53bbe2ec08097b2f358a41d0e153aeed4c132af928400872413650e7bf22f9ae428ff73770170bbd95f935e5dd1953c17de8c7264c72d1f99303bf22dfaa",
            ),
        ] {
            assert_eq!(
                b2_hex(out_len, msg),
                want,
                "{out_len}-byte digest of {} bytes",
                msg.len()
            );
        }
    }

    /// Every width `-l` accepts produces a digest exactly that wide.
    #[test]
    fn every_length_is_honoured() {
        for bits in (8..=512).step_by(8) {
            assert_eq!(Algo::Blake2b.stream(bits / 8).finish().len() * 8, bits);
        }
    }

    /// Fed in pieces that straddle the 128-byte block — and above all ending on
    /// one, where BLAKE2b must hold the block back for its final flag — the
    /// answer is the same.
    #[test]
    fn chunk_boundaries_do_not_matter() {
        let msg: Vec<u8> = (0..1024u32).map(|i| (i % 251) as u8).collect();
        let whole = b2_hex(64, &msg);
        for chunk in [1, 7, 127, 128, 129, 1000] {
            let mut h = Algo::Blake2b.stream(64);
            for piece in msg.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), whole, "chunk {chunk}");
        }
    }
}
