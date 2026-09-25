//! `md5sum` — print or check MD5 (128-bit) checksums.
//!
//! Everything lives in [`coreutils::digest`], which is upstream's
//! `src/digest.c`: the option table, the three checksum-file formats,
//! `--check`, the name escaping and the exit statuses. Upstream compiles that
//! one file once per `HASH_ALGO_*`; here the same effect is one [`Build`], so
//! this file is a `main` and the RFC 1321 vectors that check the wiring.
//!
//! # Where the MD5 is
//!
//! In the workspace's `md5` crate, which `apps/diskimager` already used. This
//! file carried a private copy until 2026-09-25 on the grounds that "MD5 has
//! exactly one consumer" — which had stopped being true when the shared crate
//! was written, and stopped being arguable when `cksum -a md5` became a second
//! consumer in this very crate. The vectors below now check the shared
//! implementation, through the same stream `--check` uses.

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Md5sum)
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

    /// Through the stream the program runs, not a one-shot helper: a vector
    /// that passed one-shot and failed incrementally would pass here and fail
    /// in the field.
    fn md5_hex(data: &[u8]) -> String {
        let mut h = Algo::Md5.stream(16);
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn the_build_hashes_with_md5() {
        assert_eq!(Build::Md5sum.algo(), Algo::Md5);
        assert_eq!(Build::Md5sum.name(), "md5sum");
    }

    // ---------------- RFC 1321 test vectors ----------------

    #[test]
    fn the_rfc_1321_vectors() {
        for (msg, want) in [
            (&b""[..], "d41d8cd98f00b204e9800998ecf8427e"),
            (b"a", "0cc175b9c0f1b6a831c399e269772661"),
            (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
            (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(md5_hex(msg), want, "{msg:?}");
        }
    }

    // ---------------- padding boundaries ----------------

    #[test]
    fn the_padding_boundaries() {
        // 55 is the last length whose padding fits in its block; 56 spills;
        // 64 fills a block and still needs a second.
        assert_eq!(md5_hex(&[b'a'; 55]), "ef1772b6dff9a122358552954ad0df65");
        assert_eq!(md5_hex(&[b'a'; 56]), "3b0c8ac703f828b04c6c197006d17218");
        assert_eq!(md5_hex(&[b'a'; 64]), "014842d480b571495a4a0363793f7367");
        // Byte-oriented, not text: 0xff and 0x00 are data.
        assert_eq!(md5_hex(&[0xffu8; 16]), "8d79cbc9a4ecdde112fc91ba625b13c2");
        assert_eq!(md5_hex(&[0u8]), "93b885adfe0da089cdf634904fd59f71");
    }

    // ---------------- streaming ----------------

    /// How the message is *divided* across `update` calls must not change the
    /// answer, and an incremental hash gets that wrong at exactly the splits
    /// that straddle a 64-byte block, so every split is tried.
    #[test]
    fn every_split_of_a_multi_block_message_agrees() {
        let msg: Vec<u8> = (0u16..200).map(|i| (i % 251) as u8).collect();
        let want = md5_hex(&msg);
        for cut in 0..=msg.len() {
            let mut h = Algo::Md5.stream(16);
            h.update(&msg[..cut]);
            h.update(&msg[cut..]);
            assert_eq!(hex(&h.finish()), want, "split at {cut} disagreed");
        }
    }

    /// A message far larger than one block, fed in chunks that are neither
    /// block-aligned nor block-sized.
    #[test]
    fn a_large_message_hashes_the_same_however_it_is_chunked() {
        let msg = vec![b'z'; 100_000];
        let want = md5_hex(&msg);
        let mut h = Algo::Md5.stream(16);
        for part in msg.chunks(4093) {
            h.update(part);
        }
        assert_eq!(hex(&h.finish()), want);
    }
}
