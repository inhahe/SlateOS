//! `sha256sum` — print or check SHA-256 (256-bit) checksums.
//!
//! Everything lives in [`coreutils::digest`], which is upstream's
//! `src/digest.c`: the option table, the three checksum-file formats,
//! `--check`, the name escaping and the exit statuses. Upstream compiles that
//! file once per `HASH_ALGO_*`; here the difference between this program and
//! `md5sum` is the [`Build`] below and nothing else.
//!
//! The hash itself is the workspace's `sha2`. This file used to carry its own —
//! in the one utility whose entire output is a digest that another machine will
//! compare against its own — and the FIPS vectors below now check the shared
//! implementation, through the incremental stream rather than the one-shot
//! `sha2::sha256`, because the incremental one is what actually runs.

use coreutils::digest::Build;
use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::digest::main(Build::Sha256sum)
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

    /// Through the same path the program uses, not through `sha2::sha256`: a
    /// vector that passes one-shot and fails incrementally would otherwise
    /// pass here and fail in the field.
    fn sha256_hex(data: &[u8]) -> String {
        let mut h = Algo::Sha256.stream(32);
        h.update(data);
        hex(&h.finish())
    }

    // ------------ the wiring ------------

    #[test]
    fn the_build_hashes_with_sha256() {
        assert_eq!(Build::Sha256sum.algo(), Algo::Sha256);
        assert_eq!(
            Algo::Sha256.stream(32).finish().len() * 8,
            Algo::Sha256.bits()
        );
    }

    /// The newtype is the only place this program and `sha2` could drift, and
    /// a silent disagreement there is a wrong checksum.
    #[test]
    fn the_stream_agrees_with_the_one_shot() {
        for msg in [&b""[..], b"abc", &vec![b'q'; 5000]] {
            assert_eq!(sha256_hex(msg), hex(&sha2::sha256(msg)));
        }
    }

    // ---------------- FIPS 180-2 test vectors ----------------

    #[test]
    fn sha256_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_alphabet() {
        assert_eq!(
            sha256_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "71c480df93d6ae2f1efad1447c66c9525e316218cf51fc8d9ed832f2daf18b73"
        );
    }

    #[test]
    fn sha256_two_block_message() {
        // 56-byte input from FIPS 180-2 Appendix B.2.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_a_thousand_a_repeats() {
        assert_eq!(
            sha256_hex(&vec![b'a'; 1000]),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    // ---------------- padding boundaries ----------------

    #[test]
    fn sha256_exactly_55_bytes() {
        // The last length at which padding still fits in the same block.
        assert_eq!(
            sha256_hex(&[b'a'; 55]),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
    }

    #[test]
    fn sha256_exactly_56_bytes() {
        // One over: padding must spill into a second block.
        assert_eq!(
            sha256_hex(&[b'a'; 56]),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
    }

    #[test]
    fn sha256_exactly_64_bytes() {
        // One full block; padding still needs a second.
        assert_eq!(
            sha256_hex(&[b'a'; 64]),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    #[test]
    fn sha256_single_zero_byte() {
        assert_eq!(
            sha256_hex(&[0u8]),
            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
        );
    }

    #[test]
    fn sha256_high_bit_bytes() {
        // Byte-oriented, not text: 0xff is data.
        assert_eq!(
            sha256_hex(&[0xffu8; 32]),
            "af9613760f72635fbdb44a5a0a63c39f12af30f950a6ee5c971be188e89c4051"
        );
    }

    // ---------------- streaming ----------------

    /// How the message is *divided* across `update` calls must not change the
    /// answer. The one-shot hash this replaced could not get that wrong; an
    /// incremental one gets it wrong at exactly the splits that straddle a
    /// 64-byte block, so every split is tried.
    #[test]
    fn every_split_of_a_multi_block_message_agrees() {
        let msg: Vec<u8> = (0u16..200).map(|i| (i % 251) as u8).collect();
        let want = sha256_hex(&msg);
        for cut in 0..=msg.len() {
            let mut h = Algo::Sha256.stream(32);
            h.update(&msg[..cut]);
            h.update(&msg[cut..]);
            assert_eq!(hex(&h.finish()), want, "split at {cut} disagreed");
        }
    }

    /// An empty `update` must be a no-op — the shared module can issue one for
    /// a zero-length read.
    #[test]
    fn empty_updates_are_ignored() {
        let mut h = Algo::Sha256.stream(32);
        h.update(b"");
        h.update(b"abc");
        h.update(b"");
        assert_eq!(
            hex(&h.finish()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
