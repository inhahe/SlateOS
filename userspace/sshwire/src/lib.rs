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
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

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
// The byte stream underneath the protocol
// ============================================================================

/// What the byte stream underneath SSH can fail to do.
///
/// Deliberately not a variant of [`WireError`]. `WireError` means "the peer's
/// bytes do not describe what they claim to" — a protocol fault. These mean the
/// bytes did not arrive at all, which is a different thing to report and, for
/// [`Closed`](Self::Closed), not a fault at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// A write failed, or accepted nothing while bytes were still owed.
    Send,
    /// A read failed.
    Recv,
    /// The peer closed the connection.
    ///
    /// A *variant*, because both ends need to tell an orderly hang-up from a
    /// failure and one of them was doing it by searching an error message for
    /// the substring `"connection closed"` — in two places, in the server's
    /// session loop, deciding whether to return `Ok` or propagate. That works
    /// until someone rewords a message, and nothing would have failed when they
    /// did: the server would simply have started reporting normal client
    /// disconnections as protocol errors.
    Closed,
}

impl core::fmt::Display for TransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Send => write!(f, "send failed"),
            Self::Recv => write!(f, "receive failed"),
            Self::Closed => write!(f, "connection closed"),
        }
    }
}

/// A bidirectional byte stream: the whole of what the SSH transport layer needs
/// from the thing underneath it.
///
/// SSH is defined over "a reliable byte-oriented stream" (RFC 4253 §1) and
/// never over TCP specifically, but both binaries were written against a raw
/// kernel handle, which is why neither can be exercised without a kernel. That
/// is not a small inconvenience: it is the reason the one test this stack most
/// needs — run the client against the server and see whether they agree — has
/// never existed, and every wire-layer bug found so far was found by reading.
///
/// Implementations live in the binaries, next to the syscalls they wrap, so
/// this crate stays free of any notion of a kernel.
pub trait Transport {
    /// Write some of `data`, returning how many bytes were accepted.
    ///
    /// # Errors
    ///
    /// [`TransportError::Send`] if the write failed.
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError>;

    /// Read into `buf`, returning the prefix actually filled.
    ///
    /// An empty slice is the peer's orderly close. Handing back the slice
    /// rather than a count is deliberate: the count is turned into a range in
    /// exactly one place, the implementation, where a length that does not fit
    /// the buffer is rejected rather than travelling into a caller's `buf[..n]`
    /// to panic there. The server's copy of this did index, under a suppressed
    /// lint; the client's did not.
    ///
    /// # Errors
    ///
    /// [`TransportError::Recv`] if the read failed.
    fn recv<'b>(&mut self, buf: &'b mut [u8]) -> Result<&'b [u8], TransportError>;

    /// Whether a [`recv`](Self::recv) would return without blocking.
    ///
    /// The server's session loop needs this: it interleaves "has the client
    /// sent anything" with "has the shell printed anything", and an
    /// unconditional read would stall the second until the user typed.
    fn readable(&self) -> bool;

    /// Release the connection. Idempotent; failures are not reportable, since
    /// the stream is unusable either way.
    fn close(&mut self);

    /// Write all of `data`.
    ///
    /// # Errors
    ///
    /// [`TransportError::Send`] if any write fails or stalls at zero bytes.
    fn send_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        // Walking a shrinking `rest` rather than an offset keeps "how much is
        // left" and "where that starts" from being two facts that can disagree.
        let mut rest = data;
        while !rest.is_empty() {
            let sent = self.send(rest)?;
            if sent == 0 {
                return Err(TransportError::Send);
            }
            rest = rest.get(sent..).ok_or(TransportError::Send)?;
        }
        Ok(())
    }
}

/// One end of an in-memory byte stream, for driving the protocol without a
/// kernel.
///
/// This exists because of a specific gap: neither binary's syscalls work on a
/// host build (they return `-ENOSYS`), so no `cargo test` in this tree has ever
/// been able to open a socket, so the one test the SSH stack most needs — run
/// the client against the server and see whether they agree — could not be
/// written. Every wire-layer disagreement found so far was found by reading two
/// files side by side. A transport that is just two queues removes the excuse.
///
/// It **blocks**, like the socket it stands in for: a read with nothing to read
/// waits until the peer writes or hangs up. A non-blocking stand-in would have
/// forced both ends to be restructured around polling in order to be testable,
/// which is the tail wagging the dog — and worse, it would have meant the code
/// under test was not the code that ships.
///
/// It is deliberately not `cfg(test)`: the point is for it to be reachable from
/// the *binaries'* tests and from a separate interop crate, neither of which
/// can see a `cfg(test)` item in this one.
#[derive(Debug)]
pub struct MemoryTransport {
    /// Bytes written by the peer, waiting to be read here.
    inbound: Arc<Duct>,
    /// Bytes written here, waiting to be read by the peer.
    outbound: Arc<Duct>,
}

/// One direction of a [`MemoryTransport`] pair: a queue and the wait for it.
#[derive(Debug, Default)]
struct Duct {
    pipe: Mutex<Pipe>,
    /// Signalled whenever `pipe` gains bytes or loses its writer.
    arrived: Condvar,
}

/// The queued bytes of one direction.
#[derive(Debug, Default)]
struct Pipe {
    bytes: VecDeque<u8>,
    /// Set when the writing end goes away, so a reader sees a close rather than
    /// waiting for bytes that will never come.
    writer_gone: bool,
}

/// A connected pair of in-memory transports: what one writes, the other reads.
#[must_use]
pub fn memory_pair() -> (MemoryTransport, MemoryTransport) {
    let a_to_b = Arc::new(Duct::default());
    let b_to_a = Arc::new(Duct::default());
    (
        MemoryTransport {
            inbound: Arc::clone(&b_to_a),
            outbound: Arc::clone(&a_to_b),
        },
        MemoryTransport {
            inbound: a_to_b,
            outbound: b_to_a,
        },
    )
}

impl Transport for MemoryTransport {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        let mut pipe = self
            .outbound
            .pipe
            .lock()
            .map_err(|_| TransportError::Send)?;
        if pipe.writer_gone {
            return Err(TransportError::Send);
        }
        pipe.bytes.extend(data.iter().copied());
        drop(pipe);
        self.outbound.arrived.notify_all();
        Ok(data.len())
    }

    fn recv<'b>(&mut self, buf: &'b mut [u8]) -> Result<&'b [u8], TransportError> {
        let mut pipe = self.inbound.pipe.lock().map_err(|_| TransportError::Recv)?;
        while pipe.bytes.is_empty() && !pipe.writer_gone {
            pipe = self
                .inbound
                .arrived
                .wait(pipe)
                .map_err(|_| TransportError::Recv)?;
        }
        let mut taken = 0usize;
        for slot in buf.iter_mut() {
            let Some(byte) = pipe.bytes.pop_front() else {
                break;
            };
            *slot = byte;
            taken = taken.saturating_add(1);
        }
        // `taken == 0` here means the loop above exited on `writer_gone`, which
        // is exactly the empty slice the trait defines as the peer's close.
        buf.get(..taken).ok_or(TransportError::Recv)
    }

    fn readable(&self) -> bool {
        self.inbound
            .pipe
            .lock()
            .map_or(true, |pipe| !pipe.bytes.is_empty() || pipe.writer_gone)
    }

    fn close(&mut self) {
        if let Ok(mut pipe) = self.outbound.pipe.lock() {
            pipe.writer_gone = true;
        }
        // Wake the peer even if the lock was poisoned: a reader blocked in
        // `recv` has no other way out, and a test that deadlocks reports far
        // less than one that fails.
        self.outbound.arrived.notify_all();
    }
}

impl Drop for MemoryTransport {
    /// Hanging up on drop is what makes a peer's `recv` return instead of
    /// blocking for ever when the other end's thread finishes.
    fn drop(&mut self) {
        self.close();
    }
}

// ============================================================================
// The randomness underneath the protocol
// ============================================================================

/// Where a session's unpredictable bytes come from.
///
/// This is the second half of the argument [`Transport`] makes. `Transport`
/// exists because both binaries were written against a raw kernel handle, so
/// neither could be exercised without a kernel; this exists because both were
/// written against `randrange::fill_secret` directly, so neither can be
/// exercised without kernel *randomness* — and on the Windows host the test
/// suite runs on, `randrange` refuses on purpose (`open-questions.md`, "The
/// test machine cannot produce random numbers, on purpose…"). A handshake
/// needs a Diffie-Hellman exponent. With no exponent there is no handshake,
/// and the one test this stack most needs — run the client against the server
/// and see whether they agree — cannot be written at all.
///
/// Making it a parameter buys two things beyond that. What is under test stops
/// depending on which platform it was compiled for, which is the same mistake
/// `randrange` made one layer down. And a test can supply a *deterministic*
/// source, which turns "both ends derived the same session identifier" into
/// "both ends derived exactly this session identifier" — an assertion that
/// fails when either end drifts, rather than only when they drift apart.
///
/// It is deliberately a plain `fn` pointer rather than a trait object: a
/// source with no state cannot accidentally acquire any, and both binaries can
/// store one in a `Copy` field without a lifetime or an allocation.
///
/// # What this is not
///
/// It is **not** a way for a caller to choose weak randomness. Both binaries
/// default to [`KERNEL_SECRETS`] and neither exposes a way to change it from
/// the command line, a config file, or the network; the only writers are their
/// own tests, in-crate. A source that could be selected by configuration would
/// be a downgrade attack with a spelling.
pub type SecretSource = fn(&mut [u8]) -> Result<(), randrange::EntropyError>;

/// The real source: the kernel CSPRNG, which fails rather than substituting
/// anything when it cannot answer.
///
/// This is what both binaries use unless a test says otherwise.
pub const KERNEL_SECRETS: SecretSource = randrange::fill_secret;

/// How much one [`StreamBuffer::fill_once`] will take at most.
const STREAM_READ_SIZE: usize = 8192;

/// How far `pos` may run ahead before the consumed prefix is reclaimed.
const STREAM_COMPACT_THRESHOLD: usize = 4096;

/// Accumulates stream bytes until a whole packet is present.
///
/// A stream has no packet boundaries, so something has to hold the partial one.
/// Both binaries held it in a private `StreamBuffer` with the same two fields
/// and the same four methods, and — as with everything else in this crate — the
/// two were not equal: the server's `fill_once` ended `&tmp[..n]`, indexing a
/// length the kernel reported, under the crate-wide panic-lint suppression.
#[derive(Debug, Default)]
pub struct StreamBuffer {
    data: Vec<u8>,
    pos: usize,
}

impl StreamBuffer {
    /// An empty buffer, sized for one read.
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(STREAM_READ_SIZE),
            pos: 0,
        }
    }

    /// The unconsumed bytes.
    ///
    /// The whole of what the packet layer needs: [`PacketCodec::decode`]
    /// decides for itself whether a whole packet is present, so there is no
    /// `available() >= n` guard for a caller to get wrong.
    #[must_use]
    pub fn unread(&self) -> &[u8] {
        self.data.get(self.pos..).unwrap_or_default()
    }

    /// Read once from `transport` and append whatever arrived.
    ///
    /// One read, not a loop: the caller decides what to do when the buffer is
    /// still short, and a session loop's answer is "go and check whether the
    /// shell has printed anything" rather than "block until the client types".
    ///
    /// # Errors
    ///
    /// [`TransportError::Closed`] when the peer has hung up — every caller is
    /// in the middle of wanting more bytes, so a zero-length read is an error
    /// here even though it is not a fault. Otherwise as [`Transport::recv`].
    pub fn fill_once(&mut self, transport: &mut dyn Transport) -> Result<(), TransportError> {
        // Reclaim the consumed prefix before growing. Doing this only past a
        // threshold keeps a long session from memmoving the tail on every
        // packet, while still bounding the buffer for one that runs for hours.
        if self.pos > STREAM_COMPACT_THRESHOLD {
            self.data.drain(..self.pos);
            self.pos = 0;
        }
        let mut tmp = [0u8; STREAM_READ_SIZE];
        let received = transport.recv(&mut tmp)?;
        if received.is_empty() {
            return Err(TransportError::Closed);
        }
        self.data.extend_from_slice(received);
        Ok(())
    }

    /// Drop the first `n` unread bytes.
    ///
    /// `n` is always a length [`PacketCodec::decode`] has just reported for a
    /// packet it took out of [`unread`](Self::unread), never a number this side
    /// chose. `saturating_add` past the end would leave the buffer empty rather
    /// than panicking, but the `min` keeps `pos` a position in `data` rather
    /// than a number that merely behaves like one.
    pub fn advance(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.data.len());
    }
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
        // `as_chunks::<4>` rather than `chunks_exact(4)` because it hands back
        // `[u8; 4]` rather than a slice that happens to be four long. That is
        // not a style preference: the column feeds the next iteration as a
        // `word`, and with a slice that assignment needed a fallible
        // `<[u8; 4]>::try_from(...)` whose failure arm had to invent a value.
        // The arm was unreachable, but "unreachable" is a claim about the
        // chunk size that the reader has to check; with an array the type
        // system makes it, and a round key can no longer be silently zeroed by
        // a conversion that was never supposed to fail.
        let (prev_cols, _) = prev.as_chunks::<4>();
        let (next_cols, _) = next.as_chunks_mut::<4>();
        for (prev_col, next_col) in prev_cols.iter().zip(next_cols) {
            for ((dst, &p), &w) in next_col.iter_mut().zip(prev_col).zip(word.iter()) {
                *dst = p ^ w;
            }
            // The column just written feeds the next one.
            word = *next_col;
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
    // `as_chunks_mut::<4>` hands out the four columns directly, so the
    // `col * 4` base and its four `off + k` offsets -- five chances to write
    // one column while reading another -- are gone. A 16-byte state is four
    // whole columns, so the remainder is always empty.
    //
    // The column arrives as `[u8; 4]` rather than a slice, which is what makes
    // the destructuring below irrefutable: `chunks_exact_mut` needed a
    // `let ... else { continue }` whose `continue` arm silently left a column
    // un-mixed if it were ever reached. It could not be, but only the reader
    // knew that; here the compiler does.
    let (columns, _) = state.as_chunks_mut::<4>();
    for col in columns {
        let [a0, a1, a2, a3] = *col;
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
// Message type codes (RFC 4253 §12, RFC 4252 §6, RFC 4254 §9)
// ============================================================================

/// The number that begins every payload, naming what the payload is.
///
/// These are the most obviously two-sided values in the protocol: the sender
/// writes one and the receiver switches on it, so a table that differs by a
/// single entry produces a client and a server that cannot talk — while each
/// one's tests, which send and receive using the same table, pass.
///
/// This table was written twice, once in `ssh` and once in `sshd`, and the two
/// copies did agree; that is luck rather than a property of the arrangement,
/// and it is the same luck the exchange hash did not have. See
/// `known-issues.md`
/// `TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`.
///
/// The table is the union of what the two binaries used, so a constant here may
/// have no caller in one of them — that is the point. A number one end has not
/// needed yet still has exactly one correct value, and the next end to need it
/// should find it rather than transcribe it from the RFC a second time.
///
/// The `KEX_DH` names keep this project's spelling; RFC 4253 §8 writes them
/// `SSH_MSG_KEXDH_INIT` and `SSH_MSG_KEXDH_REPLY`.
pub mod msg {
    // RFC 4253 §12 — transport layer, generic.
    pub const SSH_MSG_DISCONNECT: u8 = 1;
    pub const SSH_MSG_IGNORE: u8 = 2;
    pub const SSH_MSG_UNIMPLEMENTED: u8 = 3;
    pub const SSH_MSG_DEBUG: u8 = 4;
    pub const SSH_MSG_SERVICE_REQUEST: u8 = 5;
    pub const SSH_MSG_SERVICE_ACCEPT: u8 = 6;

    // RFC 4253 §12 — algorithm negotiation.
    pub const SSH_MSG_KEXINIT: u8 = 20;
    pub const SSH_MSG_NEWKEYS: u8 = 21;

    // RFC 4253 §8 — Diffie-Hellman key exchange. 30..=49 are reserved for the
    // *negotiated* method, so these two numbers mean something else entirely
    // under a different kex algorithm.
    pub const SSH_MSG_KEX_DH_INIT: u8 = 30;
    pub const SSH_MSG_KEX_DH_REPLY: u8 = 31;

    // RFC 4252 §6 — user authentication, generic.
    pub const SSH_MSG_USERAUTH_REQUEST: u8 = 50;
    pub const SSH_MSG_USERAUTH_FAILURE: u8 = 51;
    pub const SSH_MSG_USERAUTH_SUCCESS: u8 = 52;
    pub const SSH_MSG_USERAUTH_BANNER: u8 = 53;

    /// RFC 4252 §7 — method-specific, and 60..=79 are reserved for whichever
    /// method is in progress, so this number is `publickey`'s only while a
    /// `publickey` request is outstanding.
    pub const SSH_MSG_USERAUTH_PK_OK: u8 = 60;

    // RFC 4254 §9 — connection layer, global requests.
    pub const SSH_MSG_GLOBAL_REQUEST: u8 = 80;
    pub const SSH_MSG_REQUEST_SUCCESS: u8 = 81;
    pub const SSH_MSG_REQUEST_FAILURE: u8 = 82;

    // RFC 4254 §9 — connection layer, channels.
    pub const SSH_MSG_CHANNEL_OPEN: u8 = 90;
    pub const SSH_MSG_CHANNEL_OPEN_CONFIRMATION: u8 = 91;
    pub const SSH_MSG_CHANNEL_OPEN_FAILURE: u8 = 92;
    pub const SSH_MSG_CHANNEL_WINDOW_ADJUST: u8 = 93;
    pub const SSH_MSG_CHANNEL_DATA: u8 = 94;
    pub const SSH_MSG_CHANNEL_EXTENDED_DATA: u8 = 95;
    pub const SSH_MSG_CHANNEL_EOF: u8 = 96;
    pub const SSH_MSG_CHANNEL_CLOSE: u8 = 97;
    pub const SSH_MSG_CHANNEL_REQUEST: u8 = 98;
    pub const SSH_MSG_CHANNEL_SUCCESS: u8 = 99;
    pub const SSH_MSG_CHANNEL_FAILURE: u8 = 100;
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

// ============================================================================
// Minimal big-integer arithmetic for Diffie-Hellman (RFC 3526 group 14)
//
// Enough of a bignum to run the group-14 key exchange, and no more. The
// representation is little-endian 32-bit limbs: little-endian because every
// algorithm here -- addition, multiplication, shifting, division -- carries
// from the least significant end upward, so storing that end first makes a
// digit's place its own index instead of `len - 1 - i`; 32-bit limbs because
// the products fit exactly in a `u64`, which is the widest exact integer we
// have.
//
// It is here, rather than in either binary, for the reason everything else in
// this crate is: both ends compute over the same group and must agree about
// what `g^x mod p` is, and a private copy of one half of that is a copy that
// can drift. It did. `ssh` was rewritten from big-endian bytes to limbs, and
// from bit-at-a-time long division to algorithm D, when its key exchange was
// found to take over eighty seconds of CPU; `sshd` kept the original, because
// there was no shared place to put the fix. A server that spends eighty
// seconds of CPU per connection *before* the client has authenticated is not
// a slow server, it is a denial of service anyone can trigger.
// ============================================================================

/// Unsigned big integer, sufficient for Diffie-Hellman over a fixed group.
///
/// Not a general-purpose bignum: there is no signed arithmetic, no GCD, no
/// primality test, and [`sub`](Self::sub) assumes its result is non-negative.
/// What it does have is what RFC 4253 section 8 needs, with the bounds on every
/// piece of unchecked arithmetic written out where that arithmetic is.
#[derive(Clone, Debug)]
pub struct BigUint {
    /// Digits stored little-endian (least significant limb first), with no
    /// trailing zero limbs. Zero is the empty vector.
    limbs: Vec<u32>,
}

impl BigUint {
    /// The one place a limb vector becomes a `BigUint`.
    ///
    /// `mul`, `div_rem`, `shl1` and `sub` each used to end with their own copy
    /// of this -- a `while bytes.len() > 1 && bytes[0] == 0 { bytes.remove(0) }`
    /// loop followed by a `== [0]` special case. Four copies of an invariant is
    /// four chances for one to drift out of step with the others, and each copy
    /// was quadratic besides, since `Vec::remove(0)` shifts the whole buffer
    /// per zero digit. Routing every producer through here makes "no leading
    /// zeros, and zero is the empty vector" a property of construction rather
    /// than a convention every arithmetic routine has to remember to restore.
    /// In little-endian order the trimming is `pop`, so it is also linear.
    fn normalized(mut limbs: Vec<u32>) -> Self {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Self { limbs }
    }

    pub fn zero() -> Self {
        Self { limbs: Vec::new() }
    }

    pub fn one() -> Self {
        Self { limbs: vec![1] }
    }

    /// Read a big-endian byte string.
    pub fn from_bytes_be(data: &[u8]) -> Self {
        // `rchunks` groups from the least significant end, so a byte length
        // that is not a multiple of four leaves the short group at the *top*,
        // where a partial limb is exactly what is wanted. Grouping forwards
        // would put it at the bottom, where it would misalign every limb.
        let limbs = data
            .rchunks(4)
            .map(|chunk| chunk.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b)))
            .collect();
        Self::normalized(limbs)
    }

    /// Write a big-endian byte string, with no leading zero byte. Zero is a
    /// single zero byte rather than nothing, since callers hand the result to
    /// `encode_mpint`, which is what decides how zero goes on the wire.
    pub fn to_bytes_be(&self) -> Vec<u8> {
        let Some((&top, rest)) = self.limbs.split_last() else {
            return vec![0];
        };
        // Normalisation guarantees `top != 0`, so skipping its zero bytes
        // cannot consume the whole number.
        let mut out: Vec<u8> = top
            .to_be_bytes()
            .into_iter()
            .skip_while(|&b| b == 0)
            .collect();
        for &limb in rest.iter().rev() {
            out.extend_from_slice(&limb.to_be_bytes());
        }
        out
    }

    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Return the number of bits.
    pub fn bit_length(&self) -> usize {
        let Some(&top) = self.limbs.last() else {
            return 0;
        };
        let top_bits = 32usize.saturating_sub(top.leading_zeros() as usize);
        self.limbs
            .len()
            .saturating_sub(1)
            .saturating_mul(32)
            .saturating_add(top_bits)
    }

    /// Test bit at position `pos` (0 = least significant).
    ///
    /// Little-endian storage makes this a plain lookup. The big-endian version
    /// had to reverse the index with two checked subtractions, whose only job
    /// was to undo the storage order.
    pub fn bit(&self, pos: usize) -> bool {
        self.limbs
            .get(pos / 32)
            .is_some_and(|&limb| (limb >> (pos % 32)) & 1 == 1)
    }

    /// Compare magnitudes.
    pub fn cmp_unsigned(&self, other: &BigUint) -> std::cmp::Ordering {
        // Normalisation means the limb count *is* the comparison whenever the
        // counts differ; when they agree, the most significant limb that
        // differs decides, which is what comparing the reversed sequences does.
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }

    /// Modular exponentiation: self^exp mod modulus.
    /// Uses the square-and-multiply algorithm.
    pub fn mod_pow(&self, exp: &BigUint, modulus: &BigUint) -> BigUint {
        if modulus.is_zero() {
            return BigUint::zero();
        }
        let mut result = BigUint::one();
        let mut base = self.mod_reduce(modulus);
        let bits = exp.bit_length();
        for i in 0..bits {
            if exp.bit(i) {
                result = result.mod_mul(&base, modulus);
            }
            base = base.mod_mul(&base, modulus);
        }
        result
    }

    /// self * other mod modulus (schoolbook multiplication + reduction).
    pub fn mod_mul(&self, other: &BigUint, modulus: &BigUint) -> BigUint {
        let product = self.mul(other);
        product.mod_reduce(modulus)
    }

    /// self mod modulus.
    pub fn mod_reduce(&self, modulus: &BigUint) -> BigUint {
        if modulus.is_zero() {
            return BigUint::zero();
        }
        self.div_rem(modulus).1
    }

    /// Full multiplication (schoolbook, O(n*m) limb products).
    //
    // The arithmetic in the inner loop is plain rather than checked, and the
    // bound is a proof rather than a hope. With `a`, `b` and `slot` all below
    // 2^32 and `carry` below 2^32, the widest the expression can reach is
    // (2^32-1)^2 + (2^32-1) + (2^32-1) = 2^64 - 1, which is exactly `u64::MAX`
    // -- the classic reason schoolbook multiplication picks a limb half the
    // width of its accumulator. `carry` stays below 2^32 because it is that
    // value shifted right by 32. Writing these as `checked_mul(..).unwrap_or`
    // would replace a provably exact result with a silently wrong fallback on
    // a branch that cannot be taken, in the innermost loop of the key
    // exchange.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "bounds proved in the comment above; see there"
    )]
    pub fn mul(&self, other: &BigUint) -> BigUint {
        if self.is_zero() || other.is_zero() {
            return BigUint::zero();
        }
        let mut acc = vec![0u32; self.limbs.len().saturating_add(other.limbs.len())];

        for (i, &av) in self.limbs.iter().enumerate() {
            let Some(window) = acc.get_mut(i..) else {
                break;
            };
            let mut carry = 0u64;
            for (slot, &bv) in window.iter_mut().zip(&other.limbs) {
                let prod = u64::from(av) * u64::from(bv) + u64::from(*slot) + carry;
                *slot = prod as u32;
                carry = prod >> 32;
            }
            // The carry-out lands in the limb one past the ones just written.
            // That limb has never been written before -- iteration `i` reaches
            // at most `i + other.len()` -- so this is an assignment rather than
            // an addition, and the accumulator never needs a second
            // carry-propagation pass to restore "every limb is a limb".
            if let Some(slot) = window.get_mut(other.limbs.len()) {
                *slot = carry as u32;
            }
        }

        BigUint::normalized(acc)
    }

    /// Shift a limb sequence left by `bits` (0..32), appending the limb that
    /// carries out. Used by `div_rem`'s normalisation step and by `shl1`.
    //
    // `bits < 32` is enforced by the callers (it comes from `leading_zeros` of
    // a non-zero `u32`, or is the literal 1), and `u64::from(u32) << 31` is
    // under 2^63, so neither the shift nor the or can overflow.
    // (No suppression here: every operation in this function is a shift or an
    // `|`, and `arithmetic_side_effects` does not cover those. The bounds above
    // still have to hold -- a shift amount of 32 or more panics in debug -- so
    // the proof stays even though no lint is asking for it.)
    fn shifted_left(limbs: &[u32], bits: u32) -> Vec<u32> {
        let mut out = Vec::with_capacity(limbs.len().saturating_add(1));
        let mut carry = 0u64;
        for &limb in limbs {
            let widened = (u64::from(limb) << bits) | carry;
            out.push(widened as u32);
            carry = widened >> 32;
        }
        out.push(carry as u32);
        out
    }

    /// Shift a limb sequence right by `bits` (0..32). The inverse of
    /// `shifted_left` for the same `bits`, which is what `div_rem` needs to
    /// undo its normalisation before returning the remainder.
    //
    // `carry` is below `2^bits` by construction, so `carry << 32` is below
    // 2^63; and `widened >> bits` is below 2^32, so the truncation is exact.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "bounds proved in the comment above; see there"
    )]
    fn shifted_right(limbs: &[u32], bits: u32) -> Vec<u32> {
        let mut out = vec![0u32; limbs.len()];
        let mut carry = 0u64;
        for (slot, &limb) in out.iter_mut().zip(limbs).rev() {
            let widened = (carry << 32) | u64::from(limb);
            *slot = (widened >> bits) as u32;
            carry = widened & ((1u64 << bits) - 1);
        }
        out
    }

    /// Division with remainder. Returns (quotient, remainder).
    ///
    /// Knuth's algorithm D (TAOCP vol. 2 s4.3.1), in base 2^32.
    ///
    /// The previous implementation was long division one *bit* at a time, and
    /// allocated three fresh `Vec`s per bit -- one for the shift, one for the
    /// compare, one for the subtract. Reducing a 4096-bit product modulo the
    /// 2048-bit group-14 prime therefore took 4096 iterations and some twelve
    /// thousand allocations, and a key exchange, which performs a few hundred
    /// such reductions, took over eighty seconds of CPU. That is not a slow
    /// handshake, it is an unusable client. Algorithm D does the same work in
    /// one pass of `m * n` limb operations with no allocation in the inner
    /// loop at all.
    //
    // Bounds for the plain arithmetic below, each of which is the standard
    // algorithm-D argument:
    //   * `j + n` and `j + i` are in range because `j <= m`, `i < n` and `u`
    //     has exactly `m + n + 1` limbs.
    //   * `qhat * v_second` is only evaluated when the short-circuiting `||`
    //     has already established `qhat < 2^32`, so it is under 2^64.
    //   * `rhat << 32` is only reached in that same branch, where `rhat` is a
    //     remainder modulo `v_top < 2^32`.
    //   * `t` lies in `-(2^32) ..= 2^32 - 1`, so `t >> 32` is 0 or -1 -- which
    //     *is* the borrow -- and `k` stays in `0 ..= 2^32`.
    // Substituting saturating or checked forms here would not make any of
    // those true; it would only replace an exact result with a wrong one on
    // paths that cannot be taken.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "bounds proved in the comment above; see there"
    )]
    pub fn div_rem(&self, divisor: &BigUint) -> (BigUint, BigUint) {
        if divisor.is_zero() {
            return (BigUint::zero(), BigUint::zero());
        }
        if self.cmp_unsigned(divisor) == std::cmp::Ordering::Less {
            return (BigUint::zero(), self.clone());
        }

        // A single-limb divisor needs no trial quotient: a 64-by-32 divide is
        // exact, and the estimation machinery below wants two divisor limbs to
        // estimate from anyway.
        if let [d] = *divisor.limbs.as_slice() {
            let d = u64::from(d);
            let mut quotient = vec![0u32; self.limbs.len()];
            let mut rem = 0u64;
            for (slot, &limb) in quotient.iter_mut().zip(&self.limbs).rev() {
                let current = (rem << 32) | u64::from(limb);
                *slot = (current / d) as u32;
                rem = current % d;
            }
            return (
                BigUint::normalized(quotient),
                BigUint::normalized(vec![rem as u32]),
            );
        }

        let n = divisor.limbs.len();
        let Some(m) = self.limbs.len().checked_sub(n) else {
            return (BigUint::zero(), self.clone());
        };

        // D1. Normalise, so that the divisor's top limb has its high bit set.
        // That is the condition under which the trial quotient below is at
        // most two too large, which is what makes a single correction step
        // enough.
        let shift = divisor.limbs.last().map_or(0, |&top| top.leading_zeros());
        let scaled_divisor = Self::shifted_left(&divisor.limbs, shift);
        let mut u = Self::shifted_left(&self.limbs, shift);
        // `shifted_left` always appends the carry-out limb: `u` therefore has
        // exactly the `m + n + 1` limbs algorithm D wants, and the divisor's
        // extra limb is zero (the shift is chosen to fill its top limb) and is
        // dropped here.
        let (Some(v), Some(&[v_second, v_top])) = (
            scaled_divisor.get(..n),
            scaled_divisor.get(..n).and_then(<[u32]>::last_chunk::<2>),
        ) else {
            return (BigUint::zero(), self.clone());
        };

        let mut quotient = vec![0u32; m.saturating_add(1)];

        for j in (0..=m).rev() {
            // D3. Estimate this quotient limb from the top two limbs of the
            // running remainder over the divisor's top limb, then walk the
            // estimate down until it is provably right or one too large.
            let Some(&[u_low, u_mid, u_top]) = u.get(j..=j + n).and_then(<[u32]>::last_chunk::<3>)
            else {
                break;
            };
            let numerator = (u64::from(u_top) << 32) | u64::from(u_mid);
            let mut qhat = numerator / u64::from(v_top);
            let mut rhat = numerator % u64::from(v_top);
            while qhat > u64::from(u32::MAX)
                || qhat * u64::from(v_second) > (rhat << 32) | u64::from(u_low)
            {
                qhat -= 1;
                rhat += u64::from(v_top);
                if rhat > u64::from(u32::MAX) {
                    break;
                }
            }

            // D4. Multiply the divisor by the estimate and subtract, carrying
            // the product's high half and the subtraction's borrow in one
            // signed accumulator: `k` is `(product >> 32) - borrow`, so the
            // two never need separate bookkeeping.
            let Some(window) = u.get_mut(j..) else { break };
            let mut k = 0i64;
            for (slot, &vi) in window.iter_mut().zip(v) {
                let product = qhat * u64::from(vi);
                let t = i64::from(*slot) - k - i64::from((product & 0xffff_ffff) as u32);
                *slot = t as u32;
                k = (product >> 32) as i64 - (t >> 32);
            }
            let Some(top_slot) = window.get_mut(n) else {
                break;
            };
            let t = i64::from(*top_slot) - k;
            *top_slot = t as u32;

            // D5/D6. A negative top limb means the estimate was one too large
            // after all. Add the divisor back and step the quotient down; the
            // carry out of the add-back cancels the borrow that made `t`
            // negative, so it is discarded.
            if t < 0 {
                qhat -= 1;
                let mut carry = 0u64;
                for (slot, &vi) in window.iter_mut().zip(v) {
                    let sum = u64::from(*slot) + u64::from(vi) + carry;
                    *slot = sum as u32;
                    carry = sum >> 32;
                }
                if let Some(top_slot) = window.get_mut(n) {
                    *top_slot = top_slot.wrapping_add(carry as u32);
                }
            }

            if let Some(slot) = quotient.get_mut(j) {
                *slot = qhat as u32;
            }
        }

        // D8. Undo the normalisation shift on the remainder, which is what is
        // left in the low `n` limbs of `u`.
        let remainder = u.get(..n).map_or_else(BigUint::zero, |low| {
            BigUint::normalized(Self::shifted_right(low, shift))
        });
        (BigUint::normalized(quotient), remainder)
    }

    /// Subtract (self - other). Assumes self >= other.
    pub fn sub(&self, other: &BigUint) -> BigUint {
        if other.is_zero() {
            return self.clone();
        }
        // Little-endian storage means the two operands are already aligned at
        // the end the borrow starts from, so the shorter one simply runs out:
        // this states the alignment that the big-endian version computed as a
        // signed index (`i as isize - (len as isize - b.len() as isize)`), a
        // mixed-signedness expression whose only purpose was to answer "has
        // the subtrahend ended yet".
        //
        // The digit itself is computed with `overflowing_sub` rather than by
        // widening, subtracting and folding a negative result back: wrapping
        // *is* the modular result the algorithm wants, and the overflow flag
        // *is* the borrow, so neither has to be reconstructed from the sign of
        // a wider intermediate.
        let mut result: Vec<u32> = Vec::with_capacity(self.limbs.len());
        let mut borrow = 0u32;
        let mut subtrahend = other.limbs.iter();

        for &av in &self.limbs {
            let bv = subtrahend.next().copied().unwrap_or(0);
            let (partial, borrowed_b) = av.overflowing_sub(bv);
            let (digit, borrowed_carry) = partial.overflowing_sub(borrow);
            // At most one of the two can borrow: if `bv > av` then `partial` is
            // `2^32 - (bv - av)`, which is at least 1, so it cannot underflow
            // against a `borrow` of 0 or 1.
            borrow = u32::from(borrowed_b | borrowed_carry);
            result.push(digit);
        }

        BigUint::normalized(result)
    }
}

// ============================================================================
// Diffie-Hellman group 14 parameters (RFC 3526 section 3)
// ============================================================================

/// The 2048-bit MODP prime, group 14 of RFC 3526, as published.
///
/// Written once. It was written twice -- 512 hex digits transcribed into both
/// `ssh` and `sshd` -- and the two copies happened to agree, which is luck
/// rather than a property: nothing checked, and a single wrong digit in either
/// would not have failed a test in either crate. It would have produced two
/// programs that each compute a self-consistent shared secret and disagree
/// about it, surfacing four steps later as `MAC verification failed`. Worse, a
/// mistyped digit almost certainly yields a composite modulus, which is not a
/// broken handshake but a weak one, and that fails no test at all.
pub const DH_GROUP14_P_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
    "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3D",
    "C2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F",
    "83655D23DCA3AD961C62F356208552BB9ED529077096966D",
    "670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B",
    "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9",
    "DE2BCBF6955817183995497CEA956AE515D2261898FA0510",
    "15728E5A8AACAA68FFFFFFFFFFFFFFFF"
);

/// The group 14 generator, g = 2 (RFC 3526 section 3).
pub const DH_GROUP14_G: u8 = 2;

/// The group 14 prime as a [`BigUint`].
///
/// Parsed rather than stored as limbs so the constant above stays readable
/// against the RFC, which is the form anyone checking it will have.
#[must_use]
pub fn dh_group14_prime() -> BigUint {
    BigUint::from_bytes_be(&dh_group14_prime_bytes())
}

/// The group 14 prime as big-endian bytes.
///
/// Both the modulus and the exchange hash need these: the hash encodes `p` as
/// an `mpint`, so the bytes are not merely an implementation detail of the
/// `BigUint`.
#[must_use]
pub fn dh_group14_prime_bytes() -> Vec<u8> {
    // `as_chunks::<2>` *is* the "pairs of digits" rule, so there is no stride
    // arithmetic and no `i + 1 < len` guard to get wrong -- and because it
    // yields `[u8; 2]` rather than a two-long slice, the pair destructures
    // directly and there is no `first()`/`get(1)` pair with an invented `b'0'`
    // default standing in for a case the chunk size already rules out. A digit
    // that somehow failed to parse would still read as zero rather than panic;
    // the length assertion below would hold, so the test that checks the first
    // and last bytes is what actually guards the transcription.
    let (pairs, _) = DH_GROUP14_P_HEX.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .map(|&[hi, lo]| {
            let nibble = |b: u8| u8::try_from(char::from(b).to_digit(16).unwrap_or(0)).unwrap_or(0);
            (nibble(hi) << 4) | nibble(lo)
        })
        .collect()
}

// ============================================================================
// Base64 (RFC 4648 §4)
// ============================================================================

/// The standard (non-URL-safe) alphabet. RFC 4648 §4 Table 1.
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Why a base64 string could not be decoded.
///
/// One variant, deliberately. Which character was bad, and where, is the sort
/// of detail that reads as helpful and is not: every caller here is decoding a
/// key, and a key file is either well-formed or is not to be used. Reporting
/// the offset of the first bad byte in a private key is a way of describing
/// its contents to whoever provoked the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base64Error {
    /// The input contains a character outside the alphabet, or its length is
    /// not one a base64 encoder can produce.
    ///
    /// A quartet encodes 1, 2 or 3 bytes, so a group of exactly one character
    /// encodes nothing and cannot have been produced by an encoder. That case
    /// is this variant rather than a silent stop.
    Invalid,
}

impl std::fmt::Display for Base64Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Invalid => f.write_str("not valid base64"),
        }
    }
}

impl std::error::Error for Base64Error {}

/// Encode `data` as base64 **without** `=` padding.
///
/// This is the form SSH uses everywhere it puts base64 in a line of text that
/// something else parses by whitespace: `SHA256:` fingerprints, and the middle
/// field of an `authorized_keys` or `known_hosts` line.
#[must_use]
pub fn base64_encode(data: &[u8]) -> String {
    encode_inner(data, false)
}

/// Encode `data` as base64 **with** `=` padding to a multiple of four.
///
/// This is the form for anything stored as a standalone blob rather than a
/// field in a line: the body of a PEM-style private key file.
///
/// # Why both spellings exist as separate names
///
/// They existed already, in different binaries, under *one* name. `ssh`'s
/// `base64_encode` emitted no padding and `sshd`'s emitted padding, and the
/// two programs exchange the strings each produces. Nothing detected it,
/// because each crate tested its own function against its own expectation.
/// Naming the padding in the function makes choosing wrong a thing you can see
/// at the call site.
#[must_use]
pub fn base64_encode_padded(data: &[u8]) -> String {
    encode_inner(data, true)
}

/// The encoder both public spellings share; `pad` picks which one.
fn encode_inner(data: &[u8], pad: bool) -> String {
    /// One base64 digit from the low six bits of `sextet`.
    ///
    /// Masking to `0x3f` before the lookup makes the index provably in range
    /// for a 64-entry table, so the fallback cannot be reached.
    fn digit(sextet: u32) -> char {
        B64_ALPHABET
            .get((sextet & 0x3f) as usize)
            .map_or('A', |&b| char::from(b))
    }

    // `chunks(3)` states base64's own grouping rule once. The bit layout is
    // identical for a full and a partial group -- only the number of digits
    // emitted differs, and that number is the chunk's own length, so there is
    // no separate tail case to keep in agreement with the loop head.
    let mut out = String::with_capacity(data.len().div_ceil(3).saturating_mul(4));
    for chunk in data.chunks(3) {
        let [b0, b1, b2] = match *chunk {
            [b0, b1, b2] => [b0, b1, b2],
            [b0, b1] => [b0, b1, 0],
            [b0] => [b0, 0, 0],
            // `chunks(3)` yields neither an empty nor an over-long slice; this
            // arm exists only because the compiler cannot know that.
            _ => continue,
        };
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(digit(n >> 18));
        out.push(digit(n >> 12));
        if chunk.len() >= 2 {
            out.push(digit(n >> 6));
        } else if pad {
            out.push('=');
        }
        if chunk.len() >= 3 {
            out.push(digit(n));
        } else if pad {
            out.push('=');
        }
    }
    out
}

/// Decode base64, accepting either padding form and ignoring ASCII whitespace.
///
/// # Errors
///
/// [`Base64Error::Invalid`] if the input holds a character outside the
/// alphabet, or ends in a group of one character, which no encoder emits.
///
/// # Why this refuses rather than truncates
///
/// The decoder this replaces in `sshd` stopped at the first character it did
/// not recognise and returned the bytes it had decoded so far. A host key file
/// with a corrupted character in the middle therefore did not fail to load: it
/// loaded as a *shorter* key, and the daemon then either rejected it for its
/// length or — for a corruption past the 64-byte mark — started normally with
/// a key whose comment had been silently eaten. A decoder that answers `Ok`
/// for input no encoder produced is not a decoder; it is a guess.
pub fn base64_decode(input: &[u8]) -> Result<Vec<u8>, Base64Error> {
    /// Not a base64 character. Distinct from every sextet, which are `0..=63`.
    const INVALID: u8 = 0xFF;

    /// Reverse lookup, built at compile time.
    ///
    /// The indexing here is const-evaluated: an out-of-range index fails the
    /// build rather than panicking at run time, so this suppression cannot
    /// hide a reachable panic. It is scoped to the table alone, leaving the
    /// decode loop -- the part that reads bytes someone else wrote -- under
    /// the lint.
    #[expect(
        clippy::indexing_slicing,
        reason = "const-evaluated: an out-of-range index is a compile error, not a panic"
    )]
    const DECODE: [u8; 256] = {
        let mut table = [INVALID; 256];
        let mut i = 0usize;
        while i < 64 {
            table[B64_ALPHABET[i] as usize] = i as u8;
            i += 1;
        }
        table
    };

    let sextet = |&b: &u8| DECODE.get(b as usize).copied().unwrap_or(INVALID);

    // Padding and layout whitespace come off first, so the quartet loop below
    // sees only data. Whitespace is stripped rather than rejected because
    // every producer of these strings wraps them across lines; `=` because
    // both padded and unpadded input must decode to the same bytes, which is
    // what lets this one function replace three that disagreed about padding.
    let body: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| *b != b'=' && !b.is_ascii_whitespace())
        .collect();

    let mut out = Vec::with_capacity((body.len() / 4).saturating_mul(3));
    for quad in body.chunks(4) {
        // A group of one encodes no bytes at all, so it cannot have come from
        // an encoder. The decoder this replaces treated it as end-of-input.
        let (Some(a), Some(b)) = (quad.first().map(sextet), quad.get(1).map(sextet)) else {
            return Err(Base64Error::Invalid);
        };
        if a == INVALID || b == INVALID {
            return Err(Base64Error::Invalid);
        }
        // The shifts discard high bits by design: that is how six-bit sextets
        // repack into eight-bit bytes. Rust panics only on a shift whose
        // *amount* reaches the width, and every amount here is a constant
        // below 8.
        out.push((a << 2) | (b >> 4));

        if let Some(c) = quad.get(2).map(sextet) {
            if c == INVALID {
                return Err(Base64Error::Invalid);
            }
            out.push((b << 4) | (c >> 2));
            if let Some(d) = quad.get(3).map(sextet) {
                if d == INVALID {
                    return Err(Base64Error::Invalid);
                }
                out.push((c << 6) | d);
            }
        }
    }
    Ok(out)
}

// ============================================================================
// The unencrypted OpenSSH private key container (`PROTOCOL.key`)
// ============================================================================

/// The magic that opens the container, NUL included.
const OPENSSH_KEY_MAGIC: &[u8] = b"openssh-key-v1\0";

/// The PEM band around the base64 body.
const OPENSSH_KEY_HEADER: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
/// The closing band; see [`OPENSSH_KEY_HEADER`].
const OPENSSH_KEY_FOOTER: &str = "-----END OPENSSH PRIVATE KEY-----";

/// The key type this module handles. The only one SlateOS implements.
pub const OPENSSH_KEY_TYPE_ED25519: &[u8] = b"ssh-ed25519";

/// What an unencrypted Ed25519 private key file holds.
///
/// # Why the public key comes back rather than being checked here
///
/// The file stores the public key beside the seed, so the two can disagree —
/// through corruption, or through a file assembled by hand. Verifying that
/// they agree means deriving a public key from the seed, which is Ed25519
/// arithmetic, which lives in `posix`. This crate deliberately does not depend
/// on `posix`: an rlib copy of `posix` linked into a SlateOS program is a
/// second libc whose every syscall answers `-ENOSYS` (known-issues.md
/// `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`), so
/// pulling it in here to check one equality would put that hazard into both
/// binaries.
///
/// So the container codec returns both halves and each caller checks. That is
/// a real obligation and not a formality — `sshd` already does it, and a
/// caller that skips it accepts a key whose halves disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpensshPrivateKey {
    /// The 32-byte Ed25519 seed: the actual secret.
    pub seed: [u8; 32],
    /// The public key **as stored in the file**, which the caller must check
    /// against one derived from `seed`.
    pub public: [u8; 32],
    /// The trailing comment. Not authenticated by anything; treat as a label.
    pub comment: String,
}

/// Why a private key file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateKeyError {
    /// The PEM band is missing, or the footer precedes the header.
    NotPem,
    /// The body is not valid base64.
    Base64(Base64Error),
    /// The decoded bytes do not begin with `openssh-key-v1\0`.
    NotOpensshKey,
    /// A length-prefixed field ran off the end of the container.
    ///
    /// `what` names the field, so a truncated file says which part is missing
    /// rather than only that something is.
    Truncated {
        /// The field that could not be read.
        what: &'static str,
    },
    /// The key is encrypted, and nothing here can decrypt it.
    ///
    /// Carries the cipher name so the message can say what to re-create the
    /// key without. Refusing is the honest answer: `sshd` is started by init
    /// with no terminal to prompt on, and there is no `bcrypt_pbkdf` here to
    /// derive a key with even if there were.
    Encrypted {
        /// The `ciphername` field, as written in the file.
        cipher: String,
    },
    /// The container holds a number of keys other than one.
    KeyCount {
        /// The count the file declares.
        count: u32,
    },
    /// The two `checkint` words differ.
    ///
    /// In OpenSSH this is how a wrong passphrase is detected. Here, where
    /// nothing is ever encrypted, it is a free integrity check on the private
    /// section.
    CheckintMismatch,
    /// The key is of a type this does not implement.
    UnsupportedKeyType {
        /// The type named in the file.
        keytype: String,
    },
    /// The `ssh-ed25519` secret field is not the 64 bytes the format requires.
    SecretLength {
        /// The length found.
        len: usize,
    },
    /// The stored public key is not 32 bytes.
    PublicLength {
        /// The length found.
        len: usize,
    },
}

impl std::fmt::Display for PrivateKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NotPem => write!(f, "not a PEM-wrapped key: no {OPENSSH_KEY_HEADER} band"),
            Self::Base64(e) => write!(f, "key body is not valid base64: {e}"),
            Self::NotOpensshKey => f.write_str("not an openssh-key-v1 container"),
            Self::Truncated { what } => write!(f, "truncated {what}"),
            Self::Encrypted { ref cipher } => write!(
                f,
                "key is encrypted with {cipher}; there is no passphrase prompt here, \
                 re-create it with an empty passphrase"
            ),
            Self::KeyCount { count } => {
                write!(f, "expected exactly one key in the file, found {count}")
            }
            Self::CheckintMismatch => {
                f.write_str("private section checkints differ (corrupt or encrypted key)")
            }
            Self::UnsupportedKeyType { ref keytype } => write!(
                f,
                "unsupported key type {keytype}; only ssh-ed25519 is implemented"
            ),
            Self::SecretLength { len } => {
                write!(f, "ssh-ed25519 secret should be 64 bytes, found {len}")
            }
            Self::PublicLength { len } => {
                write!(f, "ssh-ed25519 public key should be 32 bytes, found {len}")
            }
        }
    }
}

impl std::error::Error for PrivateKeyError {}

impl From<Base64Error> for PrivateKeyError {
    fn from(e: Base64Error) -> Self {
        Self::Base64(e)
    }
}

/// The public-key blob for an Ed25519 key: `string("ssh-ed25519") ||
/// string(public)`.
///
/// This is the blob that appears base64-encoded in the middle field of an
/// `authorized_keys` or `known_hosts` line, inside the private key container's
/// `publickey` field, and on the wire in `SSH_MSG_KEXDH_REPLY`. One function
/// so those four spellings cannot drift apart.
#[must_use]
pub fn ed25519_public_blob(public: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(51);
    blob.extend_from_slice(&ssh_string(OPENSSH_KEY_TYPE_ED25519));
    blob.extend_from_slice(&ssh_string(public));
    blob
}

/// The exact byte sequence a `publickey` signature covers (RFC 4252 §7).
///
/// ```text
/// string    session identifier
/// byte      SSH_MSG_USERAUTH_REQUEST
/// string    user name
/// string    service name
/// string    "publickey"
/// boolean   TRUE
/// string    public key algorithm name
/// string    public key blob
/// ```
///
/// The client builds this and signs it; the server builds it again and verifies
/// against it. Nothing on the wire carries the blob itself — that is the whole
/// design, since a signature over bytes the *sender* chose would prove nothing
/// — so the two constructions must agree byte for byte with no opportunity to
/// discover that they do not. A trailing field the client includes and the
/// server omits does not produce a diagnosable error; it produces a signature
/// that simply fails to verify, indistinguishable from a wrong key or a
/// forgery.
///
/// That is why this is here rather than in `sshd`, where it was written first
/// and where it was still the only copy. The client does not yet do publickey
/// authentication; when it does, it needs these bytes, and the point of moving
/// the function now is that there is no moment at which a second copy exists to
/// drift from this one.
///
/// # What is bound, and what each binding stops
///
/// - **The session identifier** ties the signature to one connection, so one
///   captured from a session with a hostile server cannot be replayed to a
///   different one. It is the exchange hash of the *first* key exchange, which
///   the peer cannot choose alone.
/// - **The user name and service name** tie it to one account, so a signature
///   offered for `alice` cannot be presented as `root`.
/// - **The algorithm name and key blob** tie it to one key, so a signature made
///   under a weak algorithm cannot be re-labelled as one made under a strong
///   one.
///
/// `key_blob` is the wire form of the public key — [`ed25519_public_blob`] for
/// an Ed25519 key — and not the bare 32-byte point; it is the same blob that
/// appeared in the request being signed.
#[must_use]
pub fn pubkey_signed_blob(
    session_id: &[u8; 32],
    user_bytes: &[u8],
    service_bytes: &[u8],
    algorithm: &[u8],
    key_blob: &[u8],
) -> Vec<u8> {
    let mut signed = Vec::new();
    signed.extend_from_slice(&ssh_string(session_id));
    signed.push(msg::SSH_MSG_USERAUTH_REQUEST);
    signed.extend_from_slice(&ssh_string(user_bytes));
    signed.extend_from_slice(&ssh_string(service_bytes));
    // Not a parameter: §7 fixes this field to the method name of the request
    // being signed, and the only method whose request carries a signature is
    // `publickey`. A caller able to vary it could only produce a blob no
    // verifier builds.
    signed.extend_from_slice(&ssh_string(b"publickey"));
    // The boolean is TRUE by definition: a request with FALSE here carries no
    // signature, so there is nothing to sign over. `read_bool` is what turns
    // the received byte back into a bool; this is the encoding side of it.
    signed.push(1);
    signed.extend_from_slice(&ssh_string(algorithm));
    signed.extend_from_slice(&ssh_string(key_blob));
    signed
}

/// Write an unencrypted Ed25519 private key in the OpenSSH container format.
///
/// The layout, from `PROTOCOL.key` in the OpenSSH distribution:
///
/// ```text
/// "openssh-key-v1\0"
/// string  ciphername   ("none")
/// string  kdfname      ("none")
/// string  kdfoptions   ("")
/// uint32  number of keys N   (1)
/// string  publickey[0]
/// string  encrypted-private-section
/// ```
///
/// and the private section, which for `ciphername = none` is not encrypted at
/// all:
///
/// ```text
/// uint32  checkint
/// uint32  checkint   (the same value again)
/// string  keytype
/// string  public key
/// string  private key   (seed || public, 64 bytes)
/// string  comment
/// byte[]  padding 1, 2, 3, ...  to a multiple of 8
/// ```
///
/// # The `checkint` is a parameter, not drawn here
///
/// OpenSSH compares the two copies on read to detect a wrong passphrase. Since
/// nothing here encrypts, any value works provided both copies match — but
/// writing a *constant* would make every key file this project produces share
/// a recognisable byte pattern, so the caller draws one from its own CSPRNG.
/// Passing it in rather than reaching for `randrange` here is also what lets a
/// test pin an exact file: a codec that draws its own entropy cannot be
/// checked against a fixture.
#[must_use]
pub fn encode_openssh_private_key(
    seed: &[u8; 32],
    public: &[u8; 32],
    comment: &str,
    checkint: u32,
) -> String {
    let mut secret = Vec::with_capacity(64);
    secret.extend_from_slice(seed);
    secret.extend_from_slice(public);

    let mut private = Vec::new();
    private.extend_from_slice(&ssh_u32(checkint));
    private.extend_from_slice(&ssh_u32(checkint));
    private.extend_from_slice(&ssh_string(OPENSSH_KEY_TYPE_ED25519));
    private.extend_from_slice(&ssh_string(public));
    private.extend_from_slice(&ssh_string(&secret));
    private.extend_from_slice(&ssh_string(comment.as_bytes()));
    // Pad to a multiple of 8 -- the block size "none" nominally has -- with the
    // bytes 1, 2, 3, ... as PROTOCOL.key specifies. `pad` cannot reach 8, so
    // the counter cannot wrap.
    let mut pad: u8 = 1;
    while !private.len().is_multiple_of(8) {
        private.push(pad);
        pad = pad.wrapping_add(1);
    }

    let mut raw = Vec::new();
    raw.extend_from_slice(OPENSSH_KEY_MAGIC);
    raw.extend_from_slice(&ssh_string(b"none")); // ciphername
    raw.extend_from_slice(&ssh_string(b"none")); // kdfname
    raw.extend_from_slice(&ssh_string(b"")); // kdfoptions
    raw.extend_from_slice(&ssh_u32(1)); // exactly one key
    raw.extend_from_slice(&ssh_string(&ed25519_public_blob(public)));
    raw.extend_from_slice(&ssh_string(&private));

    // 70 columns is what OpenSSH writes. The width is cosmetic -- the decoder
    // strips whitespace -- but matching it keeps a diff against a file written
    // by the real ssh-keygen down to the key material.
    let mut text = String::from(OPENSSH_KEY_HEADER);
    text.push('\n');
    for chunk in base64_encode_padded(&raw).as_bytes().chunks(70) {
        text.push_str(&String::from_utf8_lossy(chunk));
        text.push('\n');
    }
    text.push_str(OPENSSH_KEY_FOOTER);
    text.push('\n');
    text
}

/// Read an unencrypted Ed25519 private key from the OpenSSH container format.
///
/// The inverse of [`encode_openssh_private_key`]; the layout is documented
/// there.
///
/// # Errors
///
/// A [`PrivateKeyError`] naming which part of the container was wrong. Every
/// failure is a refusal: this never falls back to inventing a key from a file
/// it could not parse. `sshd` once did — it hashed the first line and used
/// that as a seed — so `sshd -h /etc/ssh/ssh_host_rsa_key` started
/// successfully with a host key unrelated to the file named, and the only
/// symptom was every client reporting a changed host key.
///
/// The returned `public` is the one *stored in the file*. See
/// [`OpensshPrivateKey`] for why checking it against the seed is the caller's
/// job and not this function's.
pub fn decode_openssh_private_key(text: &str) -> Result<OpensshPrivateKey, PrivateKeyError> {
    // The band is required, not merely tolerated: a bare base64 body would
    // also decode, and accepting one means accepting a file that no tool
    // produces and that a user pasted incompletely.
    let start = text
        .find(OPENSSH_KEY_HEADER)
        .ok_or(PrivateKeyError::NotPem)?;
    let after_header = start.saturating_add(OPENSSH_KEY_HEADER.len());
    let body_end = text
        .get(after_header..)
        .and_then(|rest| rest.find(OPENSSH_KEY_FOOTER))
        .ok_or(PrivateKeyError::NotPem)?;
    let body = text
        .get(after_header..after_header.saturating_add(body_end))
        .ok_or(PrivateKeyError::NotPem)?;

    let raw = base64_decode(body.as_bytes())?;

    if raw.get(..OPENSSH_KEY_MAGIC.len()) != Some(OPENSSH_KEY_MAGIC) {
        return Err(PrivateKeyError::NotOpensshKey);
    }
    let mut off = OPENSSH_KEY_MAGIC.len();

    let (ciphername, next) = read_ssh_string(&raw, off)
        .map_err(|_| PrivateKeyError::Truncated { what: "ciphername" })?;
    off = next;
    if ciphername != b"none" {
        return Err(PrivateKeyError::Encrypted {
            cipher: String::from_utf8_lossy(ciphername).into_owned(),
        });
    }
    let (_kdfname, next) =
        read_ssh_string(&raw, off).map_err(|_| PrivateKeyError::Truncated { what: "kdfname" })?;
    off = next;
    let (_kdfopts, next) = read_ssh_string(&raw, off)
        .map_err(|_| PrivateKeyError::Truncated { what: "kdfoptions" })?;
    off = next;

    let (count, next) =
        read_u32(&raw, off).map_err(|_| PrivateKeyError::Truncated { what: "key count" })?;
    off = next;
    if count != 1 {
        return Err(PrivateKeyError::KeyCount { count });
    }

    let (_pubkey, next) = read_ssh_string(&raw, off)
        .map_err(|_| PrivateKeyError::Truncated { what: "public key" })?;
    off = next;
    let (private, _) = read_ssh_string(&raw, off).map_err(|_| PrivateKeyError::Truncated {
        what: "private section",
    })?;

    let (check1, poff) =
        read_u32(private, 0).map_err(|_| PrivateKeyError::Truncated { what: "checkint" })?;
    let (check2, poff) = read_u32(private, poff).map_err(|_| PrivateKeyError::Truncated {
        what: "second checkint",
    })?;
    if check1 != check2 {
        return Err(PrivateKeyError::CheckintMismatch);
    }

    let (keytype, poff) = read_ssh_string(private, poff)
        .map_err(|_| PrivateKeyError::Truncated { what: "key type" })?;
    if keytype != OPENSSH_KEY_TYPE_ED25519 {
        return Err(PrivateKeyError::UnsupportedKeyType {
            keytype: String::from_utf8_lossy(keytype).into_owned(),
        });
    }
    let (stored_public, poff) =
        read_ssh_string(private, poff).map_err(|_| PrivateKeyError::Truncated {
            what: "stored public key",
        })?;
    let (secret, poff) = read_ssh_string(private, poff)
        .map_err(|_| PrivateKeyError::Truncated { what: "secret" })?;
    // The comment is the last field before the padding. A file whose comment
    // is missing entirely is truncated, not comment-less: the encoder always
    // writes the field, even when empty.
    let (comment, _) = read_ssh_string(private, poff)
        .map_err(|_| PrivateKeyError::Truncated { what: "comment" })?;

    let public: [u8; 32] = stored_public
        .try_into()
        .map_err(|_| PrivateKeyError::PublicLength {
            len: stored_public.len(),
        })?;
    // §"private key" for ssh-ed25519 is seed || public, 64 bytes. We keep the
    // seed and hand back the stored public separately rather than trusting the
    // copy inside the secret, so the two can be compared.
    let seed: [u8; 32] = secret
        .get(..32)
        .and_then(|s| <[u8; 32]>::try_from(s).ok())
        .filter(|_| secret.len() == 64)
        .ok_or(PrivateKeyError::SecretLength { len: secret.len() })?;

    Ok(OpensshPrivateKey {
        seed,
        public,
        comment: String::from_utf8_lossy(comment).into_owned(),
    })
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

    // ---- Message type codes ----

    /// Every entry in [`msg`], as `(name, value)`.
    ///
    /// Written out once so the two tests below can walk the table. Keeping it
    /// beside them rather than deriving it from the module is deliberate: a
    /// macro that generated both the constants and this list would make the
    /// list agree with the constants by construction, which is the shape of
    /// self-agreement this whole crate exists to avoid.
    const ALL_MESSAGE_CODES: &[(&str, u8)] = &[
        ("SSH_MSG_DISCONNECT", msg::SSH_MSG_DISCONNECT),
        ("SSH_MSG_IGNORE", msg::SSH_MSG_IGNORE),
        ("SSH_MSG_UNIMPLEMENTED", msg::SSH_MSG_UNIMPLEMENTED),
        ("SSH_MSG_DEBUG", msg::SSH_MSG_DEBUG),
        ("SSH_MSG_SERVICE_REQUEST", msg::SSH_MSG_SERVICE_REQUEST),
        ("SSH_MSG_SERVICE_ACCEPT", msg::SSH_MSG_SERVICE_ACCEPT),
        ("SSH_MSG_KEXINIT", msg::SSH_MSG_KEXINIT),
        ("SSH_MSG_NEWKEYS", msg::SSH_MSG_NEWKEYS),
        ("SSH_MSG_KEX_DH_INIT", msg::SSH_MSG_KEX_DH_INIT),
        ("SSH_MSG_KEX_DH_REPLY", msg::SSH_MSG_KEX_DH_REPLY),
        ("SSH_MSG_USERAUTH_REQUEST", msg::SSH_MSG_USERAUTH_REQUEST),
        ("SSH_MSG_USERAUTH_FAILURE", msg::SSH_MSG_USERAUTH_FAILURE),
        ("SSH_MSG_USERAUTH_SUCCESS", msg::SSH_MSG_USERAUTH_SUCCESS),
        ("SSH_MSG_USERAUTH_BANNER", msg::SSH_MSG_USERAUTH_BANNER),
        ("SSH_MSG_USERAUTH_PK_OK", msg::SSH_MSG_USERAUTH_PK_OK),
        ("SSH_MSG_GLOBAL_REQUEST", msg::SSH_MSG_GLOBAL_REQUEST),
        ("SSH_MSG_REQUEST_SUCCESS", msg::SSH_MSG_REQUEST_SUCCESS),
        ("SSH_MSG_REQUEST_FAILURE", msg::SSH_MSG_REQUEST_FAILURE),
        ("SSH_MSG_CHANNEL_OPEN", msg::SSH_MSG_CHANNEL_OPEN),
        (
            "SSH_MSG_CHANNEL_OPEN_CONFIRMATION",
            msg::SSH_MSG_CHANNEL_OPEN_CONFIRMATION,
        ),
        (
            "SSH_MSG_CHANNEL_OPEN_FAILURE",
            msg::SSH_MSG_CHANNEL_OPEN_FAILURE,
        ),
        (
            "SSH_MSG_CHANNEL_WINDOW_ADJUST",
            msg::SSH_MSG_CHANNEL_WINDOW_ADJUST,
        ),
        ("SSH_MSG_CHANNEL_DATA", msg::SSH_MSG_CHANNEL_DATA),
        (
            "SSH_MSG_CHANNEL_EXTENDED_DATA",
            msg::SSH_MSG_CHANNEL_EXTENDED_DATA,
        ),
        ("SSH_MSG_CHANNEL_EOF", msg::SSH_MSG_CHANNEL_EOF),
        ("SSH_MSG_CHANNEL_CLOSE", msg::SSH_MSG_CHANNEL_CLOSE),
        ("SSH_MSG_CHANNEL_REQUEST", msg::SSH_MSG_CHANNEL_REQUEST),
        ("SSH_MSG_CHANNEL_SUCCESS", msg::SSH_MSG_CHANNEL_SUCCESS),
        ("SSH_MSG_CHANNEL_FAILURE", msg::SSH_MSG_CHANNEL_FAILURE),
    ];

    /// No two message codes share a number.
    ///
    /// The failure this catches is a copy-paste: a constant added by duplicating
    /// its neighbour and renaming it without changing the value. Nothing else
    /// would notice. Both constants would compile, both would be dispatched on,
    /// and the receiver would route one message type to the other's handler —
    /// which looks, from either end, like a peer that sends the wrong thing.
    #[test]
    fn no_two_message_codes_collide() {
        for (i, &(name_a, value_a)) in ALL_MESSAGE_CODES.iter().enumerate() {
            for &(name_b, value_b) in ALL_MESSAGE_CODES.iter().skip(i + 1) {
                assert_ne!(
                    value_a, value_b,
                    "{name_a} and {name_b} are both {value_a}; \
                     one message number cannot mean two things"
                );
            }
        }
    }

    /// Every code sits in the range its RFC reserves for that layer.
    ///
    /// RFC 4250 §4.1.2 partitions the byte, and the partition is what makes the
    /// numbers extensible: 30..=49 belong to whichever key exchange method was
    /// negotiated, and 60..=79 to whichever authentication method is in
    /// progress, so the *same* byte means different things at different points
    /// in one connection. A constant in the wrong band is therefore not a
    /// cosmetic mistake — it claims a number that some other feature owns.
    ///
    /// This is the check that consults something outside the table. Asserting
    /// `SSH_MSG_CHANNEL_DATA == 94` beside `SSH_MSG_CHANNEL_DATA: u8 = 94`
    /// would only restate the definition; the bands come from the registry.
    #[test]
    fn every_message_code_is_in_the_band_rfc_4250_reserves_for_it() {
        for &(name, value) in ALL_MESSAGE_CODES {
            let band = match name {
                n if n.starts_with("SSH_MSG_KEX_DH_") => (30, 49),
                n if n.starts_with("SSH_MSG_USERAUTH_PK") => (60, 79),
                n if n.starts_with("SSH_MSG_USERAUTH_") => (50, 59),
                n if n.starts_with("SSH_MSG_CHANNEL_")
                    || n.starts_with("SSH_MSG_GLOBAL_")
                    || n.starts_with("SSH_MSG_REQUEST_") =>
                {
                    (80, 127)
                }
                // Everything else is transport: 1..=19 generic, 20..=29
                // negotiation. One band, since both are the transport layer's.
                _ => (1, 29),
            };
            assert!(
                value >= band.0 && value <= band.1,
                "{name} is {value}, outside the {}..={} band RFC 4250 §4.1.2 \
                 reserves for its layer",
                band.0,
                band.1
            );
        }
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

    // ========================================================================
    // BigUint
    //
    // These came here with the type, from `ssh`, where they were the only
    // tests it had: an arithmetic slip here does not fail loudly, it produces
    // a shared secret that differs from the peer's, and that surfaces four
    // steps later as an opaque "MAC verification failed" with nothing pointing
    // back here. They are also why the type's internals -- `limbs`,
    // `normalized`, the shifts -- are private and still reachable: they are
    // private to everything outside this file, and these tests are inside it.
    // ========================================================================

    fn big(digits: &str) -> BigUint {
        BigUint::from_bytes_be(&hex(digits))
    }

    fn hex_of(n: &BigUint) -> String {
        to_hex(&n.to_bytes_be())
    }

    /// Zero is the empty vector and every other value ends with a nonzero
    /// limb. Every arithmetic routine returns through `normalized`, so this is
    /// the invariant the whole type rests on.
    #[test]
    fn construction_normalizes_leading_zeros_away() {
        assert!(big("0000").is_zero());
        assert!(BigUint::from_bytes_be(&[]).is_zero());
        assert_eq!(big("000001").limbs, vec![1]);
        assert_eq!(big("0000ff00").limbs, vec![0xff00]);
        // A limb boundary is four bytes in, so this is the case where the
        // partial top group and the whole low group have to line up.
        assert_eq!(big("0000000102030405").limbs, vec![0x0203_0405, 0x01]);
        // A normalized zero prints as a single zero byte, not as nothing.
        assert_eq!(BigUint::zero().to_bytes_be(), vec![0]);
    }

    /// Byte strings whose length is not a multiple of the limb width are the
    /// case a limb-based representation can most easily get wrong, and every
    /// SSH `mpint` is one: the group-14 prime is 256 bytes, but a shared
    /// secret is whatever length it came out.
    #[test]
    fn byte_strings_round_trip_at_every_offset_from_a_limb_boundary() {
        for hex in [
            "01",
            "0102",
            "010203",
            "01020304",
            "0102030405",
            "0102030405060708090a",
            "ff",
            "ffffffffffffffffffffff",
            "0100000000",
        ] {
            assert_eq!(hex_of(&big(hex)), hex, "round trip of {hex}");
        }
    }

    #[test]
    fn bit_length_counts_from_the_top_set_bit() {
        assert_eq!(BigUint::zero().bit_length(), 0);
        assert_eq!(big("01").bit_length(), 1);
        assert_eq!(big("02").bit_length(), 2);
        assert_eq!(big("ff").bit_length(), 8);
        assert_eq!(big("0100").bit_length(), 9);
        assert_eq!(big("ffff").bit_length(), 16);
    }

    /// `bit` indexes from the least significant end into a big-endian buffer,
    /// so the index runs backwards; a position past the top is `false`, not a
    /// panic and not a wrap.
    #[test]
    fn bit_indexes_from_the_least_significant_end() {
        let n = big("0102"); // 0b1_0000_0010
        assert!(!n.bit(0));
        assert!(n.bit(1));
        assert!(!n.bit(2));
        assert!(n.bit(8));
        assert!(!n.bit(9));
        assert!(!n.bit(1_000_000));
        assert!(!BigUint::zero().bit(0));
    }

    #[test]
    fn multiplication_matches_hand_computed_products() {
        assert_eq!(hex_of(&big("00").mul(&big("ff"))), "00");
        assert_eq!(hex_of(&big("02").mul(&big("03"))), "06");
        // 255 * 255 = 65025 = 0xFE01: the carry crosses a byte.
        assert_eq!(hex_of(&big("ff").mul(&big("ff"))), "fe01");
        // 65535 * 65535 = 4294836225 = 0xFFFE0001: carries across three.
        assert_eq!(hex_of(&big("ffff").mul(&big("ffff"))), "fffe0001");
        // Asymmetric widths, to catch an operand-order mistake.
        assert_eq!(hex_of(&big("0100").mul(&big("02"))), "0200");
        assert_eq!(hex_of(&big("02").mul(&big("0100"))), "0200");
    }

    /// The carry out of a full-width column must travel all the way up rather
    /// than land in one higher digit and be tidied later. `2^64 - 1` squared
    /// exercises eight consecutive carries.
    #[test]
    fn multiplication_propagates_carries_the_whole_way() {
        let all_ones = big("ffffffffffffffff");
        assert_eq!(
            hex_of(&all_ones.mul(&all_ones)),
            "fffffffffffffffe0000000000000001"
        );
    }

    #[test]
    fn multiplication_is_commutative_and_associative() {
        let a = big("0123456789abcdef");
        let b = big("fedcba9876543210");
        let c = big("00ff00ff00ff");
        assert_eq!(hex_of(&a.mul(&b)), hex_of(&b.mul(&a)));
        assert_eq!(hex_of(&a.mul(&b).mul(&c)), hex_of(&a.mul(&b.mul(&c))));
    }

    #[test]
    fn subtraction_borrows_across_bytes() {
        assert_eq!(hex_of(&big("0100").sub(&big("01"))), "ff");
        assert_eq!(hex_of(&big("010000").sub(&big("01"))), "ffff");
        assert_eq!(hex_of(&big("05").sub(&big("05"))), "00");
        assert_eq!(hex_of(&big("05").sub(&BigUint::zero())), "05");
        // Operands of different widths: the shorter one runs out and the
        // borrow keeps travelling.
        assert_eq!(hex_of(&big("00010000").sub(&big("0001"))), "ffff");
    }

    /// `div_rem` normalises its operands with a left shift and un-normalises
    /// the remainder with the matching right shift. If those two ever disagree
    /// the quotient still looks plausible while the remainder is silently
    /// scaled, so they are checked directly against each other.
    #[test]
    fn the_division_normalisation_shift_is_reversible() {
        let shifted = |hex: &str, bits: u32| {
            BigUint::normalized(BigUint::shifted_left(&big(hex).limbs, bits))
        };
        assert!(shifted("00", 1).is_zero());
        assert_eq!(hex_of(&shifted("01", 1)), "02");
        assert_eq!(hex_of(&shifted("80", 1)), "0100");
        assert_eq!(hex_of(&shifted("ffff", 1)), "01fffe");
        // A shift that crosses the limb boundary, which the byte-based version
        // could not have got wrong and this one can.
        assert_eq!(hex_of(&shifted("80000000", 1)), "0100000000");

        for hex in ["01", "80", "ffff", "0123456789abcdef", "ff00000000000001"] {
            for bits in [0u32, 1, 7, 31] {
                let up = BigUint::shifted_left(&big(hex).limbs, bits);
                let back = BigUint::normalized(BigUint::shifted_right(&up, bits));
                assert_eq!(hex_of(&back), hex, "{hex} shifted by {bits} and back");
            }
        }
    }

    #[test]
    fn division_returns_quotient_and_remainder() {
        let (q, r) = big("0a").div_rem(&big("03"));
        assert_eq!((hex_of(&q), hex_of(&r)), ("03".into(), "01".into()));

        // Exact division leaves no remainder.
        let (q, r) = big("0100").div_rem(&big("10"));
        assert_eq!((hex_of(&q), hex_of(&r)), ("10".into(), "00".into()));

        // A divisor larger than the dividend: quotient zero, remainder self.
        let (q, r) = big("05").div_rem(&big("0a"));
        assert_eq!((hex_of(&q), hex_of(&r)), ("00".into(), "05".into()));

        // Division by zero is defined as (0, 0) rather than a panic.
        let (q, r) = big("0a").div_rem(&BigUint::zero());
        assert!(q.is_zero() && r.is_zero());
    }

    /// The property that actually matters: `a - q*b == r`, with `r < b`. Stated
    /// as a subtraction rather than as `q*b + r == a` so that it exercises
    /// `mul`, `div_rem` and `sub` together on the same numbers.
    ///
    /// The list below is chosen for algorithm D's correction paths rather than
    /// for variety. A trial quotient estimated from two limbs can come out one
    /// or two too large, and the cases that make it do so are rare enough that
    /// random inputs essentially never find them -- which is exactly why they
    /// are the cases a hand-written divider gets wrong. `7fff800000000000 /
    /// 800000000001` and `2^127 / (2^96 - 2^64 + 1)` are the classical add-back
    /// triggers from Knuth's own exercises; the rest cover a divisor whose top
    /// limb is
    /// small (so the normalisation shift is large), a divisor that is an exact
    /// multiple, and operands that differ by many limbs.
    #[test]
    fn division_reconstructs_the_dividend() {
        for (a_hex, b_hex) in [
            ("ffffffff", "0101"),
            ("0123456789abcdef", "fedc"),
            ("8000000000000000", "03"),
            ("ff00ff00ff00ff00", "0100ff"),
            // Trial-quotient corrections.
            ("7fff800000000000", "800000000001"),
            (
                "80000000000000000000000000000000",
                "ffffffff0000000000000001",
            ),
            ("ffffffffffffffffffffffff", "ffffffff00000001"),
            // A divisor whose top limb is 1, so the normalisation shift is 31.
            ("ffffffffffffffffffffffffffffffff", "0000000100000000"),
            // Exact multiples leave a zero remainder, which is the case where
            // the final `normalized` has to collapse the limbs to nothing.
            ("0100000000000000000000000000", "0100000000"),
            // Many more limbs above than below.
            (
                "fedcba9876543210fedcba9876543210fedcba9876543210",
                "0123456789abcdef",
            ),
            ("80000000000000000000000000000001", "7fffffffffffffff"),
        ] {
            let (a, b) = (big(a_hex), big(b_hex));
            let (q, r) = a.div_rem(&b);
            assert_eq!(
                hex_of(&a.sub(&q.mul(&b))),
                hex_of(&r),
                "a - q*b != r for {a_hex} / {b_hex}"
            );
            assert_eq!(
                r.cmp_unsigned(&b),
                std::cmp::Ordering::Less,
                "remainder not less than divisor for {a_hex} / {b_hex}"
            );
        }
    }

    /// The same `a - q*b == r, r < b` property over a few hundred pseudorandom
    /// operands of assorted widths.
    ///
    /// The hand-picked cases above name the corrections algorithm D is known to
    /// need; this one is there for the ones nobody thought to name. The
    /// generator is a fixed-seed LCG rather than a real CSPRNG so that a
    /// failure is reproducible from the test alone -- a randomised test that
    /// cannot be re-run on the input that broke it is a report without a bug.
    /// It is only affordable at this size because division stopped being
    /// bit-at-a-time; the whole sweep runs in well under a second.
    #[test]
    fn division_reconstructs_the_dividend_over_pseudorandom_operands() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            // xorshift64*, chosen for being four lines rather than for quality.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };

        for case in 0..400 {
            let a_limbs = 1 + case % 9;
            let b_limbs = 1 + (case / 9) % 6;
            let build = |count: usize, next: &mut dyn FnMut() -> u64| {
                BigUint::normalized(
                    (0..count)
                        .map(|_| {
                            let word = next();
                            // Deliberately biased towards limbs that are all
                            // ones or all zeros: those are what make a trial
                            // quotient come out too large.
                            match word % 4 {
                                0 => 0,
                                1 => u32::MAX,
                                _ => word as u32,
                            }
                        })
                        .collect(),
                )
            };

            let a = build(a_limbs, &mut next);
            let b = build(b_limbs, &mut next);
            if b.is_zero() {
                continue;
            }

            let (q, r) = a.div_rem(&b);
            assert_eq!(
                hex_of(&a.sub(&q.mul(&b))),
                hex_of(&r),
                "a - q*b != r for a={} b={}",
                hex_of(&a),
                hex_of(&b)
            );
            assert_eq!(
                r.cmp_unsigned(&b),
                std::cmp::Ordering::Less,
                "remainder not less than divisor for a={} b={}",
                hex_of(&a),
                hex_of(&b)
            );
        }
    }

    #[test]
    fn modular_exponentiation_matches_known_values() {
        // 2^10 mod 1000 = 24.
        assert_eq!(hex_of(&big("02").mod_pow(&big("0a"), &big("03e8"))), "18");
        // 3^5 mod 7 = 5.
        assert_eq!(hex_of(&big("03").mod_pow(&big("05"), &big("07"))), "05");
        // Anything^0 = 1.
        assert_eq!(
            hex_of(&big("abcdef").mod_pow(&BigUint::zero(), &big("0101"))),
            "01"
        );
        // Fermat: 2^(p-1) = 1 mod p for the prime 65537.
        assert_eq!(
            hex_of(&big("02").mod_pow(&big("010000"), &big("010001"))),
            "01"
        );
    }

    /// The end-to-end property the key exchange depends on: both sides of a
    /// Diffie-Hellman over group 14 must land on the same secret. This runs the
    /// full 2048-bit modular exponentiation, which is the only exercise the
    /// carry paths get at their real width.
    #[test]
    fn diffie_hellman_over_group14_agrees_from_both_sides() {
        let p = BigUint::from_bytes_be(&hex(DH_GROUP14_P_HEX));
        let g = big("02");

        // Fixed exponents: this checks the arithmetic, not the CSPRNG.
        let a = big("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        let b = big("fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543211");

        let big_a = g.mod_pow(&a, &p); // client public
        let big_b = g.mod_pow(&b, &p); // server public

        let secret_client = big_b.mod_pow(&a, &p);
        let secret_server = big_a.mod_pow(&b, &p);

        assert_eq!(hex_of(&secret_client), hex_of(&secret_server));
        // A shared secret that came out zero or one would mean the whole
        // exponentiation collapsed rather than agreed.
        assert!(secret_client.bit_length() > 1024);
    }
    // ========================================================================
    // Diffie-Hellman group 14 (RFC 3526 section 3)
    // ========================================================================

    /// The prime is a transcription of 512 hex digits from an RFC, which is
    /// the kind of constant that is either exactly right or silently wrong.
    /// Checking length and both ends catches a truncated or shifted paste; the
    /// property below catches a digit that changed the value.
    #[test]
    fn the_group14_prime_is_the_published_2048_bit_value() {
        let bytes = dh_group14_prime_bytes();
        assert_eq!(bytes.len(), 256);
        // Both ends are eight 0xff bytes, so a paste that lost or gained a
        // line shows up here rather than as a handshake that never agrees.
        assert_eq!(bytes.first_chunk::<8>(), Some(&[0xff; 8]));
        assert_eq!(bytes.last_chunk::<8>(), Some(&[0xff; 8]));
        assert_eq!(dh_group14_prime().bit_length(), 2048);
        assert_eq!(DH_GROUP14_P_HEX.len(), 512);
        assert_eq!(DH_GROUP14_G, 2);
    }

    /// A safe prime `p = 2q + 1` has `2^((p-1)/2) = 1 mod p` exactly when 2 is
    /// a quadratic residue, which for group 14 it is -- RFC 3526's groups are
    /// chosen so that the generator 2 spans the order-`q` subgroup. A prime
    /// with any digit altered would almost certainly be composite and fail
    /// this, so it is a real check on the transcription and not a restatement
    /// of it.
    #[test]
    fn two_generates_the_prime_order_subgroup_of_group14() {
        let p = dh_group14_prime();
        let g = BigUint::from_bytes_be(&[DH_GROUP14_G]);
        // (p - 1) / 2 = q, the subgroup order.
        let (q, rem) = p
            .sub(&BigUint::one())
            .div_rem(&BigUint::from_bytes_be(&[2]));
        assert!(rem.is_zero(), "p - 1 must be even");
        assert_eq!(to_hex(&g.mod_pow(&q, &p).to_bytes_be()), "01");
    }

    // ========================================================================
    // The transport, and the buffer under the packet layer
    // ========================================================================
    //
    // The framing itself is `PacketCodec`'s and is tested above. What is tested
    // here is the plumbing around it, which both binaries used to carry
    // privately and separately: `StreamBuffer` must hand the codec every unread
    // byte, and must drop exactly the bytes the codec reports having used.
    // Getting either wrong desynchronises the stream permanently -- and neither
    // copy was ever compared against the other's.

    /// A transport that accepts at most `chunk` bytes per `send`.
    ///
    /// A real socket does this whenever its send buffer is nearly full, and a
    /// caller that ignored the short count would truncate a packet. That is
    /// what [`Transport::send_all`] exists to prevent, so it needs a transport
    /// that actually short-writes; `MemoryTransport` never does.
    struct Trickle {
        chunk: usize,
        written: Vec<u8>,
    }

    impl Transport for Trickle {
        fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
            let n = data.len().min(self.chunk);
            self.written
                .extend_from_slice(data.get(..n).unwrap_or_default());
            Ok(n)
        }

        fn recv<'b>(&mut self, buf: &'b mut [u8]) -> Result<&'b [u8], TransportError> {
            buf.get(..0).ok_or(TransportError::Recv)
        }

        fn readable(&self) -> bool {
            true
        }

        fn close(&mut self) {}
    }

    #[test]
    fn what_one_end_writes_the_other_reads() {
        let (mut a, mut b) = memory_pair();
        a.send_all(b"one").expect("the far end is alive");
        b.send_all(b"two").expect("the far end is alive");
        let mut buf = [0u8; 8];
        assert_eq!(b.recv(&mut buf).expect("bytes are waiting"), b"one");
        assert_eq!(a.recv(&mut buf).expect("bytes are waiting"), b"two");
    }

    #[test]
    fn a_read_takes_only_what_fits_and_leaves_the_rest() {
        // A stream has no message boundaries, so a buffer shorter than what
        // arrived must not lose the tail: that tail is the front of the next
        // packet.
        let (mut a, mut b) = memory_pair();
        a.send_all(b"abcdef").expect("the far end is alive");
        let mut small = [0u8; 4];
        assert_eq!(b.recv(&mut small).expect("bytes are waiting"), b"abcd");
        assert_eq!(b.recv(&mut small).expect("bytes are waiting"), b"ef");
    }

    #[test]
    fn a_hung_up_peer_reads_as_a_close_rather_than_a_wait() {
        // The property that makes this transport usable as a stand-in at all:
        // a read with no writer left must return. Without it, every test that
        // forgot to close an end would hang rather than fail, and a hang
        // reports nothing.
        let (mut near, far) = memory_pair();
        drop(far);
        let mut buf = [0u8; 8];
        assert_eq!(near.recv(&mut buf), Ok(&[][..]));
        let mut stream = StreamBuffer::new();
        assert_eq!(stream.fill_once(&mut near), Err(TransportError::Closed));
    }

    #[test]
    fn bytes_written_before_a_hangup_are_still_delivered() {
        // Closing does not discard what was already sent. A server that writes
        // SSH_MSG_DISCONNECT and immediately hangs up must still be heard, or
        // the client would report a dropped connection instead of the reason.
        let (mut near, mut far) = memory_pair();
        far.send_all(b"goodbye").expect("the near end is alive");
        drop(far);
        let mut buf = [0u8; 16];
        assert_eq!(near.recv(&mut buf).expect("queued bytes"), b"goodbye");
        assert_eq!(near.recv(&mut buf), Ok(&[][..]));
    }

    #[test]
    fn readable_is_false_only_while_a_live_peer_has_sent_nothing() {
        // The session pump reads only when this says yes, so a wrong `true`
        // blocks the shell's output behind the user's next keystroke, and a
        // wrong `false` after a hangup spins instead of ending the session.
        let (near, mut far) = memory_pair();
        assert!(!near.readable(), "nothing has been sent yet");
        far.send_all(b"x").expect("the near end is alive");
        assert!(near.readable(), "a byte is waiting");
        drop(far);
        assert!(near.readable(), "a hangup is something to read");
    }

    #[test]
    fn send_all_keeps_going_until_every_byte_is_gone() {
        let mut trickle = Trickle {
            chunk: 3,
            written: Vec::new(),
        };
        trickle
            .send_all(b"0123456789")
            .expect("every call makes progress");
        assert_eq!(trickle.written, b"0123456789");
    }

    #[test]
    fn send_all_gives_up_rather_than_spinning_on_a_transport_that_takes_nothing() {
        let mut trickle = Trickle {
            chunk: 0,
            written: Vec::new(),
        };
        assert_eq!(trickle.send_all(b"x"), Err(TransportError::Send));
    }

    /// Feed a buffer one byte at a time; report when a packet first appears.
    ///
    /// This is the session pump's exact shape -- decode what is buffered, read
    /// once, try again -- with each read carrying a single byte. That is a
    /// stream's worst case, and the one a buffer that guessed at lengths rather
    /// than asking the codec would get wrong.
    fn feed_byte_at_a_time(wire: &[u8]) -> (usize, Vec<u8>) {
        let mut codec = PacketCodec::new();
        let mut buf = StreamBuffer::new();
        let (mut near, mut far) = memory_pair();
        for (i, byte) in wire.iter().enumerate() {
            far.send_all(&[*byte]).expect("the near end is alive");
            buf.fill_once(&mut near).expect("a byte is waiting");
            match codec.decode(buf.unread()) {
                Ok(Some((payload, consumed))) => {
                    buf.advance(consumed);
                    return (i.saturating_add(1), payload);
                }
                Ok(None) => {}
                Err(e) => panic!("framing error at byte {i}: {e}"),
            }
        }
        panic!("packet never completed after {} bytes", wire.len());
    }

    #[test]
    fn a_packet_is_produced_exactly_when_its_last_byte_lands() {
        let mut codec = PacketCodec::new();
        let payload = b"hello ssh".to_vec();
        let wire = encode(&mut codec, &payload);

        let (consumed, parsed) = feed_byte_at_a_time(&wire);
        assert_eq!(parsed, payload);
        // Neither early (which would mean parsing a partial packet) nor late.
        assert_eq!(consumed, wire.len());
    }

    #[test]
    fn an_empty_buffer_asks_for_more_rather_than_failing() {
        let mut codec = PacketCodec::new();
        let buf = StreamBuffer::new();
        assert!(matches!(codec.decode(buf.unread()), Ok(None)));
    }

    #[test]
    fn a_declined_partial_packet_is_still_in_the_buffer() {
        // The bytes a `None` declined are the start of the packet the next call
        // will parse. Advancing past them would desynchronise the stream.
        let mut sender = PacketCodec::new();
        let wire = encode(&mut sender, b"payload");
        let (head, tail) = wire.split_at(wire.len().saturating_sub(1));
        let (mut near, mut far) = memory_pair();
        let mut buf = StreamBuffer::new();
        far.send_all(head).expect("the near end is alive");
        buf.fill_once(&mut near).expect("bytes are waiting");

        let mut reader = PacketCodec::new();
        assert!(matches!(reader.decode(buf.unread()), Ok(None)));
        assert_eq!(buf.unread().len(), head.len());

        far.send_all(tail).expect("the near end is alive");
        buf.fill_once(&mut near).expect("the last byte is waiting");
        let (payload, consumed) = reader
            .decode(buf.unread())
            .expect("framing")
            .expect("packet");
        buf.advance(consumed);
        assert_eq!(payload, b"payload");
        assert!(buf.unread().is_empty(), "the packet was not consumed");
    }

    #[test]
    fn several_packets_from_one_read_are_drained_one_at_a_time() {
        // A single read can carry several SSH packets. The session pump drains
        // them all before sleeping, so the buffer must give up each in turn
        // rather than asking for more bytes after the first.
        let mut sender = PacketCodec::new();
        let mut reader = PacketCodec::new();
        let mut wire = encode(&mut sender, b"first");
        wire.extend_from_slice(&encode(&mut sender, b"second"));
        let (mut near, mut far) = memory_pair();
        let mut buf = StreamBuffer::new();
        far.send_all(&wire).expect("the near end is alive");
        buf.fill_once(&mut near).expect("bytes are waiting");

        let mut drained = Vec::new();
        while let Some((payload, consumed)) = reader.decode(buf.unread()).expect("framing") {
            buf.advance(consumed);
            drained.push(payload);
        }
        assert_eq!(drained, vec![b"first".to_vec(), b"second".to_vec()]);
        assert!(buf.unread().is_empty());
    }

    #[test]
    fn a_long_session_does_not_grow_the_buffer_without_bound() {
        // `pos` only moves forward, so without the reclaim in `fill_once` a
        // session that ran for hours would still be holding every byte it had
        // ever read. The threshold is what keeps that reclaim from being a
        // memmove per packet, so what is checked here is that `data` stops
        // growing -- not that it never grows.
        let (mut near, mut far) = memory_pair();
        let mut buf = StreamBuffer::new();
        let chunk = vec![0xa5u8; 1024];
        for _ in 0..16 {
            far.send_all(&chunk).expect("the near end is alive");
            buf.fill_once(&mut near).expect("bytes are waiting");
            buf.advance(chunk.len());
            assert!(buf.unread().is_empty());
        }
        assert!(
            buf.data.len() <= STREAM_COMPACT_THRESHOLD + chunk.len(),
            "consumed bytes were never reclaimed: {} still held",
            buf.data.len()
        );
    }

    #[test]
    fn the_reclaim_keeps_the_unread_bytes_it_has_not_been_told_to_drop() {
        // The reclaim moves `pos` as well as the bytes. Draining without
        // resetting it would silently skip the front of the next packet, which
        // is the same desynchronisation as advancing too far -- and would show
        // up only after a session had run long enough to cross the threshold.
        let (mut near, mut far) = memory_pair();
        let mut buf = StreamBuffer::new();
        let filler = vec![0x11u8; STREAM_COMPACT_THRESHOLD + 1];
        far.send_all(&filler).expect("the near end is alive");
        buf.fill_once(&mut near).expect("bytes are waiting");
        buf.advance(filler.len());

        far.send_all(b"kept").expect("the near end is alive");
        // This fill is the one that crosses the threshold and reclaims.
        buf.fill_once(&mut near).expect("bytes are waiting");
        assert_eq!(buf.unread(), b"kept");
    }

    #[test]
    fn advancing_past_the_end_empties_the_buffer_rather_than_panicking() {
        // `advance` is only ever given a length the codec just reported, so a
        // correct caller cannot reach this -- but the buffer is exactly where a
        // framing bug would otherwise surface as a panic in a server's session
        // loop, which is a denial of service rather than a dropped connection.
        let mut buf = StreamBuffer::new();
        buf.advance(9999);
        assert!(buf.unread().is_empty());
    }

    // ---- The randomness underneath the protocol ----

    #[test]
    fn the_default_secret_source_is_the_kernel_and_not_a_stand_in() {
        // Asserting the *pointer*, not the behaviour, is the point: the risk
        // this guards is a binary that ships with a test source wired in, which
        // no test of "the bytes look random" would ever catch.
        let expected: SecretSource = randrange::fill_secret;
        assert!(
            core::ptr::fn_addr_eq(KERNEL_SECRETS, expected),
            "the default source must be randrange::fill_secret itself"
        );
    }

    #[test]
    fn the_kernel_source_answers_for_no_bytes_on_every_platform() {
        // Asking for nothing needs no entropy, so this holds on the target and
        // on a host that refuses -- which makes it the one assertion about the
        // real source that is not really an assertion about the machine.
        assert!(KERNEL_SECRETS(&mut []).is_ok());
    }

    /// A source that refuses, the way the kernel does when it cannot answer.
    fn refuses(_out: &mut [u8]) -> Result<(), randrange::EntropyError> {
        Err(randrange::EntropyError::Unavailable)
    }

    /// A source that answers, predictably, so a handshake is reproducible.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the Result is the SecretSource signature, not this function's choice"
    )]
    fn counts_up(out: &mut [u8]) -> Result<(), randrange::EntropyError> {
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::try_from(i % 251).unwrap_or(0);
        }
        Ok(())
    }

    #[test]
    fn a_substituted_source_is_the_one_that_gets_asked() {
        // The whole value of the seam is that a caller holding a `SecretSource`
        // reaches the substitute and not the kernel. If this ever stopped being
        // true, every deterministic handshake test would silently start
        // depending on the host's entropy again.
        let mut buf = [0xFFu8; 4];
        let source: SecretSource = counts_up;
        source(&mut buf).expect("the stand-in answers");
        assert_eq!(buf, [0, 1, 2, 3]);

        let refusing: SecretSource = refuses;
        assert_eq!(
            refusing(&mut buf),
            Err(randrange::EntropyError::Unavailable)
        );
        // A refusal must leave the caller's buffer alone rather than
        // half-filling it: a partly-written secret that a caller ignored the
        // error on is worse than an untouched one, because it looks plausible.
        assert_eq!(buf, [0, 1, 2, 3]);
    }

    // ---- base64 (RFC 4648 §4) ----

    #[test]
    fn the_rfc_4648_test_vectors_encode_as_the_rfc_says() {
        // §10. Pinning the published vectors rather than only round-tripping
        // is the point: a round-trip passes for any self-consistent alphabet,
        // including a wrong one, and these strings are read by OpenSSH.
        for (plain, padded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode_padded(plain.as_bytes()), padded, "{plain:?}");
            assert_eq!(
                base64_encode(plain.as_bytes()),
                padded.trim_end_matches('='),
                "{plain:?}"
            );
        }
    }

    #[test]
    fn the_two_encoders_differ_only_in_padding() {
        // This is the property that was false while `ssh` and `sshd` each had
        // a function named `base64_encode` and only one of them padded. Now
        // the difference is named, and it is exactly this.
        for len in 0..40usize {
            let data: Vec<u8> = (0..len)
                .map(|i| u8::try_from(i % 251).unwrap_or(0))
                .collect();
            let unpadded = base64_encode(&data);
            let padded = base64_encode_padded(&data);
            assert_eq!(padded.trim_end_matches('='), unpadded, "len {len}");
            assert!(padded.len().is_multiple_of(4), "len {len}: {padded}");
        }
    }

    #[test]
    fn both_padding_forms_decode_to_the_same_bytes() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len)
                .map(|i| u8::try_from(i % 251).unwrap_or(0))
                .collect();
            assert_eq!(
                base64_decode(base64_encode(&data).as_bytes()),
                Ok(data.clone()),
                "unpadded, len {len}"
            );
            assert_eq!(
                base64_decode(base64_encode_padded(&data).as_bytes()),
                Ok(data),
                "padded, len {len}"
            );
        }
    }

    #[test]
    fn line_wrapping_survives_a_round_trip() {
        // Every producer of these strings wraps them: PEM bodies at 70 columns,
        // `known_hosts` not at all, and a hand-edited file however the editor
        // felt. A decoder that choked on the newline would reject the files
        // this crate exists to let two programs exchange.
        let data: Vec<u8> = (0..96u8).collect();
        let mut wrapped = String::new();
        for line in base64_encode_padded(&data).as_bytes().chunks(20) {
            wrapped.push_str(&String::from_utf8_lossy(line));
            wrapped.push_str("\r\n");
        }
        assert_eq!(base64_decode(wrapped.as_bytes()), Ok(data));
    }

    #[test]
    fn a_corrupt_character_is_an_error_and_not_a_shorter_key() {
        // The bug this replaces, stated as a test. `sshd`'s decoder stopped at
        // the first unrecognised character and returned what it had, so a host
        // key file with one bad byte loaded as a *valid key of the wrong
        // length* rather than failing. Truncation that reports success is
        // indistinguishable from a legitimate shorter input.
        let good = base64_encode_padded(&[0u8; 64]);
        let mut bad = good.clone();
        bad.replace_range(30..31, "!");

        assert_eq!(base64_decode(good.as_bytes()).map(|v| v.len()), Ok(64));
        assert_eq!(base64_decode(bad.as_bytes()), Err(Base64Error::Invalid));
    }

    #[test]
    fn a_trailing_group_of_one_character_is_refused() {
        // No encoder emits it: a quartet encodes 1, 2 or 3 bytes, so a group of
        // one encodes nothing. Accepting it would mean two different strings
        // decoding to the same bytes, which for a key file is a second valid
        // spelling of the same secret.
        assert_eq!(base64_decode(b"Zm9vYmFyZ"), Err(Base64Error::Invalid));
        assert_eq!(base64_decode(b"Zm9vYmFy"), Ok(b"foobar".to_vec()));
    }

    #[test]
    fn the_url_safe_alphabet_is_not_quietly_accepted() {
        // RFC 4648 §5 swaps `+/` for `-_`. SSH uses §4, and a decoder that took
        // both would accept key files no OpenSSH tool wrote.
        assert_eq!(base64_decode(b"-_-_"), Err(Base64Error::Invalid));
        assert!(base64_decode(b"+/+/").is_ok());
    }

    // ---- the publickey signed blob (RFC 4252 §7) ----

    /// The signed blob is the exact byte sequence RFC 4252 §7 lays out.
    ///
    /// Built here a second time *without* [`ssh_string`], writing the length
    /// prefixes as literals. That is the point of the test: a round-trip
    /// through our own readers would confirm that our decoder undoes our
    /// encoder, which it does whatever order the fields are in. Only a
    /// separately-written expectation notices a swapped pair of strings — and
    /// a swap is exactly the defect that survives every internal check, since
    /// both fields are strings and both ends of a round trip would agree.
    ///
    /// Nothing on the wire carries this blob, so a fault in it does not
    /// surface as a decode error anywhere. It surfaces as a signature that
    /// does not verify, which is indistinguishable from a wrong key.
    #[test]
    fn the_signed_blob_is_the_byte_sequence_rfc_4252_lays_out() {
        let session_id = [0xAA_u8; 32];
        let got = pubkey_signed_blob(
            &session_id,
            b"alice",
            b"ssh-connection",
            b"ssh-ed25519",
            b"KEY",
        );

        let mut want: Vec<u8> = Vec::new();
        want.extend_from_slice(&[0, 0, 0, 32]); // string session identifier
        want.extend_from_slice(&session_id);
        want.push(50); // byte SSH_MSG_USERAUTH_REQUEST
        want.extend_from_slice(&[0, 0, 0, 5]); // string user name
        want.extend_from_slice(b"alice");
        want.extend_from_slice(&[0, 0, 0, 14]); // string service name
        want.extend_from_slice(b"ssh-connection");
        want.extend_from_slice(&[0, 0, 0, 9]); // string "publickey"
        want.extend_from_slice(b"publickey");
        want.push(1); // boolean TRUE
        want.extend_from_slice(&[0, 0, 0, 11]); // string algorithm name
        want.extend_from_slice(b"ssh-ed25519");
        want.extend_from_slice(&[0, 0, 0, 3]); // string public key blob
        want.extend_from_slice(b"KEY");

        assert_eq!(got, want);
    }

    /// Every field the blob binds actually changes it.
    ///
    /// The bindings are the entire security argument for the construction: the
    /// session identifier is what stops a captured signature being replayed on
    /// another connection, and the user name is what stops one offered for
    /// `alice` being presented as `root`. A field that was written into the
    /// blob but not *varied* by its input — a placeholder, a fixed string, an
    /// argument shadowed by a constant — would leave the signature valid
    /// across the boundary it is supposed to bind, and every round-trip test
    /// would still pass. This is the same defect class as the server hashing a
    /// fixed `"SSH-2.0-client"` into the exchange hash.
    #[test]
    fn changing_any_bound_field_changes_the_blob() {
        let base = pubkey_signed_blob(&[0; 32], b"alice", b"ssh-connection", b"ssh-ed25519", b"K");
        let variants = [
            (
                "session id",
                pubkey_signed_blob(&[1; 32], b"alice", b"ssh-connection", b"ssh-ed25519", b"K"),
            ),
            (
                "user name",
                pubkey_signed_blob(&[0; 32], b"root", b"ssh-connection", b"ssh-ed25519", b"K"),
            ),
            (
                "service name",
                pubkey_signed_blob(&[0; 32], b"alice", b"ssh-userauth", b"ssh-ed25519", b"K"),
            ),
            (
                "algorithm",
                pubkey_signed_blob(&[0; 32], b"alice", b"ssh-connection", b"ssh-rsa", b"K"),
            ),
            (
                "key blob",
                pubkey_signed_blob(&[0; 32], b"alice", b"ssh-connection", b"ssh-ed25519", b"J"),
            ),
        ];
        for (field, blob) in variants {
            assert_ne!(
                base, blob,
                "changing the {field} left the signed blob identical, so a \
                 signature would carry across a boundary it is meant to bind"
            );
        }
    }

    /// A string field cannot be smuggled across a boundary by its neighbour.
    ///
    /// Length-prefixed framing is what makes this true, and this test is what
    /// says so out loud: `alice` + service `x` and `alicex` + service `` are
    /// the same bytes under concatenation, and differ only because each string
    /// carries its own length. An encoder that ever dropped a prefix — for a
    /// field it thought was fixed-width, say — would let a signature for one
    /// account verify for another.
    #[test]
    fn a_field_cannot_borrow_bytes_from_the_next_one() {
        let a = pubkey_signed_blob(&[0; 32], b"alice", b"x", b"ssh-ed25519", b"K");
        let b = pubkey_signed_blob(&[0; 32], b"alicex", b"", b"ssh-ed25519", b"K");
        assert_ne!(a, b);
    }

    // ---- the OpenSSH private key container (PROTOCOL.key) ----

    /// RFC 8032 §7.1 test vector 1: a seed and the public key it derives.
    ///
    /// A published pair rather than an arbitrary one, so a reader can check
    /// the halves belong together without running any of our code.
    const TEST_SEED: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];
    const TEST_PUBLIC: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];

    #[test]
    fn a_key_written_here_is_a_key_read_here() {
        let text =
            encode_openssh_private_key(&TEST_SEED, &TEST_PUBLIC, "host@slateos", 0x1234_5678);
        assert_eq!(
            decode_openssh_private_key(&text),
            Ok(OpensshPrivateKey {
                seed: TEST_SEED,
                public: TEST_PUBLIC,
                comment: "host@slateos".to_string(),
            })
        );
    }

    #[test]
    fn the_file_is_the_shape_openssh_writes() {
        // The band and the 70-column body are what make the file loadable by
        // the real `ssh-keygen -lf`, which is the only external check this
        // project has on the container. A test that only round-trips through
        // our own codec would pass for any self-consistent format -- which is
        // exactly how `ssh-keygen` came to write a container of its own
        // invention that our own `sshd` could not read.
        let text = encode_openssh_private_key(&TEST_SEED, &TEST_PUBLIC, "c", 1);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("-----BEGIN OPENSSH PRIVATE KEY-----"));
        let body: Vec<&str> = lines
            .clone()
            .take_while(|l| !l.starts_with("-----"))
            .collect();
        assert!(!body.is_empty(), "no body between the bands");
        for line in &body {
            assert!(line.len() <= 70, "body line longer than 70 columns: {line}");
        }
        assert_eq!(
            text.lines().last(),
            Some("-----END OPENSSH PRIVATE KEY-----")
        );
        assert!(text.ends_with('\n'), "the footer must end with a newline");

        // The magic is the first thing in the decoded body, so a file that
        // decodes at all is one another implementation will recognise.
        let raw = base64_decode(body.concat().as_bytes()).expect("body is base64");
        assert!(raw.starts_with(b"openssh-key-v1\0"));
    }

    #[test]
    fn the_stored_public_key_comes_back_rather_than_being_checked() {
        // The codec must *not* silently correct a file whose halves disagree:
        // returning the stored public key is what lets the caller notice. If
        // this ever started re-deriving it, a corrupted file would load as a
        // working key with a different identity, which is the failure mode
        // that makes clients report a changed host key.
        let mut wrong = TEST_PUBLIC;
        wrong[0] ^= 0xFF;
        let text = encode_openssh_private_key(&TEST_SEED, &wrong, "c", 1);
        let parsed = decode_openssh_private_key(&text).expect("still well-formed");
        assert_eq!(parsed.seed, TEST_SEED);
        assert_eq!(
            parsed.public, wrong,
            "the file's own public key, unmodified"
        );
    }

    #[test]
    fn an_encrypted_key_is_refused_by_name() {
        // Hand-built, because the encoder here cannot produce one. The message
        // has to name the cipher: "re-create it with an empty passphrase" is
        // only actionable if the operator can see what it was encrypted with.
        let mut raw = Vec::new();
        raw.extend_from_slice(b"openssh-key-v1\0");
        raw.extend_from_slice(&ssh_string(b"aes256-ctr"));
        raw.extend_from_slice(&ssh_string(b"bcrypt"));
        raw.extend_from_slice(&ssh_string(b""));
        let text = format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n",
            base64_encode_padded(&raw)
        );
        assert_eq!(
            decode_openssh_private_key(&text),
            Err(PrivateKeyError::Encrypted {
                cipher: "aes256-ctr".to_string()
            })
        );
    }

    #[test]
    fn a_file_that_is_not_this_format_is_refused_rather_than_guessed() {
        // Each of these once had a path through some parser in this tree that
        // produced a key anyway. `sshd`'s old loader hashed the first line and
        // used the digest as a seed, so a wrong `-h` argument started the
        // daemon with a host key unrelated to the file named.
        for (name, text) in [
            ("empty", String::new()),
            ("a public key line", "ssh-ed25519 AAAAC3Nz c\n".to_string()),
            (
                "ssh-keygen's homebrew band",
                "-----BEGIN ED25519 PRIVATE KEY-----\nAAAA\n-----END ED25519 PRIVATE KEY-----\n"
                    .to_string(),
            ),
            (
                "the right band around the wrong bytes",
                format!(
                    "-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n",
                    base64_encode_padded(b"not a key at all")
                ),
            ),
        ] {
            assert!(
                decode_openssh_private_key(&text).is_err(),
                "{name} was accepted"
            );
        }
    }

    #[test]
    fn a_corrupt_body_fails_instead_of_loading_a_shorter_key() {
        // This is the pair of bugs that motivated the shared copy, as one
        // test: the old decoder truncated at the first bad character, and the
        // container parser then read whatever that left. Either the base64 or
        // the structure must object -- what must not happen is a successful
        // load of a *different* key.
        let good = encode_openssh_private_key(&TEST_SEED, &TEST_PUBLIC, "c", 1);
        for pos in [40usize, 80, 120] {
            let mut bad = good.clone();
            let body_start = bad.find('\n').expect("has a band") + 1;
            let at = body_start + pos;
            if at >= bad.len() {
                continue;
            }
            bad.replace_range(at..=at, "!");
            match decode_openssh_private_key(&bad) {
                Err(_) => {}
                Ok(k) => panic!("corruption at {pos} loaded as a key: {k:?}"),
            }
        }
    }

    #[test]
    fn the_two_checkints_must_agree() {
        // The one integrity check the container gives us for free. Build it by
        // hand, since the encoder always writes them equal.
        let mut private = Vec::new();
        private.extend_from_slice(&ssh_u32(1));
        private.extend_from_slice(&ssh_u32(2));
        private.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        private.extend_from_slice(&ssh_string(&TEST_PUBLIC));
        let mut secret = TEST_SEED.to_vec();
        secret.extend_from_slice(&TEST_PUBLIC);
        private.extend_from_slice(&ssh_string(&secret));
        private.extend_from_slice(&ssh_string(b"c"));

        let mut raw = Vec::new();
        raw.extend_from_slice(b"openssh-key-v1\0");
        raw.extend_from_slice(&ssh_string(b"none"));
        raw.extend_from_slice(&ssh_string(b"none"));
        raw.extend_from_slice(&ssh_string(b""));
        raw.extend_from_slice(&ssh_u32(1));
        raw.extend_from_slice(&ssh_string(&ed25519_public_blob(&TEST_PUBLIC)));
        raw.extend_from_slice(&ssh_string(&private));

        let text = format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n",
            base64_encode_padded(&raw)
        );
        assert_eq!(
            decode_openssh_private_key(&text),
            Err(PrivateKeyError::CheckintMismatch)
        );
    }

    #[test]
    fn an_empty_comment_survives_the_round_trip() {
        // `ssh-keygen -C ""` is legal, and the field is always written even
        // when empty -- so a missing comment is a truncated file, not an
        // absent label, and the two must not read the same.
        let text = encode_openssh_private_key(&TEST_SEED, &TEST_PUBLIC, "", 7);
        let parsed = decode_openssh_private_key(&text).expect("round trips");
        assert_eq!(parsed.comment, "");
    }

    #[test]
    fn the_public_blob_is_the_one_that_goes_on_a_known_hosts_line() {
        // The same bytes appear in four places (the container, authorized_keys,
        // known_hosts, and KEXDH_REPLY). This pins the encoding so those four
        // cannot drift.
        let blob = ed25519_public_blob(&TEST_PUBLIC);
        let (keytype, off) = read_ssh_string(&blob, 0).expect("type");
        assert_eq!(keytype, b"ssh-ed25519");
        let (key, off) = read_ssh_string(&blob, off).expect("key");
        assert_eq!(key, TEST_PUBLIC);
        assert_eq!(off, blob.len(), "nothing trails the two strings");
    }
}
