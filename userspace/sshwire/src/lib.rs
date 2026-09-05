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
// Identification string (RFC 4253 §4.2)
// ============================================================================

/// The §4.2 maximum for an identification line, *including* its CRLF.
pub const MAX_IDENTIFICATION_LINE: usize = 255;

/// Why a line could not be used as `V_C` / `V_S`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentificationError {
    /// Longer than [`MAX_IDENTIFICATION_LINE`].
    TooLong,
    /// Not valid UTF-8, so it cannot be held as a `str` and reproduced
    /// byte-for-byte. §4.2 requires printable US-ASCII, so this never happens
    /// with a conforming peer — which is exactly why it must be an error and
    /// not a lossy conversion.
    NotUtf8,
}

impl core::fmt::Display for IdentificationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Spelled out rather than named, because these reach a user as the
        // reason a connection was refused, and "NotUtf8" is not a reason.
        let reason = match *self {
            Self::TooLong => "longer than the 255 bytes RFC 4253 §4.2 allows, including its CRLF",
            Self::NotUtf8 => {
                "not valid UTF-8 (§4.2 requires printable US-ASCII); it cannot be hashed as \
                 the peer sent it, so it is refused rather than altered"
            }
        };
        f.write_str(reason)
    }
}

impl core::error::Error for IdentificationError {}

/// Remove the CR of a CRLF terminator — that one, and no other.
///
/// The caller has already removed the LF (it is what told them the line ended).
/// Only the CR immediately before it is framing; a CR anywhere else is a byte of
/// the line, and both ends must keep or drop it identically or they hash
/// different strings. sshd used to drop *every* CR in the line and the client
/// only the last, which is a third way for the two to disagree about `V_C`
/// after the first two were fixed.
#[must_use]
pub fn strip_line_terminator(line: &[u8]) -> &[u8] {
    match line.split_last() {
        Some((b'\r', rest)) => rest,
        _ => line,
    }
}

/// Is this line the identification string rather than something before it?
///
/// RFC 4253 §4.2 lets a *server* send any number of lines before its
/// identification string; the identification string is the first beginning
/// `SSH-`. A client may not send anything first, so a server calls this only to
/// reject what is not one.
#[must_use]
pub fn is_identification_line(line: &[u8]) -> bool {
    strip_line_terminator(line).starts_with(b"SSH-")
}

/// Decode an identification line into the `V_C` / `V_S` that gets hashed.
///
/// Takes everything up to but not including the LF, strips the CR of the CRLF,
/// and requires the rest to be exactly representable — because this string goes
/// into [`compute_exchange_hash`] as bytes, and the far end hashes the bytes it
/// put on the wire. Anything we cannot hand back unchanged has to be refused
/// rather than adjusted: an adjusted one produces a hash mismatch reported as a
/// bad host-key signature, which is indistinguishable from an attack.
///
/// Which protocol versions are acceptable (`SSH-2.0-`, and for a client also the
/// `SSH-1.99-` compatibility form) is the caller's policy and differs by role,
/// so it is not checked here.
///
/// # Errors
///
/// [`IdentificationError::TooLong`] past §4.2's limit, or
/// [`IdentificationError::NotUtf8`] for a line that cannot be reproduced.
pub fn decode_identification(line: &[u8]) -> Result<&str, IdentificationError> {
    let trimmed = strip_line_terminator(line);
    // The limit counts the CRLF, both bytes of it, whether or not this peer
    // sent the CR.
    if trimmed.len().saturating_add(2) > MAX_IDENTIFICATION_LINE {
        return Err(IdentificationError::TooLong);
    }
    core::str::from_utf8(trimmed).map_err(|_| IdentificationError::NotUtf8)
}

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
// Wire decoding (RFC 4253 §5)
// ============================================================================

/// Why a value could not be read out of a packet.
///
/// Both binaries convert this into their own error type, so a call site keeps
/// using `?` and keeps reporting failures the way the rest of that binary does.
/// The reason a decoder cannot simply return each binary's error type is what
/// kept the decoders duplicated until now: a shared function needs a shared
/// error, and there wasn't one.
///
/// Every variant means "the peer's bytes do not describe what they claim to" —
/// a protocol fault, never a bug on this side. That is deliberate: none of the
/// readers below can fail for any other reason, because none of them can panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// The packet ended before the value did.
    ///
    /// `what` names the value being read, `needed` is how many more bytes it
    /// required, and `available` is how many the packet still had. The counts
    /// are carried rather than formatted immediately because they are the
    /// difference between "a truncated packet" and "a peer claiming a 4 GiB
    /// string", which read identically without them.
    Truncated {
        /// The name of the value that was being read, for the message.
        what: &'static str,
        /// How many bytes the value needed.
        needed: usize,
        /// How many bytes the packet still had.
        available: usize,
    },
    /// A length prefix does not fit in this machine's address space.
    ///
    /// Unreachable on a 64-bit target, where every `u32` is a valid `usize`.
    /// It exists so the conversion is a `TryFrom` and not an `as`, which would
    /// silently truncate the length on a 16-bit target and read the wrong
    /// number of bytes rather than refusing to.
    LengthOutOfRange {
        /// The length the peer asked for.
        len: u32,
    },
}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Truncated {
                what,
                needed,
                available,
            } => write!(
                f,
                "truncated {what}: needs {needed} more bytes, packet has {available}"
            ),
            Self::LengthOutOfRange { len } => {
                write!(f, "length {len} does not fit this machine's address space")
            }
        }
    }
}

impl core::error::Error for WireError {}

/// How many bytes of `data` remain at `offset`, saturating at zero.
fn remaining(data: &[u8], offset: usize) -> usize {
    data.len().saturating_sub(offset)
}

/// Read an SSH `byte` at `offset`. Returns the value and the offset after it.
///
/// # Errors
///
/// [`WireError::Truncated`] if the packet ends at or before `offset`.
pub fn read_byte(data: &[u8], offset: usize) -> Result<(u8, usize), WireError> {
    let byte = *data.get(offset).ok_or(WireError::Truncated {
        what: "byte",
        needed: 1,
        available: remaining(data, offset),
    })?;
    Ok((byte, offset.saturating_add(1)))
}

/// Read an SSH `boolean` at `offset`, which §5 defines as any nonzero byte.
///
/// The RFC requires senders to use 1 and requires readers to accept anything
/// nonzero, so a strict reader here would reject peers the protocol permits.
///
/// # Errors
///
/// [`WireError::Truncated`] if the packet ends at or before `offset`.
pub fn read_bool(data: &[u8], offset: usize) -> Result<(bool, usize), WireError> {
    let (byte, next) = read_byte(data, offset)?;
    Ok((byte != 0, next))
}

/// Read an SSH `uint32` (big-endian) at `offset`.
///
/// # Errors
///
/// [`WireError::Truncated`] if fewer than four bytes remain at `offset`.
pub fn read_u32(data: &[u8], offset: usize) -> Result<(u32, usize), WireError> {
    let bytes = data
        .get(offset..)
        .and_then(<[u8]>::first_chunk::<4>)
        .ok_or(WireError::Truncated {
            what: "uint32",
            needed: 4,
            available: remaining(data, offset),
        })?;
    Ok((u32::from_be_bytes(*bytes), offset.saturating_add(4)))
}

/// Read an SSH `string` at `offset`: a `uint32` length, then that many bytes.
///
/// The length prefix is entirely the peer's to choose, so it is added to the
/// offset with `checked_add` and turned into a range only by `get`, which
/// returns `None` rather than panicking when the peer claims more bytes than it
/// sent. A guard of the form `offset + 4 > data.len()` — which is what both
/// binaries used to have, and what the server still had after the client was
/// fixed — is itself the hazard: the addition it performs to decide whether
/// indexing is safe can overflow, and on overflow it concludes that it is.
///
/// The bytes are borrowed from `data` rather than copied. A `string` in SSH is
/// arbitrary binary, not text, and is not decoded here — §5 says so explicitly,
/// and a decoder that guessed UTF-8 would corrupt keys and filenames alike.
///
/// # Errors
///
/// [`WireError::Truncated`] if the length or the bytes run past the end of the
/// packet, [`WireError::LengthOutOfRange`] if the length exceeds `usize`.
pub fn read_ssh_string(data: &[u8], offset: usize) -> Result<(&[u8], usize), WireError> {
    let (len, start) = read_u32(data, offset)?;
    let len_usize = usize::try_from(len).map_err(|_| WireError::LengthOutOfRange { len })?;
    let end = start
        .checked_add(len_usize)
        .ok_or(WireError::LengthOutOfRange { len })?;
    let value = data.get(start..end).ok_or(WireError::Truncated {
        what: "string",
        needed: len_usize,
        available: remaining(data, start),
    })?;
    Ok((value, end))
}

/// Read an SSH `mpint` at `offset`, returning unsigned big-endian bytes.
///
/// The sign byte [`encode_mpint`] adds is not part of the number, so it comes
/// back off here. Leaving it on would make `f` a different integer at one end
/// of the key exchange than at the other, which is a signature failure with no
/// diagnosable cause.
///
/// A negative `mpint` — one whose top bit is set after the leading zeros are
/// gone — is not rejected here, because this crate is used only where the
/// protocol defines the value as unsigned (the Diffie-Hellman `e` and `f`), and
/// range-checking those against the group prime is the caller's job and is
/// stricter than a sign check.
///
/// # Errors
///
/// As [`read_ssh_string`].
pub fn read_mpint(data: &[u8], offset: usize) -> Result<(&[u8], usize), WireError> {
    let (raw, next) = read_ssh_string(data, offset)?;
    Ok((strip_leading_zeros(raw), next))
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
// A test that gets an `Err` where it asserted a value should stop, loudly, at
// the line that was wrong -- which is what these lints exist to prevent in
// production code and what makes them counterproductive here.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // ---- identification line (RFC 4253 §4.2) ----

    #[test]
    fn the_cr_of_the_crlf_is_framing_and_comes_off() {
        assert_eq!(strip_line_terminator(b"SSH-2.0-x\r"), b"SSH-2.0-x");
    }

    #[test]
    fn a_line_with_no_cr_is_unchanged() {
        // OpenSSH tolerates a bare LF, and the hashed bytes are the same either
        // way — the terminator is framing, not part of the string.
        assert_eq!(strip_line_terminator(b"SSH-2.0-x"), b"SSH-2.0-x");
    }

    #[test]
    fn only_the_last_cr_comes_off() {
        // This is the divergence itself. sshd stripped every CR in the line and
        // the client only the trailing one, so for this input one end would have
        // hashed `SSH-2.0-ab` and the other `SSH-2.0-a\rb`.
        assert_eq!(strip_line_terminator(b"SSH-2.0-a\rb\r"), b"SSH-2.0-a\rb");
    }

    #[test]
    fn an_empty_line_survives_the_terminator_strip() {
        assert_eq!(strip_line_terminator(b""), b"");
        assert_eq!(strip_line_terminator(b"\r"), b"");
    }

    #[test]
    fn the_identification_line_is_the_one_starting_ssh() {
        assert!(is_identification_line(b"SSH-2.0-OpenSSH_9.6\r"));
        assert!(is_identification_line(b"SSH-1.99-legacy"));
        assert!(!is_identification_line(b"Authorized users only\r"));
        assert!(!is_identification_line(b""));
        // The prefix test runs after the terminator strip, so a line that is
        // *only* a terminator is not mistaken for one.
        assert!(!is_identification_line(b"\r"));
    }

    #[test]
    fn a_decoded_identification_is_byte_for_byte_what_arrived() {
        assert_eq!(
            decode_identification(b"SSH-2.0-OpenSSH_9.6\r"),
            Ok("SSH-2.0-OpenSSH_9.6")
        );
    }

    #[test]
    fn a_line_that_cannot_be_reproduced_is_refused_not_adjusted() {
        // 0xFF is not valid UTF-8 anywhere. Reading it as U+00FF — which is what
        // `char::from(u8)` does, and what the client used to do — yields a
        // string whose bytes are two where the wire had one, so the two ends
        // hash different values of V_S and the signature check fails with
        // nothing to indicate the cause.
        assert_eq!(
            decode_identification(b"SSH-2.0-bad\xFFname\r"),
            Err(IdentificationError::NotUtf8)
        );
    }

    #[test]
    fn the_length_limit_counts_the_crlf_the_rfc_counts() {
        // §4.2: 255 bytes including CR and LF, so 253 of content is the most
        // that fits, whether or not the sender included the CR.
        let longest = vec![b'x'; MAX_IDENTIFICATION_LINE - 2];
        assert!(decode_identification(&longest).is_ok());

        let one_too_many = vec![b'x'; MAX_IDENTIFICATION_LINE - 1];
        assert_eq!(
            decode_identification(&one_too_many),
            Err(IdentificationError::TooLong)
        );

        // A trailing CR is not content, so it does not consume the budget twice.
        let mut with_cr = vec![b'x'; MAX_IDENTIFICATION_LINE - 2];
        with_cr.push(b'\r');
        assert!(decode_identification(&with_cr).is_ok());
    }

    #[test]
    fn the_length_check_precedes_the_utf8_check() {
        // Both are failures, and which is reported matters only for the message
        // — but a fixed order is one less thing for the two ends to differ on.
        let mut long_and_invalid = vec![b'x'; MAX_IDENTIFICATION_LINE];
        long_and_invalid.push(0xFF);
        assert_eq!(
            decode_identification(&long_and_invalid),
            Err(IdentificationError::TooLong)
        );
    }

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

    // ---- wire decoding (RFC 4253 §5) ----

    #[test]
    fn a_byte_reads_and_advances_by_one() {
        assert_eq!(read_byte(b"\x2a\x00", 0), Ok((0x2a, 1)));
        assert_eq!(read_byte(b"\x2a\x07", 1), Ok((0x07, 2)));
    }

    #[test]
    fn a_byte_past_the_end_is_truncated_not_a_panic() {
        assert_eq!(
            read_byte(b"", 0),
            Err(WireError::Truncated {
                what: "byte",
                needed: 1,
                available: 0
            })
        );
        assert!(read_byte(b"\x00", 1).is_err());
    }

    #[test]
    fn any_nonzero_byte_is_true_because_the_rfc_says_so() {
        // §5 requires readers to accept any nonzero value, so rejecting
        // anything but 1 would refuse peers the protocol allows.
        assert_eq!(read_bool(b"\x00", 0), Ok((false, 1)));
        assert_eq!(read_bool(b"\x01", 0), Ok((true, 1)));
        assert_eq!(read_bool(b"\xff", 0), Ok((true, 1)));
    }

    #[test]
    fn a_uint32_roundtrips_through_the_shared_writer() {
        // Checked against `ssh_u32` rather than against a hand-written byte
        // string: the pair is the contract, and a reader tested only against
        // its own idea of the encoding is the failure this crate exists to end.
        for value in [0u32, 1, 0x0102_0304, u32::MAX] {
            assert_eq!(read_u32(&ssh_u32(value), 0), Ok((value, 4)));
        }
    }

    #[test]
    fn a_uint32_needs_all_four_bytes() {
        for have in 0..4usize {
            let data = vec![0u8; have];
            assert_eq!(
                read_u32(&data, 0),
                Err(WireError::Truncated {
                    what: "uint32",
                    needed: 4,
                    available: have
                }),
                "{have} bytes should not satisfy a uint32"
            );
        }
    }

    #[test]
    fn an_offset_near_the_top_of_the_address_space_does_not_wrap_into_a_read() {
        // This is the bug the `get`/`first_chunk` form exists to prevent. The
        // old guard was `offset + 4 > data.len()`: at this offset that addition
        // wraps to 3, concludes the read is in bounds, and indexes past the end
        // of an 8-byte buffer. An attacker does not reach this offset directly,
        // but every caller that adds an attacker-supplied length to an offset
        // can hand one over.
        let data = [0u8; 8];
        assert!(read_u32(&data, usize::MAX - 1).is_err());
        assert!(read_byte(&data, usize::MAX).is_err());
        assert!(read_ssh_string(&data, usize::MAX - 1).is_err());
    }

    #[test]
    fn a_string_roundtrips_through_the_shared_writer() {
        for payload in [&b""[..], &b"x"[..], &b"ssh-ed25519"[..], &[0xff; 300][..]] {
            let encoded = ssh_string(payload);
            let (value, next) = read_ssh_string(&encoded, 0).expect("roundtrip");
            assert_eq!(value, payload);
            assert_eq!(next, encoded.len());
        }
    }

    #[test]
    fn a_string_is_bytes_and_an_empty_one_is_a_value_not_an_error() {
        // A zero-length string is legal and common (an empty banner, an empty
        // language tag), so it must read back as an empty slice.
        assert_eq!(read_ssh_string(&[0, 0, 0, 0], 0), Ok((&[][..], 4)));
    }

    #[test]
    fn a_string_longer_than_the_packet_is_refused_with_both_numbers() {
        // The report has to distinguish "the packet was cut short" from "the
        // peer claimed four gigabytes", because they are the same failure with
        // very different causes.
        let mut data = ssh_u32(u32::MAX).to_vec();
        data.extend_from_slice(b"three");
        assert_eq!(
            read_ssh_string(&data, 0),
            Err(WireError::Truncated {
                what: "string",
                needed: usize::try_from(u32::MAX).expect("u32 fits usize on this target"),
                available: 5
            })
        );
    }

    #[test]
    fn reads_chain_by_offset_the_way_a_packet_is_actually_parsed() {
        let mut packet = vec![0x14u8]; // a message number
        packet.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        packet.extend_from_slice(&ssh_u32(7));
        packet.push(1); // a boolean
        let (msg, off) = read_byte(&packet, 0).expect("byte");
        let (name, off) = read_ssh_string(&packet, off).expect("string");
        let (n, off) = read_u32(&packet, off).expect("uint32");
        let (flag, off) = read_bool(&packet, off).expect("boolean");
        assert_eq!((msg, name, n, flag), (0x14, &b"ssh-ed25519"[..], 7, true));
        assert_eq!(off, packet.len(), "the parse consumed exactly the packet");
    }

    #[test]
    fn an_mpint_comes_back_without_the_sign_pad_the_writer_added() {
        // 0x80 has its top bit set, so `encode_mpint` prepends a zero to keep
        // it positive. That zero is framing: reading it as part of the number
        // makes `f` a different integer here than at the far end, and the only
        // symptom is a host-key signature that fails for no visible reason.
        let encoded = encode_mpint(&[0x80]);
        assert_eq!(encoded.len(), 6, "writer should have padded");
        assert_eq!(read_mpint(&encoded, 0), Ok((&[0x80][..], 6)));
    }

    #[test]
    fn an_mpint_roundtrips_through_the_shared_writer() {
        for value in [
            &b""[..],
            &[0x7f][..],
            &[0x80][..],
            &[0x01, 0x00, 0xff][..],
            &[0xff; 256][..],
        ] {
            let encoded = encode_mpint(value);
            let (decoded, next) = read_mpint(&encoded, 0).expect("roundtrip");
            assert_eq!(decoded, strip_leading_zeros(value));
            assert_eq!(next, encoded.len());
        }
    }

    #[test]
    fn a_zero_mpint_is_the_empty_string_in_both_directions() {
        assert_eq!(encode_mpint(&[0, 0, 0]), vec![0, 0, 0, 0]);
        assert_eq!(read_mpint(&[0, 0, 0, 0], 0), Ok((&[][..], 4)));
    }
}
