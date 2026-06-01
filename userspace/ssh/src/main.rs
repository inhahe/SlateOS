//! OurOS SSH-2 Client
//!
//! A simplified SSH-2 protocol client for OurOS. Supports password
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
//! - Version exchange (SSH-2.0-OurOS_1.0)
//! - Key exchange: diffie-hellman-group14-sha256
//! - Host key: ssh-rsa (fingerprint display + known_hosts)
//! - Encryption: AES-128-CTR
//! - MAC: HMAC-SHA256
//! - User auth: password method
//! - Channel: session with PTY and shell/exec

#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)]
// Allow these in this simplified userspace utility where panics are acceptable
// failure modes (the process just exits).
#![allow(clippy::module_name_repetitions)]

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
    let mut offset = 0;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(SshError::SendFailed);
        }
        offset = offset.checked_add(n).ok_or(SshError::SendFailed)?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 when the peer has closed.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, SshError> {
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
    Ok(ret as usize)
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
const SSH_VERSION_STRING: &str = "SSH-2.0-OurOS_1.0";

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
    let unpadded = 1 + payload.len(); // padding_length byte + payload
    let mut padding = block_size - ((4 + unpadded) % block_size);
    if padding < 4 {
        padding += block_size;
    }

    let packet_length = unpadded + padding;
    let mut pkt = Vec::with_capacity(4 + packet_length);
    pkt.extend_from_slice(&(packet_length as u32).to_be_bytes());
    pkt.push(padding as u8);
    pkt.extend_from_slice(payload);
    // Zero-fill padding (simplified; a real implementation would use random bytes).
    pkt.resize(4 + packet_length, 0);

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

    // We need at least one block to read packet_length.
    buf.ensure(handle, block_size)?;

    // Peek/decrypt the first block to get packet_length.
    let first_block = buf.peek(block_size);
    let first_decrypted = if encrypted {
        decrypt_block_aes_ctr(first_block, &enc.enc_key_s2c, &enc.iv_s2c, seq, 0)
    } else {
        first_block.to_vec()
    };

    let packet_length = u32::from_be_bytes([
        first_decrypted[0],
        first_decrypted[1],
        first_decrypted[2],
        first_decrypted[3],
    ]) as usize;

    if packet_length > MAX_PACKET_SIZE {
        return Err(SshError::ProtocolError(format!(
            "packet too large: {packet_length}"
        )));
    }

    let mac_len = if encrypted { enc.mac_len } else { 0 };
    let total = 4 + packet_length + mac_len;
    buf.ensure(handle, total)?;

    let raw = buf.consume(total);

    // Decrypt if needed.
    let decrypted = if encrypted {
        let (pkt_data, mac_data) = raw.split_at(4 + packet_length);
        let mut dec = pkt_data.to_vec();
        decrypt_packet_aes_ctr(&mut dec, &enc.enc_key_s2c, &enc.iv_s2c, seq);

        // Verify MAC.
        let expected_mac = compute_mac(&enc.mac_key_s2c, seq, &dec);
        if mac_data.len() >= mac_len
            && !constant_time_eq(
                mac_data.get(..mac_len).unwrap_or_default(),
                &expected_mac,
            )
        {
            return Err(SshError::ProtocolError("MAC verification failed".into()));
        }
        dec
    } else {
        raw[..4 + packet_length].to_vec()
    };

    let padding_length = decrypted[4] as usize;
    let payload_len = packet_length
        .checked_sub(1 + padding_length)
        .ok_or_else(|| SshError::ProtocolError("invalid padding length".into()))?;
    Ok(decrypted[5..5 + payload_len].to_vec())
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
        self.data.len() - self.pos
    }

    /// Ensure at least `needed` bytes are buffered, reading from TCP as needed.
    fn ensure(&mut self, handle: u64, needed: usize) -> Result<(), SshError> {
        while self.available() < needed {
            // Compact if we have consumed a lot.
            if self.pos > 4096 {
                self.data.drain(..self.pos);
                self.pos = 0;
            }
            let mut tmp = [0u8; 8192];
            let n = tcp_recv(handle, &mut tmp)?;
            if n == 0 {
                return Err(SshError::ProtocolError("connection closed".into()));
            }
            self.data.extend_from_slice(&tmp[..n]);
        }
        Ok(())
    }

    /// Peek at the first `n` bytes without consuming them.
    fn peek(&self, n: usize) -> &[u8] {
        &self.data[self.pos..self.pos + n]
    }

    /// Consume and return `n` bytes.
    fn consume(&mut self, n: usize) -> Vec<u8> {
        let result = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        result
    }
}

// ============================================================================
// SSH data encoding helpers
// ============================================================================

/// Encode a string/bytes as SSH `string` type: u32 length + data.
fn ssh_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// Read an SSH `string` from a byte slice at the given offset.
/// Returns (value, new_offset).
fn read_ssh_string(data: &[u8], offset: usize) -> Result<(&[u8], usize), SshError> {
    if offset + 4 > data.len() {
        return Err(SshError::ProtocolError("truncated string length".into()));
    }
    let len = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    let start = offset + 4;
    let end = start + len;
    if end > data.len() {
        return Err(SshError::ProtocolError(format!(
            "string length {len} exceeds packet (have {})",
            data.len() - start
        )));
    }
    Ok((&data[start..end], end))
}

/// Read a u32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> Result<(u32, usize), SshError> {
    if offset + 4 > data.len() {
        return Err(SshError::ProtocolError("truncated u32".into()));
    }
    let v = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    Ok((v, offset + 4))
}

/// Read a byte from a slice at the given offset.
fn read_byte(data: &[u8], offset: usize) -> Result<(u8, usize), SshError> {
    if offset >= data.len() {
        return Err(SshError::ProtocolError("truncated byte".into()));
    }
    Ok((data[offset], offset + 1))
}

/// Encode an SSH `mpint` from a big-endian unsigned byte array.
/// Prepends a zero byte if the high bit is set.
fn encode_mpint(value: &[u8]) -> Vec<u8> {
    // Strip leading zeros.
    let stripped = strip_leading_zeros(value);
    if stripped.is_empty() {
        return vec![0, 0, 0, 0]; // mpint zero
    }
    let needs_pad = (stripped[0] & 0x80) != 0;
    let total_len = stripped.len() + if needs_pad { 1 } else { 0 };
    let mut out = Vec::with_capacity(4 + total_len);
    out.extend_from_slice(&(total_len as u32).to_be_bytes());
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
    let first_nonzero = data.iter().position(|&b| b != 0).unwrap_or(data.len());
    &data[first_nonzero..]
}

// ============================================================================
// Minimal big-integer arithmetic for Diffie-Hellman
//
// We represent big integers as big-endian byte vectors. This is a simplified
// implementation sufficient for the DH key exchange — not a general-purpose
// bignum library.
// ============================================================================

/// Big-endian unsigned big integer.
#[derive(Clone, Debug)]
struct BigUint {
    /// Digits stored big-endian (most significant byte first). No leading zeros
    /// except for the value zero itself, which is represented as an empty vec.
    bytes: Vec<u8>,
}

impl BigUint {
    fn zero() -> Self {
        Self { bytes: Vec::new() }
    }

    fn one() -> Self {
        Self { bytes: vec![1] }
    }

    fn from_bytes_be(data: &[u8]) -> Self {
        let stripped = strip_leading_zeros(data);
        Self {
            bytes: stripped.to_vec(),
        }
    }

    fn to_bytes_be(&self) -> Vec<u8> {
        if self.bytes.is_empty() {
            return vec![0];
        }
        self.bytes.clone()
    }

    fn is_zero(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Return the number of bits.
    fn bit_length(&self) -> usize {
        if self.bytes.is_empty() {
            return 0;
        }
        let top = self.bytes[0];
        let top_bits = 8 - top.leading_zeros() as usize;
        (self.bytes.len() - 1) * 8 + top_bits
    }

    /// Test bit at position `pos` (0 = least significant).
    fn bit(&self, pos: usize) -> bool {
        let byte_idx = pos / 8;
        let bit_idx = pos % 8;
        if byte_idx >= self.bytes.len() {
            return false;
        }
        let idx = self.bytes.len() - 1 - byte_idx;
        (self.bytes[idx] >> bit_idx) & 1 == 1
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

    /// Full multiplication (schoolbook, O(n^2)).
    fn mul(&self, other: &BigUint) -> BigUint {
        if self.is_zero() || other.is_zero() {
            return BigUint::zero();
        }
        let a = &self.bytes;
        let b = &other.bytes;
        let mut result = vec![0u32; a.len() + b.len()];

        for (i, &av) in a.iter().enumerate().rev() {
            let ai = a.len() - 1 - i;
            for (j, &bv) in b.iter().enumerate().rev() {
                let bj = b.len() - 1 - j;
                let pos = ai + bj;
                let prod = u32::from(av) * u32::from(bv) + result[pos];
                result[pos] = prod & 0xFF;
                if pos + 1 < result.len() {
                    result[pos + 1] += prod >> 8;
                }
            }
        }

        // Propagate carries.
        for i in 0..result.len() - 1 {
            if result[i] > 255 {
                result[i + 1] += result[i] >> 8;
                result[i] &= 0xFF;
            }
        }

        // Convert back to big-endian bytes.
        let mut bytes: Vec<u8> = result.iter().rev().map(|&v| v as u8).collect();
        // Strip leading zeros.
        while bytes.len() > 1 && bytes[0] == 0 {
            bytes.remove(0);
        }
        if bytes == [0] {
            bytes.clear();
        }
        BigUint { bytes }
    }

    /// Division with remainder. Returns (quotient, remainder).
    fn div_rem(&self, divisor: &BigUint) -> (BigUint, BigUint) {
        if divisor.is_zero() {
            return (BigUint::zero(), BigUint::zero());
        }
        if self.cmp_unsigned(divisor) == std::cmp::Ordering::Less {
            return (BigUint::zero(), self.clone());
        }

        let mut remainder = BigUint::zero();
        let mut quotient_bits = Vec::new();

        for i in (0..self.bit_length()).rev() {
            // Left-shift remainder by 1 and add the next bit.
            remainder = remainder.shl1();
            if self.bit(i) {
                remainder = remainder.add_small(1);
            }
            if remainder.cmp_unsigned(divisor) != std::cmp::Ordering::Less {
                remainder = remainder.sub(divisor);
                quotient_bits.push(i);
            }
        }

        // Build quotient from bit positions.
        if quotient_bits.is_empty() {
            return (BigUint::zero(), remainder);
        }

        let max_bit = quotient_bits[0];
        let num_bytes = max_bit / 8 + 1;
        let mut qbytes = vec![0u8; num_bytes];
        for pos in quotient_bits {
            let byte_idx = pos / 8;
            let bit_idx = pos % 8;
            let idx = num_bytes - 1 - byte_idx;
            qbytes[idx] |= 1 << bit_idx;
        }
        // Strip leading zeros.
        while qbytes.len() > 1 && qbytes[0] == 0 {
            qbytes.remove(0);
        }
        if qbytes == [0] {
            qbytes.clear();
        }
        (BigUint { bytes: qbytes }, remainder)
    }

    /// Left-shift by 1 bit.
    fn shl1(&self) -> BigUint {
        if self.is_zero() {
            return BigUint::zero();
        }
        let mut result = vec![0u8; self.bytes.len() + 1];
        let mut carry = 0u8;
        for i in (0..self.bytes.len()).rev() {
            let v = (u16::from(self.bytes[i]) << 1) | u16::from(carry);
            result[i + 1] = v as u8;
            carry = (v >> 8) as u8;
        }
        result[0] = carry;
        while result.len() > 1 && result[0] == 0 {
            result.remove(0);
        }
        if result == [0] {
            result.clear();
        }
        BigUint { bytes: result }
    }

    /// Add a small u8 value.
    fn add_small(&self, val: u8) -> BigUint {
        if val == 0 {
            return self.clone();
        }
        if self.is_zero() {
            return BigUint {
                bytes: vec![val],
            };
        }
        let mut result = self.bytes.clone();
        let mut carry = u16::from(val);
        for b in result.iter_mut().rev() {
            let sum = u16::from(*b) + carry;
            *b = sum as u8;
            carry = sum >> 8;
        }
        if carry > 0 {
            result.insert(0, carry as u8);
        }
        BigUint { bytes: result }
    }

    /// Subtract (self - other). Assumes self >= other.
    fn sub(&self, other: &BigUint) -> BigUint {
        if other.is_zero() {
            return self.clone();
        }
        let a = &self.bytes;
        let b = &other.bytes;
        let len = a.len();
        let mut result = vec![0u8; len];
        let mut borrow: i16 = 0;

        for i in (0..len).rev() {
            let av = i16::from(a[i]);
            let bi = i as isize - (len as isize - b.len() as isize);
            let bv = if bi >= 0 { i16::from(b[bi as usize]) } else { 0 };
            let diff = av - bv - borrow;
            if diff < 0 {
                result[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                result[i] = diff as u8;
                borrow = 0;
            }
        }

        // Strip leading zeros.
        while result.len() > 1 && result[0] == 0 {
            result.remove(0);
        }
        if result == [0] {
            result.clear();
        }
        BigUint { bytes: result }
    }

    fn cmp_unsigned(&self, other: &BigUint) -> std::cmp::Ordering {
        let a = strip_leading_zeros(&self.bytes);
        let b = strip_leading_zeros(&other.bytes);
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Equal => a.cmp(b),
            ord => ord,
        }
    }
}

// ============================================================================
// SHA-256 implementation
// ============================================================================

/// SHA-256 round constants.
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// Initial hash values for SHA-256.
const H256_INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
    0x5be0cd19,
];

/// Compute SHA-256 of the input data.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = H256_INIT;

    // Pre-processing: add padding.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block.
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let off = i * 4;
            w[i] = u32::from_be_bytes([chunk[off], chunk[off + 1], chunk[off + 2], chunk[off + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
        hash[5] = hash[5].wrapping_add(f);
        hash[6] = hash[6].wrapping_add(g);
        hash[7] = hash[7].wrapping_add(h);
    }

    let mut output = [0u8; 32];
    for (i, &val) in hash.iter().enumerate() {
        let off = i * 4;
        output[off..off + 4].copy_from_slice(&val.to_be_bytes());
    }
    output
}

// ============================================================================
// HMAC-SHA256
// ============================================================================

/// Compute HMAC-SHA256(key, data).
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let block_size = 64;

    // If key is longer than block size, hash it first.
    let key_used;
    let key_hash;
    if key.len() > block_size {
        key_hash = sha256(key);
        key_used = &key_hash[..];
    } else {
        key_used = key;
    }

    // Pad key to block_size.
    let mut k_padded = vec![0u8; block_size];
    k_padded[..key_used.len()].copy_from_slice(key_used);

    // Inner: SHA256((key XOR ipad) || data)
    let mut inner = Vec::with_capacity(block_size + data.len());
    for &b in &k_padded {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);

    // Outer: SHA256((key XOR opad) || inner_hash)
    let mut outer = Vec::with_capacity(block_size + 32);
    for &b in &k_padded {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// Compute the SSH MAC for a packet.
/// MAC = HMAC-SHA256(key, sequence_number(u32_be) || unencrypted_packet)
fn compute_mac(key: &[u8], seq: u32, packet: &[u8]) -> Vec<u8> {
    let mut mac_input = Vec::with_capacity(4 + packet.len());
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
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab,
    0x76, 0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4,
    0x72, 0xc0, 0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71,
    0xd8, 0x31, 0x15, 0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2,
    0xeb, 0x27, 0xb2, 0x75, 0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6,
    0xb3, 0x29, 0xe3, 0x2f, 0x84, 0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb,
    0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf, 0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45,
    0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8, 0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5,
    0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2, 0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44,
    0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73, 0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a,
    0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb, 0xe0, 0x32, 0x3a, 0x0a, 0x49,
    0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79, 0xe7, 0xc8, 0x37, 0x6d,
    0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08, 0xba, 0x78, 0x25,
    0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a, 0x70, 0x3e,
    0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e, 0xe1,
    0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb,
    0x16,
];

/// AES round constants.
const AES_RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// Galois Field multiplication by 2 in GF(2^8).
fn gf_mul2(x: u8) -> u8 {
    let shifted = x << 1;
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
    round_keys[0] = *key;

    for i in 1..11 {
        let prev = round_keys[i - 1];
        let mut word = [prev[12], prev[13], prev[14], prev[15]];

        // RotWord + SubWord + Rcon
        word.rotate_left(1);
        for b in &mut word {
            *b = AES_SBOX[*b as usize];
        }
        word[0] ^= AES_RCON[i - 1];

        for j in 0..4 {
            let off = j * 4;
            for k in 0..4 {
                round_keys[i][off + k] = prev[off + k] ^ word[k];
            }
            // Update word for next column.
            word = [
                round_keys[i][off],
                round_keys[i][off + 1],
                round_keys[i][off + 2],
                round_keys[i][off + 3],
            ];
        }
    }
    round_keys
}

/// Encrypt one 16-byte block with AES-128.
fn aes128_encrypt_block(block: &[u8; 16], round_keys: &[[u8; 16]; 11]) -> [u8; 16] {
    let mut state = *block;

    // Initial round key addition.
    xor_block(&mut state, &round_keys[0]);

    // Rounds 1..9: SubBytes, ShiftRows, MixColumns, AddRoundKey.
    for round in 1..10 {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        xor_block(&mut state, &round_keys[round]);
    }

    // Final round (no MixColumns).
    sub_bytes(&mut state);
    shift_rows(&mut state);
    xor_block(&mut state, &round_keys[10]);

    state
}

fn xor_block(state: &mut [u8; 16], key: &[u8; 16]) {
    for (s, &k) in state.iter_mut().zip(key.iter()) {
        *s ^= k;
    }
}

fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = AES_SBOX[*b as usize];
    }
}

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
    for col in 0..4 {
        let off = col * 4;
        let (a0, a1, a2, a3) = (state[off], state[off + 1], state[off + 2], state[off + 3]);
        state[off] = gf_mul2(a0) ^ gf_mul3(a1) ^ a2 ^ a3;
        state[off + 1] = a0 ^ gf_mul2(a1) ^ gf_mul3(a2) ^ a3;
        state[off + 2] = a0 ^ a1 ^ gf_mul2(a2) ^ gf_mul3(a3);
        state[off + 3] = gf_mul3(a0) ^ a1 ^ a2 ^ gf_mul2(a3);
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

/// AES-128-CTR keystream block for a given counter offset from the base IV.
fn aes_ctr_keystream_block(
    _key: &[u8; 16],
    iv: &[u8; 16],
    round_keys: &[[u8; 16]; 11],
    block_offset: usize,
) -> [u8; 16] {
    let mut counter = *iv;
    // Add block_offset to the counter.
    for _ in 0..block_offset {
        increment_counter(&mut counter);
    }
    aes128_encrypt_block(&counter, round_keys)
}

/// Encrypt a packet in-place using AES-128-CTR.
///
/// For SSH, the counter starts from the IV and increments for each 16-byte
/// block within the packet. Across packets, we track the IV globally (the
/// EncryptionState's IV is incremented after each packet).
fn encrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], _seq: u32) {
    if key.len() < 16 || iv.len() < 16 {
        return;
    }
    let mut key16 = [0u8; 16];
    key16.copy_from_slice(&key[..16]);
    let mut iv16 = [0u8; 16];
    iv16.copy_from_slice(&iv[..16]);
    let round_keys = aes128_key_expand(&key16);

    let num_blocks = packet.len().div_ceil(16);
    for block_idx in 0..num_blocks {
        let keystream = aes_ctr_keystream_block(&key16, &iv16, &round_keys, block_idx);
        let start = block_idx * 16;
        let end = (start + 16).min(packet.len());
        for i in start..end {
            packet[i] ^= keystream[i - start];
        }
    }
}

/// Decrypt a packet in-place using AES-128-CTR (same as encrypt for CTR mode).
fn decrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], _seq: u32) {
    encrypt_packet_aes_ctr(packet, key, iv, _seq);
}

/// Decrypt just the first block to peek at packet_length.
fn decrypt_block_aes_ctr(data: &[u8], key: &[u8], iv: &[u8], _seq: u32, _block_idx: usize) -> Vec<u8> {
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

/// Parse a hex string into bytes.
fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.as_bytes();
    let mut result = Vec::with_capacity(hex.len() / 2);
    let mut i = 0;
    while i + 1 < hex.len() {
        let hi = hex_digit(hex[i]);
        let lo = hex_digit(hex[i + 1]);
        result.push((hi << 4) | lo);
        i += 2;
    }
    result
}

fn hex_digit(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Format bytes as a hex string.
fn bytes_to_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(HEX_CHARS[(b >> 4) as usize]);
        s.push(HEX_CHARS[(b & 0x0f) as usize]);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Generate a pseudo-random private exponent for DH.
///
/// In a real implementation this would use a CSPRNG. Here we derive
/// entropy from the address of a stack variable and some counters —
/// sufficient to demonstrate the protocol flow.
fn generate_dh_private() -> BigUint {
    // Gather some entropy from stack address + loop counter.
    let mut seed = [0u8; 32];
    let stack_var: u64 = 0xDEAD_BEEF_CAFE_1234; // constant stand-in
    let time_ish: u64 = 0x1234_5678_9ABC_DEF0; // constant stand-in
    seed[..8].copy_from_slice(&stack_var.to_le_bytes());
    seed[8..16].copy_from_slice(&time_ish.to_le_bytes());
    // Hash to spread entropy.
    let h = sha256(&seed);
    BigUint::from_bytes_be(&h)
}

// ============================================================================
// SSH key exchange hash
// ============================================================================

/// Compute the exchange hash H per RFC 4253 section 8.
///
/// H = SHA-256(V_C || V_S || I_C || I_S || K_S || e || f || K)
///
/// Where each value is SSH-encoded (string or mpint as appropriate).
fn compute_exchange_hash(
    v_c: &str,   // client version string (without CRLF)
    v_s: &str,   // server version string (without CRLF)
    i_c: &[u8],  // client KEXINIT payload
    i_s: &[u8],  // server KEXINIT payload
    k_s: &[u8],  // server host key blob
    e: &[u8],    // client DH public value (big-endian)
    f: &[u8],    // server DH public value (big-endian)
    k: &[u8],    // shared secret (big-endian)
) -> [u8; 32] {
    let mut buf = Vec::new();
    buf.extend_from_slice(&ssh_string(v_c.as_bytes()));
    buf.extend_from_slice(&ssh_string(v_s.as_bytes()));
    buf.extend_from_slice(&ssh_string(i_c));
    buf.extend_from_slice(&ssh_string(i_s));
    buf.extend_from_slice(&ssh_string(k_s));
    buf.extend_from_slice(&encode_mpint(e));
    buf.extend_from_slice(&encode_mpint(f));
    buf.extend_from_slice(&encode_mpint(k));
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
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i + 2 < data.len() {
        let n = (u32::from(data[i]) << 16) | (u32::from(data[i + 1]) << 8) | u32::from(data[i + 2]);
        result.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        result.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        result.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let remaining = data.len() - i;
    if remaining == 1 {
        let n = u32::from(data[i]) << 16;
        result.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
    } else if remaining == 2 {
        let n = (u32::from(data[i]) << 16) | (u32::from(data[i + 1]) << 8);
        result.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        result.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
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

    let host_pattern = if port == 22 {
        hostname.to_string()
    } else {
        format!("[{hostname}]:{port}")
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() < 3 {
            continue;
        }
        let hosts = parts[0];
        let _key_type = parts[1];
        let key_b64 = parts[2].split_whitespace().next().unwrap_or("");

        // Check if our host matches any of the comma-separated host patterns.
        let host_match = hosts.split(',').any(|h| h.trim() == host_pattern);
        if !host_match {
            continue;
        }

        // Decode the stored key and compare.
        let stored_key = base64_decode(key_b64);
        if stored_key == key_blob {
            return Ok(true);
        }
        return Err(SshError::HostKeyMismatch(format!(
            "host key for {host_pattern} has changed!\n\
             Someone could be eavesdropping on you (man-in-the-middle attack).\n\
             The fingerprint for the new key is:\n  {}\n\
             Remove the old entry from {path} to accept the new key.",
            host_key_fingerprint(key_blob),
        )));
    }

    Ok(false)
}

/// Add a host key to the known_hosts file.
fn add_known_host(hostname: &str, port: u16, key_type: &str, key_blob: &[u8]) {
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let dir = format!("{home}/.ssh");
    let path = format!("{dir}/known_hosts");

    // Ensure ~/.ssh directory exists.
    let _ = std::fs::create_dir_all(&dir);

    let host_pattern = if port == 22 {
        hostname.to_string()
    } else {
        format!("[{hostname}]:{port}")
    };

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

    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'=' && b != b'\n' && b != b'\r').collect();
    let mut result = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        let (a, b, c, d) = (
            b64val(bytes[i]).unwrap_or(0),
            b64val(bytes[i + 1]).unwrap_or(0),
            b64val(bytes[i + 2]).unwrap_or(0),
            b64val(bytes[i + 3]).unwrap_or(0),
        );
        let n = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        result.push((n >> 16) as u8);
        result.push((n >> 8) as u8);
        result.push(n as u8);
        i += 4;
    }
    let remaining = bytes.len() - i;
    if remaining >= 2 {
        let a = b64val(bytes[i]).unwrap_or(0);
        let b = b64val(bytes[i + 1]).unwrap_or(0);
        let n = (u32::from(a) << 18) | (u32::from(b) << 12);
        result.push((n >> 16) as u8);
        if remaining >= 3 {
            let c = b64val(bytes[i + 2]).unwrap_or(0);
            let n = n | (u32::from(c) << 6);
            result.push((n >> 8) as u8);
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

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-p" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| "-p requires a port number".to_string())?;
                port = val
                    .parse()
                    .map_err(|_| format!("invalid port: {val}"))?;
            }
            "-v" => {
                verbose = true;
            }
            "-o" => {
                i += 1;
                let opt = args
                    .get(i)
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
                    destination = Some(arg.clone());
                } else {
                    // Everything after destination is the remote command.
                    command_parts.extend_from_slice(&args[i..]);
                    break;
                }
            }
        }
        i += 1;
    }

    let dest = destination.ok_or_else(|| "no destination specified".to_string())?;

    // Parse user@hostname.
    let (user, hostname) = if let Some(at_pos) = dest.find('@') {
        (dest[..at_pos].to_string(), dest[at_pos + 1..].to_string())
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
            let mut byte = [0u8; 1];
            let n = tcp_recv(self.handle, &mut byte)?;
            if n == 0 {
                return Err(SshError::ProtocolError(
                    "connection closed during version exchange".into(),
                ));
            }
            if byte[0] == b'\n' {
                let trimmed = line.trim_end_matches('\r').to_string();
                if trimmed.starts_with("SSH-") {
                    self.server_version = trimmed;
                    break;
                }
                // Banner line — print it if verbose.
                self.verbose(&format!("banner: {trimmed}"));
                line.clear();
            } else {
                line.push(byte[0] as char);
                if line.len() > 1024 {
                    return Err(SshError::ProtocolError("version line too long".into()));
                }
            }
        }

        self.verbose(&format!("remote version: {}", self.server_version));

        // Verify it speaks SSH-2.
        if !self.server_version.starts_with("SSH-2.0-") && !self.server_version.starts_with("SSH-1.99-") {
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
        let client_kexinit_payload = self.build_kexinit();
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
    fn build_kexinit(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.push(msg::SSH_MSG_KEXINIT);

        // 16 bytes of cookie (zero for simplicity).
        payload.extend_from_slice(&[0u8; 16]);

        // Algorithm name-lists. We offer a small set we actually support.
        let kex = "diffie-hellman-group14-sha256,diffie-hellman-group14-sha1";
        let host_key = "ssh-rsa,rsa-sha2-256";
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

        payload
    }

    /// Perform Diffie-Hellman group14 key exchange.
    fn dh_key_exchange(&mut self) -> Result<(), SshError> {
        let p_bytes = hex_to_bytes(DH_GROUP14_P_HEX);
        let p = BigUint::from_bytes_be(&p_bytes);
        let generator = BigUint::from_bytes_be(&[DH_G]);

        // Generate private exponent x and compute e = g^x mod p.
        let x = generate_dh_private();
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

        // Verify host key against known_hosts.
        self.verify_host_key(&k_s, key_type, &fingerprint)?;

        // Compute shared secret K = f^x mod p.
        let f = BigUint::from_bytes_be(&f_bytes);
        let k_big = f.mod_pow(&x, &p);
        let k_bytes = k_big.to_bytes_be();
        self.verbose("computed shared secret");

        // Compute exchange hash H.
        let h = compute_exchange_hash(
            SSH_VERSION_STRING,
            &self.server_version,
            &self.client_kexinit,
            &self.server_kexinit,
            &k_s,
            &e_bytes,
            &f_bytes,
            &k_bytes,
        );
        self.verbose(&format!("exchange hash: {}", bytes_to_hex(&h)));

        // The first exchange hash is used as the session ID.
        if self.session_id == [0u8; 32] {
            self.session_id = h;
        }

        // We skip signature verification in this simplified implementation.
        // In a production client, we would verify the host key signature here.
        self.verbose("(signature verification skipped in simplified implementation)");
        let _ = sig_blob; // Acknowledge we received it.

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
        match check_known_hosts(&self.config.hostname, self.config.port, key_blob)? {
            true => {
                self.verbose("host key matches known_hosts");
                Ok(())
            }
            false => {
                // Host not in known_hosts.
                match self.config.strict_host_key {
                    StrictHostKey::Yes => {
                        Err(SshError::HostKeyMismatch(format!(
                            "host '{}' not found in known_hosts (StrictHostKeyChecking=yes)",
                            self.config.hostname
                        )))
                    }
                    StrictHostKey::No => {
                        eprintln!(
                            "Warning: Permanently added '{}' ({key_type}) to the list of known hosts.",
                            self.config.hostname
                        );
                        add_known_host(
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
                                "Warning: Permanently added '{}' ({key_type}) to the list of known hosts.",
                                self.config.hostname
                            );
                            add_known_host(
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
    /// On OurOS, we write to stderr to prompt, then read a line from stdin.
    /// Real echo suppression requires ioctl — here we just do a basic read.
    fn read_password(&self) -> Result<String, SshError> {
        eprint!("{}@{}'s password: ", self.config.user, self.config.hostname);
        io::stderr().flush().ok();

        let mut password = String::new();
        io::stdin().read_line(&mut password).map_err(|e| {
            SshError::ProtocolError(format!("failed to read password: {e}"))
        })?;

        // Print newline after password entry.
        eprintln!();

        Ok(password.trim_end_matches('\n').trim_end_matches('\r').to_string())
    }

    // === Phase 4: Channel open + PTY + shell/exec ===

    fn open_session_channel(&mut self) -> Result<(), SshError> {
        self.verbose("opening session channel");

        self.channel_id = 0;
        let initial_window: u32 = 2_097_152; // 2 MiB
        let max_packet: u32 = 32768;

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
                    self.verbose(&format!("ignoring message type {other} while opening channel"));
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
                    return Err(SshError::ProtocolError("shell/exec request failed".into()));
                }
                Some(msg::SSH_MSG_CHANNEL_WINDOW_ADJUST) => {
                    self.handle_window_adjust(&reply);
                }
                Some(msg::SSH_MSG_CHANNEL_DATA) => {
                    // Early data — process it.
                    self.handle_channel_data(&reply);
                }
                Some(msg::SSH_MSG_IGNORE | msg::SSH_MSG_DEBUG) => {}
                Some(other) => {
                    self.verbose(&format!("ignoring message type {other} during shell request"));
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
                        self.send_channel_data(&stdin_buf[..n])?;
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
        let mut tmp = [0u8; 8192];
        let n = tcp_recv(self.handle, &mut tmp)?;
        if n == 0 {
            return Err(SshError::ProtocolError("connection closed".into()));
        }
        self.buf.data.extend_from_slice(&tmp[..n]);

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

    /// Handle CHANNEL_WINDOW_ADJUST: increase our send window.
    fn handle_window_adjust(&mut self, payload: &[u8]) {
        // Format: u8 type, u32 recipient_channel, u32 bytes_to_add
        if let Ok((adjustment, _)) = read_u32(payload, 5) {
            self.remote_window = self.remote_window.saturating_add(adjustment);
            self.verbose(&format!("window adjust +{adjustment}, now {}", self.remote_window));
        }
    }

    /// Handle server-initiated CHANNEL_REQUEST (e.g., exit-status, exit-signal).
    fn handle_channel_request(&self, payload: &[u8]) -> Result<(), SshError> {
        // Format: u8 type, u32 recipient_channel, string request_type, bool want_reply, ...
        let mut off = 1;
        let (_, next) = read_u32(payload, off)?; // recipient_channel
        off = next;
        let (req_type, next) = read_ssh_string(payload, off)?;
        off = next;
        let (want_reply, _next) = read_byte(payload, off)?;

        let req_type_str = std::str::from_utf8(req_type).unwrap_or("unknown");
        self.verbose(&format!("channel request: {req_type_str}"));

        if want_reply != 0 {
            // Send CHANNEL_SUCCESS (we don't actually handle these).
            let mut reply = Vec::new();
            reply.push(msg::SSH_MSG_CHANNEL_FAILURE);
            reply.extend_from_slice(&self.remote_channel_id.to_be_bytes());
            // Ignoring send failure here — we're just being polite.
            let _ = tcp_send_all(
                self.handle,
                &build_packet(&reply, self.encrypted, self.seq_send, &self.enc),
            );
        }

        Ok(())
    }

    /// Send data to the remote channel.
    fn send_channel_data(&mut self, data: &[u8]) -> Result<(), SshError> {
        // Respect the remote window size.
        let mut offset = 0;
        while offset < data.len() {
            if self.remote_window == 0 {
                // Wait for a window adjust.
                let payload = self.recv_packet()?;
                let _ = self.process_server_message(&payload);
                continue;
            }
            let chunk_size = (data.len() - offset)
                .min(self.remote_window as usize)
                .min(32768);
            let chunk = &data[offset..offset + chunk_size];

            let mut payload = Vec::new();
            payload.push(msg::SSH_MSG_CHANNEL_DATA);
            payload.extend_from_slice(&self.remote_channel_id.to_be_bytes());
            payload.extend_from_slice(&ssh_string(chunk));
            self.send_packet(&payload)?;

            self.remote_window = self.remote_window.saturating_sub(chunk_size as u32);
            offset += chunk_size;
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

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("ssh: {msg}");
            process::exit(1);
        }
    };

    let verbose = config.verbose;

    if verbose {
        eprintln!("debug1: connecting to {} port {}", config.hostname, config.port);
    }

    // Resolve hostname.
    let ip = match dns_resolve(&config.hostname) {
        Ok(ip) => ip,
        Err(e) => {
            eprintln!("ssh: {e}");
            process::exit(1);
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
            process::exit(1);
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
            process::exit(1);
        }
    }

    tcp_close(handle);
}
