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
    /// A packet's `packet_length` field is outside what §6 permits.
    ///
    /// Both bounds are one variant because both are the same mistake seen from
    /// two ends, and separating them invited exactly the divergence this crate
    /// exists to stop: the client checked the ceiling and the floor, the server
    /// checked only the ceiling, and a `packet_length` of 0 walked past its
    /// guard to be indexed at offset 4.
    PacketLength {
        /// The length the peer announced.
        len: usize,
    },
    /// `padding_length` claims more bytes than the packet contains.
    ///
    /// §6 makes the payload `packet_length - padding_length - 1`, so a padding
    /// length at or past `packet_length` is a peer describing a negative
    /// payload — which, computed in `usize`, is a very large one.
    PaddingLength {
        /// The `packet_length` field.
        packet_length: usize,
        /// The `padding_length` byte.
        padding_length: usize,
    },
    /// The MAC on a received packet is not the one over its plaintext.
    ///
    /// Carries nothing. What the received MAC *was* is the one detail an
    /// attacker probing for an oracle would want back, and it tells whoever is
    /// debugging nothing they cannot get from the packet itself.
    MacMismatch,
    /// A caller offered a padding block of the wrong length.
    ///
    /// Ours, not the peer's — the sole variant here that is not a protocol
    /// fault. It exists because [`PacketCodec::encode`] takes its padding from
    /// the caller (the entropy source, and its failure mode, belong to the
    /// binary) and must not quietly pad the difference with zeros.
    PaddingSize {
        /// How many bytes the block alignment required.
        wanted: usize,
        /// How many the caller supplied.
        given: usize,
    },
    /// Key derivation produced fewer bytes than the algorithm needs.
    ///
    /// Unreachable while `derive_key` returns what it is asked for. Stated
    /// anyway because the alternative at the one site that can hit it is an
    /// index, and a cipher keyed from a short buffer — silently zero-extended —
    /// is worse than a refused handshake.
    ShortKey {
        /// Which derived value came back short.
        what: &'static str,
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
            Self::PacketLength { len } => write!(
                f,
                "packet length {len} is outside the {MIN_PACKET_LENGTH}..={MAX_PACKET_SIZE} the framing allows"
            ),
            Self::PaddingLength {
                packet_length,
                padding_length,
            } => write!(
                f,
                "padding length {padding_length} does not fit a packet of {packet_length}"
            ),
            Self::MacMismatch => f.write_str("MAC verification failed"),
            Self::PaddingSize { wanted, given } => {
                write!(f, "padding must be {wanted} bytes, not {given}")
            }
            Self::ShortKey { what } => {
                write!(f, "key derivation produced a short {what}")
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

// ============================================================================
// HMAC-SHA256
// ============================================================================

/// Compute HMAC-SHA256(key, data).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    /// SHA-256's block size, and therefore HMAC's (RFC 2104).
    const BLOCK_SIZE: usize = 64;

    // A key longer than one block is replaced by its digest. That is what makes
    // the pad below total rather than conditional: `key_used` is now at most 64
    // bytes, either because it already was or because it is a 32-byte hash.
    let key_hash;
    let key_used: &[u8] = if key.len() > BLOCK_SIZE {
        key_hash = sha256(key);
        &key_hash
    } else {
        key
    };

    // A fixed-size array rather than a `vec![0u8; block_size]`, so "the pad is
    // exactly one block" is the type and not a runtime length.
    let mut k_padded = [0u8; BLOCK_SIZE];
    if let Some(head) = k_padded.get_mut(..key_used.len()) {
        head.copy_from_slice(key_used);
    }

    // Inner: SHA256((key XOR ipad) || data)
    let mut inner = Vec::with_capacity(BLOCK_SIZE.saturating_add(data.len()));
    inner.extend(k_padded.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);

    // Outer: SHA256((key XOR opad) || inner_hash)
    let mut outer = Vec::with_capacity(BLOCK_SIZE.saturating_add(inner_hash.len()));
    outer.extend(k_padded.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// Compute the SSH MAC for a packet.
/// MAC = HMAC-SHA256(key, sequence_number(u32_be) || unencrypted_packet)
pub fn compute_mac(key: &[u8], seq: u32, packet: &[u8]) -> Vec<u8> {
    let mut mac_input = Vec::with_capacity(packet.len().saturating_add(4));
    mac_input.extend_from_slice(&seq.to_be_bytes());
    mac_input.extend_from_slice(packet);
    hmac_sha256(key, &mac_input).to_vec()
}

/// Constant-time comparison to prevent timing attacks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ============================================================================
// AES-128-CTR encryption/decryption
//
// A simplified AES-128 implementation for the SSH transport layer.
// Not optimized for performance — adequate for an OS utility.
// ============================================================================

/// AES S-Box lookup table.
const AES_SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// AES round constants.
const AES_RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// S-box substitution. A `u8` always indexes a 256-entry table, so the
/// fallback is unreachable; saying it with `get` costs nothing and keeps the
/// one genuinely computed index in this cipher out of the unchecked column.
fn sbox(byte: u8) -> u8 {
    AES_SBOX.get(usize::from(byte)).copied().unwrap_or(0)
}

/// Galois Field multiplication by 2 in GF(2^8).
fn gf_mul2(x: u8) -> u8 {
    // Double, then reduce if a bit fell off the top, by the AES field
    // polynomial x^8 + x^4 + x^3 + x + 1 (0x11b, low byte 0x1b). `wrapping_shl`
    // rather than `<<` because discarding that bit is the definition of the
    // operation, not an accident of the width.
    let shifted = x.wrapping_shl(1);
    if (x & 0x80) != 0 {
        shifted ^ 0x1b
    } else {
        shifted
    }
}

/// Galois Field multiplication by 3 in GF(2^8).
fn gf_mul3(x: u8) -> u8 {
    gf_mul2(x) ^ x
}

/// AES-128 key expansion. Produces 11 round keys (176 bytes total).
fn aes128_key_expand(key: &[u8; 16]) -> [[u8; 16]; 11] {
    let mut round_keys = [[0u8; 16]; 11];
    let mut prev = *key;

    // Zipping the round-constant table against the output slots states the
    // rule "one round key per round constant" once. The old `for i in 1..11`
    // had the count 11 written in the loop bound and the count 10 implied by
    // `AES_RCON`, and reached back into the array it was filling with
    // `round_keys[i - 1]` -- three facts that had to agree by hand.
    let mut slots = round_keys.iter_mut();
    if let Some(first) = slots.next() {
        *first = prev;
    }

    for (&rcon, slot) in AES_RCON.iter().zip(slots) {
        // RotWord + SubWord + Rcon over the previous key's last column.
        let mut word = *prev.last_chunk::<4>().unwrap_or(&[0; 4]);
        word.rotate_left(1);
        for b in &mut word {
            *b = sbox(*b);
        }
        if let Some(top) = word.first_mut() {
            *top ^= rcon;
        }

        let mut next = [0u8; 16];
        for (prev_col, next_col) in prev.chunks_exact(4).zip(next.chunks_exact_mut(4)) {
            for ((dst, &p), &w) in next_col.iter_mut().zip(prev_col).zip(word.iter()) {
                *dst = p ^ w;
            }
            // The column just written feeds the next one.
            word = <[u8; 4]>::try_from(&*next_col).unwrap_or([0; 4]);
        }

        *slot = next;
        prev = next;
    }
    round_keys
}

/// Encrypt one 16-byte block with AES-128.
fn aes128_encrypt_block(block: &[u8; 16], round_keys: &[[u8; 16]; 11]) -> [u8; 16] {
    // Destructuring says what `round_keys[0]`, `.take(10).skip(1)` and
    // `round_keys[10]` said, but the compiler checks the arithmetic instead of
    // the reader: "first, all but the last, last" cannot be off by one, whereas
    // a `take`/`skip` pair silently loses or repeats a round if either constant
    // drifts from the array's length.
    let [first_key, middle_keys @ .., last_key] = round_keys;
    let mut state = *block;

    // Initial round key addition.
    xor_block(&mut state, first_key);

    // Rounds 1..9: SubBytes, ShiftRows, MixColumns, AddRoundKey.
    for round_key in middle_keys {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        xor_block(&mut state, round_key);
    }

    // Final round (no MixColumns).
    sub_bytes(&mut state);
    shift_rows(&mut state);
    xor_block(&mut state, last_key);

    state
}

fn xor_block(state: &mut [u8; 16], key: &[u8; 16]) {
    for (s, &k) in state.iter_mut().zip(key.iter()) {
        *s ^= k;
    }
}

fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = sbox(*b);
    }
}

// Every index below is a literal into a `[u8; 16]`, so the bound is checked at
// compile time by the array's own type -- there is no runtime index here for
// `indexing_slicing` to be warning about. Writing the row rotations through
// `get`/`get_mut` would add sixteen `Option`s that can never be `None` and
// would obscure the one thing this function has to get right, which is which
// index moves where.
#[allow(clippy::indexing_slicing)]
fn shift_rows(state: &mut [u8; 16]) {
    // AES state is column-major: indices [row + 4*col]
    // Row 0: no shift
    // Row 1: shift left by 1
    let tmp = state[1];
    state[1] = state[5];
    state[5] = state[9];
    state[9] = state[13];
    state[13] = tmp;
    // Row 2: shift left by 2
    let (t0, t1) = (state[2], state[6]);
    state[2] = state[10];
    state[6] = state[14];
    state[10] = t0;
    state[14] = t1;
    // Row 3: shift left by 3 (= shift right by 1)
    let tmp = state[15];
    state[15] = state[11];
    state[11] = state[7];
    state[7] = state[3];
    state[3] = tmp;
}

fn mix_columns(state: &mut [u8; 16]) {
    // `chunks_exact_mut(4)` hands out the four columns directly, so the
    // `col * 4` base and its four `off + k` offsets -- five chances to write
    // one column while reading another -- are gone. A 16-byte state is four
    // whole columns, so the remainder is always empty.
    for col in state.chunks_exact_mut(4) {
        let [a0, a1, a2, a3] = *col else { continue };
        col.copy_from_slice(&[
            gf_mul2(a0) ^ gf_mul3(a1) ^ a2 ^ a3,
            a0 ^ gf_mul2(a1) ^ gf_mul3(a2) ^ a3,
            a0 ^ a1 ^ gf_mul2(a2) ^ gf_mul3(a3),
            gf_mul3(a0) ^ a1 ^ a2 ^ gf_mul2(a3),
        ]);
    }
}

// ============================================================================
// AES-128-CTR (RFC 4344 §4)
// ============================================================================

/// AES-128 in counter mode, for one direction of one SSH connection.
///
/// **The counter is state, and that is the whole point of this type.** RFC 4344
/// §4 defines a single 16-byte counter per direction, initialised to the derived
/// IV and incremented once per encrypted block, continuously for the life of the
/// key. It is never restarted and never derived from the packet sequence number.
///
/// Both ends had this wrong, in different directions, and neither test suite
/// could see it. `ssh` restarted the counter from the IV for every packet, so
/// every packet it sent was XOR-ed with the *same* keystream — an observer who
/// XORs two of our ciphertexts together cancels the key out and is left with the
/// XOR of two plaintexts, which for SSH's structured payloads is readable. `sshd`
/// computed `IV + seq * 256 + block`, a formula with no basis in the RFC, which
/// disagreed with the client on every block and collided with itself on any
/// packet over 4 KiB. See `known-issues.md`
/// `TD-B-THE-AES-CTR-COUNTER-IS-REUSED-BY-THE-CLIENT-AND-INVENTED-BY-THE-SERVER`.
///
/// Holding the counter inside the cipher is what makes those bugs unwriteable:
/// there is no `seq` parameter for a sequence number to be folded into, and
/// there is no way to encrypt without advancing.
#[derive(Clone)]
pub struct Aes128Ctr {
    round_keys: [[u8; 16]; 11],
    counter: [u8; 16],
}

impl core::fmt::Debug for Aes128Ctr {
    /// Deliberately opaque: the key schedule is key material and the counter
    /// position leaks how much traffic has passed. A derived `Debug` would put
    /// both in any log line that formatted an `EncryptionState`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Aes128Ctr { .. }")
    }
}

impl Aes128Ctr {
    /// Start a cipher from a 16-byte key and a 16-byte initial counter block.
    ///
    /// Both come from `derive_key`, which produces at least 16 bytes for each,
    /// so the fixed-size arrays are the right shape for the caller to prove
    /// once rather than for this to re-check per block.
    #[must_use]
    pub fn new(key: &[u8; 16], iv: &[u8; 16]) -> Self {
        Self {
            round_keys: aes128_key_expand(key),
            counter: *iv,
        }
    }

    /// Encrypt or decrypt `data` in place, advancing the counter past it.
    ///
    /// CTR mode is its own inverse, so this is both directions. The counter
    /// advances by one per 16-byte block including a short final one, because
    /// the block it consumed is spent either way.
    pub fn apply(&mut self, data: &mut [u8]) {
        for chunk in data.chunks_mut(16) {
            let keystream = aes128_encrypt_block(&self.counter, &self.round_keys);
            for (b, k) in chunk.iter_mut().zip(keystream) {
                *b ^= k;
            }
            increment_counter(&mut self.counter);
        }
    }

    /// Decrypt up to 16 bytes *without* advancing the counter.
    ///
    /// SSH's packet length lives in the first encrypted block, so a reader has
    /// to decrypt that block to learn how many more bytes to wait for — and
    /// then decrypt it again as part of the whole packet once they arrive.
    /// Taking `&self` is what states that the second pass starts where this one
    /// did; an `apply` here would silently drop a block of keystream and
    /// desynchronise the connection from its first encrypted packet onward.
    #[must_use]
    pub fn peek_block(&self, data: &[u8]) -> Vec<u8> {
        let keystream = aes128_encrypt_block(&self.counter, &self.round_keys);
        data.iter().zip(keystream).map(|(b, k)| b ^ k).collect()
    }
}

/// Increment a 16-byte big-endian counter by 1, wrapping at the top.
///
/// Wrapping is correct rather than merely convenient: 2^128 blocks is
/// unreachable, and the RFC's counter is defined modulo 2^128 regardless.
fn increment_counter(counter: &mut [u8; 16]) {
    for b in counter.iter_mut().rev() {
        let (val, overflow) = b.overflowing_add(1);
        *b = val;
        if !overflow {
            return;
        }
    }
}

// ============================================================================
// The binary packet protocol (RFC 4253 §6)
// ============================================================================

/// The largest `packet_length` we will accept or produce.
///
/// §6.1 obliges an implementation to handle 35000 bytes of payload; this is
/// that figure applied to the whole packet, which is what both binaries have
/// always meant by it.
pub const MAX_PACKET_SIZE: usize = 35000;

/// The smallest `packet_length` §6 can describe: one `padding_length` byte
/// plus the four bytes of padding the section makes a minimum. A packet
/// claiming less is malformed by definition, not merely empty.
pub const MIN_PACKET_LENGTH: usize = 5;

/// The multiple every packet is padded to before a cipher is negotiated.
///
/// §6: "the cipher block size or 8, whichever is larger" — and before
/// `NEWKEYS` the cipher is `none`, whose block size §6.3 fixes at 8.
const BLOCK_SIZE_UNENCRYPTED: usize = 8;

/// Which end of the connection a codec is.
///
/// RFC 4253 §7.2 derives six values labelled `A`..`F`, and three of them are
/// "client to server". Which of those three is *outbound* therefore depends on
/// who is asking, and until now each binary answered that by hand: the client
/// assigned `A`/`C`/`E` to its send direction and the server assigned them to
/// its receive direction, in two separate blocks of near-identical code that
/// had to stay each other's mirror image forever. Naming the end once and
/// letting [`PacketCodec::activate`] do the mapping is what makes the two
/// assignments a single statement instead of a coincidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The end that connects, sends `V_C`, and encrypts under the `A`/`C`/`E`
    /// values.
    Client,
    /// The end that listens, sends `V_S`, and encrypts under the `B`/`D`/`F`
    /// values.
    Server,
}

/// The SSH transport's packet layer: framing, encryption, MAC and both
/// sequence numbers.
///
/// # Why the sequence numbers live in here
///
/// §6.4 makes the MAC `HMAC(key, sequence_number || unencrypted_packet)`, so
/// the sequence number is an *input to the framing*, not bookkeeping the caller
/// keeps alongside it. Held outside, it is a value that must be incremented by
/// hand at every one of a dozen call sites and never on the error paths between
/// them — and it was not: `known-issues.md` records a send path that passed
/// `seq_send` to the packet builder and then never advanced it, so every packet
/// after the first was authenticated under a number the peer had already moved
/// past. Owning it here makes "encode a packet" and "advance the sequence
/// number" the same act, exactly as owning the counter inside [`Aes128Ctr`]
/// made "encrypt" and "advance the counter" the same act.
///
/// # What it deliberately does not do
///
/// No I/O. [`PacketCodec::decode`] is a pure function of a byte slice and
/// returns `Ok(None)` when a whole packet has not arrived, so each binary keeps
/// its own buffering and its own idea of what to do while waiting — the server
/// has a shell to service, the client has a terminal to read. It is also what
/// keeps the framing testable on a development host, which has no kernel to
/// hold a TCP connection open.
///
/// It does not generate padding either; see [`PacketCodec::encode`].
#[derive(Clone)]
pub struct PacketCodec {
    /// Cipher for packets we send. `None` before `NEWKEYS`.
    cipher_out: Option<Aes128Ctr>,
    /// Cipher for packets we receive. `None` before `NEWKEYS`.
    cipher_in: Option<Aes128Ctr>,
    mac_key_out: Vec<u8>,
    mac_key_in: Vec<u8>,
    block_size: usize,
    mac_len: usize,
    seq_out: u32,
    seq_in: u32,
}

impl core::fmt::Debug for PacketCodec {
    /// Opaque for the same reason [`Aes128Ctr`]'s is: it holds key material,
    /// and even the sequence numbers say how much traffic has passed.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PacketCodec { .. }")
    }
}

impl Default for PacketCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketCodec {
    /// A codec for the pre-`NEWKEYS` transport: no cipher, no MAC, 8-byte
    /// blocks, both sequence numbers at zero.
    ///
    /// The absent ciphers are `Option`s rather than an `encrypted: bool` beside
    /// a set of keys. Both binaries carried such a flag, and a flag reading
    /// "encrypted" next to a cipher that was never installed is a plaintext
    /// packet sent in the belief that it was protected. Only one of the two can
    /// now be true at a time.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cipher_out: None,
            cipher_in: None,
            mac_key_out: Vec::new(),
            mac_key_in: Vec::new(),
            block_size: BLOCK_SIZE_UNENCRYPTED,
            mac_len: 0,
            seq_out: 0,
            seq_in: 0,
        }
    }

    /// Install the keys `NEWKEYS` brings into force, for `aes128-ctr` with
    /// `hmac-sha2-256`.
    ///
    /// `shared_secret` is `K` as its raw big-endian bytes — [`derive_key`]
    /// applies the §5 `mpint` encoding that §7.2 actually hashes, so passing
    /// an already-encoded `K` here would encode it twice. `exchange_hash` is
    /// `H`, and `session_id` is the `H` of the *first* key exchange, which
    /// stays fixed across rekeys.
    ///
    /// The sequence numbers deliberately survive this call. §6.4 runs them for
    /// the life of the connection, not the life of a key, and resetting them at
    /// a rekey would break the MAC on the very next packet.
    ///
    /// # Errors
    ///
    /// [`WireError::ShortKey`] if a derived value came back shorter than the
    /// algorithm needs. `derive_key` returns the length it is asked for, so
    /// this is unreachable; it is reported rather than indexed past because the
    /// failure it stands for — a cipher keyed with implicit zero padding — is
    /// silent and total.
    pub fn activate(
        &mut self,
        role: Role,
        shared_secret: &[u8],
        exchange_hash: &[u8; 32],
        session_id: &[u8; 32],
    ) -> Result<(), WireError> {
        // The lengths are the negotiated algorithms', not SHA-256's: aes128-ctr
        // takes a 16-byte key and a 16-byte IV, hmac-sha2-256 a 32-byte key.
        let derive = |label: u8, len: usize| {
            derive_key(shared_secret, exchange_hash, label, session_id, len)
        };

        // §7.2 names the six values by direction, not by who is reading them.
        // This is the one place that translation happens.
        let (iv_out, key_out, mac_out, iv_in, key_in, mac_in) = match role {
            Role::Client => (b'A', b'C', b'E', b'B', b'D', b'F'),
            Role::Server => (b'B', b'D', b'F', b'A', b'C', b'E'),
        };

        let (iv_out, key_out) = (derive(iv_out, 16), derive(key_out, 16));
        let (iv_in, key_in) = (derive(iv_in, 16), derive(key_in, 16));

        // A cipher is only ever constructed from a key and an IV together, so
        // there is no window in which one is installed without the other.
        let (Some(key_out), Some(iv_out), Some(key_in), Some(iv_in)) = (
            key_out.first_chunk::<16>(),
            iv_out.first_chunk::<16>(),
            key_in.first_chunk::<16>(),
            iv_in.first_chunk::<16>(),
        ) else {
            return Err(WireError::ShortKey {
                what: "cipher key or IV",
            });
        };

        self.cipher_out = Some(Aes128Ctr::new(key_out, iv_out));
        self.cipher_in = Some(Aes128Ctr::new(key_in, iv_in));
        self.mac_key_out = derive(mac_out, 32);
        self.mac_key_in = derive(mac_in, 32);
        self.block_size = 16;
        self.mac_len = 32;
        Ok(())
    }

    /// Whether `NEWKEYS` has taken effect.
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.cipher_out.is_some()
    }

    /// The alignment a received packet's first block is read in.
    ///
    /// A caller polling a socket needs this to know how few bytes are too few
    /// to bother trying [`decode`](Self::decode) with.
    #[must_use]
    pub fn inbound_block_size(&self) -> usize {
        if self.cipher_in.is_some() {
            self.block_size.max(BLOCK_SIZE_UNENCRYPTED)
        } else {
            BLOCK_SIZE_UNENCRYPTED
        }
    }

    /// The sequence number of the packet [`decode`](Self::decode) returned
    /// last.
    ///
    /// `decode` advances `seq_in` as it hands a packet back, so the number that
    /// packet was authenticated under is one behind. A caller that must quote it
    /// — `SSH_MSG_UNIMPLEMENTED` names the sequence number it is rejecting
    /// (§11.4) — needs this rather than an arithmetic guess.
    #[must_use]
    pub fn last_inbound_seq(&self) -> u32 {
        self.seq_in.wrapping_sub(1)
    }

    /// How many padding bytes a payload of `payload_len` needs.
    ///
    /// §6: `packet_length + padding_length + payload + padding` is a multiple
    /// of the block size, with at least four bytes of padding.
    #[must_use]
    pub fn padding_len(&self, payload_len: usize) -> usize {
        let block_size = if self.cipher_out.is_some() {
            self.block_size.max(BLOCK_SIZE_UNENCRYPTED)
        } else {
            BLOCK_SIZE_UNENCRYPTED
        };
        // Saturating throughout: every input is ours (a payload we built, a
        // block size of 8 or 16), so none of it can overflow — but a saturating
        // form that produces a packet the peer rejects beats a wrapping one
        // that produces a *valid-looking* packet describing the wrong length.
        // The server's copy of this arithmetic was the plain-operator one, and
        // its `% block_size` would have divided by zero for a block size of 0.
        let unpadded = payload_len.saturating_add(1);
        let overhang = unpadded
            .saturating_add(4)
            .checked_rem(block_size)
            .unwrap_or(0);
        let padding = block_size.saturating_sub(overhang);
        if padding < 4 {
            padding.saturating_add(block_size)
        } else {
            padding
        }
    }

    /// Frame, encrypt and authenticate one packet, advancing the outbound
    /// sequence number and the cipher's counter.
    ///
    /// `padding` must be exactly [`padding_len`](Self::padding_len) bytes.
    /// §6 says those bytes SHOULD be random, and this crate does not produce
    /// them: entropy is a syscall, it can fail, and what to do when it fails is
    /// the binary's decision — the alternative is a shared layer that either
    /// panics on a host with no CSPRNG or quietly pads with zeros, which is the
    /// behaviour this parameter exists to retire.
    ///
    /// # Errors
    ///
    /// [`WireError::PaddingSize`] if `padding` is not the required length. It
    /// is an error rather than a truncate-or-extend because both silent fixes
    /// are wrong: a short block would be zero-extended into the packet, and a
    /// long one would mean the caller had computed the alignment differently
    /// from the codec that is about to state it in the length field.
    pub fn encode(&mut self, payload: &[u8], padding: &[u8]) -> Result<Vec<u8>, WireError> {
        let wanted = self.padding_len(payload.len());
        if padding.len() != wanted {
            return Err(WireError::PaddingSize {
                wanted,
                given: padding.len(),
            });
        }

        let packet_length = payload.len().saturating_add(1).saturating_add(wanted);
        let mut pkt = Vec::with_capacity(packet_length.saturating_add(4));
        pkt.extend_from_slice(
            &u32::try_from(packet_length)
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        pkt.push(u8::try_from(wanted).unwrap_or(u8::MAX));
        pkt.extend_from_slice(payload);
        pkt.extend_from_slice(padding);

        if let Some(cipher) = self.cipher_out.as_mut() {
            // §6: the MAC covers the *plaintext* packet, so it is computed
            // before the cipher runs and appended after it.
            let mac = compute_mac(&self.mac_key_out, self.seq_out, &pkt);
            cipher.apply(&mut pkt);
            pkt.extend_from_slice(&mac);
        }

        self.seq_out = self.seq_out.wrapping_add(1);
        Ok(pkt)
    }

    /// Decode one packet from the front of `buf`, if a whole one is there.
    ///
    /// Returns the payload and how many bytes of `buf` it used, or `Ok(None)`
    /// when more bytes are needed — in which case nothing has changed, and the
    /// caller may read more and try again on the same bytes. That the cipher
    /// survives an `Ok(None)` untouched is why [`Aes128Ctr::peek_block`] takes
    /// `&self`: the length has to be decrypted to be read, and consuming that
    /// keystream would leave this counter a block ahead of the sender's for the
    /// rest of the session.
    ///
    /// On success the inbound sequence number and the cipher's counter have
    /// both advanced; the caller's only remaining duty is to drop the consumed
    /// bytes.
    ///
    /// # Errors
    ///
    /// [`WireError::PacketLength`] for a length outside §6's range,
    /// [`WireError::Truncated`] for a MAC shorter than the algorithm's,
    /// [`WireError::MacMismatch`] for a packet that is not authentic, and
    /// [`WireError::PaddingLength`] for a padding length that does not fit the
    /// packet. Every one of them is the peer's fault and none is recoverable:
    /// a codec that has decrypted a packet has moved its counter, so there is
    /// no resynchronising after a rejection — the connection must close.
    pub fn decode(&mut self, buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>, WireError> {
        let block_size = self.inbound_block_size();
        let Some(first_block) = buf.get(..block_size) else {
            return Ok(None);
        };

        // The length is inside the first encrypted block, so that block must be
        // decrypted before we know how many more bytes to wait for — and
        // decrypted again below as part of the whole packet.
        let first = match self.cipher_in.as_ref() {
            Some(cipher) => cipher.peek_block(first_block),
            None => first_block.to_vec(),
        };
        let (packet_length, _) = read_u32(&first, 0)?;
        let packet_length = usize::try_from(packet_length)
            .map_err(|_| WireError::LengthOutOfRange { len: packet_length })?;

        // Both bounds, and the floor is the one the server was missing: a peer
        // announcing a `packet_length` of 0 or 1 produced a four- or five-byte
        // packet, and the `padding_length` byte at offset 4 — which §6 puts
        // *inside* the packet — was then read off the end of it.
        if !(MIN_PACKET_LENGTH..=MAX_PACKET_SIZE).contains(&packet_length) {
            return Err(WireError::PacketLength { len: packet_length });
        }

        let mac_len = if self.cipher_in.is_some() {
            self.mac_len
        } else {
            0
        };
        let body_len = packet_length.saturating_add(4);
        let total = body_len.saturating_add(mac_len);
        let Some(raw) = buf.get(..total) else {
            return Ok(None);
        };
        // `body_len <= total == raw.len()` by construction, but the checked
        // split says so to the compiler rather than to a reader, and `split_at`
        // is a panic where this is a `?`.
        let (body, mac_data) = raw
            .split_at_checked(body_len)
            .ok_or(WireError::PacketLength { len: packet_length })?;

        // Past this point the packet is consumed whatever happens: the cipher's
        // counter moves, so every remaining failure is fatal to the connection.
        let plain = if let Some(cipher) = self.cipher_in.as_mut() {
            let mut plain = body.to_vec();
            cipher.apply(&mut plain);

            // A short MAC must *reject*. The server's copy guarded this
            // comparison with `mac_data.len() >= mac_len`, so a truncated MAC
            // skipped verification entirely and the packet was accepted
            // unauthenticated — from a peer that, in a daemon, has not yet
            // authenticated anything.
            let received = mac_data.get(..mac_len).ok_or(WireError::Truncated {
                what: "MAC",
                needed: mac_len,
                available: mac_data.len(),
            })?;
            let expected = compute_mac(&self.mac_key_in, self.seq_in, &plain);
            if !constant_time_eq(received, &expected) {
                return Err(WireError::MacMismatch);
            }
            plain
        } else {
            body.to_vec()
        };

        // Layout: [0..4] length, [4] padding_length, [5..] payload then padding.
        let (padding_length, payload_start) = read_byte(&plain, 4)?;
        let padding_length = usize::from(padding_length);
        let payload_len = packet_length
            .checked_sub(1)
            .and_then(|n| n.checked_sub(padding_length))
            .ok_or(WireError::PaddingLength {
                packet_length,
                padding_length,
            })?;
        let payload_end = payload_start.saturating_add(payload_len);
        let payload = plain
            .get(payload_start..payload_end)
            .ok_or(WireError::PaddingLength {
                packet_length,
                padding_length,
            })?
            .to_vec();

        self.seq_in = self.seq_in.wrapping_add(1);
        Ok(Some((payload, total)))
    }
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

    // ---- HMAC-SHA256 and the packet MAC (RFC 4253 §6.4, RFC 4231) ----

    /// Decode a hex string used to quote a published test vector verbatim.
    fn hex(s: &str) -> Vec<u8> {
        assert!(s.len().is_multiple_of(2), "hex string has an odd length");
        s.as_bytes()
            .chunks(2)
            .map(|pair| {
                let text = core::str::from_utf8(pair).expect("ascii hex");
                u8::from_str_radix(text, 16).expect("hex digit pair")
            })
            .collect()
    }

    fn to_hex(bytes: &[u8]) -> String {
        use core::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
    }

    /// RFC 4231 §4.2–4.4, the published HMAC-SHA-256 cases.
    ///
    /// Stated against the RFC rather than against our own output, which is the
    /// whole distinction this crate exists to draw: the two ends previously each
    /// checked their HMAC against a value produced by that same HMAC.
    #[test]
    fn the_rfc_4231_hmac_sha256_cases_come_out_as_published() {
        for (key, data, want) in [
            (
                hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"),
                b"Hi There".to_vec(),
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe".to_vec(),
                b"what do ya want for nothing?".to_vec(),
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                hex("aa").repeat(20),
                hex("dd").repeat(50),
                "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            ),
        ] {
            assert_eq!(to_hex(&hmac_sha256(&key, &data)), want);
        }
    }

    /// RFC 4231 §4.7: a key longer than SHA-256's 64-byte block is hashed first.
    ///
    /// Its own case because it is the only branch in `hmac_sha256` that a short
    /// key never reaches, and SSH does not itself use a key that long — so
    /// without this vector the branch would be reached only by a future caller,
    /// with no evidence it was ever right.
    #[test]
    fn a_key_longer_than_the_hash_block_is_hashed_down_first() {
        let key = hex("aa").repeat(131);
        let data = b"Test Using Larger Than Block-Size Key - Hash Key First";
        assert_eq!(
            to_hex(&hmac_sha256(&key, data)),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    /// The packet MAC is the sequence number, big-endian, then the *plaintext*
    /// packet — and the sequence number is what makes an identical packet
    /// replayed later fail to verify.
    #[test]
    fn the_packet_mac_covers_the_sequence_number_ahead_of_the_packet() {
        let key = [0x5au8; 32];
        let packet = b"the plaintext packet";

        let mut expected_input = 7u32.to_be_bytes().to_vec();
        expected_input.extend_from_slice(packet);
        assert_eq!(
            compute_mac(&key, 7, packet),
            hmac_sha256(&key, &expected_input)
        );

        assert_ne!(compute_mac(&key, 7, packet), compute_mac(&key, 8, packet));
    }

    // ---- Constant-time comparison ----

    #[test]
    fn a_constant_time_comparison_still_answers_the_question() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"hello", b"world"));
        // A difference in the last byte is the case a short-circuiting compare
        // would take longest to reach, and is therefore the one worth stating.
        assert!(!constant_time_eq(b"hellp", b"hello"));
        assert!(!constant_time_eq(b"short", b"longer"));
        assert!(!constant_time_eq(b"prefix", b"prefixed"));
    }

    // ---- AES-128 (FIPS-197) ----

    /// FIPS-197 Appendix B and §C.1, the two published AES-128 vectors.
    ///
    /// This is the block cipher on its own, with no counter mode around it. It
    /// is worth stating separately because a cipher that is wrong here is wrong
    /// in a way no CTR-mode roundtrip can show: encrypt-then-decrypt with the
    /// same broken S-box returns the plaintext perfectly.
    #[test]
    fn the_fips_197_aes_128_vectors_encrypt_as_published() {
        for (key, plain, want) in [
            (
                "000102030405060708090a0b0c0d0e0f",
                "00112233445566778899aabbccddeeff",
                "69c4e0d86a7b0430d8cdb78070b4c55a",
            ),
            (
                "2b7e151628aed2a6abf7158809cf4f3c",
                "3243f6a8885a308d313198a2e0370734",
                "3925841d02dc09fbdc118597196a0b32",
            ),
        ] {
            let key: [u8; 16] = hex(key).try_into().expect("16-byte key");
            let plain: [u8; 16] = hex(plain).try_into().expect("16-byte block");
            let round_keys = aes128_key_expand(&key);
            assert_eq!(to_hex(&aes128_encrypt_block(&plain, &round_keys)), want);
        }
    }

    /// FIPS-197 Appendix A.1: the key schedule itself.
    ///
    /// Checking the *last* round key as well as the first two is what catches an
    /// expansion that went wrong partway and then stayed wrong -- a schedule
    /// that is right for round 1 and wrong for round 10 still produces a cipher
    /// that roundtrips against itself perfectly.
    #[test]
    fn the_aes_128_key_schedule_matches_the_published_one() {
        let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f")
            .try_into()
            .expect("16-byte key");
        let round_keys = aes128_key_expand(&key);
        assert_eq!(to_hex(&round_keys[0]), "000102030405060708090a0b0c0d0e0f");
        assert_eq!(to_hex(&round_keys[1]), "d6aa74fdd2af72fadaa678f1d6ab76fe");
        assert_eq!(to_hex(&round_keys[10]), "13111d7fe3944a17f307a78b4d2b30c5");
    }

    // ---- AES-128-CTR (RFC 4344 §4, vectors from RFC 3686) ----

    /// NIST SP 800-38A §F.5.1, CTR-AES128.Encrypt: four blocks in one call.
    ///
    /// A second, independent source for the same counter rule as RFC 3686's
    /// vectors, and the longest one -- the counter has to advance three times
    /// within a single `apply`, so a walk that is right for one block and wrong
    /// for the next shows up here and nowhere else.
    #[test]
    fn the_nist_sp_800_38a_ctr_vector_encrypts_as_published() {
        let key: [u8; 16] = hex("2b7e151628aed2a6abf7158809cf4f3c")
            .try_into()
            .expect("16-byte key");
        let iv: [u8; 16] = hex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff")
            .try_into()
            .expect("16-byte counter block");
        let mut data = hex(concat!(
            "6bc1bee22e409f96e93d7e117393172a",
            "ae2d8a571e03ac9c9eb76fac45af8e51",
            "30c81c46a35ce411e5fbc1191a0a52ef",
            "f69f2445df4f9b17ad2b417be66c3710",
        ));

        Aes128Ctr::new(&key, &iv).apply(&mut data);

        assert_eq!(
            to_hex(&data),
            concat!(
                "874d6191b620e3261bef6864990db6ce",
                "9806f66b7970fdff8617187bb9fffdff",
                "5ae4df3edbd5d35e5b4f09020db03eab",
                "1e031dda2fbe03d1792170a0f3009cee",
            )
        );
    }

    /// RFC 3686 §6 test vectors 1 and 3.
    ///
    /// Vector 3 is 36 bytes: two whole blocks and a 4-byte remainder, so it also
    /// states that a short final block takes the leading bytes of that block's
    /// keystream and no others.
    #[test]
    fn the_rfc_3686_aes_ctr_vectors_encrypt_as_published() {
        for (key, iv, plain, want) in [
            (
                "ae6852f8121067cc4bf7a5765577f39e",
                "00000030000000000000000000000001",
                "53696e676c6520626c6f636b206d7367",
                "e4095d4fb7a7b3792d6175a3261311b8",
            ),
            (
                "7691be035e5020a8ac6e618529f9a0dc",
                "00e0017b27777f3f4a1786f000000001",
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20212223",
                "c1cf48a89f2ffdd9cf4652e9efdb72d74540a42bde6d7836d59a5ceaaef3105325b2072f",
            ),
        ] {
            let key: [u8; 16] = hex(key).try_into().expect("16-byte key");
            let iv: [u8; 16] = hex(iv).try_into().expect("16-byte counter block");
            let mut data = hex(plain);

            let mut cipher = Aes128Ctr::new(&key, &iv);
            cipher.apply(&mut data);
            assert_eq!(to_hex(&data), want);

            // CTR is its own inverse, so a fresh cipher on the same key and IV
            // must take it back. A *fresh* one: reusing the first would be the
            // very keystream reuse this type exists to prevent.
            let mut back = Aes128Ctr::new(&key, &iv);
            back.apply(&mut data);
            assert_eq!(to_hex(&data), plain);
        }
    }

    /// The bug itself.
    ///
    /// `ssh` built a counter from the IV alone at the top of every packet, so
    /// two packets of identical plaintext encrypted to identical ciphertext and
    /// an observer could XOR the key out of the traffic entirely. No roundtrip
    /// test could see that — decrypting with the same wrong counter returns the
    /// plaintext perfectly — so this asserts the property a roundtrip cannot:
    /// that the second packet's keystream is not the first's.
    #[test]
    fn the_keystream_never_repeats_across_packets() {
        let mut cipher = Aes128Ctr::new(&[0x11; 16], &[0x22; 16]);

        let plaintext = *b"an identical packet, sent twice.";
        let mut first = plaintext;
        cipher.apply(&mut first);
        let mut second = plaintext;
        cipher.apply(&mut second);

        assert_ne!(first, second, "two packets shared one stretch of keystream");

        // Stated the way an attacker would exploit it: XOR-ing the two
        // ciphertexts must not cancel the keystream and leave the plaintexts.
        let xored: Vec<u8> = first.iter().zip(second).map(|(a, b)| a ^ b).collect();
        assert_ne!(xored, vec![0u8; plaintext.len()]);
    }

    /// A short packet still spends its whole counter block.
    ///
    /// Otherwise two ends that disagree about whether a 1-byte tail consumed a
    /// block would part company on the *next* packet rather than this one, which
    /// is the hardest kind of divergence to trace back.
    #[test]
    fn a_partial_final_block_still_advances_the_counter_by_one() {
        let mut short_tail = Aes128Ctr::new(&[7; 16], &[0; 16]);
        let mut one_byte = [0u8; 1];
        short_tail.apply(&mut one_byte);
        let mut after_short = [0u8; 16];
        short_tail.apply(&mut after_short);

        let mut whole_block = Aes128Ctr::new(&[7; 16], &[0; 16]);
        let mut sixteen = [0u8; 16];
        whole_block.apply(&mut sixteen);
        let mut after_whole = [0u8; 16];
        whole_block.apply(&mut after_whole);

        assert_eq!(after_short, after_whole);
    }

    /// `try_parse_packet` decrypts the first block to learn the packet length,
    /// may then find the rest has not arrived, and decrypts it again later as
    /// part of the whole packet. If the peek consumed the block, the connection
    /// would desynchronise on its first encrypted packet.
    #[test]
    fn a_peek_does_not_consume_the_block_the_next_apply_needs() {
        let key = [0x33; 16];
        let iv = [0x44; 16];
        let packet = *b"a whole packet spanning three blocks of it";

        let mut sender = Aes128Ctr::new(&key, &iv);
        let mut wire = packet;
        sender.apply(&mut wire);

        let mut receiver = Aes128Ctr::new(&key, &iv);
        let peeked = receiver.peek_block(wire.get(..16).expect("packet is longer than a block"));
        assert_eq!(peeked, packet.get(..16).expect("same"));

        // Peeking twice must also be harmless: a caller that returned
        // `Ok(None)` for want of bytes will come back and peek the same block.
        assert_eq!(
            receiver.peek_block(wire.get(..16).expect("packet is longer than a block")),
            peeked
        );

        let mut decrypted = wire;
        receiver.apply(&mut decrypted);
        assert_eq!(decrypted, packet);
    }

    /// The counter is 128 bits big-endian and carries across every byte.
    ///
    /// Reached in practice only after 2^8k blocks, so the only way it is ever
    /// exercised is here.
    #[test]
    fn the_counter_carries_across_all_sixteen_bytes() {
        let mut counter = [0u8; 16];
        counter[15] = 0xff;
        increment_counter(&mut counter);
        assert_eq!(counter[15], 0x00);
        assert_eq!(counter[14], 0x01);

        let mut all_ones = [0xffu8; 16];
        increment_counter(&mut all_ones);
        assert_eq!(
            all_ones, [0u8; 16],
            "the counter wraps rather than panicking"
        );

        let mut low = [0u8; 16];
        increment_counter(&mut low);
        assert_eq!(low[15], 1);
        assert_eq!(low[..15], [0u8; 15]);
    }

    /// Two directions derived from the same handshake must not share keystream,
    /// and neither must the same key with a different IV. Both are ways the
    /// two-time pad could come back without the counter ever being reset.
    #[test]
    fn a_different_key_or_iv_gives_a_different_keystream() {
        let keystream = |key: [u8; 16], iv: [u8; 16]| {
            let mut c = Aes128Ctr::new(&key, &iv);
            let mut zeros = [0u8; 32];
            c.apply(&mut zeros);
            zeros
        };
        let base = keystream([1; 16], [2; 16]);
        assert_ne!(base, keystream([9; 16], [2; 16]));
        assert_ne!(base, keystream([1; 16], [9; 16]));
    }

    /// `Debug` must not print the key schedule or the counter position: the
    /// first is key material and the second says how much traffic has passed.
    #[test]
    fn the_cipher_does_not_print_its_key_schedule() {
        let cipher = Aes128Ctr::new(&[0xab; 16], &[0xcd; 16]);
        let shown = format!("{cipher:?}");
        assert_eq!(shown, "Aes128Ctr { .. }");
        assert!(!shown.contains("ab"));
        assert!(!shown.contains("cd"));
    }

    // ---- The binary packet protocol (RFC 4253 §6) ----

    /// A client and a server codec keyed from one handshake.
    ///
    /// Returned as a pair on purpose: every test below that matters is about
    /// the two of them agreeing, which is the property neither binary's own
    /// test suite could ever state while each carried its own framing.
    fn a_keyed_pair() -> (PacketCodec, PacketCodec) {
        let secret = encode_mpint(&[0x2au8; 32]);
        let hash = [0x5bu8; 32];
        let session = [0x77u8; 32];
        let mut client = PacketCodec::new();
        let mut server = PacketCodec::new();
        client
            .activate(Role::Client, &secret, &hash, &session)
            .expect("32-byte inputs derive");
        server
            .activate(Role::Server, &secret, &hash, &session)
            .expect("32-byte inputs derive");
        (client, server)
    }

    /// Encode `payload` with zero padding of whatever length the codec asks
    /// for. What the padding *is* is the subject of its own test; everywhere
    /// else it is noise.
    fn encode(codec: &mut PacketCodec, payload: &[u8]) -> Vec<u8> {
        let padding = vec![0u8; codec.padding_len(payload.len())];
        codec.encode(payload, &padding).expect("padding fits")
    }

    /// §6's alignment rule, stated over the range of payload lengths that
    /// crosses two block boundaries.
    #[test]
    fn a_packet_is_block_aligned_with_at_least_four_bytes_of_padding() {
        for &(encrypted, block) in &[(false, 8usize), (true, 16usize)] {
            let codec = if encrypted {
                a_keyed_pair().0
            } else {
                PacketCodec::new()
            };
            for payload_len in 0..40usize {
                let padding = codec.padding_len(payload_len);
                assert!(padding >= 4, "§6 requires at least four bytes of padding");
                assert!(padding <= 255, "padding_length is one byte");
                let total = payload_len + 1 + padding + 4;
                assert_eq!(
                    total % block,
                    0,
                    "a {payload_len}-byte payload is not aligned to {block}"
                );
            }
        }
    }

    /// The unencrypted packet, byte for byte, against §6 written out by hand.
    #[test]
    fn a_plaintext_packet_is_laid_out_as_the_rfc_describes_it() {
        let mut codec = PacketCodec::new();
        let payload = b"hello";
        let padding = vec![0xeeu8; codec.padding_len(payload.len())];
        let pkt = codec.encode(payload, &padding).expect("padding fits");

        let mut expected = Vec::new();
        // packet_length covers padding_length + payload + padding, not itself.
        let packet_length = 1 + payload.len() + padding.len();
        expected.extend_from_slice(&u32::try_from(packet_length).expect("small").to_be_bytes());
        expected.push(u8::try_from(padding.len()).expect("small"));
        expected.extend_from_slice(payload);
        expected.extend_from_slice(&padding);
        assert_eq!(pkt, expected);
        // No MAC before NEWKEYS.
        assert_eq!(pkt.len(), packet_length + 4);
    }

    /// The test that could not previously exist: one end's packets decode at
    /// the other end.
    ///
    /// Both directions, because the six §7.2 letters mean opposite things to
    /// the two ends and a codec that assigned them by hand — as both binaries
    /// did — could get one direction right and the other backwards. That is not
    /// a hypothetical shape of bug in this stack: an exchange hash, two packet
    /// readers and a counter rule have each already been wrong in exactly the
    /// way that only the far end could notice.
    #[test]
    fn what_one_end_encodes_the_other_end_decodes() {
        let (mut client, mut server) = a_keyed_pair();

        for payload in [&b""[..], &b"x"[..], &[0xa5; 3000][..]] {
            let wire = encode(&mut client, payload);
            let (got, consumed) = server
                .decode(&wire)
                .expect("a whole packet")
                .expect("a whole packet");
            assert_eq!(got, payload);
            assert_eq!(consumed, wire.len());

            let wire = encode(&mut server, payload);
            let (got, consumed) = client
                .decode(&wire)
                .expect("a whole packet")
                .expect("a whole packet");
            assert_eq!(got, payload);
            assert_eq!(consumed, wire.len());
        }
    }

    /// Two ends that used the same letters for both directions would still pass
    /// a same-codec roundtrip. This states what that would break: the
    /// directions are keyed differently.
    #[test]
    fn the_two_directions_do_not_share_a_key() {
        let (mut client, mut server) = a_keyed_pair();
        let payload = b"the same payload, both ways";
        assert_ne!(
            encode(&mut client, payload),
            encode(&mut server, payload),
            "the two directions produced identical ciphertext"
        );
    }

    /// A packet arriving in pieces: every prefix returns `Ok(None)` and changes
    /// nothing, so the whole packet still decodes when the last byte lands.
    ///
    /// This is the case the cipher's `peek_block` exists for. A peek that
    /// consumed its keystream would leave the counter ahead of the sender's
    /// from the first partially-arrived packet onwards — and a TCP stream
    /// delivers a partial packet whenever it feels like it.
    #[test]
    fn a_packet_that_arrives_in_pieces_is_decoded_once_it_is_whole() {
        let (mut client, mut server) = a_keyed_pair();
        let payload = b"a packet split across several reads";
        let wire = encode(&mut client, payload);

        for prefix in 0..wire.len() {
            assert_eq!(
                server.decode(wire.get(..prefix).expect("in range")),
                Ok(None),
                "a {prefix}-byte prefix was treated as a whole packet"
            );
        }
        let (got, consumed) = server
            .decode(&wire)
            .expect("a whole packet")
            .expect("a whole packet");
        assert_eq!(got, payload);
        assert_eq!(consumed, wire.len());
    }

    /// Trailing bytes are left alone: `decode` reports what it used, and a
    /// second call on the remainder finds the next packet.
    #[test]
    fn two_packets_in_one_buffer_are_taken_one_at_a_time() {
        let (mut client, mut server) = a_keyed_pair();
        let mut stream = encode(&mut client, b"first");
        stream.extend_from_slice(&encode(&mut client, b"second"));

        let (first, used) = server
            .decode(&stream)
            .expect("a whole packet")
            .expect("a whole packet");
        assert_eq!(first, b"first");
        let (second, _) = server
            .decode(stream.get(used..).expect("in range"))
            .expect("a whole packet")
            .expect("a whole packet");
        assert_eq!(second, b"second");
    }

    /// §6.4 runs the sequence number over the connection, so a packet is
    /// authentic only in its place in the stream.
    ///
    /// Replay is the attack this stops, and dropping a packet is the bug: a
    /// send path that framed a packet without advancing its sequence number is
    /// a fault already recorded against this code, and it fails here.
    ///
    /// The rejection is a length error rather than a MAC one, and that is not
    /// an accident worth papering over: the receiving counter has moved on, so
    /// a replayed packet decrypts to noise and its *length field* is noise
    /// too. It is refused before the MAC is reached. What matters is that no
    /// replayed packet is ever returned as a payload.
    #[test]
    fn a_packet_replayed_out_of_order_is_refused() {
        let (mut client, mut server) = a_keyed_pair();
        let first = encode(&mut client, b"the first packet");
        let _second = encode(&mut client, b"the second packet");

        // Deliver the first, then the first again where the second belongs.
        server.decode(&first).expect("authentic").expect("whole");
        assert!(server.decode(&first).is_err(), "a replay was accepted");
    }

    /// A single flipped bit anywhere in the packet must be caught.
    ///
    /// Where it is flipped decides *which* refusal, and the length field is the
    /// interesting case. §6 puts the MAC over the whole plaintext packet, so
    /// the length has to be trusted before there is anything to check it
    /// against — it is encrypted, but it is not authenticated ahead of the
    /// packet it introduces. A flip there therefore does not fail a MAC: it
    /// either falls outside §6's range, or leaves the receiver waiting for a
    /// packet of the wrong size, which fails the MAC once those bytes arrive.
    /// What must hold in every case is that no altered packet is ever returned
    /// as a payload. Stated as three separate outcomes rather than one
    /// "rejected somehow", because that weaker claim is also satisfied by a
    /// decoder that rejects everything.
    #[test]
    fn a_modified_packet_is_refused() {
        let payload = b"a packet an attacker would like to alter";

        // Top byte: the length becomes enormous and is out of range at once.
        let (mut client, mut server) = a_keyed_pair();
        let mut wire = encode(&mut client, payload);
        *wire.get_mut(0).expect("in range") ^= 0x01;
        assert!(matches!(
            server.decode(&wire),
            Err(WireError::PacketLength { .. })
        ));

        // Bottom byte: the length is still plausible, so the decoder asks for
        // more bytes — and rejects the packet once it has them.
        let (mut client, mut server) = a_keyed_pair();
        let mut wire = encode(&mut client, payload);
        *wire.get_mut(3).expect("in range") ^= 0x01;
        assert_eq!(server.decode(&wire), Ok(None));
        wire.resize(wire.len() + 16, 0);
        assert_eq!(server.decode(&wire), Err(WireError::MacMismatch));

        // Anywhere after the first block: straight to the MAC.
        for index in [16usize, 20, 44] {
            let (mut client, mut server) = a_keyed_pair();
            let mut wire = encode(&mut client, payload);
            *wire.get_mut(index).expect("in range") ^= 0x01;
            assert_eq!(
                server.decode(&wire),
                Err(WireError::MacMismatch),
                "a flipped bit at {index} was accepted"
            );
        }
    }

    /// The fail-open bug, stated.
    ///
    /// The server's copy of this check read `if mac_data.len() >= mac_len &&
    /// !constant_time_eq(...)`, so a peer that sent a *short* MAC skipped
    /// verification entirely and had its packet accepted unauthenticated —
    /// before it had authenticated anything at all. It is unreachable through
    /// that binary's own buffering, which waits for the full length, which is
    /// exactly why nothing found it: the guard was wrong, and the caller was
    /// what made it harmless.
    #[test]
    fn a_short_mac_is_rejected_rather_than_skipped() {
        let (mut client, mut server) = a_keyed_pair();
        let wire = encode(&mut client, b"unauthenticated");

        // Cut the last MAC byte off, and claim the packet is that much shorter
        // by handing over only those bytes. The MAC is now short, not absent.
        let truncated = wire.get(..wire.len() - 1).expect("in range");
        assert_eq!(server.decode(truncated), Ok(None));
    }

    /// §6's floor and ceiling, both checked.
    ///
    /// The floor is the one the server did not have. `packet_length` 0 through
    /// 4 leaves no room for the `padding_length` byte the format puts at offset
    /// 4, and the server's parser read it anyway.
    #[test]
    fn a_length_outside_the_permitted_range_is_refused() {
        for len in [0u32, 1, 4, 35001, 0xffff_ffff] {
            let mut codec = PacketCodec::new();
            let mut wire = len.to_be_bytes().to_vec();
            wire.resize(64, 0);
            assert_eq!(
                codec.decode(&wire),
                Err(WireError::PacketLength { len: len as usize }),
                "a packet length of {len} was not refused"
            );
        }
        // The floor itself is legal: five bytes is a padding-length byte and
        // the four-byte minimum padding, i.e. an empty payload.
        let mut codec = PacketCodec::new();
        let mut wire = 5u32.to_be_bytes().to_vec();
        wire.push(4);
        wire.resize(9, 0);
        assert_eq!(codec.decode(&wire), Ok(Some((Vec::new(), 9))));
    }

    /// A `padding_length` that claims more bytes than the packet holds.
    ///
    /// The subtraction it feeds is `packet_length - 1 - padding_length` in
    /// `usize`, where "negative" is a number near 2^64 and the slice that
    /// follows would be an out-of-bounds read.
    #[test]
    fn padding_longer_than_the_packet_is_refused() {
        let mut codec = PacketCodec::new();
        let mut wire = 12u32.to_be_bytes().to_vec();
        wire.push(200); // padding_length, against a packet_length of 12
        wire.resize(16, 0);
        assert_eq!(
            codec.decode(&wire),
            Err(WireError::PaddingLength {
                packet_length: 12,
                padding_length: 200
            })
        );
    }

    /// `encode` will not silently fix a padding block of the wrong size.
    ///
    /// Both silent fixes are wrong. Extending a short block pads the packet
    /// with the zeros §6 asks us to stop sending; accepting a long one means
    /// the caller and the codec disagree about the alignment the length field
    /// is about to assert.
    #[test]
    fn padding_of_the_wrong_length_is_refused_rather_than_adjusted() {
        let mut codec = PacketCodec::new();
        let wanted = codec.padding_len(3);
        assert_eq!(
            codec.encode(b"abc", &vec![0; wanted - 1]),
            Err(WireError::PaddingSize {
                wanted,
                given: wanted - 1
            })
        );
        assert_eq!(
            codec.encode(b"abc", &vec![0; wanted + 1]),
            Err(WireError::PaddingSize {
                wanted,
                given: wanted + 1
            })
        );
        // And a refusal must not have moved the sequence number, or the next
        // packet the caller does send would be numbered past the peer's.
        assert_eq!(
            codec.encode(b"abc", &vec![0; wanted]).map(|p| p.len()),
            Ok(4 + 1 + 3 + wanted)
        );
    }

    /// The padding bytes reach the wire unaltered, which is what makes the
    /// binaries' switch to CSPRNG padding observable rather than decorative.
    #[test]
    fn the_padding_the_caller_supplies_is_what_goes_on_the_wire() {
        let mut codec = PacketCodec::new();
        let payload = b"payload";
        let padding: Vec<u8> = (0..codec.padding_len(payload.len()))
            .map(|i| 0xa0 ^ u8::try_from(i).unwrap_or(0))
            .collect();
        let pkt = codec.encode(payload, &padding).expect("padding fits");
        assert_eq!(
            pkt.get(pkt.len() - padding.len()..).expect("in range"),
            padding.as_slice()
        );
    }

    /// `NEWKEYS` changes the keys, not the sequence numbers.
    ///
    /// §6.4 runs them for the life of the *connection*. Resetting them when a
    /// rekey installs new keys would break the MAC on the very next packet —
    /// and this codec is what a future rekey will call, so the property is
    /// stated before there is a rekey to get it wrong.
    #[test]
    fn activating_keys_does_not_rewind_the_sequence_numbers() {
        let mut client = PacketCodec::new();
        let mut server = PacketCodec::new();
        // Three packets each way in the clear, as a real handshake sends.
        for _ in 0..3 {
            let wire = encode(&mut client, b"KEXINIT");
            server.decode(&wire).expect("plaintext").expect("whole");
            let wire = encode(&mut server, b"KEXINIT");
            client.decode(&wire).expect("plaintext").expect("whole");
        }

        let secret = encode_mpint(&[9u8; 32]);
        client
            .activate(Role::Client, &secret, &[1; 32], &[2; 32])
            .expect("derives");
        server
            .activate(Role::Server, &secret, &[1; 32], &[2; 32])
            .expect("derives");

        // If either end had restarted at zero, this MAC would not verify.
        let wire = encode(&mut client, b"the first encrypted packet");
        assert_eq!(
            server.decode(&wire).expect("authentic").expect("whole").0,
            b"the first encrypted packet"
        );
        assert_eq!(server.last_inbound_seq(), 3);
    }

    /// Before `NEWKEYS` the transport is plaintext, unauthenticated and aligned
    /// to 8; after it, encrypted, authenticated and aligned to 16.
    #[test]
    fn activation_switches_the_transport_over_completely() {
        let mut codec = PacketCodec::new();
        assert!(!codec.is_encrypted());
        assert_eq!(codec.inbound_block_size(), 8);
        let plain = encode(&mut codec, b"KEXINIT");
        assert_eq!(plain.len() % 8, 0);

        codec
            .activate(Role::Client, &encode_mpint(&[3u8; 32]), &[4; 32], &[5; 32])
            .expect("derives");
        assert!(codec.is_encrypted());
        assert_eq!(codec.inbound_block_size(), 16);
        let sealed = encode(&mut codec, b"KEXINIT");
        // Body aligned to 16, plus a 32-byte HMAC-SHA256 tag that the
        // plaintext packet did not carry at all.
        assert_eq!((sealed.len() - 32) % 16, 0);
        assert_eq!(sealed.len() - 32, plain.len());
        assert_eq!(sealed.len(), 48);
    }

    /// `last_inbound_seq` names the packet just returned, not the next one.
    ///
    /// §11.4 has `SSH_MSG_UNIMPLEMENTED` quote the sequence number it is
    /// rejecting, and the number wanted there is the one the packet came in
    /// under — which the counter has already moved past by the time the caller
    /// has the packet to reject.
    #[test]
    fn the_last_inbound_sequence_number_is_the_packet_just_returned() {
        let (mut client, mut server) = a_keyed_pair();
        // Before anything arrives, "the last one" is the wrap-around, which is
        // the honest answer and not a panic.
        assert_eq!(server.last_inbound_seq(), u32::MAX);
        for expected in 0..3u32 {
            let wire = encode(&mut client, b"packet");
            server.decode(&wire).expect("authentic").expect("whole");
            assert_eq!(server.last_inbound_seq(), expected);
        }
    }

    /// `Debug` must not print keys, counters or traffic volume.
    #[test]
    fn the_codec_does_not_print_its_keys() {
        let (client, _) = a_keyed_pair();
        assert_eq!(format!("{client:?}"), "PacketCodec { .. }");
    }
}
