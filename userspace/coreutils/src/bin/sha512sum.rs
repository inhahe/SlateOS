//! `sha512sum` — print or check SHA-512 (512-bit) checksums.
//!
//! Everything lives in [`coreutils::digest`], upstream's `src/digest.c`, which
//! upstream compiles once per `HASH_ALGO_*`; this file is the [`Build`] and the
//! vectors that check the wiring. The hash is the workspace's `sha2` crate,
//! whose SHA-512 is ported from RustCrypto's (`design-decisions.md` §539).

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Sha512sum)
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
        let mut h = Algo::Sha512.stream(64);
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn the_build_hashes_with_sha512() {
        assert_eq!(Build::Sha512sum.algo(), Algo::Sha512);
        assert_eq!(Algo::Sha512.stream(64).finish().len() * 8, 512);
    }

    /// The FIPS 180-4 examples and the padding boundaries of a 128-byte block;
    /// cross-checked against Python's `hashlib`.
    #[test]
    fn the_known_answers() {
        for (msg, want) in [
            (Vec::new(), "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"),
            (b"abc".to_vec(), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"),
            (b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu".to_vec(), "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"),
            (vec![b'x'; 111], "9a2a120825c2319867758ec277924f6faa254968bf752046dacdd948d8ad299b10359fd04bfd7d3810b5fa1b16a294236138baff981cbb85248478053ac4d3dd"),
            (vec![b'x'; 112], "a3722b515ef40c910f2419f6e0da8ca51d410114ce6272faae64045f9e9f630e7fa8dd5a3243c9860b899d148c3da4bc0f9e07454542604d030bb55531fe0d5b"),
            (vec![b'x'; 128], "e2e22f8422b54b06e35c3ea30a383d1de7a8fbc27992923074103117020d8dd7024c3ecf7d6d1a15a6de5a75ff32fb486b9e8ced4c02ffe05822bf2cb734d0e0"),
        ] {
            assert_eq!(digest_hex(&msg), want, "{} bytes", msg.len());
        }
    }

    #[test]
    fn one_million_a() {
        let mut h = Algo::Sha512.stream(64);
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex(&h.finish()),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }

    /// Fed in pieces that straddle the block, the answer is the same.
    #[test]
    fn chunk_boundaries_do_not_matter() {
        let msg: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let whole = digest_hex(&msg);
        for chunk in [1, 7, 127, 128, 129, 999] {
            let mut h = Algo::Sha512.stream(64);
            for piece in msg.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), whole, "chunk {chunk}");
        }
    }
}
