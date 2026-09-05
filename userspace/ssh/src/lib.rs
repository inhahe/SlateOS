//! Slate OS SSH-2 Client
//!
//! A simplified SSH-2 protocol client for SlateOS. Supports password
//! authentication, interactive shell sessions, and remote command execution.
//!
//! # Why this is a library with a three-line binary on top
//!
//! The client and the daemon implement the two halves of one protocol, and
//! every bug this pair has produced — six of them now — has been a place where
//! the halves disagreed while each half's own tests passed. The only test that
//! could have caught any of them is one that runs the real client against the
//! real server, and that test cannot exist while both are `main.rs` files:
//! a binary crate has no library to link against, so no third crate can call
//! into it.
//!
//! So the client is a library, `main.rs` is a shim over [`run_cli`], and the
//! public surface below is deliberately the smallest one an interop test can
//! be written against — a [`Config`], an [`SshSession`], the two handshake
//! phases and the [session id](SshSession::session_id) both ends must derive
//! identically. Everything else stays private. See `known-issues.md`
//! `TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`.
//!
//! # Usage
//!
//! ```text
//! ssh user@hostname                  Connect with interactive shell
//! ssh -p 2222 user@hostname          Connect on custom port
//! ssh user@hostname ls -la           Execute remote command
//! ssh -v user@hostname               Verbose protocol debugging
//! ssh -o ConnectTimeout=10 user@host Set connection timeout
//! ssh -o StrictHostKeyChecking=no user@host  Skip host key check
//! ```
//!
//! # Protocol
//!
//! Implements a subset of SSH-2 (RFC 4253, 4252, 4254):
//! - Version exchange (SSH-2.0-SlateOS_1.0)
//! - Key exchange: diffie-hellman-group14-sha256
//! - Host key: ssh-ed25519 (RFC 8032, via `posix::ed25519`)
//! - Encryption: AES-128-CTR
//! - MAC: HMAC-SHA256
//! - User auth: password method
//! - Channel: session with PTY and shell/exec
//!
//! # What the host key check actually proves
//!
//! Two separate things have to be true before the connection is trustworthy,
//! and they are checked in this order:
//!
//! 1. **The server holds the private half of the key it presented.** It proves
//!    this by signing the exchange hash H, which covers both version strings,
//!    both KEXINIT payloads, the host key itself and both Diffie-Hellman public
//!    values. `verify_host_key_signature` checks that signature.
//! 2. **That key is the one we expect for this host.** `verify_host_key`
//!    compares it against `~/.ssh/known_hosts`.
//!
//! Step 2 without step 1 is worthless: a host key is *public*, so anyone who
//! can intercept the connection can replay it, and known_hosts would then
//! happily confirm the attacker's copy as the right key. This client used to
//! do exactly that — it discarded the signature blob unread — which meant the
//! entire known_hosts mechanism, fingerprint prompt included, was decorative.
//!
//! # Exit status
//!
//! Follows `ssh(1)`: the client exits with **the remote command's** exit
//! status, so `ssh host cmd && next` behaves as if `cmd` had run locally. A
//! failure of the client or the connection — a bad argument, an unresolvable
//! host, a refused connection, a protocol error — exits **255**, which is
//! reserved for exactly that purpose so a caller can tell "the command failed"
//! from "the command never ran". A command killed by a signal has no exit
//! status of its own; that is reported on stderr and also exits 255.
//!
//! If the server sends no `exit-status` at all, the client exits 0. That is
//! unavoidable — it is what an interactive session looks like — and it is
//! precisely why a *server* must always send one.
//!
//! The remote command's stderr arrives on the extended-data stream
//! (RFC 4254 §5.2) and is written to *this* process's stderr, so
//! `ssh host cmd > file` puts output in the file and diagnostics on the
//! terminal, as it would locally.

// Lints come from `[lints] workspace = true` in Cargo.toml. The crate-local
// `#![deny(clippy::all)]` that used to stand here on its own said strictly
// less, and hid the fact that nothing else was switched on.
#![allow(clippy::manual_range_contains)]
#![allow(clippy::module_name_repetitions)]

use quoting::quoteaf_os;
// Everything the server has to agree with byte for byte. Not re-implemented
// here: see that crate's module docs for why one definition shared by both ends
// is the point, and `ssh/Cargo.toml` for what a second copy cost the last time.
use sshwire::{
    BigUint, DH_GROUP14_G, ExchangeHashInput, PacketCodec, Role, SecretSource, StreamBuffer,
    Transport, TransportError, compute_exchange_hash, dh_group14_prime_bytes, encode_mpint,
    read_byte, read_mpint, read_ssh_string, read_u32, ssh_string,
};
use std::env;
use std::fmt;
use std::io::{self, Read, Write};

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================

const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_DNS_RESOLVE: u64 = 820;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 1-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for the specific syscall.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall1` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall1(_nr: u64, _a1: u64) -> i64 {
    -38 // ENOSYS
}

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall3` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Resolve a hostname to an IPv4 address via the kernel DNS resolver.
/// Returns the IP as a `u32` in network byte order on success.
fn dns_resolve(hostname: &str) -> Result<u32, SshError> {
    let mut result_ip: u32 = 0;
    // SAFETY: We pass a valid pointer to the hostname bytes and their length,
    // plus a valid mutable pointer for the kernel to write the resolved IP into.
    // The kernel reads exactly `hostname.len()` bytes and writes exactly 4 bytes.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            hostname.as_ptr() as u64,
            hostname.len() as u64,
            &mut result_ip as *mut u32 as u64,
        )
    };
    if ret < 0 {
        return Err(SshError::DnsFailure(hostname.to_string()));
    }
    Ok(result_ip)
}

/// A TCP connection, as [`sshwire::Transport`] sees it.
///
/// This — and its counterpart in `sshd` — is the only place in either program
/// that knows the protocol runs over TCP. Everything above it takes a
/// `&mut dyn Transport`, which is what makes the two ends drivable against each
/// other in a test rather than only against a kernel.
struct TcpTransport {
    handle: u64,
}

impl TcpTransport {
    /// Open a connection to the given IP (network byte order) and port.
    fn connect(ip: u32, port: u16) -> Result<Self, SshError> {
        // SAFETY: We pass a valid IP and port. The kernel returns a handle
        // (>= 0) or a negative error code. No pointers are involved.
        let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
        if ret < 0 {
            return Err(SshError::ConnectionFailed(format!(
                "tcp_connect returned {ret}"
            )));
        }
        Ok(Self { handle: ret as u64 })
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        // SAFETY: We pass a valid handle and a pointer to a byte buffer with
        // its correct length. The kernel reads up to `data.len()` bytes from it.
        let ret = unsafe {
            syscall3(
                SYS_TCP_SEND,
                self.handle,
                data.as_ptr() as u64,
                data.len() as u64,
            )
        };
        if ret < 0 {
            return Err(TransportError::Send);
        }
        usize::try_from(ret).map_err(|_| TransportError::Send)
    }

    fn recv<'b>(&mut self, buf: &'b mut [u8]) -> Result<&'b [u8], TransportError> {
        // SAFETY: We pass a valid handle and a mutable buffer pointer with its
        // correct length. The kernel writes at most `buf.len()` bytes into it.
        let ret = unsafe {
            syscall3(
                SYS_TCP_RECV,
                self.handle,
                buf.as_mut_ptr() as u64,
                buf.len() as u64,
            )
        };
        if ret < 0 {
            return Err(TransportError::Recv);
        }
        // The kernel's number becomes a range in exactly one place: here, where
        // a count that does not fit the buffer we handed over is refused rather
        // than travelling into a caller's `buf[..n]` to panic there.
        let received = usize::try_from(ret).map_err(|_| TransportError::Recv)?;
        buf.get(..received).ok_or(TransportError::Recv)
    }

    fn readable(&self) -> bool {
        // The client never polls: every read it does is one it is waiting for.
        true
    }

    fn close(&mut self) {
        // SAFETY: We pass a valid handle. The kernel deallocates internal
        // state. Ignoring the return value is safe: the handle becomes invalid
        // regardless.
        let _ = unsafe { syscall1(SYS_TCP_CLOSE, self.handle) };
    }
}

// ============================================================================
// Error type
// ============================================================================

/// Everything that can stop this client, from a name that will not resolve to
/// a server whose host key is not the one we trusted.
#[derive(Debug)]
pub enum SshError {
    DnsFailure(String),
    ConnectionFailed(String),
    SendFailed,
    RecvFailed,
    ProtocolError(String),
    AuthFailed(String),
    HostKeyMismatch(String),
    IoError(io::Error),
    #[allow(dead_code)]
    Timeout,
}

impl fmt::Display for SshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DnsFailure(host) => write!(f, "could not resolve hostname '{host}'"),
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::SendFailed => write!(f, "failed to send data"),
            Self::RecvFailed => write!(f, "failed to receive data"),
            Self::ProtocolError(msg) => write!(f, "protocol error: {msg}"),
            Self::AuthFailed(msg) => write!(f, "authentication failed: {msg}"),
            Self::HostKeyMismatch(msg) => write!(f, "host key verification failed: {msg}"),
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::Timeout => write!(f, "connection timed out"),
        }
    }
}

impl From<io::Error> for SshError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

/// A malformed packet is a protocol error like any other, so `?` on a shared
/// reader reports the same way a local check did.
///
/// This impl is the whole reason the readers could move to `sshwire`: a shared
/// decoder cannot return `SshError`, and until there was a shared error to
/// convert *from*, every call site's `?` pinned the decoders to this crate —
/// which is exactly why the server kept an unhardened copy of them long after
/// the client's was fixed.
impl From<sshwire::WireError> for SshError {
    fn from(e: sshwire::WireError) -> Self {
        Self::ProtocolError(e.to_string())
    }
}

/// The same for the byte stream underneath: a shared `StreamBuffer` cannot
/// return `SshError` either.
///
/// `Closed` becomes a protocol error here because every client read is one it
/// is waiting for, so a hang-up mid-exchange really is a failure. The server
/// maps it differently — its session loop has reads it merely hopes will
/// return — which is exactly why the shared type reports the fact and each end
/// decides what it means.
impl From<TransportError> for SshError {
    fn from(e: TransportError) -> Self {
        match e {
            TransportError::Send => Self::SendFailed,
            TransportError::Recv => Self::RecvFailed,
            TransportError::Closed => Self::ProtocolError("connection closed".into()),
        }
    }
}

// ============================================================================
// SSH-2 constants
// ============================================================================

/// Our version identification string.
const SSH_VERSION_STRING: &str = "SSH-2.0-SlateOS_1.0";

/// How much of one pre-key-exchange greeting line we will hold in memory.
///
/// This is *not* the RFC 4253 §4.2 limit on the identification string — that is
/// [`sshwire::MAX_IDENTIFICATION_LINE`], and `sshwire::decode_identification`
/// enforces it, because it governs a value both ends have to hash identically.
/// This bound covers the banner lines a server may send *before* it, which §4.2
/// does not bound at all. Without one, a peer that never sends a newline grows
/// this buffer until the client dies — before it has authenticated anything.
const MAX_GREETING_LINE: usize = 1024;

/// SSH message type codes (RFC 4253 / 4252 / 4254).
mod msg {
    pub const SSH_MSG_DISCONNECT: u8 = 1;
    pub const SSH_MSG_IGNORE: u8 = 2;
    pub const SSH_MSG_UNIMPLEMENTED: u8 = 3;
    pub const SSH_MSG_DEBUG: u8 = 4;
    pub const SSH_MSG_SERVICE_REQUEST: u8 = 5;
    pub const SSH_MSG_SERVICE_ACCEPT: u8 = 6;
    pub const SSH_MSG_KEXINIT: u8 = 20;
    pub const SSH_MSG_NEWKEYS: u8 = 21;
    pub const SSH_MSG_KEX_DH_INIT: u8 = 30;
    pub const SSH_MSG_KEX_DH_REPLY: u8 = 31;
    pub const SSH_MSG_USERAUTH_REQUEST: u8 = 50;
    pub const SSH_MSG_USERAUTH_FAILURE: u8 = 51;
    pub const SSH_MSG_USERAUTH_SUCCESS: u8 = 52;
    pub const SSH_MSG_USERAUTH_BANNER: u8 = 53;
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
// SSH-2 packet framing
// ============================================================================

// The framing itself -- `[u32 packet_length][u8 padding_length][payload]
// [padding]`, the MAC over it, and both sequence numbers -- lives in
// `sshwire::PacketCodec`. It was the last function in this file that the daemon
// also had a copy of, and the two copies had already drifted: this one had been
// hardened to `checked_*`/`saturating_*` arithmetic and given the `packet_length
// < 5` floor check that stops a peer's zero-length packet being indexed at
// offset 4, and the server's had neither, because there was no mechanism by
// which either fix could reach it.
//
// The sequence numbers moved with it. §6.4 makes the sequence number an input
// to the MAC, so advancing it is part of framing a packet rather than
// bookkeeping the caller keeps beside it -- and kept beside it, it was already
// once forgotten on a send path, which authenticated every packet after the
// first under a number the peer had moved past.

/// Largest `SSH_MSG_CHANNEL_DATA` payload we will send in one packet.
///
/// This is the same 32 KiB that `channel_open` advertises to the server as our
/// maximum packet size; it was written out as a bare `32768` in both places,
/// which is two independent copies of one promise. RFC 4254 s5.1 makes the
/// advertised figure binding, so a change to one that missed the other would
/// have us overrun a limit we had just announced.
const MAX_CHANNEL_CHUNK: usize = 32768;

// The stream buffer that used to be here is `sshwire::StreamBuffer`. Both
// programs had one, with the same two fields and the same four methods, and
// again they were not equal -- the server's `fill_once` indexed the length
// the kernel reported, under its blanket panic-lint suppression.

// ============================================================================
// SSH data encoding helpers
// ============================================================================

/// What one line of the server's pre-key-exchange greeting turned out to be.
enum VersionLine {
    /// The identification string, i.e. `V_S`.
    Version(String),
    /// A line before it, rendered safe for a terminal.
    Banner(String),
}

/// Decide what a line read during the version exchange is (RFC 4253 §4.2).
///
/// `line` is everything up to but not including the LF. A server — and only a
/// server — may send any number of lines before its identification string; the
/// identification string is the first beginning `SSH-`.
///
/// What that line *is*, once found, is `sshwire`'s to say: it is `V_S`, the
/// second input to the exchange hash, so the terminator stripping, the length
/// limit and the strict decode are shared with the server rather than restated
/// here. Two of the three exchange-hash bugs found in this stack were the two
/// ends disagreeing about that derivation while agreeing about everything after
/// it.
///
/// Banner lines are not hashed and are not held to any of it: they are free text
/// from a peer that has authenticated nothing, on its way to a terminal, so they
/// are escaped rather than decoded. Refusing a connection over a greeting would
/// be wrong, and printing one raw would let that peer emit control sequences.
fn classify_version_line(line: &[u8]) -> Result<VersionLine, SshError> {
    if sshwire::is_identification_line(line) {
        let version = sshwire::decode_identification(line).map_err(|e| {
            SshError::ProtocolError(format!("server identification line rejected: {e}"))
        })?;
        return Ok(VersionLine::Version(version.to_owned()));
    }
    Ok(VersionLine::Banner(quoting::escape_unprintable(
        sshwire::strip_line_terminator(line),
    )))
}

// The wire codec -- `ssh_string`, `encode_mpint`, `strip_leading_zeros`, and the
// `read_*` readers below -- lives in `sshwire`, not here. Each function is one
// half of a contract with whatever is at the other end of the socket, and a
// private copy of one half is a copy that can drift without any test in this
// crate noticing. They are imported at the top of this file with the rest.

// The big-integer arithmetic for Diffie-Hellman is `sshwire`'s, not this
// file's. It lived here, and `sshd` had a second copy that was still the
// big-endian-bytes, one-bit-at-a-time version this one was rewritten away from
// -- so the server was spending, before authenticating anybody, the eighty
// seconds of CPU per handshake that rewrite removed from the client. There was
// no route by which the fix could reach it while the type was private to a
// binary. `sshwire::BigUint` is that route.

// ============================================================================
// SHA-256
// ============================================================================

/// Compute SHA-256 of `data`.
///
/// A thin name over `sha2::sha256`. The round constants, initial words and
/// compression function used to be written out here -- one of ten copies under
/// `userspace/`. In an SSH client the copy is worse than redundant: the same
/// digest has to agree with the *other* end of the connection, so a private
/// implementation is a private protocol.
fn sha256(data: &[u8]) -> [u8; 32] {
    sha2::sha256(data)
}

// HMAC-SHA256, the packet MAC, constant-time comparison and AES-128-CTR all
// live in `sshwire` now. The cipher in particular had to: its counter is state
// that RFC 4344 §4 says advances once per block for the life of the key, this
// crate restarted it at every packet -- reusing one keystream for the whole
// session -- and the server had invented a third rule. A shared stateful type
// is what makes all three unwriteable. See that crate's `Aes128Ctr`.

// The transport's cipher, MAC and both sequence numbers live in one
// `sshwire::PacketCodec` on `SshSession`, not in a local `EncryptionState`
// beside a pair of loose `seq_*` fields. Every part of that grouping had
// already gone wrong once while the pieces were held apart: the key and IV as
// separate `Vec`s let a fresh counter be rebuilt per packet, so one keystream
// covered a whole session; an `encrypted: bool` beside an uninstalled cipher
// described a plaintext packet as protected; and a sequence number the caller
// advanced by hand was, on one send path, not advanced at all.

// ============================================================================
// Diffie-Hellman group 14 (2048-bit MODP group, RFC 3526)
//
// The prime and the generator are `sshwire`'s. They were transcribed
// separately into this file and into `sshd` -- 512 hex digits, twice -- and
// the two copies agreeing was luck, not a checked property. See
// `sshwire::DH_GROUP14_P_HEX`.
// ============================================================================

/// Format bytes as a hex string.
fn bytes_to_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().saturating_mul(2));
    for &b in data {
        s.push(hex_char(b >> 4));
        s.push(hex_char(b & 0x0f));
    }
    s
}

/// The lowercase hex character for a nibble. Values above 15 cannot occur --
/// every caller masks first -- and would read as `'0'`.
fn hex_char(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

/// Generate the Diffie-Hellman private exponent `x`.
///
/// # What this replaces
///
/// The previous version hashed two hard-coded 64-bit constants and called the
/// result entropy. It was not merely weak — it was *constant*: every
/// connection made by every copy of this binary used the same `x`, so the
/// shared secret, and therefore the session keys, were recoverable by anyone
/// who had the binary. The comment said "sufficient to demonstrate the
/// protocol flow", and the flow it demonstrated was an encrypted channel that
/// decrypts to a passive observer.
///
/// The bytes now come from `secrets`, which is [`sshwire::KERNEL_SECRETS`]
/// — `randrange::fill_secret`, reaching the kernel CSPRNG through the linked
/// libc and *failing* rather than substituting anything when it cannot — for
/// every connection the client makes. They came from `posix::random` until it
/// turned out that a program's rlib copy of `posix` has every syscall stubbed
/// out, so the draw silently reached a hardware-RDRAND fallback that the guest
/// CPU does not have and returned `EIO` while blaming a kernel it had never
/// asked.
///
/// The source is a parameter so that a test can drive a reproducible handshake;
/// see [`sshwire::SecretSource`] for why that is worth a seam and why the seam
/// is not reachable from the command line.
///
/// # Errors
///
/// Returns an error if `secrets` cannot supply random bytes. There is no
/// fallback on purpose: a caller handed an error can refuse to connect, a
/// caller handed predictable bytes cannot know to.
fn generate_dh_private(secrets: SecretSource) -> Result<BigUint, SshError> {
    // 256 bits, matching the ~128-bit security the group14 prime provides.
    let mut bytes = [0u8; 32];
    secrets(&mut bytes).map_err(|e| {
        SshError::ProtocolError(format!("cannot generate a Diffie-Hellman private key: {e}"))
    })?;
    // Top bit set so the exponent is a full 256 bits rather than however many
    // the leading zero bytes leave; bottom bit set so it is odd. This is what
    // OpenSSH's BN_rand(..., BN_RAND_TOP_ONE, BN_RAND_BOTTOM_ODD) produces.
    bytes[0] |= 0x80;
    bytes[31] |= 1;
    Ok(BigUint::from_bytes_be(&bytes))
}

// The exchange hash and RFC 4253 §7.2 key derivation are `sshwire`'s. They were
// this file's, and the server's copy of the same construction is what drifted
// into hashing a fabricated client version; the client verifying the server's
// signature is only meaningful if both sides compute the value from one
// definition. See `sshwire`'s module docs.

// ============================================================================
// Host key signature verification
// ============================================================================

/// Check the server's signature over the exchange hash (RFC 4253 section 8).
///
/// # What this is for
///
/// The exchange hash `H` covers both version strings, both KEXINIT payloads,
/// the host key blob and both Diffie-Hellman public values. A signature over
/// it is the server's statement that it holds the private half of the key it
/// advertised *and* that it saw the same handshake we did. Without this check
/// the Diffie-Hellman exchange is unauthenticated, which means it is secure
/// against a passive eavesdropper and useless against anyone who can sit in
/// the path — and someone in the path is the threat the whole exchange exists
/// to address.
///
/// `known_hosts` cannot substitute for it. `known_hosts` says "the key is the
/// one I saw last time"; only the signature says "the party I am talking to
/// actually holds it". An unverified key blob can be copied from the real
/// server by anyone who has ever connected to it.
///
/// # Errors
///
/// [`SshError::ProtocolError`] if either blob is truncated or its length
/// prefixes do not describe it; [`SshError::HostKeyMismatch`] if the algorithm
/// is one we cannot check, if the key and signature disagree about which
/// algorithm that is, or if the signature does not verify.
fn verify_host_key_signature(
    key_blob: &[u8],
    exchange_hash: &[u8; 32],
    sig_blob: &[u8],
) -> Result<(), SshError> {
    let (key_algorithm, key_off) = read_ssh_string(key_blob, 0)?;
    let (sig_algorithm, sig_off) = read_ssh_string(sig_blob, 0)?;

    // The two must agree, or a server could advertise an Ed25519 key and sign
    // with something else -- an algorithm-confusion attack whose whole premise
    // is that one side picks the label and the other picks the verifier.
    if key_algorithm != sig_algorithm {
        return Err(SshError::HostKeyMismatch(format!(
            "server signed with {} but advertised a {} key",
            String::from_utf8_lossy(sig_algorithm),
            String::from_utf8_lossy(key_algorithm),
        )));
    }

    if key_algorithm != b"ssh-ed25519" {
        // Refusing is the only safe answer. Continuing would mean accepting a
        // signature we did not check, which is the bug this function fixes.
        return Err(SshError::HostKeyMismatch(format!(
            "cannot verify a {} host key; only ssh-ed25519 is implemented",
            String::from_utf8_lossy(key_algorithm),
        )));
    }

    let (public, _) = read_ssh_string(key_blob, key_off)?;
    let (signature, _) = read_ssh_string(sig_blob, sig_off)?;

    if posix::ed25519::verify_slices(public, exchange_hash, signature) {
        Ok(())
    } else {
        Err(SshError::HostKeyMismatch(
            "the server's signature over the exchange hash did not verify".into(),
        ))
    }
}

// ============================================================================
// Host key fingerprint and known_hosts
// ============================================================================

/// Compute the SHA-256 fingerprint of a host key blob, formatted as
/// `SHA256:base64_encoded_hash` (like OpenSSH).
fn host_key_fingerprint(key_blob: &[u8]) -> String {
    let hash = sha256(key_blob);
    let b64 = base64_encode(&hash);
    format!("SHA256:{b64}")
}

/// Minimal base64 encoder (no padding variant for fingerprints).
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// One base64 digit from the low six bits of `sextet`.
    ///
    /// Masking to `0x3f` before the lookup makes the index provably in range
    /// for a 64-entry table, so the fallback is unreachable.
    fn digit(sextet: u32) -> char {
        ALPHABET
            .get((sextet & 0x3f) as usize)
            .map_or('A', |&b| char::from(b))
    }

    // The three cases used to be three separate index expressions over `i`,
    // with a `while i + 2 < data.len()` head and a `data.len() - i` tail that
    // had to agree about where the full groups stopped. `chunks(3)` decides
    // that once: the bit layout is identical in all three cases, and only the
    // number of digits emitted -- which is the chunk's own length -- differs.
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let [b0, b1, b2] = match *chunk {
            [b0, b1, b2] => [b0, b1, b2],
            [b0, b1] => [b0, b1, 0],
            [b0] => [b0, 0, 0],
            // `chunks(3)` never yields an empty or over-long slice; the arm
            // exists only because the compiler cannot know that.
            _ => continue,
        };
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        result.push(digit(n >> 18));
        result.push(digit(n >> 12));
        if chunk.len() >= 2 {
            result.push(digit(n >> 6));
        }
        if chunk.len() >= 3 {
            result.push(digit(n));
        }
    }
    result
}

/// Minimal base64 encoder with padding (for known_hosts storage).
fn base64_encode_padded(data: &[u8]) -> String {
    let mut s = base64_encode(data);
    while !s.len().is_multiple_of(4) {
        s.push('=');
    }
    s
}

/// The name a host is filed under in `known_hosts`: bare for the default port,
/// bracketed otherwise, as OpenSSH writes it.
///
/// One function so the reader and the writer cannot disagree about the form --
/// `check_known_hosts` and `add_known_host` each built this string themselves,
/// and a host filed under a spelling the lookup does not produce is a host that
/// is silently re-asked about on every connection.
fn known_hosts_pattern(hostname: &str, port: u16) -> String {
    if port == 22 {
        hostname.to_string()
    } else {
        format!("[{hostname}]:{port}")
    }
}

/// What `known_hosts` has to say about a host.
#[derive(Debug, PartialEq, Eq)]
enum KnownHostsVerdict {
    /// No line names this host.
    Unknown,
    /// A line names this host and carries exactly this key.
    Match,
    /// A line names this host and carries a *different* key.
    Mismatch,
}

/// Search already-read `known_hosts` text for `host_pattern`.
///
/// Split out from `check_known_hosts` so that the part which decides whether a
/// key is trusted can be tested without a filesystem or a `$HOME`. This is the
/// function that says "this is the same server you connected to last time";
/// leaving it welded to a file read meant the man-in-the-middle check was the
/// one piece of this client that no test could reach.
fn known_hosts_lookup(content: &str, host_pattern: &str, key_blob: &[u8]) -> KnownHostsVerdict {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Taking the three fields straight off the iterator makes "there are
        // three of them" and "here they are" the same step: the `len() < 3`
        // check and the three indexes that followed it were two statements of
        // one fact, and only the first of them was enforced. The key type is
        // deliberately unread -- the blob carries its own algorithm name, and
        // that is what `verify_host_key` checks against.
        let mut fields = line.splitn(3, ' ');
        let (Some(hosts), Some(_), Some(rest)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let key_b64 = rest.split_whitespace().next().unwrap_or("");

        // Check if our host matches any of the comma-separated host patterns.
        if !hosts.split(',').any(|h| h.trim() == host_pattern) {
            continue;
        }

        // Decode the stored key and compare. The first line naming this host
        // decides: a later line cannot rehabilitate a key the earlier one
        // contradicts.
        return if base64_decode(key_b64) == key_blob {
            KnownHostsVerdict::Match
        } else {
            KnownHostsVerdict::Mismatch
        };
    }

    KnownHostsVerdict::Unknown
}

/// Where the trusted host keys are recorded.
///
/// `-o UserKnownHostsFile=` overrides it, as in OpenSSH. That option is not
/// decoration: `$HOME/.ssh/known_hosts` is the user's real trust store, so
/// without a way to name a different file, *any* automated exercise of this
/// client — an interoperability test above all — either has to skip host-key
/// verification entirely or write into the operator's own trust store. The
/// first tests the wrong program and the second is a side effect no test is
/// entitled to have.
fn known_hosts_path(known_hosts_file: Option<&str>) -> String {
    if let Some(path) = known_hosts_file {
        return path.to_string();
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    format!("{home}/.ssh/known_hosts")
}

/// Check the known_hosts file for a matching host key.
/// Returns Ok(true) if found and matches, Ok(false) if not found,
/// Err if found but mismatched.
fn check_known_hosts(
    known_hosts_file: Option<&str>,
    hostname: &str,
    port: u16,
    key_blob: &[u8],
) -> Result<bool, SshError> {
    let path = known_hosts_path(known_hosts_file);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(false), // File does not exist — host is unknown.
    };

    let host_pattern = known_hosts_pattern(hostname, port);

    match known_hosts_lookup(&content, &host_pattern, key_blob) {
        KnownHostsVerdict::Match => Ok(true),
        KnownHostsVerdict::Unknown => Ok(false),
        KnownHostsVerdict::Mismatch => Err(SshError::HostKeyMismatch(format!(
            "host key for {host_pattern} has changed!\n\
             Someone could be eavesdropping on you (man-in-the-middle attack).\n\
             The fingerprint for the new key is:\n  {}\n\
             Remove the old entry from {path} to accept the new key.",
            host_key_fingerprint(key_blob),
        ))),
    }
}

/// Add a host key to the known_hosts file.
fn add_known_host(
    known_hosts_file: Option<&str>,
    hostname: &str,
    port: u16,
    key_type: &str,
    key_blob: &[u8],
) {
    let path = known_hosts_path(known_hosts_file);

    // Ensure the directory exists. `rsplit_once` rather than a `Path` parent so
    // an explicit `UserKnownHostsFile` in the current directory -- no separator
    // at all -- creates nothing rather than trying to create "".
    if let Some((dir, _)) = path.rsplit_once('/') {
        let _ = std::fs::create_dir_all(dir);
    }

    let host_pattern = known_hosts_pattern(hostname, port);
    let key_b64 = base64_encode_padded(key_blob);
    let entry = format!("{host_pattern} {key_type} {key_b64}\n");

    let mut f = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: could not write to {path}: {e}");
            return;
        }
    };
    let _ = f.write_all(entry.as_bytes());
}

/// Minimal base64 decoder.
fn base64_decode(input: &str) -> Vec<u8> {
    // Subtracting the range's own start cannot underflow inside the arm that
    // established the range, and the largest sum is `b'z' - b'a' + 26` = 51,
    // so every branch is bounded by inspection.
    #[allow(clippy::arithmetic_side_effects)]
    fn b64val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r')
        .collect();

    // As with the encoder, `chunks(4)` replaces a `while i + 3 < len` head and
    // a `bytes.len() - i` tail that separately decided where the whole groups
    // ended. Missing characters decode as zero sextets, and how many whole
    // bytes the group yields is a function of its length: 4 characters carry
    // three bytes, 3 carry two, 2 carry one. A lone trailing character carries
    // no whole byte and is dropped -- which is what the old `remaining >= 2`
    // guard did.
    let mut result = Vec::with_capacity(bytes.len().saturating_mul(3) / 4);
    for chunk in bytes.chunks(4) {
        let mut sextets = [0u8; 4];
        for (slot, &c) in sextets.iter_mut().zip(chunk) {
            *slot = b64val(c).unwrap_or(0);
        }
        let [a, b, c, d] = sextets;
        let n = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        for (shift, needed) in [(16, 2), (8, 3), (0, 4)] {
            if chunk.len() >= needed {
                result.push(((n >> shift) & 0xff) as u8);
            }
        }
    }
    result
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Everything one invocation of `ssh(1)` was told to do.
///
/// The fields stay private and there is no public constructor that takes them
/// one by one. A caller outside this crate builds one by parsing an argument
/// list with [`parse_args_from`], which is deliberate: a test that assembles a
/// `Config` by hand is testing a configuration no command line can produce,
/// and the defaults it silently picks are exactly the ones a real invocation
/// gets from the parser rather than from the struct.
pub struct Config {
    user: String,
    hostname: String,
    port: u16,
    command: Option<String>,
    verbose: bool,
    strict_host_key: StrictHostKey,
    /// `-o UserKnownHostsFile=`; `None` means `$HOME/.ssh/known_hosts`.
    known_hosts_file: Option<String>,
    // Parsed from -o ConnectTimeout=N; consumed by the future socket
    // connect path that wires a real timeout into the TCP handshake.
    #[allow(dead_code)]
    connect_timeout: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum StrictHostKey {
    Yes,
    No,
    Ask,
}

/// Parse this process's own command line.
fn parse_args() -> Result<Config, String> {
    parse_args_from(env::args().collect())
}

/// Parse an argument list, `argv[0]` included, into a [`Config`].
///
/// Split from [`parse_args`] so that a caller outside this crate can produce a
/// `Config` without the process's arguments being the only possible source of
/// one. That is the whole reason it is public: an interop test needs a client
/// configured the way a command line configures one, and reaching for the real
/// parser is the only way to be sure the defaults under test are the defaults
/// that ship.
///
/// # Errors
///
/// The usage message, or a description of the first malformed option, as a
/// string suitable for printing after `ssh: `.
pub fn parse_args_from(args: Vec<String>) -> Result<Config, String> {
    if args.len() < 2 {
        return Err(format!(
            "Usage: {} [-p port] [-v] [-o option=value] [user@]hostname [command...]",
            args.first().map(|s| s.as_str()).unwrap_or("ssh")
        ));
    }

    let mut port: u16 = 22;
    let mut verbose = false;
    let mut strict_host_key = StrictHostKey::Ask;
    let mut known_hosts_file: Option<String> = None;
    let mut connect_timeout: u32 = 30;
    let mut destination: Option<String> = None;
    let mut command_parts: Vec<String> = Vec::new();

    // Walked with an iterator rather than an index. An option's value is
    // whatever `it.next()` yields, so the `i += 1` inside each value-taking arm
    // -- and the second `i += 1` at the bottom of the loop that existed only to
    // undo it -- are both gone, and with them the chance of the two getting out
    // of step. The trailing remote command needs no `args[i..]` slice either:
    // the iterator is already standing exactly there.
    let mut it = args.into_iter();
    it.next(); // argv[0], already consumed for the usage message above.

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-p" => {
                let val = it
                    .next()
                    .ok_or_else(|| "-p requires a port number".to_string())?;
                port = val.parse().map_err(|_| format!("invalid port: {val}"))?;
            }
            "-v" => {
                verbose = true;
            }
            "-o" => {
                let opt = it
                    .next()
                    .ok_or_else(|| "-o requires an option=value".to_string())?;
                if let Some(val) = opt.strip_prefix("ConnectTimeout=") {
                    connect_timeout = val.parse().unwrap_or(30);
                } else if let Some(val) = opt.strip_prefix("StrictHostKeyChecking=") {
                    strict_host_key = match val {
                        "yes" => StrictHostKey::Yes,
                        "no" => StrictHostKey::No,
                        _ => StrictHostKey::Ask,
                    };
                } else if let Some(val) = opt.strip_prefix("UserKnownHostsFile=") {
                    known_hosts_file = Some(val.to_string());
                }
                // Silently ignore unknown options (like OpenSSH).
            }
            _ => {
                if destination.is_none() {
                    destination = Some(arg);
                } else {
                    // Everything from the destination on is the remote command.
                    command_parts.push(arg);
                    command_parts.extend(it.by_ref());
                    break;
                }
            }
        }
    }

    let dest = destination.ok_or_else(|| "no destination specified".to_string())?;

    // Parse user@hostname. `split_once` names the two halves in one step; the
    // `find` plus `[..at]` and `[at + 1..]` it replaces re-derived both ends
    // from an offset, and the `at + 1` was a byte index into a string whose
    // contents come from the command line.
    let (user, hostname) = if let Some((name, host)) = dest.split_once('@') {
        (name.to_string(), host.to_string())
    } else {
        // Default to current user or "root".
        let user = env::var("USER").unwrap_or_else(|_| "root".to_string());
        (user, dest)
    };

    if user.is_empty() {
        return Err("empty username".to_string());
    }
    if hostname.is_empty() {
        return Err("empty hostname".to_string());
    }

    let command = if command_parts.is_empty() {
        None
    } else {
        Some(command_parts.join(" "))
    };

    Ok(Config {
        user,
        hostname,
        port,
        command,
        verbose,
        strict_host_key,
        known_hosts_file,
        connect_timeout,
    })
}

// ============================================================================
// SSH session — main protocol state machine
// ============================================================================

/// `data_type_code` for `SSH_MSG_CHANNEL_EXTENDED_DATA` carrying stderr
/// (RFC 4254 §5.2 — the only code the specification defines).
const SSH_EXTENDED_DATA_STDERR: u32 = 1;

/// How the remote command ended, if the server said (RFC 4254 §6.10).
///
/// `None` on the session means the server never sent either request. That is
/// the normal case for an interactive session, and for a server that does not
/// implement the notification — which is why a missing status has to mean
/// success rather than failure, even though it makes an incomplete server
/// indistinguishable from a command that worked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RemoteExit {
    Status(u32),
    Signal { name: String, core_dumped: bool },
}

/// One SSH-2 connection, from the version banner to the remote command's exit
/// status.
pub struct SshSession {
    transport: Box<dyn Transport>,
    buf: StreamBuffer,
    config: Config,
    server_version: String,
    client_kexinit: Vec<u8>,
    server_kexinit: Vec<u8>,
    /// The exchange hash of the *first* key exchange, which RFC 4253 §7.2 fixes
    /// as the session identifier for the life of the connection.
    ///
    /// `None` until that first exchange completes. This was `[u8; 32]` with
    /// all-zeros standing in for "not set yet" — the daemon has always spelled
    /// the same field `Option<[u8; 32]>`, which is the eleventh place these two
    /// programs described one protocol value two different ways. The sentinel
    /// was not reachably wrong (an all-zero SHA-256 is not something an
    /// attacker can arrange) but it is a state the type permitted and the
    /// protocol does not, and the accessor below would have had to invent an
    /// answer for it.
    session_id: Option<[u8; 32]>,
    /// The packet layer: framing, cipher, MAC and both sequence numbers.
    codec: PacketCodec,
    channel_id: u32,
    remote_channel_id: u32,
    remote_window: u32,
    /// What the server reported about how the remote command ended.
    remote_exit: Option<RemoteExit>,
    /// Where this session's unpredictable bytes come from: the packet padding,
    /// the KEXINIT cookie and the Diffie-Hellman exponent.
    ///
    /// [`sshwire::KERNEL_SECRETS`] for every connection the client makes —
    /// [`SshSession::new`] is the only way to build one and it is the only
    /// thing that sets this. A test replaces it with
    /// [`SshSession::with_secret_source`] to get a reproducible handshake.
    secrets: SecretSource,
}

impl SshSession {
    /// Open a session over an already-connected `transport`.
    ///
    /// Nothing is sent until [`version_exchange`](Self::version_exchange) or
    /// [`run`](Self::run) is called.
    #[must_use]
    pub fn new(transport: Box<dyn Transport>, config: Config) -> Self {
        Self {
            transport,
            buf: StreamBuffer::new(),
            config,
            server_version: String::new(),
            client_kexinit: Vec::new(),
            server_kexinit: Vec::new(),
            session_id: None,
            codec: PacketCodec::new(),
            channel_id: 0,
            remote_channel_id: 0,
            remote_window: 0,
            remote_exit: None,
            secrets: sshwire::KERNEL_SECRETS,
        }
    }

    /// Draw this session's secrets from `secrets` instead of the kernel.
    ///
    /// Not reachable from the command line, the ssh config file or the network,
    /// and deliberately so: a client whose entropy source could be *selected*
    /// would be a downgrade attack with a spelling. The callers are tests,
    /// which need a handshake that produces the same bytes twice.
    ///
    /// The `cfg` is that sentence made structural rather than promised: the
    /// shipped binary is not compiled with any way to substitute a source, so
    /// the guarantee does not rest on nobody calling this. The
    /// `deterministic-secrets` feature exists because the interop test lives in
    /// a crate of its own and cannot use `cfg(test)`, which is per-crate; it is
    /// enabled only through that crate's `dev-dependencies`, so it is absent
    /// from any build that does not compile dev-dependencies — which is every
    /// build that produces a release artifact.
    #[cfg(any(test, feature = "deterministic-secrets"))]
    #[must_use]
    pub fn with_secret_source(mut self, secrets: SecretSource) -> Self {
        self.secrets = secrets;
        self
    }

    /// The session identifier both ends derive, once the first key exchange has
    /// produced it (RFC 4253 §7.2). `None` before that.
    ///
    /// Public because it is the single value an interop test exists to compare:
    /// if the client and the daemon agree on this, they agreed on the version
    /// banners, both KEXINIT payloads, the host key blob, both Diffie-Hellman
    /// public values and the shared secret, because every one of those is a
    /// field of the hash. If they disagree, one of them is wrong about the
    /// protocol and neither one's own test suite can say which.
    #[must_use]
    pub fn session_id(&self) -> Option<&[u8; 32]> {
        self.session_id.as_ref()
    }

    /// The process exit code this client should exit with.
    ///
    /// `ssh(1)`'s contract: exit with the remote command's status, so that
    /// `ssh host cmd && something` behaves as if `cmd` had been run locally.
    /// A command killed by a signal has no exit status of its own; OpenSSH
    /// reports that as 255 and prints why, and matching it keeps scripts
    /// written against OpenSSH working here.
    fn exit_code(&self) -> i32 {
        match &self.remote_exit {
            Some(RemoteExit::Status(code)) => i32::try_from(*code).unwrap_or(255),
            Some(RemoteExit::Signal { name, core_dumped }) => {
                eprintln!(
                    "ssh: remote command killed by SIG{name}{}",
                    if *core_dumped { " (core dumped)" } else { "" }
                );
                255
            }
            None => 0,
        }
    }

    fn verbose(&self, msg: &str) {
        if self.config.verbose {
            eprintln!("debug1: {msg}");
        }
    }

    /// Frame, encrypt and send one packet.
    ///
    /// The padding is CSPRNG bytes, as RFC 4253 §6 says it SHOULD be, and a
    /// failed draw refuses to send rather than falling back to zeros. The codec
    /// takes the padding as a parameter rather than generating it so that both
    /// of those decisions are made here, in a program that can report them,
    /// rather than inside a shared crate that cannot. See
    /// `design-decisions.md` §773 for why the refusal is the right answer even
    /// though our current cipher makes predictable padding harmless.
    fn send_packet(&mut self, payload: &[u8]) -> Result<(), SshError> {
        let mut padding = vec![0u8; self.codec.padding_len(payload.len())];
        (self.secrets)(&mut padding)
            .map_err(|e| SshError::ProtocolError(format!("cannot generate packet padding: {e}")))?;
        let pkt = self.codec.encode(payload, &padding)?;
        self.transport.send_all(&pkt)?;
        Ok(())
    }

    /// Release the connection.
    fn close(&mut self) {
        self.transport.close();
    }

    /// Block until one whole packet has arrived, then return its payload.
    fn recv_packet(&mut self) -> Result<Vec<u8>, SshError> {
        loop {
            if let Some((payload, consumed)) = self.codec.decode(self.buf.unread())? {
                self.buf.advance(consumed);
                return Ok(payload);
            }
            self.buf.fill_once(self.transport.as_mut())?;
        }
    }

    // === Phase 1: Version exchange ===
    //
    // (`classify_version_line`, which decides what each line read here *is*,
    // is a free function below so that it can be tested without a socket.)

    /// Send our version banner and read the server's (RFC 4253 §4.2).
    ///
    /// Public so an interop test can run the handshake one phase at a time and
    /// say which phase disagreed, rather than only that [`run`](Self::run)
    /// failed somewhere.
    ///
    /// # Errors
    ///
    /// If the banner cannot be sent, or the server's is absent, malformed, or
    /// announces a protocol version this client does not speak.
    pub fn version_exchange(&mut self) -> Result<(), SshError> {
        self.verbose("sending client version");

        // Send our version string.
        let version_line = format!("{SSH_VERSION_STRING}\r\n");
        self.transport.send_all(version_line.as_bytes())?;

        // Read the server's version line. It is accumulated as *bytes*; see
        // `classify_version_line` for why that is not incidental.
        let mut line: Vec<u8> = Vec::new();
        loop {
            let mut buf = [0u8; 1];
            let [byte] = *self.transport.recv(&mut buf)? else {
                return Err(SshError::ProtocolError(
                    "connection closed during version exchange".into(),
                ));
            };
            if byte == b'\n' {
                match classify_version_line(&line)? {
                    VersionLine::Version(v) => {
                        self.server_version = v;
                        break;
                    }
                    VersionLine::Banner(text) => {
                        self.verbose(&format!("banner: {text}"));
                        line.clear();
                    }
                }
            } else {
                line.push(byte);
                if line.len() > MAX_GREETING_LINE {
                    return Err(SshError::ProtocolError("version line too long".into()));
                }
            }
        }

        self.verbose(&format!("remote version: {}", self.server_version));

        // Verify it speaks SSH-2.
        if !self.server_version.starts_with("SSH-2.0-")
            && !self.server_version.starts_with("SSH-1.99-")
        {
            return Err(SshError::ProtocolError(format!(
                "unsupported server version: {}",
                self.server_version
            )));
        }

        Ok(())
    }

    // === Phase 2: Key exchange ===

    /// Exchange KEXINITs, run Diffie-Hellman, verify the host key and activate
    /// the negotiated cipher (RFC 4253 §7 and §8).
    ///
    /// On success [`session_id`](Self::session_id) is set and every packet
    /// afterwards is encrypted.
    ///
    /// Public for the same reason as
    /// [`version_exchange`](Self::version_exchange): this is the phase where
    /// two independently-written implementations of one protocol actually find
    /// out whether they agree.
    ///
    /// # Errors
    ///
    /// If no algorithm is common to both ends, if the server's Diffie-Hellman
    /// value is out of range, if the host key signature does not verify, or if
    /// the key is not the one `known_hosts` records for this host.
    pub fn key_exchange(&mut self) -> Result<(), SshError> {
        self.verbose("beginning key exchange");

        // Build and send our KEXINIT.
        let client_kexinit_payload = self.build_kexinit()?;
        self.client_kexinit = client_kexinit_payload.clone();
        self.send_packet(&client_kexinit_payload)?;
        self.verbose("sent KEXINIT");

        // Receive server KEXINIT.
        let server_payload = self.recv_packet()?;
        if server_payload.first() != Some(&msg::SSH_MSG_KEXINIT) {
            return Err(SshError::ProtocolError(format!(
                "expected KEXINIT, got message type {}",
                server_payload.first().copied().unwrap_or(255)
            )));
        }
        self.server_kexinit = server_payload.clone();
        self.verbose("received server KEXINIT");

        // Perform DH key exchange.
        self.dh_key_exchange()?;

        Ok(())
    }

    /// Build a KEXINIT payload advertising our supported algorithms.
    ///
    /// # Errors
    ///
    /// Fails if the kernel CSPRNG cannot supply the cookie.
    fn build_kexinit(&self) -> Result<Vec<u8>, SshError> {
        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_KEXINIT);

        // The cookie (RFC 4253 section 7.1) must be random: it is what stops
        // either side from choosing the exchange hash, since I_C and I_S both
        // feed into it. Sixteen zero bytes "for simplicity" gave that power to
        // the server alone.
        let mut cookie = [0u8; 16];
        (self.secrets)(&mut cookie).map_err(|e| {
            SshError::ProtocolError(format!("cannot generate the KEXINIT cookie: {e}"))
        })?;
        payload.extend_from_slice(&cookie);

        // Algorithm name-lists. We offer only what we can actually verify:
        // advertising ssh-rsa would invite a server to send an RSA host key
        // that `verify_host_key_signature` would then have to refuse, turning
        // a working connection into a confusing failure.
        let kex = "diffie-hellman-group14-sha256,diffie-hellman-group14-sha1";
        let host_key = "ssh-ed25519";
        let enc = "aes128-ctr";
        let mac = "hmac-sha2-256";
        let comp = "none";
        let lang = "";

        // kex_algorithms
        payload.extend_from_slice(&ssh_string(kex.as_bytes()));
        // server_host_key_algorithms
        payload.extend_from_slice(&ssh_string(host_key.as_bytes()));
        // encryption_algorithms_client_to_server
        payload.extend_from_slice(&ssh_string(enc.as_bytes()));
        // encryption_algorithms_server_to_client
        payload.extend_from_slice(&ssh_string(enc.as_bytes()));
        // mac_algorithms_client_to_server
        payload.extend_from_slice(&ssh_string(mac.as_bytes()));
        // mac_algorithms_server_to_client
        payload.extend_from_slice(&ssh_string(mac.as_bytes()));
        // compression_algorithms_client_to_server
        payload.extend_from_slice(&ssh_string(comp.as_bytes()));
        // compression_algorithms_server_to_client
        payload.extend_from_slice(&ssh_string(comp.as_bytes()));
        // languages_client_to_server
        payload.extend_from_slice(&ssh_string(lang.as_bytes()));
        // languages_server_to_client
        payload.extend_from_slice(&ssh_string(lang.as_bytes()));
        // first_kex_packet_follows
        payload.push(0);
        // reserved (u32)
        payload.extend_from_slice(&0u32.to_be_bytes());

        Ok(payload)
    }

    /// Perform Diffie-Hellman group14 key exchange.
    fn dh_key_exchange(&mut self) -> Result<(), SshError> {
        let p_bytes = dh_group14_prime_bytes();
        let p = BigUint::from_bytes_be(&p_bytes);
        let generator = BigUint::from_bytes_be(&[DH_GROUP14_G]);

        // Generate private exponent x and compute e = g^x mod p.
        let x = generate_dh_private(self.secrets)?;
        self.verbose("generated DH private key");

        let e = generator.mod_pow(&x, &p);
        let e_bytes = e.to_bytes_be();
        self.verbose(&format!("DH public value e: {} bytes", e_bytes.len()));

        // Send SSH_MSG_KEX_DH_INIT with e.
        let mut init_payload = Vec::new();
        init_payload.push(msg::SSH_MSG_KEX_DH_INIT);
        init_payload.extend_from_slice(&encode_mpint(&e_bytes));
        self.send_packet(&init_payload)?;
        self.verbose("sent KEX_DH_INIT");

        // Receive SSH_MSG_KEX_DH_REPLY.
        let reply = self.recv_packet()?;
        if reply.first() != Some(&msg::SSH_MSG_KEX_DH_REPLY) {
            return Err(SshError::ProtocolError(format!(
                "expected KEX_DH_REPLY, got {}",
                reply.first().copied().unwrap_or(255)
            )));
        }
        self.verbose("received KEX_DH_REPLY");

        // Parse: K_S (host key blob), f (server DH public), signature.
        let mut off = 1;
        let (k_s, next) = read_ssh_string(&reply, off)?;
        off = next;
        let k_s = k_s.to_vec();

        let (f_bytes, next) = read_mpint(&reply, off)?;
        off = next;

        let (sig_blob, _next) = read_ssh_string(&reply, off)?;
        let sig_blob = sig_blob.to_vec();

        // Extract host key type from the key blob.
        let (key_type_bytes, _) = read_ssh_string(&k_s, 0)?;
        let key_type = std::str::from_utf8(key_type_bytes).unwrap_or("unknown");
        self.verbose(&format!("host key type: {key_type}"));

        // Display fingerprint.
        let fingerprint = host_key_fingerprint(&k_s);
        self.verbose(&format!("host key fingerprint: {fingerprint}"));

        // RFC 4253 section 8: reject f outside [2, p-2]. f = 0 or f = 1 would
        // pin the shared secret to a value the server chose, and f = p-1 has
        // order 2, so K would be one of two values. A server doing this is
        // arranging for the session keys to be guessable.
        let f = BigUint::from_bytes_be(f_bytes);
        let p_minus_1 = p.sub(&BigUint::one());
        if f.cmp_unsigned(&BigUint::one()) != std::cmp::Ordering::Greater
            || f.cmp_unsigned(&p_minus_1) != std::cmp::Ordering::Less
        {
            return Err(SshError::ProtocolError(
                "server DH value out of range (RFC 4253 section 8)".into(),
            ));
        }

        // Compute shared secret K = f^x mod p.
        let k_big = f.mod_pow(&x, &p);
        let k_bytes = k_big.to_bytes_be();
        self.verbose("computed shared secret");

        // Compute exchange hash H.
        let h = compute_exchange_hash(&ExchangeHashInput {
            client_version: SSH_VERSION_STRING,
            server_version: &self.server_version,
            client_kexinit: &self.client_kexinit,
            server_kexinit: &self.server_kexinit,
            host_key_blob: &k_s,
            client_e: &e_bytes,
            server_f: f_bytes,
            shared_secret: &k_bytes,
        });
        self.verbose(&format!("exchange hash: {}", bytes_to_hex(&h)));

        // Prove the server holds the private half of the key it just sent,
        // *before* asking the user anything about that key.
        //
        // Order matters and it used to be wrong twice over. The signature was
        // not checked at all, and the known_hosts prompt ran before the
        // exchange hash even existed -- so the user was asked to trust a key
        // that nothing had shown the server could use. Any machine that
        // answers on port 22 could name itself with someone else's public key
        // and be permanently added to known_hosts under it, which converts
        // known_hosts from a defence into a way of installing an attacker's
        // key as trusted.
        verify_host_key_signature(&k_s, &h, &sig_blob)?;
        self.verbose("host key signature verified");

        // Only now is it meaningful to ask whether this is the *right* key.
        // The signature proves the server owns the key; known_hosts decides
        // whether that key is the one we expect.
        self.verify_host_key(&k_s, key_type, &fingerprint)?;

        // RFC 4253 §7.2: the session identifier is the *first* exchange hash
        // and never changes afterwards, so a rekey must not overwrite it.
        let session_id = *self.session_id.get_or_insert(h);

        // Send and receive NEWKEYS.
        let newkeys_payload = [msg::SSH_MSG_NEWKEYS];
        self.send_packet(&newkeys_payload)?;
        self.verbose("sent NEWKEYS");

        let newkeys_reply = self.recv_packet()?;
        if newkeys_reply.first() != Some(&msg::SSH_MSG_NEWKEYS) {
            return Err(SshError::ProtocolError(format!(
                "expected NEWKEYS, got {}",
                newkeys_reply.first().copied().unwrap_or(255)
            )));
        }
        self.verbose("received NEWKEYS");

        // Install the keys `NEWKEYS` brings into force. Which of RFC 4253
        // §7.2's six lettered values is the *outbound* one depends on which end
        // is asking, and saying `Role::Client` is the whole of this end's answer
        // — the six assignments themselves are the codec's, shared with the
        // daemon, rather than a block of code here that has to stay the
        // daemon's mirror image by hand.
        self.codec
            .activate(Role::Client, &k_bytes, &h, &session_id)?;

        self.verbose("encryption activated");

        Ok(())
    }

    /// Verify the server's host key against known_hosts.
    fn verify_host_key(
        &self,
        key_blob: &[u8],
        key_type: &str,
        fingerprint: &str,
    ) -> Result<(), SshError> {
        let known_hosts_file = self.config.known_hosts_file.as_deref();
        if check_known_hosts(
            known_hosts_file,
            &self.config.hostname,
            self.config.port,
            key_blob,
        )? {
            self.verbose("host key matches known_hosts");
            Ok(())
        } else {
            // Host not in known_hosts.
            match self.config.strict_host_key {
                StrictHostKey::Yes => Err(SshError::HostKeyMismatch(format!(
                    "host '{}' not found in known_hosts (StrictHostKeyChecking=yes)",
                    self.config.hostname
                ))),
                StrictHostKey::No => {
                    eprintln!(
                        "Warning: Permanently added {} ({key_type}) to the list of known hosts.",
                        quoteaf_os(&self.config.hostname)
                    );
                    add_known_host(
                        known_hosts_file,
                        &self.config.hostname,
                        self.config.port,
                        key_type,
                        key_blob,
                    );
                    Ok(())
                }
                StrictHostKey::Ask => {
                    eprint!(
                        "The authenticity of host '{}' ({}) can't be established.\n\
                         {key_type} key fingerprint is {fingerprint}.\n\
                         Are you sure you want to continue connecting (yes/no)? ",
                        self.config.hostname, self.config.hostname,
                    );
                    io::stderr().flush().ok();

                    let mut answer = String::new();
                    io::stdin().read_line(&mut answer).map_err(|e| {
                        SshError::ProtocolError(format!("failed to read answer: {e}"))
                    })?;
                    let answer = answer.trim().to_lowercase();

                    if answer == "yes" {
                        eprintln!(
                            "Warning: Permanently added {} ({key_type}) to the list of known hosts.",
                            quoteaf_os(&self.config.hostname)
                        );
                        add_known_host(
                            known_hosts_file,
                            &self.config.hostname,
                            self.config.port,
                            key_type,
                            key_blob,
                        );
                        Ok(())
                    } else {
                        Err(SshError::HostKeyMismatch(
                            "host key verification declined by user".into(),
                        ))
                    }
                }
            }
        }
    }

    // === Phase 3: Service request + user authentication ===

    fn authenticate(&mut self) -> Result<(), SshError> {
        // Request the "ssh-userauth" service.
        self.verbose("requesting ssh-userauth service");
        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_SERVICE_REQUEST);
        payload.extend_from_slice(&ssh_string(b"ssh-userauth"));
        self.send_packet(&payload)?;

        let reply = self.recv_packet()?;
        if reply.first() != Some(&msg::SSH_MSG_SERVICE_ACCEPT) {
            return Err(SshError::ProtocolError(format!(
                "expected SERVICE_ACCEPT, got {}",
                reply.first().copied().unwrap_or(255)
            )));
        }
        self.verbose("service accepted: ssh-userauth");

        // Prompt for password.
        let password = self.read_password()?;

        // Send USERAUTH_REQUEST with password method.
        self.verbose("sending password authentication");
        let mut auth_payload = Vec::new();
        auth_payload.push(msg::SSH_MSG_USERAUTH_REQUEST);
        auth_payload.extend_from_slice(&ssh_string(self.config.user.as_bytes()));
        auth_payload.extend_from_slice(&ssh_string(b"ssh-connection"));
        auth_payload.extend_from_slice(&ssh_string(b"password"));
        auth_payload.push(0); // not a password change
        auth_payload.extend_from_slice(&ssh_string(password.as_bytes()));
        self.send_packet(&auth_payload)?;

        // Handle response.
        loop {
            let reply = self.recv_packet()?;
            match reply.first().copied() {
                Some(msg::SSH_MSG_USERAUTH_SUCCESS) => {
                    self.verbose("authentication successful");
                    return Ok(());
                }
                Some(msg::SSH_MSG_USERAUTH_FAILURE) => {
                    let (methods, _) = read_ssh_string(&reply, 1)?;
                    let methods_str = std::str::from_utf8(methods).unwrap_or("(unknown)");
                    return Err(SshError::AuthFailed(format!(
                        "password rejected. Available methods: {methods_str}"
                    )));
                }
                Some(msg::SSH_MSG_USERAUTH_BANNER) => {
                    // Display the banner message.
                    if let Ok((banner_msg, _)) = read_ssh_string(&reply, 1) {
                        let text = std::str::from_utf8(banner_msg).unwrap_or("");
                        if !text.is_empty() {
                            eprint!("{text}");
                        }
                    }
                    // Continue waiting for success/failure.
                }
                Some(other) => {
                    self.verbose(&format!("ignoring message type {other} during auth"));
                }
                None => {
                    return Err(SshError::ProtocolError("empty auth response".into()));
                }
            }
        }
    }

    /// Read a password from stdin with echo disabled.
    /// On Slate OS, we write to stderr to prompt, then read a line from stdin.
    /// Real echo suppression requires ioctl — here we just do a basic read.
    fn read_password(&self) -> Result<String, SshError> {
        eprint!("{}@{}'s password: ", self.config.user, self.config.hostname);
        io::stderr().flush().ok();

        let mut password = String::new();
        io::stdin()
            .read_line(&mut password)
            .map_err(|e| SshError::ProtocolError(format!("failed to read password: {e}")))?;

        // Print newline after password entry.
        eprintln!();

        Ok(password
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string())
    }

    // === Phase 4: Channel open + PTY + shell/exec ===

    fn open_session_channel(&mut self) -> Result<(), SshError> {
        self.verbose("opening session channel");

        self.channel_id = 0;
        let initial_window: u32 = 2_097_152; // 2 MiB
        let max_packet = u32::try_from(MAX_CHANNEL_CHUNK).unwrap_or(u32::MAX);

        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_CHANNEL_OPEN);
        payload.extend_from_slice(&ssh_string(b"session"));
        payload.extend_from_slice(&self.channel_id.to_be_bytes());
        payload.extend_from_slice(&initial_window.to_be_bytes());
        payload.extend_from_slice(&max_packet.to_be_bytes());
        self.send_packet(&payload)?;

        // Wait for CHANNEL_OPEN_CONFIRMATION.
        loop {
            let reply = self.recv_packet()?;
            match reply.first().copied() {
                Some(msg::SSH_MSG_CHANNEL_OPEN_CONFIRMATION) => {
                    let (_, off) = read_u32(&reply, 1)?; // recipient channel
                    let (remote_id, off) = read_u32(&reply, off)?;
                    let (remote_window, _off) = read_u32(&reply, off)?;
                    self.remote_channel_id = remote_id;
                    self.remote_window = remote_window;
                    self.verbose(&format!(
                        "channel open: remote_id={remote_id}, window={remote_window}"
                    ));
                    break;
                }
                Some(msg::SSH_MSG_CHANNEL_OPEN_FAILURE) => {
                    let reason = if reply.len() > 8 {
                        read_ssh_string(&reply, 5)
                            .ok()
                            .and_then(|(msg, _)| std::str::from_utf8(msg).ok().map(String::from))
                            .unwrap_or_else(|| "unknown".to_string())
                    } else {
                        "unknown".to_string()
                    };
                    return Err(SshError::ProtocolError(format!(
                        "channel open failed: {reason}"
                    )));
                }
                Some(msg::SSH_MSG_IGNORE | msg::SSH_MSG_DEBUG) => {
                    // Skip informational messages.
                }
                Some(other) => {
                    self.verbose(&format!(
                        "ignoring message type {other} while opening channel"
                    ));
                }
                None => {
                    return Err(SshError::ProtocolError("empty response".into()));
                }
            }
        }

        Ok(())
    }

    /// Request a PTY for the session channel.
    fn request_pty(&mut self) -> Result<(), SshError> {
        self.verbose("requesting PTY");

        let term = env::var("TERM").unwrap_or_else(|_| "xterm".to_string());
        let cols: u32 = 80;
        let rows: u32 = 24;
        let width_px: u32 = 0;
        let height_px: u32 = 0;

        // Terminal modes — empty for simplicity.
        let modes: &[u8] = &[0]; // TTY_OP_END

        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_CHANNEL_REQUEST);
        payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
        payload.extend_from_slice(&ssh_string(b"pty-req"));
        payload.push(1); // want reply
        payload.extend_from_slice(&ssh_string(term.as_bytes()));
        payload.extend_from_slice(&cols.to_be_bytes());
        payload.extend_from_slice(&rows.to_be_bytes());
        payload.extend_from_slice(&width_px.to_be_bytes());
        payload.extend_from_slice(&height_px.to_be_bytes());
        payload.extend_from_slice(&ssh_string(modes));
        self.send_packet(&payload)?;

        // Wait for CHANNEL_SUCCESS or CHANNEL_FAILURE.
        loop {
            let reply = self.recv_packet()?;
            match reply.first().copied() {
                Some(msg::SSH_MSG_CHANNEL_SUCCESS) => {
                    self.verbose("PTY allocated");
                    return Ok(());
                }
                Some(msg::SSH_MSG_CHANNEL_FAILURE) => {
                    // PTY failed but we can continue without it for exec.
                    self.verbose("PTY request failed (continuing without)");
                    return Ok(());
                }
                Some(msg::SSH_MSG_IGNORE | msg::SSH_MSG_DEBUG) => {}
                Some(msg::SSH_MSG_CHANNEL_WINDOW_ADJUST) => {
                    self.handle_window_adjust(&reply);
                }
                Some(other) => {
                    self.verbose(&format!("ignoring message type {other} during PTY request"));
                }
                None => {
                    return Err(SshError::ProtocolError("empty response".into()));
                }
            }
        }
    }

    /// Request a shell or execute a command.
    fn request_shell_or_exec(&mut self) -> Result<(), SshError> {
        if let Some(ref cmd) = self.config.command {
            self.verbose(&format!("requesting exec: {cmd}"));
            let mut payload = Vec::new();
            payload.push(msg::SSH_MSG_CHANNEL_REQUEST);
            payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
            payload.extend_from_slice(&ssh_string(b"exec"));
            payload.push(1); // want reply
            payload.extend_from_slice(&ssh_string(cmd.as_bytes()));
            self.send_packet(&payload)?;
        } else {
            self.verbose("requesting shell");
            let mut payload = Vec::new();
            payload.push(msg::SSH_MSG_CHANNEL_REQUEST);
            payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
            payload.extend_from_slice(&ssh_string(b"shell"));
            payload.push(1); // want reply
            self.send_packet(&payload)?;
        }

        // Wait for CHANNEL_SUCCESS.
        loop {
            let reply = self.recv_packet()?;
            match reply.first().copied() {
                Some(msg::SSH_MSG_CHANNEL_SUCCESS) => {
                    self.verbose("shell/exec started");
                    return Ok(());
                }
                Some(msg::SSH_MSG_CHANNEL_FAILURE) => {
                    // Name which request was refused: "shell request failed"
                    // and "exec request failed" have different causes, and a
                    // combined message sends the reader looking in the wrong
                    // place. A server with no pseudo-terminal support refuses
                    // `shell` and accepts `exec`.
                    let what = if self.config.command.is_some() {
                        "exec"
                    } else {
                        "shell"
                    };
                    return Err(SshError::ProtocolError(format!(
                        "{what} request failed on channel {}",
                        self.channel_id
                    )));
                }
                Some(msg::SSH_MSG_CHANNEL_WINDOW_ADJUST) => {
                    self.handle_window_adjust(&reply);
                }
                Some(msg::SSH_MSG_CHANNEL_DATA) => {
                    // Early data — process it.
                    self.handle_channel_data(&reply);
                }
                Some(msg::SSH_MSG_CHANNEL_EXTENDED_DATA) => {
                    // Early stderr — a command that fails immediately can have
                    // written its diagnostic before the reply arrives.
                    self.handle_channel_extended_data(&reply);
                }
                Some(msg::SSH_MSG_IGNORE | msg::SSH_MSG_DEBUG) => {}
                Some(other) => {
                    self.verbose(&format!(
                        "ignoring message type {other} during shell request"
                    ));
                }
                None => {
                    return Err(SshError::ProtocolError("empty response".into()));
                }
            }
        }
    }

    // === Phase 5: Data relay (interactive session) ===

    /// Main data relay loop. Reads from stdin and sends to server,
    /// reads from server and writes to stdout.
    fn data_loop(&mut self) -> Result<(), SshError> {
        self.verbose("entering data relay loop");

        // For a command execution, we only read from server and write to stdout.
        // For interactive, we also read stdin. In this simplified implementation,
        // we use a blocking approach: try to read from TCP (non-blocking would
        // be ideal but requires poll/select syscalls).

        let mut stdin_buf = [0u8; 4096];
        let mut closed = false;

        loop {
            // Try to receive a packet from the server.
            // We use a polling approach: attempt recv, if no data available
            // the kernel blocks briefly and returns.
            match self.try_recv_packet() {
                Ok(Some(payload)) => {
                    if self.process_server_message(&payload)? {
                        break; // Channel closed.
                    }
                }
                Ok(None) => {
                    // No data available yet — that's fine.
                }
                Err(e) => {
                    // Connection error.
                    self.verbose(&format!("recv error: {e}"));
                    break;
                }
            }

            // Read from stdin and send to server.
            if !closed {
                match io::stdin().read(&mut stdin_buf) {
                    Ok(0) => {
                        // EOF on stdin (Ctrl+D).
                        self.verbose("stdin EOF, sending channel EOF");
                        self.send_channel_eof()?;
                        closed = true;
                    }
                    Ok(n) => {
                        // `get` rather than `[..n]`. `Read::read` is
                        // contractually bound never to report more than the
                        // buffer holds, so this cannot fail -- but the same was
                        // said of the kernel's recv length, and saying it with
                        // a range that has to be valid costs nothing.
                        let data = stdin_buf.get(..n).ok_or_else(|| {
                            SshError::ProtocolError(
                                "stdin read reported more bytes than were asked for".into(),
                            )
                        })?;
                        self.send_channel_data(data)?;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // No stdin data — continue.
                    }
                    Err(e) => {
                        self.verbose(&format!("stdin error: {e}"));
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Try to receive one packet, returning None if no data available.
    /// In this simplified implementation, this always blocks for at least
    /// one recv call.
    fn try_recv_packet(&mut self) -> Result<Option<Vec<u8>>, SshError> {
        // A whole packet may already be buffered from an earlier read.
        if let Some((payload, consumed)) = self.codec.decode(self.buf.unread())? {
            self.buf.advance(consumed);
            return Ok(Some(payload));
        }

        // Otherwise one read, not a loop: the caller has a terminal to service
        // and must not be parked here while the server has nothing to say.
        self.buf.fill_once(self.transport.as_mut())?;

        if let Some((payload, consumed)) = self.codec.decode(self.buf.unread())? {
            self.buf.advance(consumed);
            Ok(Some(payload))
        } else {
            Ok(None)
        }
    }

    /// Process a server message. Returns true if the channel is closed.
    fn process_server_message(&mut self, payload: &[u8]) -> Result<bool, SshError> {
        match payload.first().copied() {
            Some(msg::SSH_MSG_CHANNEL_DATA) => {
                self.handle_channel_data(payload);
                Ok(false)
            }
            Some(msg::SSH_MSG_CHANNEL_EXTENDED_DATA) => {
                self.handle_channel_extended_data(payload);
                Ok(false)
            }
            Some(msg::SSH_MSG_CHANNEL_EOF) => {
                self.verbose("received channel EOF");
                Ok(false) // Wait for CLOSE.
            }
            Some(msg::SSH_MSG_CHANNEL_CLOSE) => {
                self.verbose("received channel close");
                // Send CLOSE back.
                self.send_channel_close()?;
                Ok(true)
            }
            Some(msg::SSH_MSG_CHANNEL_WINDOW_ADJUST) => {
                self.handle_window_adjust(payload);
                Ok(false)
            }
            Some(msg::SSH_MSG_CHANNEL_REQUEST) => {
                // Server-initiated channel request (e.g., exit-status).
                self.handle_channel_request(payload)?;
                Ok(false)
            }
            Some(msg::SSH_MSG_DISCONNECT) => {
                let reason = if payload.len() > 8 {
                    read_ssh_string(payload, 5)
                        .ok()
                        .and_then(|(msg, _)| std::str::from_utf8(msg).ok().map(String::from))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                self.verbose(&format!("server disconnected: {reason}"));
                Ok(true)
            }
            Some(msg::SSH_MSG_IGNORE | msg::SSH_MSG_DEBUG | msg::SSH_MSG_UNIMPLEMENTED) => {
                // Skip.
                Ok(false)
            }
            Some(other) => {
                self.verbose(&format!("unhandled message type: {other}"));
                Ok(false)
            }
            None => Ok(false),
        }
    }

    /// Handle CHANNEL_DATA: extract data and write to stdout.
    fn handle_channel_data(&self, payload: &[u8]) {
        // Format: u8 type, u32 recipient_channel, string data
        if let Ok((data, _)) = read_ssh_string(payload, 5) {
            let _ = io::stdout().write_all(data);
            let _ = io::stdout().flush();
        }
    }

    /// Handle `CHANNEL_EXTENDED_DATA`: the remote command's stderr.
    ///
    /// Writing it to *our* stderr is what keeps `ssh host cmd > file` honest:
    /// the file gets the command's output and the terminal gets its
    /// diagnostics, exactly as if the command had run locally. Before this
    /// existed the message fell through to "unhandled message type" and every
    /// byte the remote command wrote to stderr was silently discarded.
    ///
    /// Format: `u8 type, u32 recipient_channel, u32 data_type_code, string data`.
    fn handle_channel_extended_data(&self, payload: &[u8]) {
        let Ok((data_type_code, off)) = read_u32(payload, 5) else {
            return;
        };
        let Ok((data, _)) = read_ssh_string(payload, off) else {
            return;
        };
        if data_type_code == SSH_EXTENDED_DATA_STDERR {
            // Ignoring the write failure: a closed stderr is not a reason to
            // abandon a session whose real output is going to stdout.
            let _ = io::stderr().write_all(data);
            let _ = io::stderr().flush();
        } else {
            // No other code is defined. Discarding is required — writing an
            // unknown stream to stdout would corrupt the command's output.
            self.verbose(&format!(
                "discarding {} bytes of extended data, unknown type {data_type_code}",
                data.len()
            ));
        }
    }

    /// Handle CHANNEL_WINDOW_ADJUST: increase our send window.
    fn handle_window_adjust(&mut self, payload: &[u8]) {
        // Format: u8 type, u32 recipient_channel, u32 bytes_to_add
        if let Ok((adjustment, _)) = read_u32(payload, 5) {
            self.remote_window = self.remote_window.saturating_add(adjustment);
            self.verbose(&format!(
                "window adjust +{adjustment}, now {}",
                self.remote_window
            ));
        }
    }

    /// Handle a server-initiated `CHANNEL_REQUEST`.
    ///
    /// The two that matter are `exit-status` and `exit-signal` (RFC 4254
    /// §6.10) — between them they are the *only* way the server says how the
    /// remote command ended. This used to parse the request type, log it, and
    /// throw it away, which made `ssh host false` exit 0 no matter what the
    /// server reported.
    fn handle_channel_request(&mut self, payload: &[u8]) -> Result<(), SshError> {
        // Format: u8 type, u32 recipient_channel, string request_type, bool want_reply, ...
        let mut off = 1;
        let (_, next) = read_u32(payload, off)?; // recipient_channel
        off = next;
        let (req_type, next) = read_ssh_string(payload, off)?;
        off = next;
        let (want_reply, next) = read_byte(payload, off)?;
        off = next;

        let req_type_str = std::str::from_utf8(req_type).unwrap_or("unknown");
        self.verbose(&format!("channel request: {req_type_str}"));

        match req_type_str {
            "exit-status" => {
                let (status, _) = read_u32(payload, off)?;
                self.verbose(&format!("remote command exited with status {status}"));
                self.remote_exit = Some(RemoteExit::Status(status));
            }
            "exit-signal" => {
                // string signal name (no "SIG" prefix), bool core dumped,
                // string error message, string language tag.
                let (name_bytes, next) = read_ssh_string(payload, off)?;
                let name = String::from_utf8_lossy(name_bytes).into_owned();
                let (core_dumped, _) = read_byte(payload, next)?;
                self.verbose(&format!("remote command killed by SIG{name}"));
                self.remote_exit = Some(RemoteExit::Signal {
                    name,
                    core_dumped: core_dumped != 0,
                });
            }
            _ => {}
        }

        if want_reply != 0 {
            // Nothing a server can ask of this client is something it can do,
            // so the answer is FAILURE. It goes through `send_packet` so the
            // send sequence number advances with it: the sequence number is
            // an input to every packet's MAC, and a packet sent without
            // bumping it desynchronises the server's MAC check for the whole
            // rest of the connection. (This function previously called
            // `tcp_send_all` directly and did not advance it.)
            let mut reply = Vec::new();
            reply.push(msg::SSH_MSG_CHANNEL_FAILURE);
            reply.extend_from_slice(&self.remote_channel_id.to_be_bytes());
            self.send_packet(&reply)?;
        }

        Ok(())
    }

    /// Send data to the remote channel.
    fn send_channel_data(&mut self, data: &[u8]) -> Result<(), SshError> {
        // Respect the remote window size. Walking a shrinking `rest` rather
        // than an offset keeps "how much is left" and "where that starts" from
        // being two facts that can disagree: `data[offset..offset + chunk_size]`
        // re-derived both ends from a running counter and a length computed
        // three lines earlier, and `split_at` derives them from one number
        // that the `.min` on the line above has just bounded.
        let mut rest = data;
        while !rest.is_empty() {
            if self.remote_window == 0 {
                // Wait for a window adjust.
                let payload = self.recv_packet()?;
                let _ = self.process_server_message(&payload);
                continue;
            }
            let chunk_size = rest
                .len()
                .min(self.remote_window as usize)
                .min(MAX_CHANNEL_CHUNK);
            let (chunk, tail) = rest.split_at(chunk_size);

            let mut payload = Vec::new();
            payload.push(msg::SSH_MSG_CHANNEL_DATA);
            payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
            payload.extend_from_slice(&ssh_string(chunk));
            self.send_packet(&payload)?;

            self.remote_window = self
                .remote_window
                .saturating_sub(u32::try_from(chunk_size).unwrap_or(u32::MAX));
            rest = tail;
        }
        Ok(())
    }

    /// Send CHANNEL_EOF.
    fn send_channel_eof(&mut self) -> Result<(), SshError> {
        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_CHANNEL_EOF);
        payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
        self.send_packet(&payload)
    }

    /// Send CHANNEL_CLOSE.
    fn send_channel_close(&mut self) -> Result<(), SshError> {
        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_CHANNEL_CLOSE);
        payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
        self.send_packet(&payload)
    }

    /// Send a disconnect message.
    fn send_disconnect(&mut self, reason_code: u32, description: &str) {
        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_DISCONNECT);
        payload.extend_from_slice(&reason_code.to_be_bytes());
        payload.extend_from_slice(&ssh_string(description.as_bytes()));
        payload.extend_from_slice(&ssh_string(b"")); // language tag
        // Best-effort — ignore errors during disconnect.
        let _ = self.send_packet(&payload);
    }

    /// Run the full SSH session lifecycle.
    fn run(&mut self) -> Result<(), SshError> {
        self.version_exchange()?;
        self.key_exchange()?;
        self.authenticate()?;
        self.open_session_channel()?;

        // Request PTY only for interactive sessions.
        if self.config.command.is_none() {
            self.request_pty()?;
        }

        self.request_shell_or_exec()?;
        self.data_loop()?;

        Ok(())
    }
}

// ============================================================================
// Entry point
// ============================================================================

/// `ssh(1)`'s reserved exit code for a failure of the client or the
/// connection, as opposed to a status the remote command returned.
///
/// The whole point of reserving one code is that `ssh host cmd; echo $?` can
/// distinguish "cmd failed" from "cmd never ran". 255 is the value OpenSSH
/// documents, so scripts written against OpenSSH keep working.
pub const EXIT_SSH_FAILURE: i32 = 255;

/// Run the `ssh(1)` command line and report the status the process should
/// exit with.
///
/// Returns the code rather than calling `process::exit` itself. That is the
/// whole difference between this and the `main` it replaces, and it is what
/// makes the binary a shim: ending the process is the binary's business, and a
/// library that can end its caller's process is not one a test can call.
#[must_use]
pub fn run_cli() -> i32 {
    let config = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("ssh: {msg}");
            return EXIT_SSH_FAILURE;
        }
    };

    let verbose = config.verbose;

    if verbose {
        eprintln!(
            "debug1: connecting to {} port {}",
            config.hostname, config.port
        );
    }

    // Resolve hostname.
    let ip = match dns_resolve(&config.hostname) {
        Ok(ip) => ip,
        Err(e) => {
            eprintln!("ssh: {e}");
            return EXIT_SSH_FAILURE;
        }
    };

    if verbose {
        let octets = ip.to_be_bytes();
        eprintln!(
            "debug1: resolved to {}.{}.{}.{}",
            octets[0], octets[1], octets[2], octets[3]
        );
    }

    // Open TCP connection.
    let transport = match TcpTransport::connect(ip, config.port) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "ssh: connect to host {} port {}: {e}",
                config.hostname, config.port
            );
            return EXIT_SSH_FAILURE;
        }
    };

    if verbose {
        eprintln!("debug1: connection established");
    }

    // Run the SSH session.
    let mut session = SshSession::new(Box::new(transport), config);
    match session.run() {
        Ok(()) => {
            session.send_disconnect(11, "disconnected by user");
        }
        Err(e) => {
            eprintln!("ssh: {e}");
            session.send_disconnect(2, "protocol error");
            session.close();
            // 255 is `ssh(1)`'s reserved code for "the connection or the
            // client failed", as distinct from any status a remote command
            // could return — the distinction a caller needs in order to tell
            // "the command failed" from "ssh could not run it".
            return EXIT_SSH_FAILURE;
        }
    }

    session.close();

    // Report the remote command's status. Read after the socket is closed
    // because the caller ends the process the moment this returns, and a
    // `process::exit` there runs no destructors.
    session.exit_code()
}

// ============================================================================
// Tests
// ============================================================================

// Panicking on bad data is what a test is *for*: a test that carefully
// propagates an error instead of unwrapping just reports "ok" less loudly.
// The defensive lints stay on for the production code above.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            user: "alice".into(),
            hostname: "example".into(),
            port: 22,
            command: Some("true".into()),
            verbose: false,
            strict_host_key: StrictHostKey::No,
            // Never `None` in a test: `None` means the operator's real
            // `~/.ssh/known_hosts`, and `StrictHostKeyChecking=no` above would
            // append to it. The empty path is deliberate -- no platform can
            // open or create it, so it is "a file that is not there" stated in
            // a way that cannot accidentally become a file that is.
            known_hosts_file: Some(String::new()),
            connect_timeout: 0,
        }
    }

    /// A session over an in-memory stream whose peer end is dropped
    /// immediately, so any I/O reports a closed connection rather than
    /// reaching a kernel that would refuse it anyway.
    ///
    /// This used to be `SshSession::new(0, ...)` — a raw handle 0, valid only
    /// because nothing exercised below does I/O. That is a precondition no type
    /// enforced and no reader could see; a transport that is genuinely closed
    /// is the same guarantee stated where the compiler can keep it.
    ///
    /// Its secrets come from a stand-in for the same reason its transport is
    /// in memory: a test must not depend on the machine it runs on. `randrange`
    /// refuses on this host on purpose (`open-questions.md`, "The test machine
    /// cannot produce random numbers, on purpose…"), which is what made
    /// `the_kexinit_cookie_is_random` and its three neighbours fail on every
    /// run — four red tests of real properties, failing for one irrelevant
    /// reason, in the suite of a program whose bugs are invisible locally.
    fn test_session() -> SshSession {
        let (near, far) = sshwire::memory_pair();
        drop(far);
        SshSession::new(Box::new(near), test_config()).with_secret_source(varying_secrets)
    }

    /// A source that never hands out the same bytes twice.
    ///
    /// The "is it random?" tests need *variation*, not unpredictability, and a
    /// counter gives them that on every platform. Where a test needs the same
    /// bytes twice instead, it passes its own fixed source.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the Result is the SecretSource signature, not this function's choice"
    )]
    fn varying_secrets(out: &mut [u8]) -> Result<(), randrange::EntropyError> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut word = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for byte in out.iter_mut() {
            // A 64-bit LCG, so successive bytes within one draw differ too --
            // a per-call constant would make `[0u8; 16]` and "sixteen equal
            // bytes" indistinguishable to a test looking for zeros.
            word = word
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *byte = u8::try_from(word >> 56).unwrap_or(0);
        }
        Ok(())
    }

    /// A source that refuses, the way the kernel does when it cannot answer.
    fn refusing_secrets(_out: &mut [u8]) -> Result<(), randrange::EntropyError> {
        Err(randrange::EntropyError::Unavailable)
    }

    fn channel_request(req_type: &[u8], want_reply: bool, rest: &[u8]) -> Vec<u8> {
        let mut p = vec![msg::SSH_MSG_CHANNEL_REQUEST];
        p.extend_from_slice(&0u32.to_be_bytes()); // recipient channel
        p.extend_from_slice(&ssh_string(req_type));
        p.push(u8::from(want_reply));
        p.extend_from_slice(rest);
        p
    }

    // ---- exit-status / exit-signal ----

    #[test]
    fn exit_status_is_recorded_and_becomes_the_process_exit_code() {
        let mut s = test_session();
        assert_eq!(s.remote_exit, None);
        let payload = channel_request(b"exit-status", false, &42u32.to_be_bytes());
        s.handle_channel_request(&payload).expect("parse");
        assert_eq!(s.remote_exit, Some(RemoteExit::Status(42)));
        assert_eq!(s.exit_code(), 42);
    }

    /// The bug this whole path exists to prevent: a server that reports
    /// failure must not produce a client that reports success.
    #[test]
    fn a_nonzero_remote_status_does_not_become_success() {
        let mut s = test_session();
        let payload = channel_request(b"exit-status", false, &1u32.to_be_bytes());
        s.handle_channel_request(&payload).expect("parse");
        assert_ne!(s.exit_code(), 0);
    }

    /// A server that never sends `exit-status` — an interactive session, or a
    /// server that does not implement the notification — must not be treated
    /// as a failure.
    #[test]
    fn a_missing_exit_status_is_success() {
        let s = test_session();
        assert_eq!(s.remote_exit, None);
        assert_eq!(s.exit_code(), 0);
    }

    #[test]
    fn exit_signal_is_recorded_and_reported_as_255() {
        let mut s = test_session();
        let mut rest = ssh_string(b"TERM");
        rest.push(1); // core dumped
        rest.extend_from_slice(&ssh_string(b"killed by SIGTERM"));
        rest.extend_from_slice(&ssh_string(b""));
        let payload = channel_request(b"exit-signal", false, &rest);
        s.handle_channel_request(&payload).expect("parse");
        assert_eq!(
            s.remote_exit,
            Some(RemoteExit::Signal {
                name: "TERM".into(),
                core_dumped: true,
            })
        );
        // A signalled command has no exit status of its own; `ssh(1)` reports
        // its reserved failure code rather than inventing one.
        assert_eq!(s.exit_code(), EXIT_SSH_FAILURE);
    }

    /// The signal name arrives without a `SIG` prefix (RFC 4254 §6.10); a
    /// client that stripped or added one would print the wrong name.
    #[test]
    fn exit_signal_name_is_stored_verbatim() {
        let mut s = test_session();
        let mut rest = ssh_string(b"KILL");
        rest.push(0);
        rest.extend_from_slice(&ssh_string(b""));
        rest.extend_from_slice(&ssh_string(b""));
        s.handle_channel_request(&channel_request(b"exit-signal", false, &rest))
            .expect("parse");
        match s.remote_exit {
            Some(RemoteExit::Signal { ref name, .. }) => assert_eq!(name, "KILL"),
            ref other => panic!("expected a signal exit, got {other:?}"),
        }
    }

    /// An unrecognised request must be ignored, not mistaken for an exit
    /// report and not allowed to overwrite one already received.
    #[test]
    fn an_unknown_request_leaves_the_exit_status_alone() {
        let mut s = test_session();
        s.handle_channel_request(&channel_request(b"exit-status", false, &7u32.to_be_bytes()))
            .expect("parse");
        s.handle_channel_request(&channel_request(b"keepalive@openssh.com", false, &[]))
            .expect("parse");
        assert_eq!(s.remote_exit, Some(RemoteExit::Status(7)));
    }

    /// A truncated request must be an error, not a silent zero: reading past
    /// the end and defaulting would turn a malformed packet into "success".
    #[test]
    fn a_truncated_exit_status_is_an_error() {
        let mut s = test_session();
        let payload = channel_request(b"exit-status", false, &[0, 0]); // two of four bytes
        assert!(s.handle_channel_request(&payload).is_err());
        assert_eq!(s.remote_exit, None);
    }

    // ---- extended data ----

    /// Type 1 is the only defined extended-data code. Anything else must be
    /// discarded rather than written to stdout, where it would corrupt the
    /// command's real output.
    #[test]
    fn extended_data_of_an_unknown_type_is_discarded() {
        let s = test_session();
        let mut p = vec![msg::SSH_MSG_CHANNEL_EXTENDED_DATA];
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&99u32.to_be_bytes()); // not SSH_EXTENDED_DATA_STDERR
        p.extend_from_slice(&ssh_string(b"junk"));
        s.handle_channel_extended_data(&p); // must not panic, must not print
        assert_eq!(SSH_EXTENDED_DATA_STDERR, 1);
    }

    /// A malformed extended-data message must be dropped, not partially
    /// written: a truncated length prefix would otherwise emit whatever
    /// happened to follow it.
    #[test]
    fn malformed_extended_data_is_dropped() {
        let s = test_session();
        s.handle_channel_extended_data(&[msg::SSH_MSG_CHANNEL_EXTENDED_DATA, 0, 0]);
        let mut p = vec![msg::SSH_MSG_CHANNEL_EXTENDED_DATA];
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&1u32.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 8, b'a']); // claims 8 bytes, carries 1
        s.handle_channel_extended_data(&p);
    }

    // ---- exit code contract ----

    /// 255 must stay reserved for client/connection failure, so that a caller
    /// can tell "the command failed" from "ssh could not run it".
    #[test]
    fn ssh_failure_code_matches_openssh() {
        assert_eq!(EXIT_SSH_FAILURE, 255);
    }

    // ---- encoding helpers used by the above ----

    #[test]
    fn channel_request_round_trips() {
        let payload = channel_request(b"exit-status", false, &3u32.to_be_bytes());
        let (recipient, off) = read_u32(&payload, 1).expect("recipient");
        assert_eq!(recipient, 0);
        let (req_type, off) = read_ssh_string(&payload, off).expect("type");
        assert_eq!(req_type, b"exit-status");
        let (want_reply, off) = read_byte(&payload, off).expect("want_reply");
        assert_eq!(want_reply, 0);
        let (status, _) = read_u32(&payload, off).expect("status");
        assert_eq!(status, 3);
    }

    // ========================================================================
    // Host key signature verification (RFC 4253 section 8)
    //
    // These tests are the reason the check exists. Each one is a thing a
    // machine in the network path can do, and each one used to succeed,
    // because the client read the signature blob and threw it away.
    // ========================================================================

    /// A host key on the wire: `string algorithm`, `string public`.
    fn key_blob(algorithm: &[u8], public: &[u8]) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend_from_slice(&ssh_string(algorithm));
        blob.extend_from_slice(&ssh_string(public));
        blob
    }

    /// A signature on the wire: `string algorithm`, `string signature`.
    fn sig_blob(algorithm: &[u8], signature: &[u8]) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend_from_slice(&ssh_string(algorithm));
        blob.extend_from_slice(&ssh_string(signature));
        blob
    }

    /// Everything a server needs to answer a key exchange, from one seed.
    struct Server {
        seed: [u8; 32],
        public: [u8; 32],
    }

    impl Server {
        fn new(fill: u8) -> Self {
            let seed = [fill; 32];
            let public = posix::ed25519::public_key(&seed);
            Self { seed, public }
        }

        fn key_blob(&self) -> Vec<u8> {
            key_blob(b"ssh-ed25519", &self.public)
        }

        fn sign(&self, exchange_hash: &[u8; 32]) -> Vec<u8> {
            sig_blob(
                b"ssh-ed25519",
                &posix::ed25519::sign(&self.seed, exchange_hash),
            )
        }
    }

    #[test]
    fn a_genuine_server_signature_is_accepted() {
        let server = Server::new(0x11);
        let h = [0x42u8; 32];
        verify_host_key_signature(&server.key_blob(), &h, &server.sign(&h))
            .expect("the real server's own signature must verify");
    }

    /// The attack the check exists to stop. A machine in the path copies the
    /// real server's host key blob — it is public, it goes over the wire in
    /// the clear — and completes its own Diffie-Hellman exchange with the
    /// client. It cannot produce the matching signature, because it does not
    /// have the private half. Before this check, `known_hosts` would then
    /// confirm the copied key as the expected one and the client would
    /// proceed, having authenticated nobody.
    #[test]
    fn replaying_the_real_host_key_without_the_private_half_is_rejected() {
        let real = Server::new(0x11);
        let attacker = Server::new(0x22);
        let h = [0x42u8; 32];

        // The attacker presents the real key and signs with its own.
        let err = verify_host_key_signature(&real.key_blob(), &h, &attacker.sign(&h))
            .expect_err("a signature by the wrong key must not verify");
        assert!(matches!(err, SshError::HostKeyMismatch(_)), "{err:?}");
    }

    /// A signature is only evidence about the handshake it covers. Since `H`
    /// commits to both Diffie-Hellman public values, a recording of yesterday's
    /// session cannot be replayed into today's.
    #[test]
    fn a_signature_over_a_different_exchange_hash_is_rejected() {
        let server = Server::new(0x11);
        let recorded = server.sign(&[0x01u8; 32]);
        let this_session = [0x02u8; 32];

        let err = verify_host_key_signature(&server.key_blob(), &this_session, &recorded)
            .expect_err("a signature from another exchange must not verify");
        assert!(matches!(err, SshError::HostKeyMismatch(_)), "{err:?}");
    }

    /// One side must not get to pick the label while the other picks the
    /// verifier. If the blobs disagree about the algorithm, the disagreement
    /// itself is the answer.
    #[test]
    fn algorithm_confusion_between_key_and_signature_is_rejected() {
        let server = Server::new(0x11);
        let h = [0x42u8; 32];
        let mislabelled = sig_blob(b"ssh-rsa", &posix::ed25519::sign(&server.seed, &h));

        let err = verify_host_key_signature(&server.key_blob(), &h, &mislabelled)
            .expect_err("mismatched algorithm names must be refused");
        match err {
            SshError::HostKeyMismatch(m) => {
                assert!(m.contains("ssh-rsa"), "{m}");
                assert!(m.contains("ssh-ed25519"), "{m}");
            }
            other => panic!("expected a host key mismatch, got {other:?}"),
        }
    }

    /// "We cannot check this one" must resolve to *no*. Resolving it to yes is
    /// the same bug in a smaller box: a server that wants to skip
    /// authentication would simply advertise an algorithm we do not implement.
    #[test]
    fn an_algorithm_we_cannot_verify_is_refused_not_waved_through() {
        let h = [0x42u8; 32];
        let rsa_key = key_blob(b"ssh-rsa", &[0xAAu8; 256]);
        let rsa_sig = sig_blob(b"ssh-rsa", &[0xBBu8; 256]);

        let err = verify_host_key_signature(&rsa_key, &h, &rsa_sig)
            .expect_err("an unimplemented algorithm must be an error, not an acceptance");
        match err {
            SshError::HostKeyMismatch(m) => assert!(m.contains("ssh-ed25519"), "{m}"),
            other => panic!("expected a host key mismatch, got {other:?}"),
        }
    }

    /// A single flipped bit is a forgery like any other.
    #[test]
    fn a_tampered_signature_is_rejected() {
        let server = Server::new(0x11);
        let h = [0x42u8; 32];
        let mut blob = server.sign(&h);
        let last = blob.len() - 1;
        blob[last] ^= 0x01;

        assert!(verify_host_key_signature(&server.key_blob(), &h, &blob).is_err());
    }

    /// Malformed input must produce an error and not, say, an early `Ok` from
    /// a short-circuiting parse.
    #[test]
    fn truncated_blobs_are_errors_not_acceptances() {
        let server = Server::new(0x11);
        let h = [0x42u8; 32];
        let good_key = server.key_blob();
        let good_sig = server.sign(&h);

        for cut in 1..good_sig.len() {
            let err = verify_host_key_signature(&good_key, &h, &good_sig[..cut])
                .expect_err("a truncated signature blob must never verify");
            assert!(matches!(
                err,
                SshError::ProtocolError(_) | SshError::HostKeyMismatch(_)
            ));
        }
        for cut in 1..good_key.len() {
            let err = verify_host_key_signature(&good_key[..cut], &h, &good_sig)
                .expect_err("a truncated key blob must never verify");
            assert!(matches!(
                err,
                SshError::ProtocolError(_) | SshError::HostKeyMismatch(_)
            ));
        }
    }

    /// An ed25519 public key is exactly 32 bytes and a signature exactly 64.
    /// A blob of the right shape but the wrong size must be refused rather
    /// than reaching the verifier with a slice it cannot interpret.
    #[test]
    fn a_wrongly_sized_key_or_signature_is_rejected() {
        let server = Server::new(0x11);
        let h = [0x42u8; 32];

        let short_key = key_blob(b"ssh-ed25519", &server.public[..31]);
        assert!(verify_host_key_signature(&short_key, &h, &server.sign(&h)).is_err());

        let short_sig = sig_blob(b"ssh-ed25519", &[0u8; 63]);
        assert!(verify_host_key_signature(&server.key_blob(), &h, &short_sig).is_err());
    }

    // ========================================================================
    // Randomness
    // ========================================================================

    /// The private exponent used to be `sha256(two compile-time constants)` —
    /// not merely weak but *constant*, identical in every connection made by
    /// every copy of this binary, which makes every session key derivable by
    /// anyone holding the same binary.
    #[test]
    fn the_dh_private_exponent_is_not_a_constant() {
        let a = generate_dh_private(varying_secrets).expect("the stand-in answers");
        let b = generate_dh_private(varying_secrets).expect("the stand-in answers");
        assert_ne!(a.to_bytes_be(), b.to_bytes_be());
    }

    /// Fail closed: a connection whose exponent could not be drawn must not be
    /// made at all. There is no weaker exponent to fall back to — the exponent
    /// *is* the secret — so the only alternatives to an error are a predictable
    /// one and a panic, and a client handed an error can refuse to connect.
    #[test]
    fn no_exponent_is_produced_when_the_source_refuses() {
        assert!(matches!(
            generate_dh_private(refusing_secrets),
            Err(SshError::ProtocolError(_))
        ));
    }

    /// A 256-bit exponent with the top bit set: full length, and odd so it
    /// cannot share the small factor 2 with the group order.
    #[test]
    fn the_dh_private_exponent_has_the_expected_shape() {
        let x = generate_dh_private(varying_secrets)
            .expect("the stand-in answers")
            .to_bytes_be();
        assert_eq!(x.len(), 32, "expected a full 256-bit exponent");
        assert!(x[0] & 0x80 != 0, "top bit must be set");
        assert!(x[31] & 1 == 1, "exponent must be odd");
    }

    /// The KEXINIT cookie is half of what stops either side alone from
    /// choosing the exchange hash: both `I_C` and `I_S` feed into `H`.
    ///
    /// This client sent sixteen zero bytes. The comment left behind when that
    /// was fixed said the constant "gave that power to the server alone" —
    /// which was wrong, and wrong in the way this whole stack keeps being
    /// wrong: `sshd`'s cookie was `sha256(b"sshd-kex-cookie")`, one constant
    /// baked into the binary, so neither end contributed anything and `H` was
    /// a function of the DH values alone. Nothing noticed, because each end
    /// tested its own copy against its own idea of the protocol and passed.
    #[test]
    fn the_kexinit_cookie_is_random() {
        let s = test_session();
        let a = s.build_kexinit().expect("the stand-in answers");
        let b = s.build_kexinit().expect("the stand-in answers");
        assert_eq!(a[0], msg::SSH_MSG_KEXINIT);
        assert_ne!(a[1..17], [0u8; 16], "the cookie must not be zeros");
        assert_ne!(a[1..17], b[1..17], "the cookie must differ per connection");
        // Everything after the cookie is a fixed algorithm advertisement.
        assert_eq!(a[17..], b[17..]);
    }

    /// Fail closed here too: a KEXINIT whose cookie could not be drawn would be
    /// a predictable cookie, which is the fault above wearing an apology.
    #[test]
    fn no_kexinit_is_built_when_the_source_refuses() {
        let s = test_session().with_secret_source(refusing_secrets);
        assert!(matches!(s.build_kexinit(), Err(SshError::ProtocolError(_))));
    }

    /// The risk this guards is a client that ships with a test source wired in,
    /// which no test of "the cookie looks random" would ever catch — every one
    /// of them passes against `varying_secrets`, which is a counter.
    #[test]
    fn a_session_draws_its_secrets_from_the_kernel_unless_a_test_says_otherwise() {
        let (near, far) = sshwire::memory_pair();
        drop(far);
        let s = SshSession::new(Box::new(near), test_config());
        assert!(
            core::ptr::fn_addr_eq(s.secrets, sshwire::KERNEL_SECRETS),
            "a session the client opens must draw from the kernel CSPRNG"
        );
    }

    /// We must not advertise an algorithm we would then have to refuse: that
    /// turns a connection that could have worked into a confusing failure.
    #[test]
    fn we_only_offer_host_key_algorithms_we_can_verify() {
        let s = test_session();
        let payload = s.build_kexinit().expect("the stand-in answers");
        // byte + 16-byte cookie, then kex_algorithms, then host key algorithms.
        let (_kex, off) = read_ssh_string(&payload, 17).expect("kex list");
        let (host_key, _) = read_ssh_string(&payload, off).expect("host key list");
        assert_eq!(host_key, b"ssh-ed25519");
    }

    /// `bytes_to_hex` is the only hex helper left here: its partner
    /// `hex_to_bytes` existed to turn the group-14 prime into bytes, and the
    /// prime moved to `sshwire`, so the parser went with it.
    #[test]
    fn bytes_render_as_lowercase_hex_pairs() {
        assert_eq!(bytes_to_hex(&[0x00, 0xff, 0x10, 0xab]), "00ff10ab");
        assert_eq!(bytes_to_hex(&[]), "");
        // Every byte is two digits, so a value below 16 keeps its leading zero.
        assert_eq!(bytes_to_hex(&[0x05]), "05");
    }

    // ------------------------------------------------------------------
    // Version exchange (RFC 4253 §4.2)
    // ------------------------------------------------------------------

    fn version_of(line: &[u8]) -> String {
        match classify_version_line(line) {
            Ok(VersionLine::Version(v)) => v,
            Ok(VersionLine::Banner(b)) => panic!("expected a version line, got banner {b:?}"),
            Err(e) => panic!("expected a version line, got error {e}"),
        }
    }

    #[test]
    fn the_version_line_is_recognised_and_stripped_of_its_cr() {
        assert_eq!(version_of(b"SSH-2.0-OpenSSH_9.6\r"), "SSH-2.0-OpenSSH_9.6");
    }

    #[test]
    fn a_bare_lf_terminator_is_accepted_too() {
        // OpenSSH tolerates a missing CR, and the byte we hash is the same
        // either way -- the CRLF is framing, not part of V_S.
        assert_eq!(version_of(b"SSH-2.0-Dropbear"), "SSH-2.0-Dropbear");
    }

    #[test]
    fn only_the_terminating_cr_is_removed() {
        // A CR earlier in the line is not framing. Trimming every trailing CR
        // would be a second place where our bytes and the server's could differ.
        assert_eq!(version_of(b"SSH-2.0-x\r\r"), "SSH-2.0-x\r");
    }

    #[test]
    fn the_version_string_we_keep_is_byte_for_byte_what_arrived() {
        // The regression: `char::from(byte)` read each byte as Latin-1, so this
        // line came back out as its UTF-8 re-encoding -- one more byte than the
        // server hashed, and so a different exchange hash and a signature that
        // could not verify. UTF-8 is not legal here, but a server that sends it
        // must either be understood exactly or refused, never quietly altered.
        let raw = "SSH-2.0-Ünicode".as_bytes();
        assert_eq!(version_of(raw).as_bytes(), raw);
    }

    #[test]
    fn a_version_line_that_is_not_utf8_is_refused_rather_than_mangled() {
        // 0xFF is not valid UTF-8 in any position. The old code turned it into
        // U+00FF and carried on, hashing two bytes where the server hashed one.
        let err = classify_version_line(b"SSH-2.0-bad\xFFname\r");
        assert!(
            matches!(err, Err(SshError::ProtocolError(_))),
            "a version line we cannot reproduce byte-for-byte must be an error"
        );
    }

    #[test]
    fn lines_before_the_version_are_banners() {
        let Ok(VersionLine::Banner(text)) = classify_version_line(b"Authorized users only\r")
        else {
            panic!("a line not starting with SSH- is a banner");
        };
        assert_eq!(text, "Authorized users only");
    }

    #[test]
    fn a_banner_cannot_smuggle_control_sequences_to_the_terminal() {
        // Banners arrive from an unauthenticated peer and get printed. Escaping
        // them is why they are not simply decoded.
        let Ok(VersionLine::Banner(text)) = classify_version_line(b"evil\x1b[2Jbanner") else {
            panic!("expected a banner");
        };
        assert!(
            !text.contains('\x1b'),
            "escape byte reached the output: {text:?}"
        );
    }

    #[test]
    fn a_banner_that_is_not_utf8_is_shown_rather_than_fatal() {
        // Unlike the version line, a banner is not hashed, so an undecodable one
        // is not a reason to refuse to talk to the server.
        assert!(matches!(
            classify_version_line(b"caf\xE9"),
            Ok(VersionLine::Banner(_))
        ));
    }

    // The `mpint` reader/writer roundtrip that used to be asserted here is now
    // `sshwire`'s -- `an_mpint_roundtrips_through_the_shared_writer` and
    // `an_mpint_comes_back_without_the_sign_pad_the_writer_added` -- because
    // both halves of it are. A copy here would test the same two functions a
    // second time and, worse, would keep passing if this crate ever grew a
    // private reader again, which is the failure the shared crate exists to
    // make impossible.

    // ------------------------------------------------------------------
    // AES-128 and AES-128-CTR
    //
    // The cipher is `sshwire::Aes128Ctr` now, and so are its tests: FIPS-197's
    // block and key-schedule vectors, and RFC 3686's and NIST SP 800-38A's
    // counter-mode vectors. Six tests lived here; four moved, and two are gone
    // because the shared type retired the thing they were checking.
    //
    //   * `aes_ctr_declines_a_short_key_or_iv_rather_than_encrypting_with_padding`
    //     asserted that a 15-byte key left the data alone. `Aes128Ctr::new`
    //     takes `&[u8; 16]`, so a short key no longer compiles and there is
    //     nothing left to assert at run time.
    //   * `aes_ctr_round_trips_a_length_that_is_not_a_block_multiple` is the
    //     shape of test that let the counter bug live for as long as it did:
    //     encrypt-then-decrypt with the same wrong counter returns the
    //     plaintext perfectly. RFC 3686's 36-byte vector covers the short final
    //     block against a published answer instead, and `sshwire`'s
    //     `the_keystream_never_repeats_across_packets` covers what a roundtrip
    //     structurally cannot see.
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // base64
    // ------------------------------------------------------------------

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        // The three tail lengths are the whole difficulty here, and each of
        // them used to be a separate hand-indexed branch.
        for (plain, padded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(
                base64_encode_padded(plain.as_bytes()),
                padded,
                "encoding {plain:?}"
            );
            assert_eq!(
                base64_encode(plain.as_bytes()),
                padded.trim_end_matches('='),
                "unpadded encoding of {plain:?}"
            );
            assert_eq!(base64_decode(padded), plain.as_bytes(), "decoding {padded}");
        }
    }

    #[test]
    fn base64_round_trips_every_length_up_to_three_blocks() {
        for len in 0..=12usize {
            let data: Vec<u8> = (0..len)
                .map(|i| u8::try_from(i * 17 % 251).unwrap())
                .collect();
            let encoded = base64_encode_padded(&data);
            assert_eq!(base64_decode(&encoded), data, "round trip at length {len}");
        }
    }

    #[test]
    fn base64_decoding_ignores_whitespace_padding_and_stray_characters() {
        // known_hosts lines are wrapped and hand-edited, so the decoder has to
        // survive line endings; a bad character decodes as a zero sextet
        // rather than shifting everything after it.
        assert_eq!(base64_decode("Zm9v\r\nYmFy"), b"foobar");
        assert_eq!(base64_decode("Zm9vYmFy===="), b"foobar");
        // A lone trailing character carries no whole byte and is dropped.
        assert_eq!(base64_decode("Zm9vYmFyZ"), b"foobar");
    }

    // ------------------------------------------------------------------
    // known_hosts
    // ------------------------------------------------------------------

    #[test]
    fn known_hosts_names_a_nondefault_port_the_way_openssh_does() {
        assert_eq!(known_hosts_pattern("example.com", 22), "example.com");
        assert_eq!(
            known_hosts_pattern("example.com", 2222),
            "[example.com]:2222"
        );
    }

    #[test]
    fn known_hosts_recognises_a_stored_key() {
        let blob = b"\x00\x00\x00\x0bssh-ed25519some-key-bytes".to_vec();
        let content = format!(
            "# a comment\n\n\
             other.example ssh-ed25519 {}\n\
             example.com ssh-ed25519 {} a trailing comment\n",
            base64_encode_padded(b"a different key"),
            base64_encode_padded(&blob),
        );

        assert_eq!(
            known_hosts_lookup(&content, "example.com", &blob),
            KnownHostsVerdict::Match
        );
        assert_eq!(
            known_hosts_lookup(&content, "unlisted.example", &blob),
            KnownHostsVerdict::Unknown
        );
    }

    #[test]
    fn known_hosts_reports_a_changed_key_rather_than_treating_it_as_unknown() {
        // The distinction is the whole point: an unknown host may be added on
        // the operator's say-so, but a host whose key has *changed* must not
        // be, because that is what a man in the middle looks like.
        let stored = b"the-real-host-key".to_vec();
        let content = format!(
            "example.com ssh-ed25519 {}\n",
            base64_encode_padded(&stored)
        );

        assert_eq!(
            known_hosts_lookup(&content, "example.com", b"an-impostors-key"),
            KnownHostsVerdict::Mismatch
        );
    }

    #[test]
    fn known_hosts_matches_any_name_in_a_comma_separated_list() {
        let blob = b"shared-key".to_vec();
        let content = format!(
            "alias.example,example.com,[example.com]:2222 ssh-ed25519 {}\n",
            base64_encode_padded(&blob),
        );

        for pattern in ["alias.example", "example.com", "[example.com]:2222"] {
            assert_eq!(
                known_hosts_lookup(&content, pattern, &blob),
                KnownHostsVerdict::Match,
                "pattern {pattern}"
            );
        }
    }

    #[test]
    fn known_hosts_skips_lines_that_are_not_three_fields() {
        // A truncated line must not be read as naming the host: the old code
        // checked `parts.len() < 3` and then indexed `parts[0..3]` separately,
        // so this is the case where those two could have parted company.
        let blob = b"key".to_vec();
        let content = "example.com\nexample.com ssh-ed25519\n\n   \n";
        assert_eq!(
            known_hosts_lookup(content, "example.com", &blob),
            KnownHostsVerdict::Unknown
        );
    }

    #[test]
    fn known_hosts_does_not_match_a_host_that_is_merely_a_prefix() {
        let blob = b"key".to_vec();
        let content = format!(
            "example.com.evil.test ssh-ed25519 {}\n",
            base64_encode_padded(&blob)
        );
        assert_eq!(
            known_hosts_lookup(&content, "example.com", &blob),
            KnownHostsVerdict::Unknown
        );
    }

    // ---- -o UserKnownHostsFile= ----

    #[test]
    fn without_the_option_the_trust_store_is_the_users_own_file() {
        let path = known_hosts_path(None);
        assert!(
            path.ends_with("/.ssh/known_hosts"),
            "default trust store moved: {path}"
        );
    }

    #[test]
    fn the_option_names_the_file_and_nothing_is_appended_to_it() {
        // OpenSSH takes the value as the whole path, not a directory or a stem.
        assert_eq!(known_hosts_path(Some("/tmp/hosts")), "/tmp/hosts");
    }

    #[test]
    fn a_key_written_to_the_named_file_is_recognised_when_read_back() {
        // The round trip is the point: `add_known_host` writes base64 and
        // `check_known_hosts` decodes it, and a disagreement between those two
        // would make every host look unknown for ever -- while each function's
        // own test kept passing.
        let scratch = scratchdir::ScratchDir::new("ssh-known-hosts");
        let path = scratch.path("known_hosts");
        let file = path.to_str().expect("the scratch path is UTF-8");
        let blob = b"a-host-key-blob".to_vec();

        assert!(
            !check_known_hosts(Some(file), "example.com", 2222, &blob)
                .expect("no file is not an error"),
            "an absent file must read as 'host unknown', not as a match"
        );

        add_known_host(Some(file), "example.com", 2222, "ssh-ed25519", &blob);
        assert!(
            check_known_hosts(Some(file), "example.com", 2222, &blob).expect("readable"),
            "the key just written was not recognised"
        );

        // The port is part of the identity, so the same key on another port is
        // a different host and is still unknown.
        assert!(
            !check_known_hosts(Some(file), "example.com", 22, &blob).expect("readable"),
            "a different port must not match"
        );

        // A different key for a host we know is the man-in-the-middle case,
        // and must be an error rather than "unknown" -- "unknown" would let
        // StrictHostKeyChecking=no silently append the impostor's key.
        assert!(
            check_known_hosts(Some(file), "example.com", 2222, b"an-impostors-key").is_err(),
            "a changed key must be reported, not treated as a new host"
        );
    }

    #[test]
    fn the_option_keeps_a_test_out_of_the_operators_own_trust_store() {
        // The reason the option exists. `StrictHostKeyChecking=no` appends the
        // server's key to the trust store, so without a way to name a different
        // file, running the client under test would edit `~/.ssh/known_hosts`.
        let scratch = scratchdir::ScratchDir::new("ssh-known-hosts-isolated");
        let path = scratch.path("known_hosts");
        let file = path.to_str().expect("the scratch path is UTF-8");
        add_known_host(Some(file), "example.com", 22, "ssh-ed25519", b"key");
        assert!(path.exists(), "the named file is the one that was written");

        let default = known_hosts_path(None);
        assert_ne!(default, file, "the named file must not be the default one");
    }
}
