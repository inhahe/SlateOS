//! SSH-2 wire encoding and key-exchange arithmetic, shared by `ssh` and `sshd`.
//!
//! # Why this crate exists
//!
//! Every function here is a **contract between two programs**. The client
//! encodes a value and the server decodes it; the client computes a hash and
//! the server computes the same hash and signs it. If the two ends disagree by
//! a single byte, nothing works — and, crucially, *neither end's tests notice*,
//! because each one checks its own implementation against its own idea of the
//! protocol and passes.
//!
//! That is not hypothetical. `userspace/ssh` and `userspace/sshd` each carried a
//! private copy of the RFC 4253 §8 exchange hash. The server's copy filled in a
//! fixed `"SSH-2.0-client"` where the client's real identification string
//! belonged, with a comment explaining that it did not store the real one. The
//! exchange hash is what the server signs with its host key and what the client
//! independently recomputes to check that signature, so the two hashes could
//! never match: the daemon could not complete a handshake with *any* client,
//! including our own. Both test suites were green throughout. See
//! `known-issues.md`
//! `TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT`.
//!
//! The lesson is not "be more careful with the exchange hash". It is that a
//! shared definition is the only thing that makes agreement structural rather
//! than aspirational. `sha2`, `randrange` and `posix::ed25519` were already
//! pulled out of both crates on exactly this reasoning; the reasoning simply
//! stopped one layer short of the constructions built on top of them.
//!
//! # What belongs here
//!
//! Anything whose correctness is defined by the *other* end agreeing with it:
//! wire encodings, the exchange hash, key derivation. Anything a single end
//! decides for itself — configuration, policy, state machines, message ordering
//! — does not, and a shared crate says nothing about those.
//!
//! # What does not belong here
//!
//! A private copy of any of this, in either binary. "Just this one is
//! different" is how the last drift started.

use sha2::sha256;

// ============================================================================
// Wire encoding (RFC 4253 §5)
// ============================================================================

/// Encode a byte string as SSH `string`: a `uint32` length, then the bytes.
///
/// The length is saturated rather than truncated. Truncating would encode a
/// length that disagrees with the payload that follows it, which desynchronises
/// the reader for the rest of the connection; saturating produces a value the
/// far end will reject as over-long, which is a diagnosable failure. Neither is
/// reachable at SSH's packet sizes — this is about which way an unreachable
/// case fails.
#[must_use]
pub fn ssh_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().saturating_add(4));
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// Encode a `u32` as SSH `uint32` (big-endian).
#[must_use]
pub fn ssh_u32(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

/// Encode a big-endian unsigned integer as SSH `mpint` (RFC 4253 §5).
///
/// `mpint` is two's complement, so a value whose most significant byte has the
/// top bit set would read as negative. The leading zero byte is what keeps it
/// unsigned — omitting it does not make the number smaller, it makes it a
/// different number, and every digest computed over it diverges.
///
/// Zero is the empty string, i.e. a length of zero and no bytes.
#[must_use]
pub fn encode_mpint(value: &[u8]) -> Vec<u8> {
    let stripped = strip_leading_zeros(value);
    let Some(&high) = stripped.first() else {
        return vec![0, 0, 0, 0];
    };
    let needs_pad = (high & 0x80) != 0;
    let total_len = stripped.len().saturating_add(usize::from(needs_pad));
    let mut out = Vec::with_capacity(total_len.saturating_add(4));
    out.extend_from_slice(&u32::try_from(total_len).unwrap_or(u32::MAX).to_be_bytes());
    if needs_pad {
        out.push(0);
    }
    out.extend_from_slice(stripped);
    out
}

/// Drop leading zero bytes, as `mpint` requires of its canonical form.
///
/// An all-zero input yields an empty slice, which [`encode_mpint`] encodes as
/// the zero `mpint`.
#[must_use]
pub fn strip_leading_zeros(data: &[u8]) -> &[u8] {
    let first_nonzero = data.iter().position(|&b| b != 0).unwrap_or(data.len());
    data.get(first_nonzero..).unwrap_or(&[])
}

// ============================================================================
// Key exchange (RFC 4253 §7.2, §8)
// ============================================================================

/// The eight inputs to the SSH key-exchange hash, in RFC 4253 §8 order.
///
/// They are a struct rather than eight positional parameters because they are
/// eight values of three types, several of them byte slices that are trivially
/// swappable at a call site — and swapping two of them produces a hash that is
/// wrong in a way no type checker and no single-ended test can see.
#[derive(Debug, Clone, Copy)]
pub struct ExchangeHashInput<'a> {
    /// `V_C` — the client's identification string, without its CRLF.
    ///
    /// This is the field that drifted. It must be what the client *actually
    /// sent*, not what the local end calls itself and not a constant.
    pub client_version: &'a str,
    /// `V_S` — the server's identification string, without its CRLF.
    pub server_version: &'a str,
    /// `I_C` — the client's `SSH_MSG_KEXINIT` payload, including its message
    /// byte.
    pub client_kexinit: &'a [u8],
    /// `I_S` — the server's `SSH_MSG_KEXINIT` payload, including its message
    /// byte.
    pub server_kexinit: &'a [u8],
    /// `K_S` — the server's public host key blob.
    pub host_key_blob: &'a [u8],
    /// `e` — the client's Diffie-Hellman public value, big-endian.
    pub client_e: &'a [u8],
    /// `f` — the server's Diffie-Hellman public value, big-endian.
    pub server_f: &'a [u8],
    /// `K` — the shared secret, big-endian.
    pub shared_secret: &'a [u8],
}

/// Compute the exchange hash `H` (RFC 4253 §8).
///
/// `H = HASH(V_C || V_S || I_C || I_S || K_S || e || f || K)`, with the two
/// version strings and the three blobs encoded as `string` and the three
/// numbers as `mpint`.
///
/// `H` is the value the server signs with its host key and the client
/// recomputes independently to verify that signature, and the `H` of the
/// *first* exchange becomes the session ID for the life of the connection
/// (§7.2) — which is in turn bound into the publickey authentication signed
/// blob (RFC 4252 §7). It is the single value the whole handshake turns on, so
/// there is exactly one definition of it and both ends call this.
#[must_use]
pub fn compute_exchange_hash(input: &ExchangeHashInput<'_>) -> [u8; 32] {
    let mut buf = Vec::new();
    buf.extend_from_slice(&ssh_string(input.client_version.as_bytes()));
    buf.extend_from_slice(&ssh_string(input.server_version.as_bytes()));
    buf.extend_from_slice(&ssh_string(input.client_kexinit));
    buf.extend_from_slice(&ssh_string(input.server_kexinit));
    buf.extend_from_slice(&ssh_string(input.host_key_blob));
    buf.extend_from_slice(&encode_mpint(input.client_e));
    buf.extend_from_slice(&encode_mpint(input.server_f));
    buf.extend_from_slice(&encode_mpint(input.shared_secret));
    sha256(&buf)
}

/// Derive `needed` bytes of key material (RFC 4253 §7.2).
///
/// `K1 = HASH(K || H || X || session_id)`, and where more bytes are wanted,
/// `K2 = HASH(K || H || K1)`, `K3 = HASH(K || H || K1 || K2)`, and so on, with
/// the key being the concatenation truncated to length.
///
/// `x` is the single-character identifier the RFC assigns to each key:
///
/// | `x` | Key |
/// |---|---|
/// | `A` | initial IV, client to server |
/// | `B` | initial IV, server to client |
/// | `C` | encryption key, client to server |
/// | `D` | encryption key, server to client |
/// | `E` | integrity key, client to server |
/// | `F` | integrity key, server to client |
///
/// `session_id` is the *first* exchange hash and does not change when the
/// connection rekeys; `h` is the current exchange hash and does. On the first
/// exchange the two are equal, which is why passing `h` for both looks correct
/// until the day rekeying is implemented.
#[must_use]
pub fn derive_key(k: &[u8], h: &[u8; 32], x: u8, session_id: &[u8; 32], needed: usize) -> Vec<u8> {
    let k_enc = encode_mpint(k);

    let mut buf = Vec::new();
    buf.extend_from_slice(&k_enc);
    buf.extend_from_slice(h);
    buf.push(x);
    buf.extend_from_slice(session_id);
    let mut result = sha256(&buf).to_vec();

    while result.len() < needed {
        let mut more = Vec::new();
        more.extend_from_slice(&k_enc);
        more.extend_from_slice(h);
        more.extend_from_slice(&result);
        result.extend_from_slice(&sha256(&more));
    }

    result.truncate(needed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ssh_string ----

    #[test]
    fn an_empty_string_is_four_zero_bytes() {
        assert_eq!(ssh_string(b""), vec![0, 0, 0, 0]);
    }

    #[test]
    fn a_string_is_its_length_then_its_bytes() {
        assert_eq!(ssh_string(b"abc"), vec![0, 0, 0, 3, b'a', b'b', b'c']);
    }

    #[test]
    fn a_string_may_contain_any_byte_including_nul() {
        assert_eq!(ssh_string(&[0x00, 0xFF]), vec![0, 0, 0, 2, 0x00, 0xFF]);
    }

    // ---- encode_mpint ----

    #[test]
    fn mpint_zero_is_the_empty_string() {
        // RFC 4253 §5: "the value zero MUST be stored as a string with zero
        // bytes of data" -- not as a single 0x00 byte.
        assert_eq!(encode_mpint(&[]), vec![0, 0, 0, 0]);
        assert_eq!(encode_mpint(&[0]), vec![0, 0, 0, 0]);
        assert_eq!(encode_mpint(&[0, 0, 0]), vec![0, 0, 0, 0]);
    }

    #[test]
    fn mpint_strips_leading_zeros_to_reach_its_canonical_form() {
        assert_eq!(encode_mpint(&[0, 0, 0x09]), encode_mpint(&[0x09]));
    }

    #[test]
    fn mpint_pads_a_value_whose_top_bit_is_set() {
        // Without the pad, 0x80 reads as -128 rather than 128, and every digest
        // computed over it diverges from the far end's.
        assert_eq!(encode_mpint(&[0x80]), vec![0, 0, 0, 2, 0x00, 0x80]);
        assert_eq!(
            encode_mpint(&[0xFF, 0x01]),
            vec![0, 0, 0, 3, 0x00, 0xFF, 0x01]
        );
    }

    #[test]
    fn mpint_does_not_pad_a_value_whose_top_bit_is_clear() {
        assert_eq!(encode_mpint(&[0x7F]), vec![0, 0, 0, 1, 0x7F]);
    }

    #[test]
    fn the_rfc_4251_mpint_examples_encode_as_published() {
        // RFC 4251 §5's own table, which is the only external oracle available
        // for this encoding.
        assert_eq!(
            encode_mpint(&[0x9a, 0x37, 0x87, 0x0c, 0xe1, 0x5e, 0xa0]),
            vec![
                0x00, 0x00, 0x00, 0x08, 0x00, 0x9a, 0x37, 0x87, 0x0c, 0xe1, 0x5e, 0xa0
            ]
        );
        assert_eq!(
            encode_mpint(&[0x80]),
            vec![0x00, 0x00, 0x00, 0x02, 0x00, 0x80]
        );
    }

    // ---- strip_leading_zeros ----

    #[test]
    fn an_all_zero_value_strips_to_nothing() {
        assert_eq!(strip_leading_zeros(&[0, 0, 0]), &[] as &[u8]);
        assert_eq!(strip_leading_zeros(&[]), &[] as &[u8]);
    }

    #[test]
    fn interior_zeros_are_kept() {
        assert_eq!(strip_leading_zeros(&[0, 1, 0, 2]), &[1, 0, 2]);
    }

    // ---- compute_exchange_hash ----

    fn an_input() -> ExchangeHashInput<'static> {
        ExchangeHashInput {
            client_version: "SSH-2.0-SlateOS_1.0",
            server_version: "SSH-2.0-SlateOS_SSHD_1.0",
            client_kexinit: &[0x14, 1, 2, 3],
            server_kexinit: &[0x14, 9, 8, 7],
            host_key_blob: &[0xAA; 51],
            client_e: &[0x11; 32],
            server_f: &[0x22; 32],
            shared_secret: &[0x33; 32],
        }
    }

    #[test]
    fn the_exchange_hash_covers_its_eight_inputs_in_rfc_order() {
        // The §8 construction written out here rather than called, so this is a
        // statement of what the hash should be and not a restatement of what it
        // is. A test that calls the function it is testing to compute its own
        // expectation cannot fail.
        let i = an_input();
        let mut expected = Vec::new();
        expected.extend_from_slice(&ssh_string(i.client_version.as_bytes()));
        expected.extend_from_slice(&ssh_string(i.server_version.as_bytes()));
        expected.extend_from_slice(&ssh_string(i.client_kexinit));
        expected.extend_from_slice(&ssh_string(i.server_kexinit));
        expected.extend_from_slice(&ssh_string(i.host_key_blob));
        expected.extend_from_slice(&encode_mpint(i.client_e));
        expected.extend_from_slice(&encode_mpint(i.server_f));
        expected.extend_from_slice(&encode_mpint(i.shared_secret));
        assert_eq!(compute_exchange_hash(&i), sha256(&expected));
    }

    #[test]
    fn every_input_is_load_bearing() {
        // Each field changed alone must move the digest. The bug this crate was
        // created over was one field that did not: the server substituted a
        // constant for `client_version`, so its hash was independent of what the
        // client said and could never match the client's own.
        let base = compute_exchange_hash(&an_input());

        let mut i = an_input();
        i.client_version = "SSH-2.0-OpenSSH_9.6";
        assert_ne!(compute_exchange_hash(&i), base, "V_C");

        let mut i = an_input();
        i.server_version = "SSH-2.0-Other";
        assert_ne!(compute_exchange_hash(&i), base, "V_S");

        let mut i = an_input();
        i.client_kexinit = &[0x14, 1, 2, 4];
        assert_ne!(compute_exchange_hash(&i), base, "I_C");

        let mut i = an_input();
        i.server_kexinit = &[0x14, 9, 8, 6];
        assert_ne!(compute_exchange_hash(&i), base, "I_S");

        let mut i = an_input();
        i.host_key_blob = &[0xAB; 51];
        assert_ne!(compute_exchange_hash(&i), base, "K_S");

        let mut i = an_input();
        i.client_e = &[0x12; 32];
        assert_ne!(compute_exchange_hash(&i), base, "e");

        let mut i = an_input();
        i.server_f = &[0x23; 32];
        assert_ne!(compute_exchange_hash(&i), base, "f");

        let mut i = an_input();
        i.shared_secret = &[0x34; 32];
        assert_ne!(compute_exchange_hash(&i), base, "K");
    }

    #[test]
    fn the_two_version_strings_are_not_interchangeable() {
        // Ordering, not just presence. Swapping V_C and V_S leaves the same
        // bytes in the buffer and must still change the digest, or a client and
        // server that disagreed about which came first would interoperate by
        // accident and fail the moment the strings differed in length.
        let mut swapped = an_input();
        swapped.client_version = an_input().server_version;
        swapped.server_version = an_input().client_version;
        assert_ne!(
            compute_exchange_hash(&swapped),
            compute_exchange_hash(&an_input())
        );
    }

    // ---- derive_key ----

    #[test]
    fn each_key_identifier_yields_different_material() {
        let (k, h, sid) = (&[0x42u8; 32][..], &[0x01u8; 32], &[0x02u8; 32]);
        let a = derive_key(k, h, b'A', sid, 16);
        let b = derive_key(k, h, b'B', sid, 16);
        let c = derive_key(k, h, b'C', sid, 16);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn a_derived_key_is_exactly_as_long_as_asked_for() {
        let (k, h, sid) = (&[0x42u8; 32][..], &[0x01u8; 32], &[0x02u8; 32]);
        for needed in [1usize, 16, 32, 33, 64, 100] {
            assert_eq!(derive_key(k, h, b'A', sid, needed).len(), needed);
        }
    }

    #[test]
    fn the_first_thirty_two_bytes_do_not_change_when_more_are_requested() {
        // The extension rounds append; they must not disturb K1. If they did, a
        // peer asking for a 16-byte IV and one asking for a 32-byte key would
        // derive different leading bytes from the same secret.
        let (k, h, sid) = (&[0x42u8; 32][..], &[0x01u8; 32], &[0x02u8; 32]);
        let short = derive_key(k, h, b'C', sid, 32);
        let long = derive_key(k, h, b'C', sid, 96);
        assert_eq!(long.get(..32), Some(&short[..]));
    }

    #[test]
    fn the_session_id_is_an_input_distinct_from_the_exchange_hash() {
        // On the first key exchange H and the session ID are equal, which hides
        // a swap of the two. They diverge on the first rekey, so the derivation
        // has to depend on each of them separately.
        let k = &[0x42u8; 32][..];
        let both_same = derive_key(k, &[0x01; 32], b'C', &[0x01; 32], 32);
        let h_moved = derive_key(k, &[0x99; 32], b'C', &[0x01; 32], 32);
        let sid_moved = derive_key(k, &[0x01; 32], b'C', &[0x99; 32], 32);
        assert_ne!(both_same, h_moved);
        assert_ne!(both_same, sid_moved);
        assert_ne!(h_moved, sid_moved);
    }

    #[test]
    fn the_shared_secret_enters_as_an_mpint_not_as_raw_bytes() {
        // K is encoded, so a value with a leading zero byte and the same value
        // without it are the same number and must derive the same key. Hashing
        // raw bytes would make them differ -- and the two ends strip leading
        // zeros at different points, so this is a real way to disagree.
        let h = &[0x01u8; 32];
        let sid = &[0x02u8; 32];
        assert_eq!(
            derive_key(&[0x00, 0x09], h, b'C', sid, 32),
            derive_key(&[0x09], h, b'C', sid, 32)
        );
    }
}
