//! Slate OS SSH-2 Client
//!
//! A simplified SSH-2 protocol client for SlateOS. Supports password
//! authentication, interactive shell sessions, and remote command execution.
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
use std::env;
use std::fmt;
use std::io::{self, Read, Write};
use std::process;

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
#[cfg(target_arch = "x86_64")]
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

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
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

/// Open a TCP connection to the given IP (network byte order) and port.
/// Returns a handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, SshError> {
    // SAFETY: We pass a valid IP and port. The kernel returns a handle (>= 0)
    // or a negative error code. No pointers are involved.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        return Err(SshError::ConnectionFailed(format!(
            "tcp_connect returned {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Send data on a TCP connection. Returns the number of bytes actually sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, SshError> {
    // SAFETY: We pass a valid handle and a pointer to a byte buffer with its
    // correct length. The kernel reads up to `data.len()` bytes from the buffer.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshError::SendFailed);
    }
    Ok(ret as usize)
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), SshError> {
    // Walking a shrinking `rest` rather than an offset keeps "how much is left"
    // and "where that starts" from being two facts that can disagree.
    let mut rest = data;
    while !rest.is_empty() {
        let sent = tcp_send(handle, rest)?;
        if sent == 0 {
            return Err(SshError::SendFailed);
        }
        rest = rest.get(sent..).ok_or(SshError::SendFailed)?;
    }
    Ok(())
}

/// Receive from a TCP connection, returning the prefix of `buf` the kernel
/// actually filled. An empty slice means the peer has closed.
///
/// Handing back the slice rather than a count is deliberate: the kernel's
/// number is turned into a range in exactly one place, here, where a byte count
/// that does not fit the buffer we handed over is rejected as `RecvFailed`
/// rather than travelling into a caller's `buf[..n]` and panicking there.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<&[u8], SshError> {
    // SAFETY: We pass a valid handle and a mutable buffer pointer with its
    // correct length. The kernel writes at most `buf.len()` bytes into the buffer.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshError::RecvFailed);
    }
    let received = usize::try_from(ret).map_err(|_| SshError::RecvFailed)?;
    buf.get(..received).ok_or(SshError::RecvFailed)
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: We pass a valid handle. The kernel deallocates internal state.
    // Ignoring the return value is safe: the handle becomes invalid regardless.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum SshError {
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

// ============================================================================
// SSH-2 constants
// ============================================================================

/// Our version identification string.
const SSH_VERSION_STRING: &str = "SSH-2.0-SlateOS_1.0";

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

/// Maximum SSH packet payload size we handle.
const MAX_PACKET_SIZE: usize = 35000;

/// Minimum block size for packet alignment.
const BLOCK_SIZE_UNENCRYPTED: usize = 8;

/// Largest `SSH_MSG_CHANNEL_DATA` payload we will send in one packet.
///
/// This is the same 32 KiB that `channel_open` advertises to the server as our
/// maximum packet size; it was written out as a bare `32768` in both places,
/// which is two independent copies of one promise. RFC 4254 s5.1 makes the
/// advertised figure binding, so a change to one that missed the other would
/// have us overrun a limit we had just announced.
const MAX_CHANNEL_CHUNK: usize = 32768;

/// Build a raw SSH binary packet from a payload.
///
/// Format: `[u32 packet_length][u8 padding_length][payload][random_padding]`
///
/// Before encryption is active, the MAC is empty and padding is zero-filled.
fn build_packet(payload: &[u8], encrypted: bool, seq: u32, enc: &EncryptionState) -> Vec<u8> {
    let block_size = if encrypted {
        enc.block_size.max(8)
    } else {
        BLOCK_SIZE_UNENCRYPTED
    };

    // Compute padding: packet_length + padding_length + payload must be
    // a multiple of block_size, with at least 4 bytes of padding.
    //
    // Saturating rather than wrapping throughout: every input here is ours
    // (a payload we built, a block size of 8 or 16), so none of these can
    // actually overflow — but a saturating form that produces a too-long
    // packet the server rejects beats a wrapping one that silently produces a
    // *valid-looking* packet describing the wrong length.
    let unpadded = payload.len().saturating_add(1); // padding_length byte + payload
    let overhang = unpadded
        .saturating_add(4)
        .checked_rem(block_size)
        .unwrap_or(0);
    let mut padding = block_size.saturating_sub(overhang);
    if padding < 4 {
        padding = padding.saturating_add(block_size);
    }

    let packet_length = unpadded.saturating_add(padding);
    let total_len = packet_length.saturating_add(4);
    let mut pkt = Vec::with_capacity(total_len);
    pkt.extend_from_slice(
        &u32::try_from(packet_length)
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    pkt.push(u8::try_from(padding).unwrap_or(u8::MAX));
    pkt.extend_from_slice(payload);
    // Zero-fill padding (simplified; a real implementation would use random bytes).
    pkt.resize(total_len, 0);

    if encrypted {
        // Compute MAC over sequence number + unencrypted packet.
        let mac = compute_mac(&enc.mac_key_c2s, seq, &pkt);
        // Encrypt the packet portion (after constructing MAC on plaintext).
        encrypt_packet_aes_ctr(&mut pkt, &enc.enc_key_c2s, &enc.iv_c2s, seq);
        pkt.extend_from_slice(&mac);
    }

    pkt
}

/// Read one SSH binary packet from the TCP stream. Returns the payload.
fn read_packet(
    handle: u64,
    buf: &mut StreamBuffer,
    encrypted: bool,
    seq: u32,
    enc: &EncryptionState,
) -> Result<Vec<u8>, SshError> {
    let block_size = if encrypted {
        enc.block_size.max(8)
    } else {
        BLOCK_SIZE_UNENCRYPTED
    };

    // Peek/decrypt the first block to get packet_length.
    let first_block = buf.peek(handle, block_size)?;
    let first_decrypted = if encrypted {
        decrypt_block_aes_ctr(first_block, &enc.enc_key_s2c, &enc.iv_s2c, seq, 0)
    } else {
        first_block.to_vec()
    };

    let [len_b0, len_b1, len_b2, len_b3, ..] = *first_decrypted.as_slice() else {
        return Err(SshError::ProtocolError("short packet header".into()));
    };
    let packet_length = u32::from_be_bytes([len_b0, len_b1, len_b2, len_b3]) as usize;

    // Both bounds matter, and only the upper one used to be checked. A server
    // announcing `packet_length` 0 or 1 produced a `decrypted` of four or five
    // bytes, and the `decrypted[4]` below — the padding-length byte, which the
    // wire format says is inside the packet — then indexed off the end and
    // panicked. RFC 4253 s6 puts the floor at one padding-length byte plus the
    // four-byte minimum padding, so anything under 5 is malformed by
    // definition and is rejected here rather than surviving to be indexed.
    if packet_length < 5 || packet_length > MAX_PACKET_SIZE {
        return Err(SshError::ProtocolError(format!(
            "bad packet length: {packet_length}"
        )));
    }

    let mac_len = if encrypted { enc.mac_len } else { 0 };
    let body_len = packet_length
        .checked_add(4)
        .ok_or_else(|| SshError::ProtocolError("packet length overflow".into()))?;
    let total = body_len
        .checked_add(mac_len)
        .ok_or_else(|| SshError::ProtocolError("packet length overflow".into()))?;

    let raw = buf.take(handle, total)?;

    // Decrypt if needed.
    let decrypted = if encrypted {
        let (pkt_data, mac_data) = raw.split_at(body_len);
        let mut dec = pkt_data.to_vec();
        decrypt_packet_aes_ctr(&mut dec, &enc.enc_key_s2c, &enc.iv_s2c, seq);

        // Verify MAC. The `get` failing means the server sent a short MAC, and
        // that has to *reject*: the previous form guarded the comparison with
        // `mac_data.len() >= mac_len`, so a truncated MAC skipped the check
        // entirely and the packet was accepted unauthenticated.
        let expected_mac = compute_mac(&enc.mac_key_s2c, seq, &dec);
        let received_mac = mac_data
            .get(..mac_len)
            .ok_or_else(|| SshError::ProtocolError("truncated MAC".into()))?;
        if !constant_time_eq(received_mac, &expected_mac) {
            return Err(SshError::ProtocolError("MAC verification failed".into()));
        }
        dec
    } else {
        raw.get(..body_len)
            .ok_or_else(|| SshError::ProtocolError("short packet".into()))?
            .to_vec()
    };

    // Layout: [0..4] length, [4] padding_length, [5..] payload then padding.
    let padding_length = usize::from(
        *decrypted
            .get(4)
            .ok_or_else(|| SshError::ProtocolError("short packet".into()))?,
    );
    let payload_len = packet_length
        .checked_sub(1)
        .and_then(|n| n.checked_sub(padding_length))
        .ok_or_else(|| SshError::ProtocolError("invalid padding length".into()))?;
    let payload_end = payload_len
        .checked_add(5)
        .ok_or_else(|| SshError::ProtocolError("invalid padding length".into()))?;
    Ok(decrypted
        .get(5..payload_end)
        .ok_or_else(|| SshError::ProtocolError("short packet".into()))?
        .to_vec())
}

// ============================================================================
// Stream buffer — accumulates TCP data for packet parsing
// ============================================================================

struct StreamBuffer {
    data: Vec<u8>,
    pos: usize,
}

impl StreamBuffer {
    fn new() -> Self {
        Self {
            data: Vec::with_capacity(8192),
            pos: 0,
        }
    }

    /// Return how many unconsumed bytes are buffered.
    fn available(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// The unconsumed bytes.
    fn unread(&self) -> &[u8] {
        self.data.get(self.pos..).unwrap_or_default()
    }

    /// Read once from TCP and append. Errors if the peer has closed.
    fn fill_once(&mut self, handle: u64) -> Result<(), SshError> {
        // Compact if we have consumed a lot, so a long session does not grow
        // `data` without bound behind an ever-advancing `pos`.
        if self.pos > 4096 {
            self.data.drain(..self.pos);
            self.pos = 0;
        }
        let mut tmp = [0u8; 8192];
        let received = tcp_recv(handle, &mut tmp)?;
        if received.is_empty() {
            return Err(SshError::ProtocolError("connection closed".into()));
        }
        self.data.extend_from_slice(received);
        Ok(())
    }

    /// Read from TCP until at least `n` bytes are buffered, then return them
    /// without consuming.
    ///
    /// Fusing "make sure `n` bytes are there" with "hand me `n` bytes" is the
    /// point: as two separate calls it was possible — and, at the two
    /// `available() >= block_size` shortcuts in `try_recv_packet`, actually
    /// the case — for a caller to reach the taking half with a different `n`
    /// than it had ensured, and the taking half indexed unchecked.
    fn peek(&mut self, handle: u64, n: usize) -> Result<&[u8], SshError> {
        while self.available() < n {
            self.fill_once(handle)?;
        }
        self.unread().get(..n).ok_or(SshError::RecvFailed)
    }

    /// Read from TCP until at least `n` bytes are buffered, then consume them.
    fn take(&mut self, handle: u64, n: usize) -> Result<Vec<u8>, SshError> {
        let taken = self.peek(handle, n)?.to_vec();
        self.pos = self.pos.saturating_add(n);
        Ok(taken)
    }
}

// ============================================================================
// SSH data encoding helpers
// ============================================================================

/// Encode a string/bytes as SSH `string` type: u32 length + data.
fn ssh_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().saturating_add(4));
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// Read an SSH `string` from a byte slice at the given offset.
/// Returns (value, new_offset).
///
/// The length prefix is entirely the server's to choose, so it is added to the
/// offset with `checked_add` and turned into a range only by `get`, which
/// returns `None` rather than panicking when the server claims more bytes than
/// it sent. The previous `offset + 4 > data.len()` guard was itself the hazard:
/// the addition it performed to decide whether indexing was safe could
/// overflow, and on overflow it concluded that it was.
fn read_ssh_string(data: &[u8], offset: usize) -> Result<(&[u8], usize), SshError> {
    let (len, start) = read_u32(data, offset)?;
    let len = usize::try_from(len)
        .map_err(|_| SshError::ProtocolError(format!("string length {len} out of range")))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| SshError::ProtocolError("string length overflow".into()))?;
    let value = data.get(start..end).ok_or_else(|| {
        SshError::ProtocolError(format!(
            "string length {len} exceeds packet (have {})",
            data.len().saturating_sub(start)
        ))
    })?;
    Ok((value, end))
}

/// Read a u32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> Result<(u32, usize), SshError> {
    let bytes = data
        .get(offset..)
        .and_then(<[u8]>::first_chunk::<4>)
        .ok_or_else(|| SshError::ProtocolError("truncated u32".into()))?;
    Ok((u32::from_be_bytes(*bytes), offset.saturating_add(4)))
}

/// Read a byte from a slice at the given offset.
fn read_byte(data: &[u8], offset: usize) -> Result<(u8, usize), SshError> {
    let byte = *data
        .get(offset)
        .ok_or_else(|| SshError::ProtocolError("truncated byte".into()))?;
    Ok((byte, offset.saturating_add(1)))
}

/// Encode an SSH `mpint` from a big-endian unsigned byte array.
/// Prepends a zero byte if the high bit is set.
fn encode_mpint(value: &[u8]) -> Vec<u8> {
    // Strip leading zeros.
    let stripped = strip_leading_zeros(value);
    let Some(&high) = stripped.first() else {
        return vec![0, 0, 0, 0]; // mpint zero
    };
    // A leading byte with the top bit set would read as negative, and mpint is
    // two's complement; the zero pad is what keeps it unsigned.
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

/// Read an SSH `mpint` from a byte slice, returning unsigned big-endian bytes.
fn read_mpint(data: &[u8], offset: usize) -> Result<(Vec<u8>, usize), SshError> {
    let (raw, next) = read_ssh_string(data, offset)?;
    // Strip leading zero padding that SSH adds for sign.
    let stripped = strip_leading_zeros(raw);
    Ok((stripped.to_vec(), next))
}

fn strip_leading_zeros(data: &[u8]) -> &[u8] {
    // `trim_ascii_start` is for whitespace; this is the same shape for zeros,
    // expressed so the "everything was zero" case is the empty slice by
    // construction rather than by a `position`/`unwrap_or(len)` pairing.
    let mut rest = data;
    while let [0, tail @ ..] = rest {
        rest = tail;
    }
    rest
}

// ============================================================================
// Minimal big-integer arithmetic for Diffie-Hellman
//
// Enough of a bignum to run the group-14 key exchange, and no more. The
// representation is little-endian 32-bit limbs: little-endian because every
// algorithm here -- addition, multiplication, shifting, division -- carries
// from the least significant end upward, so storing that end first makes a
// digit's place its own index instead of `len - 1 - i`; 32-bit limbs because
// the products fit exactly in a `u64`, which is the widest exact integer we
// have.
//
// This was previously big-endian *bytes*, which cost on both counts. See
// `div_rem` for what that cost in practice.
// ============================================================================

/// Unsigned big integer.
#[derive(Clone, Debug)]
struct BigUint {
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

    fn zero() -> Self {
        Self { limbs: Vec::new() }
    }

    fn one() -> Self {
        Self { limbs: vec![1] }
    }

    /// Read a big-endian byte string.
    fn from_bytes_be(data: &[u8]) -> Self {
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
    fn to_bytes_be(&self) -> Vec<u8> {
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

    fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Return the number of bits.
    fn bit_length(&self) -> usize {
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
    fn bit(&self, pos: usize) -> bool {
        self.limbs
            .get(pos / 32)
            .is_some_and(|&limb| (limb >> (pos % 32)) & 1 == 1)
    }

    /// Compare magnitudes.
    fn cmp_unsigned(&self, other: &BigUint) -> std::cmp::Ordering {
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
    fn mod_pow(&self, exp: &BigUint, modulus: &BigUint) -> BigUint {
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
    fn mod_mul(&self, other: &BigUint, modulus: &BigUint) -> BigUint {
        let product = self.mul(other);
        product.mod_reduce(modulus)
    }

    /// self mod modulus.
    fn mod_reduce(&self, modulus: &BigUint) -> BigUint {
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
    #[allow(clippy::arithmetic_side_effects)]
    fn mul(&self, other: &BigUint) -> BigUint {
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
    #[allow(clippy::arithmetic_side_effects)]
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
    #[allow(clippy::arithmetic_side_effects)]
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
    #[allow(clippy::arithmetic_side_effects)]
    fn div_rem(&self, divisor: &BigUint) -> (BigUint, BigUint) {
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
    fn sub(&self, other: &BigUint) -> BigUint {
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

// ============================================================================
// HMAC-SHA256
// ============================================================================

/// Compute HMAC-SHA256(key, data).
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
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
fn compute_mac(key: &[u8], seq: u32, packet: &[u8]) -> Vec<u8> {
    let mut mac_input = Vec::with_capacity(packet.len().saturating_add(4));
    mac_input.extend_from_slice(&seq.to_be_bytes());
    mac_input.extend_from_slice(packet);
    hmac_sha256(key, &mac_input).to_vec()
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
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

/// Increment a 16-byte big-endian counter by 1.
fn increment_counter(counter: &mut [u8; 16]) {
    for b in counter.iter_mut().rev() {
        let (val, overflow) = b.overflowing_add(1);
        *b = val;
        if !overflow {
            return;
        }
    }
}

/// Encrypt a packet in-place using AES-128-CTR.
///
/// For SSH, the counter starts from the IV and increments for each 16-byte
/// block within the packet. Across packets, we track the IV globally (the
/// EncryptionState's IV is incremented after each packet).
fn encrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], _seq: u32) {
    // `first_chunk` performs the length check and the copy in one step, so the
    // `len() < 16` guard and the `[..16]` slice that followed it can no longer
    // disagree about which 16 bytes were checked.
    let (Some(key16), Some(iv16)) = (key.first_chunk::<16>(), iv.first_chunk::<16>()) else {
        return;
    };
    let round_keys = aes128_key_expand(key16);

    // One counter walked forward across the packet. It used to be re-derived
    // from the IV for every block, by incrementing a fresh copy `block_idx`
    // times, which made an N-block packet cost O(N^2) counter steps: a 32 KiB
    // packet paid over two million increments to produce 2048 blocks. Walking
    // it also removes the `start`/`end`/`i - start` index triple, whose short
    // final block was the only thing keeping the last `min` honest.
    let mut counter = *iv16;
    for chunk in packet.chunks_mut(16) {
        let keystream = aes128_encrypt_block(&counter, &round_keys);
        for (b, k) in chunk.iter_mut().zip(keystream) {
            *b ^= k;
        }
        increment_counter(&mut counter);
    }
}

/// Decrypt a packet in-place using AES-128-CTR (same as encrypt for CTR mode).
fn decrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], _seq: u32) {
    encrypt_packet_aes_ctr(packet, key, iv, _seq);
}

/// Decrypt just the first block to peek at packet_length.
fn decrypt_block_aes_ctr(
    data: &[u8],
    key: &[u8],
    iv: &[u8],
    _seq: u32,
    _block_idx: usize,
) -> Vec<u8> {
    let mut result = data.to_vec();
    encrypt_packet_aes_ctr(&mut result, key, iv, _seq);
    result
}

// ============================================================================
// Encryption state tracking
// ============================================================================

struct EncryptionState {
    /// Encryption key for client-to-server.
    enc_key_c2s: Vec<u8>,
    /// Encryption key for server-to-client.
    enc_key_s2c: Vec<u8>,
    /// MAC key for client-to-server.
    mac_key_c2s: Vec<u8>,
    /// MAC key for server-to-client.
    mac_key_s2c: Vec<u8>,
    /// IV for client-to-server.
    iv_c2s: Vec<u8>,
    /// IV for server-to-client.
    iv_s2c: Vec<u8>,
    /// Block size for the cipher.
    block_size: usize,
    /// MAC length in bytes.
    mac_len: usize,
}

impl EncryptionState {
    fn new() -> Self {
        Self {
            enc_key_c2s: Vec::new(),
            enc_key_s2c: Vec::new(),
            mac_key_c2s: Vec::new(),
            mac_key_s2c: Vec::new(),
            iv_c2s: Vec::new(),
            iv_s2c: Vec::new(),
            block_size: 16,
            mac_len: 32, // HMAC-SHA256
        }
    }
}

// ============================================================================
// Diffie-Hellman group 14 (2048-bit MODP group, RFC 3526)
// ============================================================================

/// The 2048-bit MODP prime from RFC 3526 (group 14).
const DH_GROUP14_P_HEX: &str = concat!(
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

/// DH generator g = 2.
const DH_G: u8 = 2;

/// Parse a hex string into bytes. A trailing odd digit is dropped.
fn hex_to_bytes(hex: &str) -> Vec<u8> {
    // `chunks_exact(2)` *is* the "pairs of digits, ignore a dangling one"
    // rule, so the `i + 1 < len` guard and the manual `i += 2` stride that
    // used to encode it — the two places a hex walk goes wrong — are gone.
    hex.as_bytes()
        .chunks_exact(2)
        .filter_map(|pair| match *pair {
            [hi, lo] => Some((hex_digit(hi) << 4) | hex_digit(lo)),
            _ => None,
        })
        .collect()
}

fn hex_digit(c: u8) -> u8 {
    // A non-hex byte reads as 0; every caller passes a literal from this file.
    char::from(c)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
        .unwrap_or(0)
}

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
/// The bytes now come from `posix::random`, which reaches the kernel CSPRNG
/// and *fails* rather than substituting anything when it cannot.
///
/// # Errors
///
/// Returns an error if the kernel cannot supply random bytes. There is no
/// fallback on purpose: a caller handed an error can refuse to connect, a
/// caller handed predictable bytes cannot know to.
fn generate_dh_private() -> Result<BigUint, SshError> {
    // 256 bits, matching the ~128-bit security the group14 prime provides.
    let mut bytes = [0u8; 32];
    posix::random::fill(&mut bytes).map_err(|e| {
        SshError::ProtocolError(format!(
            "cannot generate a Diffie-Hellman private key: CSPRNG errno {e}"
        ))
    })?;
    // Top bit set so the exponent is a full 256 bits rather than however many
    // the leading zero bytes leave; bottom bit set so it is odd. This is what
    // OpenSSH's BN_rand(..., BN_RAND_TOP_ONE, BN_RAND_BOTTOM_ODD) produces.
    bytes[0] |= 0x80;
    bytes[31] |= 1;
    Ok(BigUint::from_bytes_be(&bytes))
}

// ============================================================================
// SSH key exchange hash
// ============================================================================

/// Compute the exchange hash H per RFC 4253 section 8.
///
/// H = SHA-256(V_C || V_S || I_C || I_S || K_S || e || f || K)
///
/// Where each value is SSH-encoded (string or mpint as appropriate).
/// Inputs to the SSH key-exchange hash, per RFC 4253 section 8.
struct ExchangeHashInput<'a> {
    /// Client version string (without CRLF).
    v_c: &'a str,
    /// Server version string (without CRLF).
    v_s: &'a str,
    /// Client KEXINIT payload.
    i_c: &'a [u8],
    /// Server KEXINIT payload.
    i_s: &'a [u8],
    /// Server host key blob.
    k_s: &'a [u8],
    /// Client DH public value (big-endian).
    e: &'a [u8],
    /// Server DH public value (big-endian).
    f: &'a [u8],
    /// Shared secret (big-endian).
    k: &'a [u8],
}

fn compute_exchange_hash(input: &ExchangeHashInput<'_>) -> [u8; 32] {
    let mut buf = Vec::new();
    buf.extend_from_slice(&ssh_string(input.v_c.as_bytes()));
    buf.extend_from_slice(&ssh_string(input.v_s.as_bytes()));
    buf.extend_from_slice(&ssh_string(input.i_c));
    buf.extend_from_slice(&ssh_string(input.i_s));
    buf.extend_from_slice(&ssh_string(input.k_s));
    buf.extend_from_slice(&encode_mpint(input.e));
    buf.extend_from_slice(&encode_mpint(input.f));
    buf.extend_from_slice(&encode_mpint(input.k));
    sha256(&buf)
}

/// Derive a key from the shared secret K, exchange hash H, a single-char
/// identifier, and the session ID, per RFC 4253 section 7.2.
///
/// key = SHA-256(K || H || id_char || session_id)
///
/// If more bytes are needed, additional rounds are computed by hashing
/// K || H || <previous_key_material>.
fn derive_key(k: &[u8], h: &[u8; 32], id: u8, session_id: &[u8; 32], needed: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&encode_mpint(k));
    buf.extend_from_slice(h);
    buf.push(id);
    buf.extend_from_slice(session_id);
    let mut result = sha256(&buf).to_vec();

    // Extend if needed.
    while result.len() < needed {
        let mut ext_buf = Vec::new();
        ext_buf.extend_from_slice(&encode_mpint(k));
        ext_buf.extend_from_slice(h);
        ext_buf.extend_from_slice(&result);
        result.extend_from_slice(&sha256(&ext_buf));
    }

    result.truncate(needed);
    result
}

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

/// Check the known_hosts file for a matching host key.
/// Returns Ok(true) if found and matches, Ok(false) if not found,
/// Err if found but mismatched.
fn check_known_hosts(hostname: &str, port: u16, key_blob: &[u8]) -> Result<bool, SshError> {
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let path = format!("{home}/.ssh/known_hosts");

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
fn add_known_host(hostname: &str, port: u16, key_type: &str, key_blob: &[u8]) {
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let dir = format!("{home}/.ssh");
    let path = format!("{dir}/known_hosts");

    // Ensure ~/.ssh directory exists.
    let _ = std::fs::create_dir_all(&dir);

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

struct Config {
    user: String,
    hostname: String,
    port: u16,
    command: Option<String>,
    verbose: bool,
    strict_host_key: StrictHostKey,
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

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(format!(
            "Usage: {} [-p port] [-v] [-o option=value] [user@]hostname [command...]",
            args.first().map(|s| s.as_str()).unwrap_or("ssh")
        ));
    }

    let mut port: u16 = 22;
    let mut verbose = false;
    let mut strict_host_key = StrictHostKey::Ask;
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

struct SshSession {
    handle: u64,
    buf: StreamBuffer,
    config: Config,
    server_version: String,
    client_kexinit: Vec<u8>,
    server_kexinit: Vec<u8>,
    session_id: [u8; 32],
    enc: EncryptionState,
    encrypted: bool,
    seq_send: u32,
    seq_recv: u32,
    channel_id: u32,
    remote_channel_id: u32,
    remote_window: u32,
    /// What the server reported about how the remote command ended.
    remote_exit: Option<RemoteExit>,
}

impl SshSession {
    fn new(handle: u64, config: Config) -> Self {
        Self {
            handle,
            buf: StreamBuffer::new(),
            config,
            server_version: String::new(),
            client_kexinit: Vec::new(),
            server_kexinit: Vec::new(),
            session_id: [0u8; 32],
            enc: EncryptionState::new(),
            encrypted: false,
            seq_send: 0,
            seq_recv: 0,
            channel_id: 0,
            remote_channel_id: 0,
            remote_window: 0,
            remote_exit: None,
        }
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

    /// Send an SSH packet (handles encryption and sequence numbering).
    fn send_packet(&mut self, payload: &[u8]) -> Result<(), SshError> {
        let pkt = build_packet(payload, self.encrypted, self.seq_send, &self.enc);
        tcp_send_all(self.handle, &pkt)?;
        self.seq_send = self.seq_send.wrapping_add(1);
        Ok(())
    }

    /// Receive an SSH packet (handles decryption and sequence numbering).
    fn recv_packet(&mut self) -> Result<Vec<u8>, SshError> {
        let payload = read_packet(
            self.handle,
            &mut self.buf,
            self.encrypted,
            self.seq_recv,
            &self.enc,
        )?;
        self.seq_recv = self.seq_recv.wrapping_add(1);
        Ok(payload)
    }

    // === Phase 1: Version exchange ===

    fn version_exchange(&mut self) -> Result<(), SshError> {
        self.verbose("sending client version");

        // Send our version string.
        let version_line = format!("{SSH_VERSION_STRING}\r\n");
        tcp_send_all(self.handle, version_line.as_bytes())?;

        // Read server version line. The server may send banner lines first;
        // the version line starts with "SSH-".
        let mut line = String::new();
        loop {
            let mut buf = [0u8; 1];
            let [byte] = *tcp_recv(self.handle, &mut buf)? else {
                return Err(SshError::ProtocolError(
                    "connection closed during version exchange".into(),
                ));
            };
            if byte == b'\n' {
                let trimmed = line.trim_end_matches('\r').to_string();
                if trimmed.starts_with("SSH-") {
                    self.server_version = trimmed;
                    break;
                }
                // Banner line — print it if verbose.
                self.verbose(&format!("banner: {trimmed}"));
                line.clear();
            } else {
                line.push(char::from(byte));
                if line.len() > 1024 {
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

    fn key_exchange(&mut self) -> Result<(), SshError> {
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
        posix::random::fill(&mut cookie).map_err(|e| {
            SshError::ProtocolError(format!("cannot generate the KEXINIT cookie: errno {e}"))
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
        let p_bytes = hex_to_bytes(DH_GROUP14_P_HEX);
        let p = BigUint::from_bytes_be(&p_bytes);
        let generator = BigUint::from_bytes_be(&[DH_G]);

        // Generate private exponent x and compute e = g^x mod p.
        let x = generate_dh_private()?;
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
        let f = BigUint::from_bytes_be(&f_bytes);
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
            v_c: SSH_VERSION_STRING,
            v_s: &self.server_version,
            i_c: &self.client_kexinit,
            i_s: &self.server_kexinit,
            k_s: &k_s,
            e: &e_bytes,
            f: &f_bytes,
            k: &k_bytes,
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

        // The first exchange hash is used as the session ID.
        if self.session_id == [0u8; 32] {
            self.session_id = h;
        }

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

        // Derive encryption keys.
        // RFC 4253 section 7.2:
        //   Initial IV c2s:    HASH(K || H || "A" || session_id)
        //   Initial IV s2c:    HASH(K || H || "B" || session_id)
        //   Encryption key c2s: HASH(K || H || "C" || session_id)
        //   Encryption key s2c: HASH(K || H || "D" || session_id)
        //   Integrity key c2s: HASH(K || H || "E" || session_id)
        //   Integrity key s2c: HASH(K || H || "F" || session_id)
        self.enc.iv_c2s = derive_key(&k_bytes, &h, b'A', &self.session_id, 16);
        self.enc.iv_s2c = derive_key(&k_bytes, &h, b'B', &self.session_id, 16);
        self.enc.enc_key_c2s = derive_key(&k_bytes, &h, b'C', &self.session_id, 16);
        self.enc.enc_key_s2c = derive_key(&k_bytes, &h, b'D', &self.session_id, 16);
        self.enc.mac_key_c2s = derive_key(&k_bytes, &h, b'E', &self.session_id, 32);
        self.enc.mac_key_s2c = derive_key(&k_bytes, &h, b'F', &self.session_id, 32);

        self.encrypted = true;
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
        if check_known_hosts(&self.config.hostname, self.config.port, key_blob)? {
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
                    add_known_host(&self.config.hostname, self.config.port, key_type, key_blob);
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
                        add_known_host(&self.config.hostname, self.config.port, key_type, key_blob);
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
        // If we have buffered data, try to parse a packet.
        let block_size = if self.encrypted {
            self.enc.block_size.max(8)
        } else {
            BLOCK_SIZE_UNENCRYPTED
        };

        if self.buf.available() >= block_size {
            let payload = read_packet(
                self.handle,
                &mut self.buf,
                self.encrypted,
                self.seq_recv,
                &self.enc,
            )?;
            self.seq_recv = self.seq_recv.wrapping_add(1);
            return Ok(Some(payload));
        }

        // Try one non-blocking recv.
        self.buf.fill_once(self.handle)?;

        if self.buf.available() >= block_size {
            let payload = read_packet(
                self.handle,
                &mut self.buf,
                self.encrypted,
                self.seq_recv,
                &self.enc,
            )?;
            self.seq_recv = self.seq_recv.wrapping_add(1);
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
const EXIT_SSH_FAILURE: i32 = 255;

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("ssh: {msg}");
            process::exit(EXIT_SSH_FAILURE);
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
            process::exit(EXIT_SSH_FAILURE);
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
    let handle = match tcp_connect(ip, config.port) {
        Ok(h) => h,
        Err(e) => {
            eprintln!(
                "ssh: connect to host {} port {}: {e}",
                config.hostname, config.port
            );
            process::exit(EXIT_SSH_FAILURE);
        }
    };

    if verbose {
        eprintln!("debug1: connection established");
    }

    // Run the SSH session.
    let mut session = SshSession::new(handle, config);
    match session.run() {
        Ok(()) => {
            session.send_disconnect(11, "disconnected by user");
        }
        Err(e) => {
            eprintln!("ssh: {e}");
            session.send_disconnect(2, "protocol error");
            tcp_close(handle);
            // 255 is `ssh(1)`'s reserved code for "the connection or the
            // client failed", as distinct from any status a remote command
            // could return — the distinction a caller needs in order to tell
            // "the command failed" from "ssh could not run it".
            process::exit(EXIT_SSH_FAILURE);
        }
    }

    tcp_close(handle);

    // Exit with the remote command's status. Computed after the socket is
    // closed because `process::exit` runs no destructors.
    let code = session.exit_code();
    if code != 0 {
        process::exit(code);
    }
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
            connect_timeout: 0,
        }
    }

    /// A session with no socket. Every function exercised below either does no
    /// I/O at all or does it only when `want_reply` is set, which these
    /// payloads never set.
    fn test_session() -> SshSession {
        SshSession::new(0, test_config())
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
        let a = generate_dh_private().expect("CSPRNG");
        let b = generate_dh_private().expect("CSPRNG");
        assert_ne!(a.to_bytes_be(), b.to_bytes_be());
    }

    /// A 256-bit exponent with the top bit set: full length, and odd so it
    /// cannot share the small factor 2 with the group order.
    #[test]
    fn the_dh_private_exponent_has_the_expected_shape() {
        let x = generate_dh_private().expect("CSPRNG").to_bytes_be();
        assert_eq!(x.len(), 32, "expected a full 256-bit exponent");
        assert!(x[0] & 0x80 != 0, "top bit must be set");
        assert!(x[31] & 1 == 1, "exponent must be odd");
    }

    /// The KEXINIT cookie is half of what stops either side alone from
    /// choosing the exchange hash: both `I_C` and `I_S` feed into `H`. Sixteen
    /// zero bytes handed that power to the server.
    #[test]
    fn the_kexinit_cookie_is_random() {
        let s = test_session();
        let a = s.build_kexinit().expect("CSPRNG");
        let b = s.build_kexinit().expect("CSPRNG");
        assert_eq!(a[0], msg::SSH_MSG_KEXINIT);
        assert_ne!(a[1..17], [0u8; 16], "the cookie must not be zeros");
        assert_ne!(a[1..17], b[1..17], "the cookie must differ per connection");
        // Everything after the cookie is a fixed algorithm advertisement.
        assert_eq!(a[17..], b[17..]);
    }

    /// We must not advertise an algorithm we would then have to refuse: that
    /// turns a connection that could have worked into a confusing failure.
    #[test]
    fn we_only_offer_host_key_algorithms_we_can_verify() {
        let s = test_session();
        let payload = s.build_kexinit().expect("CSPRNG");
        // byte + 16-byte cookie, then kex_algorithms, then host key algorithms.
        let (_kex, off) = read_ssh_string(&payload, 17).expect("kex list");
        let (host_key, _) = read_ssh_string(&payload, off).expect("host key list");
        assert_eq!(host_key, b"ssh-ed25519");
    }

    // ========================================================================
    // BigUint
    //
    // These had no tests at all, which is the worst place in this file to have
    // none: an arithmetic slip here does not fail loudly, it produces a shared
    // secret that differs from the server's, and that surfaces four steps later
    // as an opaque "MAC verification failed" with nothing pointing back here.
    // ========================================================================

    fn big(hex: &str) -> BigUint {
        BigUint::from_bytes_be(&hex_to_bytes(hex))
    }

    fn hex_of(n: &BigUint) -> String {
        bytes_to_hex(&n.to_bytes_be())
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
        let p = BigUint::from_bytes_be(&hex_to_bytes(DH_GROUP14_P_HEX));
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

    #[test]
    fn hex_helpers_round_trip() {
        assert_eq!(bytes_to_hex(&hex_to_bytes("00ff10ab")), "00ff10ab");
        assert_eq!(hex_to_bytes(""), Vec::<u8>::new());
        // A dangling digit is dropped rather than read as half a byte.
        assert_eq!(hex_to_bytes("abc"), vec![0xab]);
    }

    #[test]
    fn mpint_encoding_pads_a_high_bit_and_strips_leading_zeros() {
        // 0 encodes as an empty string.
        assert_eq!(encode_mpint(&[]), vec![0, 0, 0, 0]);
        assert_eq!(encode_mpint(&[0, 0, 0]), vec![0, 0, 0, 0]);
        // Top bit clear: no pad.
        assert_eq!(encode_mpint(&[0x7f]), vec![0, 0, 0, 1, 0x7f]);
        // Top bit set: a zero byte goes in front so it does not read negative.
        assert_eq!(encode_mpint(&[0x80]), vec![0, 0, 0, 2, 0x00, 0x80]);
        // Leading zeros in the input are not part of the value.
        assert_eq!(encode_mpint(&[0, 0, 0x01]), vec![0, 0, 0, 1, 0x01]);
    }

    // ------------------------------------------------------------------
    // AES-128 and AES-128-CTR
    //
    // The cipher had no tests at all, which is how `aes128_key_expand`,
    // `mix_columns` and the CTR counter walk could all be rewritten with
    // nothing but the compiler checking the result. These are the published
    // known-answer vectors, so they check this implementation against the
    // standard rather than against its own previous output.
    // ------------------------------------------------------------------

    #[test]
    fn aes128_matches_the_fips_197_known_answer() {
        // FIPS-197 appendix C.1.
        let key: [u8; 16] = hex_to_bytes("000102030405060708090a0b0c0d0e0f")
            .try_into()
            .unwrap();
        let plain: [u8; 16] = hex_to_bytes("00112233445566778899aabbccddeeff")
            .try_into()
            .unwrap();

        let round_keys = aes128_key_expand(&key);
        let cipher = aes128_encrypt_block(&plain, &round_keys);

        assert_eq!(bytes_to_hex(&cipher), "69c4e0d86a7b0430d8cdb78070b4c55a");
    }

    #[test]
    fn aes128_key_expansion_matches_the_published_schedule() {
        // FIPS-197 appendix A.1: the last round key for the all-zero-ish
        // C.1 key. Checking the *final* round key is what catches an
        // expansion that went wrong partway and then stayed wrong.
        let key: [u8; 16] = hex_to_bytes("000102030405060708090a0b0c0d0e0f")
            .try_into()
            .unwrap();
        let round_keys = aes128_key_expand(&key);

        assert_eq!(
            bytes_to_hex(&round_keys[0]),
            "000102030405060708090a0b0c0d0e0f"
        );
        assert_eq!(
            bytes_to_hex(&round_keys[1]),
            "d6aa74fdd2af72fadaa678f1d6ab76fe"
        );
        assert_eq!(
            bytes_to_hex(&round_keys[10]),
            "13111d7fe3944a17f307a78b4d2b30c5"
        );
    }

    #[test]
    fn aes_ctr_matches_the_nist_sp_800_38a_vector() {
        // NIST SP 800-38A F.5.1, CTR-AES128.Encrypt. Four blocks, so the
        // counter has to advance three times: this is the test that would
        // have caught the counter walk being wrong, which the previous
        // O(N^2) "re-derive from the IV each block" form got right only by
        // repeating the whole increment sequence from scratch.
        let key = hex_to_bytes("2b7e151628aed2a6abf7158809cf4f3c");
        let iv = hex_to_bytes("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let mut data = hex_to_bytes(concat!(
            "6bc1bee22e409f96e93d7e117393172a",
            "ae2d8a571e03ac9c9eb76fac45af8e51",
            "30c81c46a35ce411e5fbc1191a0a52ef",
            "f69f2445df4f9b17ad2b417be66c3710",
        ));

        encrypt_packet_aes_ctr(&mut data, &key, &iv, 0);

        assert_eq!(
            bytes_to_hex(&data),
            concat!(
                "874d6191b620e3261bef6864990db6ce",
                "9806f66b7970fdff8617187bb9fffdff",
                "5ae4df3edbd5d35e5b4f09020db03eab",
                "1e031dda2fbe03d1792170a0f3009cee",
            )
        );
    }

    #[test]
    fn aes_ctr_round_trips_a_length_that_is_not_a_block_multiple() {
        // CTR is its own inverse, and the short final block is the part the
        // old `(start + 16).min(len)` arithmetic had to get right by hand.
        let key = hex_to_bytes("2b7e151628aed2a6abf7158809cf4f3c");
        let iv = hex_to_bytes("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let original: Vec<u8> = (0u8..37).collect();

        let mut data = original.clone();
        encrypt_packet_aes_ctr(&mut data, &key, &iv, 0);
        assert_ne!(data, original, "encryption left the plaintext alone");
        assert_eq!(data.len(), original.len());

        decrypt_packet_aes_ctr(&mut data, &key, &iv, 0);
        assert_eq!(data, original);
    }

    #[test]
    fn aes_ctr_declines_a_short_key_or_iv_rather_than_encrypting_with_padding() {
        let mut data = vec![1, 2, 3, 4];
        encrypt_packet_aes_ctr(&mut data, &[0; 15], &[0; 16], 0);
        assert_eq!(data, vec![1, 2, 3, 4], "a 15-byte key must not be used");
        encrypt_packet_aes_ctr(&mut data, &[0; 16], &[0; 15], 0);
        assert_eq!(data, vec![1, 2, 3, 4], "a 15-byte IV must not be used");
    }

    #[test]
    fn the_ctr_counter_carries_across_byte_boundaries() {
        let mut counter = [0xffu8; 16];
        increment_counter(&mut counter);
        assert_eq!(counter, [0u8; 16], "an all-ones counter wraps to zero");

        let mut counter = [0u8; 16];
        counter[15] = 0xff;
        counter[14] = 0x01;
        increment_counter(&mut counter);
        assert_eq!(counter[14], 0x02);
        assert_eq!(counter[15], 0x00);
    }

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
}
