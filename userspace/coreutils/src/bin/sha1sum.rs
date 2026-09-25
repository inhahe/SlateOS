//! `sha1sum` — print or check SHA-1 (160-bit) checksums.
//!
//! Everything lives in [`coreutils::digest`], upstream's `src/digest.c`; this
//! file is the [`Build`] and the vectors that check the wiring. The hash is the
//! workspace's `sha1` crate, which says in its own docs why nothing should use
//! SHA-1 to decide what to trust — and why reproducing a checksum somebody
//! else published is the job it is here for.

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Sha1sum)
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

    /// Through the incremental hash, because that is what `digest::main` runs.
    fn sha1_hex(data: &[u8]) -> String {
        let mut h = Algo::Sha1.stream(20);
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn the_build_hashes_with_sha1() {
        assert_eq!(Build::Sha1sum.algo(), Algo::Sha1);
    }

    #[test]
    fn the_stream_agrees_with_the_one_shot() {
        for msg in [&b""[..], b"abc", &vec![b'q'; 5000]] {
            assert_eq!(sha1_hex(msg), hex(&sha1::sha1(msg)));
        }
    }

    /// The FIPS 180 vectors, which a SHA-1 either reproduces or is not one.
    #[test]
    fn the_fips_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            sha1_hex(&vec![b'a'; 1_000_000]),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    /// Fed in pieces that straddle the 64-byte block, the answer is the same.
    #[test]
    fn chunk_boundaries_do_not_matter() {
        let msg: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let whole = sha1_hex(&msg);
        for chunk in [1, 7, 63, 64, 65, 999] {
            let mut h = Algo::Sha1.stream(20);
            for piece in msg.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), whole, "chunk {chunk}");
        }
    }
}
