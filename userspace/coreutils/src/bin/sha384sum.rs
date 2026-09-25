//! `sha384sum` — print or check SHA-384 (384-bit) checksums.
//!
//! Everything lives in [`coreutils::digest`], upstream's `src/digest.c`, which
//! upstream compiles once per `HASH_ALGO_*`; this file is the [`Build`] and the
//! vectors that check the wiring. The hash is the workspace's `sha2` crate,
//! whose SHA-384 is SHA-512 from other initial values, cut to 48 bytes, and
//! whose SHA-512 is ported from RustCrypto's (`design-decisions.md` §539).

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Sha384sum)
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
        let mut h = Algo::Sha384.stream(48);
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn the_build_hashes_with_sha384() {
        assert_eq!(Build::Sha384sum.algo(), Algo::Sha384);
        assert_eq!(Algo::Sha384.stream(48).finish().len() * 8, 384);
    }

    /// The FIPS 180-4 examples and the padding boundaries of a 128-byte block;
    /// cross-checked against Python's `hashlib`.
    #[test]
    fn the_known_answers() {
        for (msg, want) in [
            (Vec::new(), "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"),
            (b"abc".to_vec(), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"),
            (b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu".to_vec(), "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"),
            (vec![b'x'; 111], "dfec6588f894c2b089119e1884a23941e5aa69ed38702839cf7352ac6d155d315693bb5d26b7468d8b69ecf4631ef419"),
            (vec![b'x'; 112], "d8489693bc428374931aedf508740398d9d4a92887116fecb2fcdd91c68b9db329fc4a474e05e78c3eb34e649a6b5c77"),
            (vec![b'x'; 128], "e660584956c8b1df44c92acb7c8eccfe0dca5255627c9fb44637c15363b772e5709edcf35b07bf43531951ab2fd51130"),
        ] {
            assert_eq!(digest_hex(&msg), want, "{} bytes", msg.len());
        }
    }

    #[test]
    fn one_million_a() {
        let mut h = Algo::Sha384.stream(48);
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex(&h.finish()),
            "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985"
        );
    }

    /// Fed in pieces that straddle the block, the answer is the same.
    #[test]
    fn chunk_boundaries_do_not_matter() {
        let msg: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let whole = digest_hex(&msg);
        for chunk in [1, 7, 127, 128, 129, 999] {
            let mut h = Algo::Sha384.stream(48);
            for piece in msg.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), whole, "chunk {chunk}");
        }
    }
}
