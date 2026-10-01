//! `sha224sum` — print or check SHA-224 (224-bit) checksums.
//!
//! Everything lives in [`coreutils::digest`], upstream's `src/digest.c`, which
//! upstream compiles once per `HASH_ALGO_*`; this file is the [`Build`] and the
//! vectors that check the wiring. The hash is the workspace's `sha2` crate,
//! which runs SHA-256's compression function from SHA-224's own initial
//! values and keeps the first 28 bytes (FIPS 180-4 §6.3).

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Sha224sum)
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

    /// Through the incremental stream, because that is what `digest::main`
    /// runs.
    fn digest_hex(data: &[u8]) -> String {
        let mut h = Algo::Sha224.stream(28);
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn the_build_hashes_with_sha224() {
        assert_eq!(Build::Sha224sum.algo(), Algo::Sha224);
        assert_eq!(Algo::Sha224.stream(28).finish().len() * 8, 224);
    }

    /// The FIPS 180-4 examples, the padding boundaries of a 64-byte block, and
    /// the 112-byte two-block message; cross-checked against Python's `hashlib`.
    #[test]
    fn the_known_answers() {
        for (msg, want) in [
            (
                Vec::new(),
                "d14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f",
            ),
            (
                b"abc".to_vec(),
                "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".to_vec(),
                "75388b16512776cc5dba5da1fd890150b0c6455cb4f58b1952522525",
            ),
            (
                vec![b'x'; 55],
                "2791c7d25712eb5be75c30b2d45f6fce96e5a50ee64cf373056704a3",
            ),
            (
                vec![b'x'; 56],
                "82199913c2712c46707ad08c8bfa2a69f68e9092dc5cdfaaaf07d77f",
            ),
            (
                vec![b'x'; 64],
                "08c3050e95fe11eacb9dc7824bf6a92bcf2d59c21701321fba0e62c5",
            ),
        ] {
            assert_eq!(digest_hex(&msg), want, "{} bytes", msg.len());
        }
    }

    #[test]
    fn one_million_a() {
        let mut h = Algo::Sha224.stream(28);
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex(&h.finish()),
            "20794655980c91d8bbb4c1ea97618a4bf03f42581948b2ee4ee7ad67"
        );
    }

    /// Fed in pieces that straddle the block, the answer is the same.
    #[test]
    fn chunk_boundaries_do_not_matter() {
        let msg: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let whole = digest_hex(&msg);
        for chunk in [1, 7, 63, 64, 65, 999] {
            let mut h = Algo::Sha224.stream(28);
            for piece in msg.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), whole, "chunk {chunk}");
        }
    }
}
