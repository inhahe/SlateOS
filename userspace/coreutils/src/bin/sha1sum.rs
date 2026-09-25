//! `sha1sum` — print or check SHA-1 (160-bit) checksums.
//!
//! Everything except the hash lives in [`coreutils::digest`], which is
//! upstream's `src/digest.c`: the option table, the three checksum-file
//! formats, `--check`, the name escaping and the exit statuses. The difference
//! between this program and `sha256sum` is the [`Algorithm`] constant below and
//! nothing else, which is what that module's shape was for.
//!
//! The hash is the shared `sha1` crate. Its documentation is blunt that SHA-1
//! must not decide what to trust, and this program does not ask it to: it
//! reproduces a checksum somebody else already chose to publish, so that a
//! download can be compared against the page it came from. Refusing to compute
//! it would not make anything safer; it would only leave that question
//! unanswerable.

use coreutils::digest::{Algorithm, Stream};
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// The `#if HASH_ALGO_SHA1` block of upstream's `digest.c`, as data.
static SHA1: Algorithm = Algorithm {
    program: "sha1sum",
    tag: "SHA1",
    bits: 160,
    reference: "FIPS-180-1",
    new: || Box::new(Sha1Stream(sha1::Sha1::new())),
};

fn main() -> ExitCode {
    coreutils::digest::main(&SHA1)
}

/// [`sha1::Sha1`] under the shared module's trait; a newtype for the reason
/// `sha256sum`'s is one.
struct Sha1Stream(sha1::Sha1);

impl Stream for Sha1Stream {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().to_vec()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::arithmetic_side_effects)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn hex(digest: &[u8]) -> String {
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Through the incremental hash, because that is what `digest::main` runs.
    fn sha1_hex(data: &[u8]) -> String {
        let mut h = (SHA1.new)();
        h.update(data);
        hex(&h.finish())
    }

    #[test]
    fn digest_length_matches_the_declared_bits() {
        assert_eq!((SHA1.new)().finish().len() * 8, SHA1.bits);
        assert_eq!(SHA1.hex_len(), 40);
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
            let mut h = (SHA1.new)();
            for piece in msg.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), whole, "chunk {chunk}");
        }
    }
}
