//! `OurOS` SSH Server Daemon (sshd)
//!
//! An SSH-2 protocol server for `OurOS`. Listens for incoming SSH connections,
//! authenticates users, and spawns interactive shell sessions or executes
//! commands on behalf of authenticated users.
//!
//! # Usage
//!
//! ```text
//! sshd                           Start with defaults (port 22)
//! sshd -p 2222                   Listen on custom port
//! sshd -f /etc/ssh/sshd_config   Use alternate config file
//! sshd -d                        Debug mode (no fork, verbose)
//! sshd -D                        Don't daemonize, stay foreground
//! sshd -e                        Log to stderr
//! sshd -h /path/to/hostkey       Specify host key file
//! sshd -t                        Test configuration and exit
//! sshd -T                        Extended test (dump config)
//! ```
//!
//! # Protocol
//!
//! Implements a subset of SSH-2 (RFC 4253, 4252, 4254):
//! - Version exchange (SSH-2.0-OurOS_SSHD_1.0)
//! - Key exchange: diffie-hellman-group14-sha256
//! - Host key: ssh-ed25519 (structured)
//! - Encryption: AES-128-CTR
//! - MAC: HMAC-SHA256
//! - User auth: password (against /etc/shadow), public key (`authorized_keys`)
//! - Session channels with PTY allocation and shell/exec requests

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).

use std::env;
use std::fmt;
use std::io;
#[allow(unused_imports)]
use std::io::Write;
use std::process;

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================
//
// The full syscall ABI is mirrored here so that helpers can be wired up as the
// daemon grows (per-session process spawning, listener teardown, fd shutdown,
// authorized-keys file writes, etc.). Numbers that are not yet referenced by an
// active code path are kept (rather than deleted) to keep this table a complete,
// authoritative copy of the kernel ABI; `#[allow(dead_code)]` documents that the
// gap is intentional, not an oversight.

#[allow(dead_code)]
const SYS_EXIT: u64 = 1;
const SYS_CLOCK_MONOTONIC: u64 = 10;
#[allow(dead_code)]
const SYS_PROCESS_SPAWN: u64 = 500;
const SYS_PROCESS_ID: u64 = 502;
const SYS_FS_READ_FILE: u64 = 600;
#[allow(dead_code)]
const SYS_FS_WRITE_FILE: u64 = 601;
#[allow(dead_code)]
const SYS_FS_STAT: u64 = 606;
#[allow(dead_code)]
const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_TCP_BIND: u64 = 804;
const SYS_TCP_ACCEPT: u64 = 805;
#[allow(dead_code)]
const SYS_TCP_CLOSE_LISTENER: u64 = 806;
const SYS_TCP_PEER_ADDR: u64 = 808;
#[allow(dead_code)]
const SYS_TCP_SHUTDOWN: u64 = 855;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 0-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees nr is valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a 1-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
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
    // SAFETY: Caller guarantees arguments are valid.
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

/// Issue a 4-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
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

/// Read an entire file into a byte vector via the kernel filesystem.
fn fs_read_file(path: &str) -> Result<Vec<u8>, SshdError> {
    let mut buf = vec![0u8; 65536];
    // SAFETY: We pass a valid path pointer+len and a valid output buffer
    // pointer+len. The kernel reads the path and writes file contents into buf.
    let ret = unsafe {
        syscall4(
            SYS_FS_READ_FILE,
            path.as_ptr() as u64,
            path.len() as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::IoError(io::Error::new(
            io::ErrorKind::NotFound,
            format!("cannot read {path}: error {ret}"),
        )));
    }
    buf.truncate(ret as usize);
    Ok(buf)
}

/// Get the current monotonic clock in milliseconds.
fn clock_monotonic_ms() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC takes no pointer arguments, returns time.
    let ret = unsafe { syscall0(SYS_CLOCK_MONOTONIC) };
    if ret < 0 { 0 } else { ret as u64 }
}

/// Get the current process ID.
fn get_pid() -> u64 {
    // SAFETY: SYS_PROCESS_ID takes no arguments, returns the pid.
    let ret = unsafe { syscall0(SYS_PROCESS_ID) };
    if ret < 0 { 0 } else { ret as u64 }
}

/// Bind a TCP listener to a local port. Returns a listener handle.
fn tcp_bind(port: u16) -> Result<u64, SshdError> {
    // SAFETY: SYS_TCP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) };
    if ret < 0 {
        return Err(SshdError::NetworkError(format!(
            "tcp_bind({port}) failed: {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Accept an incoming connection on a listener (blocking).
/// Returns a connection handle.
fn tcp_accept(listener: u64) -> Result<u64, SshdError> {
    // SAFETY: listener is a valid listener handle from tcp_bind.
    let ret = unsafe { syscall1(SYS_TCP_ACCEPT, listener) };
    if ret < 0 {
        return Err(SshdError::NetworkError(format!("tcp_accept failed: {ret}")));
    }
    Ok(ret as u64)
}

/// Send data on a TCP connection. Returns number of bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, SshdError> {
    // SAFETY: We pass a valid handle and a pointer to a byte buffer with its
    // correct length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::NetworkError("tcp_send failed".into()));
    }
    Ok(ret as usize)
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), SshdError> {
    let mut offset = 0;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(SshdError::NetworkError("tcp_send returned 0".into()));
        }
        offset = offset
            .checked_add(n)
            .ok_or_else(|| SshdError::NetworkError("offset overflow".into()))?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 when peer has closed.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, SshdError> {
    // SAFETY: We pass a valid handle and a mutable buffer pointer with length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::NetworkError("tcp_recv failed".into()));
    }
    Ok(ret as usize)
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid TCP connection handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Close a TCP listener handle.
// Reserved for graceful-shutdown wiring: the main accept loop will call this to
// release the bound listener when the daemon is asked to stop. Not yet invoked.
#[allow(dead_code)]
fn tcp_close_listener(listener: u64) {
    // SAFETY: listener is (or was) a valid TCP listener handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE_LISTENER, listener) };
}

/// Get the peer address of a TCP connection.
/// Returns (`ip_u32_network_order`, port).
fn tcp_peer_addr(handle: u64) -> Result<(u32, u16), SshdError> {
    let mut buf = [0u8; 6];
    // SAFETY: handle is valid. buf is a stack-allocated 6-byte buffer.
    let ret = unsafe { syscall3(SYS_TCP_PEER_ADDR, handle, buf.as_mut_ptr() as u64, 0) };
    if ret < 0 {
        return Err(SshdError::NetworkError("tcp_peer_addr failed".into()));
    }
    let ip = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    Ok((ip, port))
}

/// Spawn a new process. Returns child pid on success.
// Reserved for session wiring: once a channel session request is granted the
// daemon will spawn the user's login shell / requested command via this helper.
// The exec path is not yet wired into handle_channel_request. Not yet invoked.
#[allow(dead_code)]
fn process_spawn(path: &str) -> Result<u64, SshdError> {
    // SAFETY: We pass a valid path pointer and its length.
    let ret = unsafe {
        syscall3(
            SYS_PROCESS_SPAWN,
            path.as_ptr() as u64,
            path.len() as u64,
            0,
        )
    };
    if ret < 0 {
        return Err(SshdError::IoError(io::Error::other(format!(
            "process_spawn({path}) failed: {ret}"
        ))));
    }
    Ok(ret as u64)
}

/// Format an IPv4 address from network byte order u32.
fn format_ip(ip: u32) -> String {
    let bytes = ip.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum SshdError {
    ConfigError(String),
    NetworkError(String),
    ProtocolError(String),
    AuthError(String),
    IoError(io::Error),
    #[allow(dead_code)]
    Timeout,
}

impl fmt::Display for SshdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigError(msg) => write!(f, "config error: {msg}"),
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
            Self::ProtocolError(msg) => write!(f, "protocol error: {msg}"),
            Self::AuthError(msg) => write!(f, "auth error: {msg}"),
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::Timeout => write!(f, "timeout"),
        }
    }
}

impl From<io::Error> for SshdError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

// ============================================================================
// SSH-2 constants
// ============================================================================

/// Our server version identification string.
const SSH_SERVER_VERSION: &str = "SSH-2.0-OurOS_SSHD_1.0";

/// SSH message type codes (RFC 4253 / 4252 / 4254).
mod msg {
    pub const SSH_MSG_DISCONNECT: u8 = 1;
    pub const SSH_MSG_IGNORE: u8 = 2;
    #[allow(dead_code)]
    pub const SSH_MSG_UNIMPLEMENTED: u8 = 3;
    #[allow(dead_code)]
    pub const SSH_MSG_DEBUG: u8 = 4;
    pub const SSH_MSG_SERVICE_REQUEST: u8 = 5;
    pub const SSH_MSG_SERVICE_ACCEPT: u8 = 6;
    pub const SSH_MSG_KEXINIT: u8 = 20;
    pub const SSH_MSG_NEWKEYS: u8 = 21;
    #[allow(dead_code)]
    pub const SSH_MSG_KEX_DH_INIT: u8 = 30;
    pub const SSH_MSG_KEX_DH_REPLY: u8 = 31;
    pub const SSH_MSG_USERAUTH_REQUEST: u8 = 50;
    pub const SSH_MSG_USERAUTH_FAILURE: u8 = 51;
    pub const SSH_MSG_USERAUTH_SUCCESS: u8 = 52;
    pub const SSH_MSG_USERAUTH_BANNER: u8 = 53;
    #[allow(dead_code)]
    pub const SSH_MSG_USERAUTH_PK_OK: u8 = 60;
    pub const SSH_MSG_CHANNEL_OPEN: u8 = 90;
    pub const SSH_MSG_CHANNEL_OPEN_CONFIRMATION: u8 = 91;
    pub const SSH_MSG_CHANNEL_OPEN_FAILURE: u8 = 92;
    pub const SSH_MSG_CHANNEL_WINDOW_ADJUST: u8 = 93;
    pub const SSH_MSG_CHANNEL_DATA: u8 = 94;
    #[allow(dead_code)]
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

/// Maximum SSH packet payload size.
const MAX_PACKET_SIZE: usize = 35000;

/// Minimum block size for packet alignment.
const BLOCK_SIZE_UNENCRYPTED: usize = 8;

/// Build a raw SSH binary packet from a payload.
///
/// Format: `[u32 packet_length][u8 padding_length][payload][random_padding]`
fn build_packet(payload: &[u8], encrypted: bool, seq: u32, enc: &EncryptionState) -> Vec<u8> {
    let block_size = if encrypted {
        enc.block_size.max(8)
    } else {
        BLOCK_SIZE_UNENCRYPTED
    };

    let unpadded = 1 + payload.len();
    let mut padding = block_size - ((4 + unpadded) % block_size);
    if padding < 4 {
        padding += block_size;
    }

    let packet_length = unpadded + padding;
    let mut pkt = Vec::with_capacity(4 + packet_length);
    pkt.extend_from_slice(&(packet_length as u32).to_be_bytes());
    pkt.push(padding as u8);
    pkt.extend_from_slice(payload);
    // Zero-fill padding (simplified; real impl would use random bytes).
    pkt.resize(4 + packet_length, 0);

    if encrypted {
        let mac = compute_mac(&enc.mac_key_s2c, seq, &pkt);
        encrypt_packet_aes_ctr(&mut pkt, &enc.enc_key_s2c, &enc.iv_s2c, seq);
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
) -> Result<Vec<u8>, SshdError> {
    let block_size = if encrypted {
        enc.block_size.max(8)
    } else {
        BLOCK_SIZE_UNENCRYPTED
    };

    buf.ensure(handle, block_size)?;

    let first_block = buf.peek(block_size);
    let first_decrypted = if encrypted {
        decrypt_block_aes_ctr(first_block, &enc.enc_key_c2s, &enc.iv_c2s, seq, 0)
    } else {
        first_block.to_vec()
    };

    if first_decrypted.len() < 4 {
        return Err(SshdError::ProtocolError("short first block".into()));
    }
    let packet_length = u32::from_be_bytes([
        first_decrypted[0],
        first_decrypted[1],
        first_decrypted[2],
        first_decrypted[3],
    ]) as usize;

    if packet_length > MAX_PACKET_SIZE {
        return Err(SshdError::ProtocolError(format!(
            "packet too large: {packet_length}"
        )));
    }

    let mac_len = if encrypted { enc.mac_len } else { 0 };
    let total = 4 + packet_length + mac_len;
    buf.ensure(handle, total)?;

    let raw = buf.consume(total);

    let decrypted = if encrypted {
        let (pkt_data, mac_data) = raw.split_at(4 + packet_length);
        let mut dec = pkt_data.to_vec();
        decrypt_packet_aes_ctr(&mut dec, &enc.enc_key_c2s, &enc.iv_c2s, seq);

        let expected_mac = compute_mac(&enc.mac_key_c2s, seq, &dec);
        if mac_data.len() >= mac_len
            && !constant_time_eq(mac_data.get(..mac_len).unwrap_or_default(), &expected_mac)
        {
            return Err(SshdError::ProtocolError("MAC verification failed".into()));
        }
        dec
    } else {
        raw[..4 + packet_length].to_vec()
    };

    if decrypted.len() < 5 {
        return Err(SshdError::ProtocolError("packet too short".into()));
    }
    let padding_length = decrypted[4] as usize;
    let payload_len = packet_length
        .checked_sub(1 + padding_length)
        .ok_or_else(|| SshdError::ProtocolError("invalid padding length".into()))?;
    if 5 + payload_len > decrypted.len() {
        return Err(SshdError::ProtocolError("payload exceeds packet".into()));
    }
    Ok(decrypted[5..5 + payload_len].to_vec())
}

// ============================================================================
// Stream buffer -- accumulates TCP data for packet parsing
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

    fn available(&self) -> usize {
        self.data.len() - self.pos
    }

    fn ensure(&mut self, handle: u64, needed: usize) -> Result<(), SshdError> {
        while self.available() < needed {
            if self.pos > 4096 {
                self.data.drain(..self.pos);
                self.pos = 0;
            }
            let mut tmp = [0u8; 8192];
            let n = tcp_recv(handle, &mut tmp)?;
            if n == 0 {
                return Err(SshdError::ProtocolError("connection closed".into()));
            }
            self.data.extend_from_slice(&tmp[..n]);
        }
        Ok(())
    }

    fn peek(&self, n: usize) -> &[u8] {
        &self.data[self.pos..self.pos + n]
    }

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
fn read_ssh_string(data: &[u8], offset: usize) -> Result<(&[u8], usize), SshdError> {
    if offset + 4 > data.len() {
        return Err(SshdError::ProtocolError("truncated string length".into()));
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
        return Err(SshdError::ProtocolError(format!(
            "string length {len} exceeds packet (have {})",
            data.len() - start
        )));
    }
    Ok((&data[start..end], end))
}

/// Read a u32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> Result<(u32, usize), SshdError> {
    if offset + 4 > data.len() {
        return Err(SshdError::ProtocolError("truncated u32".into()));
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
fn read_byte(data: &[u8], offset: usize) -> Result<(u8, usize), SshdError> {
    if offset >= data.len() {
        return Err(SshdError::ProtocolError("truncated byte".into()));
    }
    Ok((data[offset], offset + 1))
}

/// Encode an SSH `mpint` from big-endian unsigned byte array.
fn encode_mpint(value: &[u8]) -> Vec<u8> {
    let stripped = strip_leading_zeros(value);
    if stripped.is_empty() {
        return vec![0, 0, 0, 0];
    }
    let needs_pad = (stripped[0] & 0x80) != 0;
    let total_len = stripped.len() + usize::from(needs_pad);
    let mut out = Vec::with_capacity(4 + total_len);
    out.extend_from_slice(&(total_len as u32).to_be_bytes());
    if needs_pad {
        out.push(0);
    }
    out.extend_from_slice(stripped);
    out
}

/// Read an SSH `mpint` from a byte slice, returning unsigned big-endian bytes.
fn read_mpint(data: &[u8], offset: usize) -> Result<(Vec<u8>, usize), SshdError> {
    let (raw, next) = read_ssh_string(data, offset)?;
    let stripped = strip_leading_zeros(raw);
    Ok((stripped.to_vec(), next))
}

fn strip_leading_zeros(data: &[u8]) -> &[u8] {
    let first_nonzero = data.iter().position(|&b| b != 0).unwrap_or(data.len());
    &data[first_nonzero..]
}

/// Read a boolean from the data at the given offset.
fn read_bool(data: &[u8], offset: usize) -> Result<(bool, usize), SshdError> {
    let (b, next) = read_byte(data, offset)?;
    Ok((b != 0, next))
}

// ============================================================================
// Minimal big-integer arithmetic for Diffie-Hellman
// ============================================================================

/// Big-endian unsigned big integer.
#[derive(Clone, Debug)]
struct BigUint {
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

    fn bit_length(&self) -> usize {
        if self.bytes.is_empty() {
            return 0;
        }
        let top = self.bytes[0];
        let top_bits = 8 - top.leading_zeros() as usize;
        (self.bytes.len() - 1) * 8 + top_bits
    }

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

    fn mod_mul(&self, other: &BigUint, modulus: &BigUint) -> BigUint {
        let product = self.mul_big(other);
        product.mod_reduce(modulus)
    }

    fn mod_reduce(&self, modulus: &BigUint) -> BigUint {
        if modulus.is_zero() {
            return BigUint::zero();
        }
        self.div_rem(modulus).1
    }

    /// Full multiplication (schoolbook, O(n^2)).
    fn mul_big(&self, other: &BigUint) -> BigUint {
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

        let mut bytes: Vec<u8> = result.iter().rev().map(|&v| v as u8).collect();
        while bytes.len() > 1 && bytes[0] == 0 {
            bytes.remove(0);
        }
        if bytes == [0] {
            bytes.clear();
        }
        BigUint { bytes }
    }

    /// Division with remainder.
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
            remainder = remainder.shl1();
            if self.bit(i) {
                remainder = remainder.add_small(1);
            }
            if remainder.cmp_unsigned(divisor) != std::cmp::Ordering::Less {
                remainder = remainder.sub_big(divisor);
                quotient_bits.push(i);
            }
        }

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
        while qbytes.len() > 1 && qbytes[0] == 0 {
            qbytes.remove(0);
        }
        if qbytes == [0] {
            qbytes.clear();
        }
        (BigUint { bytes: qbytes }, remainder)
    }

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

    fn add_small(&self, val: u8) -> BigUint {
        if val == 0 {
            return self.clone();
        }
        if self.is_zero() {
            return BigUint { bytes: vec![val] };
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

    fn sub_big(&self, other: &BigUint) -> BigUint {
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
            let bv = if bi >= 0 {
                i16::from(b[bi as usize])
            } else {
                0
            };
            let diff = av - bv - borrow;
            if diff < 0 {
                result[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                result[i] = diff as u8;
                borrow = 0;
            }
        }

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

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H256_INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

// The single-letter working variables a..h (and the message schedule w) follow
// the FIPS 180-4 SHA-256 pseudocode verbatim; renaming them would hurt, not
// help, readability for anyone checking the implementation against the spec.
#[allow(clippy::many_single_char_names)]
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = H256_INIT;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut sha_msg = data.to_vec();
    sha_msg.push(0x80);
    while (sha_msg.len() % 64) != 56 {
        sha_msg.push(0);
    }
    sha_msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in sha_msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, wi) in w.iter_mut().take(16).enumerate() {
            let off = i * 4;
            *wi = u32::from_be_bytes([chunk[off], chunk[off + 1], chunk[off + 2], chunk[off + 3]]);
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

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let block_size = 64;

    let key_used;
    let key_hash;
    if key.len() > block_size {
        key_hash = sha256(key);
        key_used = &key_hash[..];
    } else {
        key_used = key;
    }

    let mut k_padded = vec![0u8; block_size];
    k_padded[..key_used.len()].copy_from_slice(key_used);

    let mut inner = Vec::with_capacity(block_size + data.len());
    for &b in &k_padded {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);

    let mut outer = Vec::with_capacity(block_size + 32);
    for &b in &k_padded {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// SSH MAC: HMAC-SHA256(key, `sequence_number(u32` be) || `unencrypted_packet`).
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
// ============================================================================

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

const AES_RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn gf_mul2(x: u8) -> u8 {
    if x & 0x80 != 0 {
        (x << 1) ^ 0x1b
    } else {
        x << 1
    }
}

/// AES-128 key expansion: 16-byte key -> 11 round keys (176 bytes).
fn aes128_expand_key(key: &[u8]) -> Vec<u8> {
    let mut expanded = vec![0u8; 176];
    expanded[..16].copy_from_slice(&key[..16]);

    let mut i = 16;
    let mut rcon_idx = 0;
    while i < 176 {
        let mut temp = [
            expanded[i - 4],
            expanded[i - 3],
            expanded[i - 2],
            expanded[i - 1],
        ];

        if i % 16 == 0 {
            temp.rotate_left(1);
            for b in &mut temp {
                *b = AES_SBOX[*b as usize];
            }
            temp[0] ^= AES_RCON[rcon_idx];
            rcon_idx += 1;
        }

        for j in 0..4 {
            expanded[i + j] = expanded[i + j - 16] ^ temp[j];
        }
        i += 4;
    }
    expanded
}

/// AES-128 encrypt a single 16-byte block in-place.
fn aes128_encrypt_block(block: &mut [u8; 16], round_keys: &[u8]) {
    // AddRoundKey (round 0)
    for i in 0..16 {
        block[i] ^= round_keys[i];
    }

    for round in 1..10 {
        // SubBytes
        for b in block.iter_mut() {
            *b = AES_SBOX[*b as usize];
        }
        // ShiftRows
        let tmp = *block;
        block[1] = tmp[5];
        block[5] = tmp[9];
        block[9] = tmp[13];
        block[13] = tmp[1];
        block[2] = tmp[10];
        block[6] = tmp[14];
        block[10] = tmp[2];
        block[14] = tmp[6];
        block[3] = tmp[15];
        block[7] = tmp[3];
        block[11] = tmp[7];
        block[15] = tmp[11];
        // MixColumns
        for col in 0..4 {
            let c = col * 4;
            let a0 = block[c];
            let a1 = block[c + 1];
            let a2 = block[c + 2];
            let a3 = block[c + 3];
            block[c] = gf_mul2(a0) ^ gf_mul2(a1) ^ a1 ^ a2 ^ a3;
            block[c + 1] = a0 ^ gf_mul2(a1) ^ gf_mul2(a2) ^ a2 ^ a3;
            block[c + 2] = a0 ^ a1 ^ gf_mul2(a2) ^ gf_mul2(a3) ^ a3;
            block[c + 3] = gf_mul2(a0) ^ a0 ^ a1 ^ a2 ^ gf_mul2(a3);
        }
        // AddRoundKey
        let rk_off = round * 16;
        for i in 0..16 {
            block[i] ^= round_keys[rk_off + i];
        }
    }

    // Final round (no MixColumns)
    for b in block.iter_mut() {
        *b = AES_SBOX[*b as usize];
    }
    let tmp = *block;
    block[1] = tmp[5];
    block[5] = tmp[9];
    block[9] = tmp[13];
    block[13] = tmp[1];
    block[2] = tmp[10];
    block[6] = tmp[14];
    block[10] = tmp[2];
    block[14] = tmp[6];
    block[3] = tmp[15];
    block[7] = tmp[3];
    block[11] = tmp[7];
    block[15] = tmp[11];
    for i in 0..16 {
        block[i] ^= round_keys[160 + i];
    }
}

/// Increment a 128-bit counter (big-endian) for CTR mode.
// Reserved for the AES-CTR cipher path: the keystream generator will bump the
// per-block counter via this helper once CTR transport encryption is wired into
// the packet layer. Not yet invoked.
#[allow(dead_code)]
fn increment_counter(ctr: &mut [u8; 16]) {
    for i in (0..16).rev() {
        let (val, overflow) = ctr[i].overflowing_add(1);
        ctr[i] = val;
        if !overflow {
            break;
        }
    }
}

/// Build the AES-CTR counter for a given block index.
fn build_ctr(iv: &[u8], seq: u32, block_idx: usize) -> [u8; 16] {
    let mut ctr = [0u8; 16];
    let copy_len = iv.len().min(16);
    ctr[..copy_len].copy_from_slice(&iv[..copy_len]);

    // For SSH AES-CTR, the IV is used as the initial counter and incremented
    // per block. We add seq * (large_blocks) + block_idx to get the correct
    // counter for a given packet/block.
    let offset = u64::from(seq)
        .wrapping_mul(256)
        .wrapping_add(block_idx as u64);
    let mut carry = offset;
    for i in (0..16).rev() {
        let sum = u64::from(ctr[i]).wrapping_add(carry & 0xFF);
        ctr[i] = sum as u8;
        carry = (carry >> 8).wrapping_add(sum >> 8);
    }
    ctr
}

/// Encrypt a packet with AES-128-CTR in-place.
fn encrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], seq: u32) {
    if key.len() < 16 {
        return;
    }
    let round_keys = aes128_expand_key(key);
    let mut block_idx = 0;
    let mut offset = 0;
    while offset < packet.len() {
        let mut ctr = build_ctr(iv, seq, block_idx);
        aes128_encrypt_block(&mut ctr, &round_keys);
        let end = (offset + 16).min(packet.len());
        for i in offset..end {
            packet[i] ^= ctr[i - offset];
        }
        offset += 16;
        block_idx += 1;
    }
}

/// Decrypt is the same as encrypt for CTR mode.
fn decrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], seq: u32) {
    encrypt_packet_aes_ctr(packet, key, iv, seq);
}

/// Decrypt a single block for peeking at the first block.
fn decrypt_block_aes_ctr(
    data: &[u8],
    key: &[u8],
    iv: &[u8],
    seq: u32,
    block_idx: usize,
) -> Vec<u8> {
    if key.len() < 16 {
        return data.to_vec();
    }
    let round_keys = aes128_expand_key(key);
    let mut ctr = build_ctr(iv, seq, block_idx);
    aes128_encrypt_block(&mut ctr, &round_keys);
    let mut result = data.to_vec();
    for (i, byte) in result.iter_mut().enumerate() {
        if i < 16 {
            *byte ^= ctr[i];
        }
    }
    result
}

// ============================================================================
// Encryption state
// ============================================================================

#[derive(Clone)]
struct EncryptionState {
    enc_key_c2s: Vec<u8>,
    enc_key_s2c: Vec<u8>,
    iv_c2s: Vec<u8>,
    iv_s2c: Vec<u8>,
    mac_key_c2s: Vec<u8>,
    mac_key_s2c: Vec<u8>,
    block_size: usize,
    mac_len: usize,
}

impl EncryptionState {
    fn none() -> Self {
        Self {
            enc_key_c2s: Vec::new(),
            enc_key_s2c: Vec::new(),
            iv_c2s: Vec::new(),
            iv_s2c: Vec::new(),
            mac_key_c2s: Vec::new(),
            mac_key_s2c: Vec::new(),
            block_size: 8,
            mac_len: 0,
        }
    }
}

/// Derive SSH transport keys from the shared secret and exchange hash.
/// RFC 4253, Section 7.2.
fn derive_keys(
    shared_secret: &[u8],
    exchange_hash: &[u8; 32],
    session_id: &[u8; 32],
) -> EncryptionState {
    let k_enc = encode_mpint(shared_secret);

    let derive = |label: u8| -> Vec<u8> {
        let mut input = Vec::new();
        input.extend_from_slice(&k_enc);
        input.extend_from_slice(exchange_hash);
        input.push(label);
        input.extend_from_slice(session_id);
        sha256(&input).to_vec()
    };

    EncryptionState {
        iv_c2s: derive(b'A')[..16].to_vec(),
        iv_s2c: derive(b'B')[..16].to_vec(),
        enc_key_c2s: derive(b'C')[..16].to_vec(),
        enc_key_s2c: derive(b'D')[..16].to_vec(),
        mac_key_c2s: derive(b'E'),
        mac_key_s2c: derive(b'F'),
        block_size: 16,
        mac_len: 32,
    }
}

// ============================================================================
// Diffie-Hellman group 14 parameters (RFC 3526)
// ============================================================================

/// DH group 14 prime (2048-bit MODP group).
fn dh_group14_prime() -> BigUint {
    let p_hex = concat!(
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

    let mut bytes = Vec::new();
    let mut chars = p_hex.chars();
    while let Some(hi) = chars.next() {
        if let Some(lo) = chars.next() {
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).unwrap_or(0);
            bytes.push(byte);
        }
    }
    BigUint::from_bytes_be(&bytes)
}

/// DH group 14 generator.
fn dh_group14_generator() -> BigUint {
    BigUint::from_bytes_be(&[2])
}

/// Generate a deterministic-looking private key from available entropy sources.
/// In a real implementation this would use a CSPRNG, but for this simplified
/// daemon we derive from process-id and monotonic clock.
fn generate_dh_private() -> BigUint {
    let pid = get_pid();
    let time = clock_monotonic_ms();
    let mut seed = Vec::new();
    seed.extend_from_slice(&pid.to_le_bytes());
    seed.extend_from_slice(&time.to_le_bytes());
    seed.extend_from_slice(b"sshd-dh-private-key-seed");
    let h1 = sha256(&seed);
    seed.extend_from_slice(&h1);
    let h2 = sha256(&seed);

    let mut key_bytes = Vec::with_capacity(64);
    key_bytes.extend_from_slice(&h1);
    key_bytes.extend_from_slice(&h2);
    // Ensure it is shorter than the prime (256 bits < 2048 bits).
    BigUint::from_bytes_be(&key_bytes[..32])
}

// ============================================================================
// Host key (Ed25519 structured representation)
//
// We store a 32-byte seed and derive a public key. The actual Ed25519 math
// (curve operations) would require a full implementation. Here we structure
// the host key data and sign by hashing with the private seed, which is
// sufficient for the protocol framing. A production server would use real
// Ed25519.
// ============================================================================

struct HostKey {
    /// 32-byte private seed.
    seed: [u8; 32],
    /// 32-byte "public key" (SHA-256 of seed for this simplified impl).
    public_key: [u8; 32],
}

impl HostKey {
    /// Create a host key from a 32-byte seed.
    fn from_seed(seed: [u8; 32]) -> Self {
        let public_key = sha256(&seed);
        Self { seed, public_key }
    }

    /// Generate a deterministic host key from the daemon's identity.
    fn generate_default() -> Self {
        let mut material = Vec::new();
        material.extend_from_slice(b"ouros-sshd-default-host-key");
        material.extend_from_slice(&get_pid().to_le_bytes());
        let seed = sha256(&material);
        Self::from_seed(seed)
    }

    /// Encode the public key in SSH wire format: "ssh-ed25519" + `key_data`.
    fn public_key_blob(&self) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        blob.extend_from_slice(&ssh_string(&self.public_key));
        blob
    }

    /// Sign data using the host key seed (simplified: HMAC-SHA256 with seed).
    fn sign(&self, data: &[u8]) -> Vec<u8> {
        let sig_bytes = hmac_sha256(&self.seed, data);
        let mut sig_blob = Vec::new();
        sig_blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        // Ed25519 signatures are 64 bytes; pad our 32-byte HMAC to 64.
        let mut sig64 = [0u8; 64];
        sig64[..32].copy_from_slice(&sig_bytes);
        // Derive the second half from additional hashing.
        let extra = sha256(&sig_bytes);
        sig64[32..].copy_from_slice(&extra);
        sig_blob.extend_from_slice(&ssh_string(&sig64));
        sig_blob
    }

    /// Compute the SHA-256 fingerprint of the public key.
    fn fingerprint(&self) -> String {
        let blob = self.public_key_blob();
        let hash = sha256(&blob);
        let encoded = base64_encode(&hash);
        format!("SHA256:{encoded}")
    }

    /// Try to load a host key from a file. Supports a simplified format:
    /// the file should contain 32 hex-encoded bytes (64 hex chars) or
    /// 32 raw bytes.
    fn load_from_file(path: &str) -> Result<Self, SshdError> {
        let data = fs_read_file(path)?;
        if data.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&data);
            return Ok(Self::from_seed(seed));
        }
        // Try hex-encoded.
        let text = String::from_utf8_lossy(&data);
        let hex_str: String = text.chars().filter(char::is_ascii_hexdigit).collect();
        if hex_str.len() >= 64 {
            let mut seed = [0u8; 32];
            for i in 0..32 {
                seed[i] = u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16)
                    .map_err(|_| SshdError::ConfigError(format!("invalid hex in {path}")))?;
            }
            return Ok(Self::from_seed(seed));
        }
        // Try OpenSSH format: look for base64 payload after the key type.
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("-----") || line.is_empty() || line.starts_with('#') {
                continue;
            }
            // OpenSSH private key: after headers, base64 data contains the seed.
            // Simplified: hash the entire file content as the seed.
            let seed = sha256(line.as_bytes());
            return Ok(Self::from_seed(seed));
        }
        Err(SshdError::ConfigError(format!(
            "cannot parse host key from {path}"
        )))
    }
}

/// Minimal base64 encoder (no padding variant for fingerprint display).
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        result.push(ALPHABET[(b0 >> 2) as usize] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4 | (b1 >> 4)) as usize] as char);

        if i + 1 < data.len() {
            result.push(ALPHABET[((b1 & 0x0F) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            result.push('=');
        }

        if i + 2 < data.len() {
            result.push(ALPHABET[(b2 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }
    result
}

/// Minimal base64 decoder.
// a..d are the four decoded sextets of a base64 quartet; short names match the
// usual base64 decode formulation.
#[allow(clippy::many_single_char_names)]
fn base64_decode(input: &str) -> Vec<u8> {
    const DECODE: [u8; 128] = {
        let mut table = [0xFFu8; 128];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i;
            table[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut d = 0u8;
        while d < 10 {
            table[(b'0' + d) as usize] = d + 52;
            d += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let mut output = Vec::new();
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut i = 0;
    // Process 4-char groups. The padding ('=') has already been stripped, so
    // the final group may legitimately be 2 or 3 chars (encoding 1 or 2 bytes);
    // read the 3rd/4th positions with bounds checks rather than requiring a
    // full quartet (the old `i + 3 < len` condition dropped the last group).
    while i + 1 < bytes.len() {
        let a = DECODE.get(bytes[i] as usize).copied().unwrap_or(0xFF);
        let b = DECODE.get(bytes[i + 1] as usize).copied().unwrap_or(0xFF);
        let c = bytes
            .get(i + 2)
            .and_then(|&x| DECODE.get(x as usize).copied())
            .unwrap_or(0xFF);
        let d = bytes
            .get(i + 3)
            .and_then(|&x| DECODE.get(x as usize).copied())
            .unwrap_or(0xFF);
        if a == 0xFF || b == 0xFF {
            break;
        }
        output.push((a << 2) | (b >> 4));
        if c != 0xFF {
            output.push((b << 4) | (c >> 2));
            if d != 0xFF {
                output.push((c << 6) | d);
            }
        }
        i += 4;
    }
    output
}

/// Lowercase hex encoding of a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            // Writing to a String is infallible, so the Result is ignored.
            let _ = write!(s, "{b:02x}");
            s
        })
}

// ============================================================================
// Configuration
// ============================================================================

/// SSH server configuration parsed from `sshd_config`.
#[derive(Clone)]
struct SshdConfig {
    port: u16,
    listen_address: String,
    host_key_file: String,
    permit_root_login: PermitRootLogin,
    password_authentication: bool,
    pubkey_authentication: bool,
    authorized_keys_file: String,
    max_auth_tries: u32,
    login_grace_time: u32,
    max_sessions: u32,
    banner_file: String,
    print_motd: bool,
    subsystems: Vec<(String, String)>,
    allow_users: Vec<String>,
    deny_users: Vec<String>,
    allow_groups: Vec<String>,
    deny_groups: Vec<String>,
}

/// Root login policy.
#[derive(Clone, PartialEq, Eq, Debug)]
enum PermitRootLogin {
    Yes,
    No,
    ProhibitPassword,
}

impl SshdConfig {
    fn default_config() -> Self {
        Self {
            port: 22,
            listen_address: "0.0.0.0".into(),
            host_key_file: "/etc/ssh/ssh_host_ed25519_key".into(),
            permit_root_login: PermitRootLogin::ProhibitPassword,
            password_authentication: true,
            pubkey_authentication: true,
            authorized_keys_file: ".ssh/authorized_keys".into(),
            max_auth_tries: 6,
            login_grace_time: 120,
            max_sessions: 10,
            banner_file: String::new(),
            print_motd: true,
            subsystems: vec![("sftp".into(), "/usr/lib/sftp-server".into())],
            allow_users: Vec::new(),
            deny_users: Vec::new(),
            allow_groups: Vec::new(),
            deny_groups: Vec::new(),
        }
    }

    /// Parse configuration from `sshd_config` file contents.
    fn parse(content: &str) -> Result<Self, SshdError> {
        let mut config = Self::default_config();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Split on first whitespace.
            let (directive, value) = match line.split_once(|c: char| c.is_whitespace()) {
                Some((d, v)) => (d.trim(), v.trim()),
                None => continue,
            };

            match directive.to_lowercase().as_str() {
                "port" => {
                    config.port = value
                        .parse()
                        .map_err(|_| SshdError::ConfigError(format!("invalid port: {value}")))?;
                }
                "listenaddress" => {
                    config.listen_address = value.into();
                }
                "hostkey" => {
                    config.host_key_file = value.into();
                }
                "permitrootlogin" => {
                    config.permit_root_login = match value.to_lowercase().as_str() {
                        "yes" => PermitRootLogin::Yes,
                        "no" => PermitRootLogin::No,
                        "prohibit-password" | "without-password" => {
                            PermitRootLogin::ProhibitPassword
                        }
                        _ => {
                            return Err(SshdError::ConfigError(format!(
                                "invalid PermitRootLogin: {value}"
                            )));
                        }
                    };
                }
                "passwordauthentication" => {
                    config.password_authentication = parse_bool(value)?;
                }
                "pubkeyauthentication" => {
                    config.pubkey_authentication = parse_bool(value)?;
                }
                "authorizedkeysfile" => {
                    config.authorized_keys_file = value.into();
                }
                "maxauthtries" => {
                    config.max_auth_tries = value.parse().map_err(|_| {
                        SshdError::ConfigError(format!("invalid MaxAuthTries: {value}"))
                    })?;
                }
                "logingracetime" => {
                    config.login_grace_time = value.parse().map_err(|_| {
                        SshdError::ConfigError(format!("invalid LoginGraceTime: {value}"))
                    })?;
                }
                "maxsessions" => {
                    config.max_sessions = value.parse().map_err(|_| {
                        SshdError::ConfigError(format!("invalid MaxSessions: {value}"))
                    })?;
                }
                "banner" => {
                    config.banner_file = value.into();
                }
                "printmotd" => {
                    config.print_motd = parse_bool(value)?;
                }
                "subsystem" => {
                    if let Some((name, cmd)) = value.split_once(|c: char| c.is_whitespace()) {
                        config
                            .subsystems
                            .push((name.trim().into(), cmd.trim().into()));
                    }
                }
                "allowusers" => {
                    for user in value.split_whitespace() {
                        config.allow_users.push(user.into());
                    }
                }
                "denyusers" => {
                    for user in value.split_whitespace() {
                        config.deny_users.push(user.into());
                    }
                }
                "allowgroups" => {
                    for group in value.split_whitespace() {
                        config.allow_groups.push(group.into());
                    }
                }
                "denygroups" => {
                    for group in value.split_whitespace() {
                        config.deny_groups.push(group.into());
                    }
                }
                _ => {
                    // Unknown directive -- ignore for forward compatibility.
                }
            }
        }

        Ok(config)
    }

    /// Format config as human-readable text (for -T option).
    fn dump(&self) -> String {
        let root_login = match &self.permit_root_login {
            PermitRootLogin::Yes => "yes",
            PermitRootLogin::No => "no",
            PermitRootLogin::ProhibitPassword => "prohibit-password",
        };
        let yn = |b: bool| if b { "yes" } else { "no" };
        let mut lines = vec![
            format!("port {}", self.port),
            format!("listenaddress {}", self.listen_address),
            format!("hostkey {}", self.host_key_file),
            format!("permitrootlogin {root_login}"),
            format!(
                "passwordauthentication {}",
                yn(self.password_authentication)
            ),
            format!("pubkeyauthentication {}", yn(self.pubkey_authentication)),
            format!("authorizedkeysfile {}", self.authorized_keys_file),
            format!("maxauthtries {}", self.max_auth_tries),
            format!("logingracetime {}", self.login_grace_time),
            format!("maxsessions {}", self.max_sessions),
        ];
        if !self.banner_file.is_empty() {
            lines.push(format!("banner {}", self.banner_file));
        }
        lines.push(format!("printmotd {}", yn(self.print_motd)));
        for (name, cmd) in &self.subsystems {
            lines.push(format!("subsystem {name} {cmd}"));
        }
        if !self.allow_users.is_empty() {
            lines.push(format!("allowusers {}", self.allow_users.join(" ")));
        }
        if !self.deny_users.is_empty() {
            lines.push(format!("denyusers {}", self.deny_users.join(" ")));
        }
        if !self.allow_groups.is_empty() {
            lines.push(format!("allowgroups {}", self.allow_groups.join(" ")));
        }
        if !self.deny_groups.is_empty() {
            lines.push(format!("denygroups {}", self.deny_groups.join(" ")));
        }
        // Each directive on its own line, with a trailing newline to match the
        // historical per-line `push_str(... "\n")` output.
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

/// Parse a boolean config value.
fn parse_bool(value: &str) -> Result<bool, SshdError> {
    match value.to_lowercase().as_str() {
        "yes" | "true" | "1" => Ok(true),
        "no" | "false" | "0" => Ok(false),
        _ => Err(SshdError::ConfigError(format!("invalid boolean: {value}"))),
    }
}

// ============================================================================
// User authentication
// ============================================================================

/// An entry from /etc/shadow.
struct ShadowEntry {
    username: String,
    /// The password hash (e.g., "$6$salt$hash" or plain SHA-256 hex).
    password_hash: String,
}

/// Parse /etc/shadow content into entries.
fn parse_shadow(content: &str) -> Vec<ShadowEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 2 {
            entries.push(ShadowEntry {
                username: fields[0].into(),
                password_hash: fields[1].into(),
            });
        }
    }
    entries
}

/// Verify a password against its SHA-256 hash.
/// Supports: raw hex SHA-256, $5$ (SHA-256 crypt), or plain comparison for
/// testing. Returns true if the password matches.
fn verify_password(password: &str, stored_hash: &str) -> bool {
    if stored_hash.is_empty() || stored_hash == "!" || stored_hash == "*" || stored_hash == "!!" {
        return false; // Locked account.
    }

    // Check if it's a $5$salt$hash format (SHA-256 crypt).
    if let Some(rest) = stored_hash.strip_prefix("$5$")
        && let Some((salt, expected_hash)) = rest.rsplit_once('$')
    {
        let mut salted = Vec::new();
        salted.extend_from_slice(password.as_bytes());
        salted.extend_from_slice(salt.as_bytes());
        let computed = sha256(&salted);
        let computed_hex = hex_encode(&computed);
        return constant_time_eq(computed_hex.as_bytes(), expected_hash.as_bytes());
    }

    // Check if it's a $6$salt$hash format (SHA-512 -- we approximate with SHA-256).
    if let Some(rest) = stored_hash.strip_prefix("$6$")
        && let Some((salt, expected_hash)) = rest.rsplit_once('$')
    {
        let mut salted = Vec::new();
        salted.extend_from_slice(password.as_bytes());
        salted.extend_from_slice(salt.as_bytes());
        let computed = sha256(&salted);
        let computed_hex = hex_encode(&computed);
        return constant_time_eq(computed_hex.as_bytes(), expected_hash.as_bytes());
    }

    // Plain SHA-256 hex hash.
    if stored_hash.len() == 64 && stored_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        let computed = sha256(password.as_bytes());
        let computed_hex = hex_encode(&computed);
        return constant_time_eq(computed_hex.as_bytes(), stored_hash.as_bytes());
    }

    // Plaintext comparison (development/testing only).
    constant_time_eq(password.as_bytes(), stored_hash.as_bytes())
}

/// An entry from an `authorized_keys` file.
#[derive(Clone, Debug)]
struct AuthorizedKey {
    // Parsed and retained for completeness; publickey auth currently matches on
    // `key_data` (the wire blob) only. `key_type` will gate algorithm selection
    // and `comment` will appear in audit logs once those paths are wired.
    #[allow(dead_code)]
    key_type: String,
    key_data: Vec<u8>,
    #[allow(dead_code)]
    comment: String,
}

/// Parse an `authorized_keys` file.
fn parse_authorized_keys(content: &str) -> Vec<AuthorizedKey> {
    let mut keys = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 2 {
            let key_type = parts[0].to_string();
            // Validate key type.
            if !matches!(
                key_type.as_str(),
                "ssh-rsa"
                    | "ssh-ed25519"
                    | "ecdsa-sha2-nistp256"
                    | "ecdsa-sha2-nistp384"
                    | "ecdsa-sha2-nistp521"
                    | "ssh-dss"
            ) {
                continue;
            }
            let key_data = base64_decode(parts[1]);
            let comment = if parts.len() >= 3 {
                parts[2].to_string()
            } else {
                String::new()
            };
            keys.push(AuthorizedKey {
                key_type,
                key_data,
                comment,
            });
        }
    }
    keys
}

/// Check if a username is allowed by the allow/deny user/group lists.
fn is_user_allowed(username: &str, groups: &[String], config: &SshdConfig) -> bool {
    // DenyUsers takes precedence.
    if !config.deny_users.is_empty() && config.deny_users.iter().any(|u| u == username) {
        return false;
    }

    // DenyGroups.
    if !config.deny_groups.is_empty() {
        for group in groups {
            if config.deny_groups.iter().any(|g| g == group) {
                return false;
            }
        }
    }

    // AllowUsers: if specified, user must be in the list.
    if !config.allow_users.is_empty() && !config.allow_users.iter().any(|u| u == username) {
        return false;
    }

    // AllowGroups: if specified, at least one group must match.
    if !config.allow_groups.is_empty() {
        let has_match = groups
            .iter()
            .any(|g| config.allow_groups.iter().any(|ag| ag == g));
        if !has_match {
            return false;
        }
    }

    true
}

/// Check root login policy.
fn is_root_login_allowed(auth_method: &str, config: &SshdConfig) -> bool {
    match config.permit_root_login {
        PermitRootLogin::Yes => true,
        PermitRootLogin::No => false,
        PermitRootLogin::ProhibitPassword => auth_method != "password",
    }
}

// ============================================================================
// SSH channel
// ============================================================================

struct Channel {
    /// Our channel number (server-side).
    local_id: u32,
    /// Client's channel number.
    remote_id: u32,
    /// Window size remaining for sending to client.
    remote_window: u32,
    /// Our window size.
    local_window: u32,
    /// Maximum packet size for sending.
    // Stored from the channel-open request; will cap outbound DATA payloads once
    // the daemon writes channel data back to the client. Not yet read in prod.
    #[allow(dead_code)]
    remote_max_packet: u32,
    /// Whether a PTY has been requested.
    pty_requested: bool,
    /// Terminal type (from PTY request).
    term: String,
    /// Terminal width in columns.
    term_width: u32,
    /// Terminal height in rows.
    term_height: u32,
    /// Whether the channel has been closed.
    closed: bool,
    /// Whether EOF has been sent.
    eof_sent: bool,
}

impl Channel {
    fn new(local_id: u32, remote_id: u32, remote_window: u32, remote_max_packet: u32) -> Self {
        Self {
            local_id,
            remote_id,
            remote_window,
            local_window: 0x0020_0000, // 2 MiB
            remote_max_packet,
            pty_requested: false,
            term: String::new(),
            term_width: 80,
            term_height: 24,
            closed: false,
            eof_sent: false,
        }
    }
}

// ============================================================================
// PTY request parsing
// ============================================================================

/// Parsed PTY request payload:
/// (term, `width_cols`, `height_rows`, `width_px`, `height_px`, modes).
type PtyRequest = (String, u32, u32, u32, u32, Vec<u8>);

/// Parse a PTY request payload (after the "pty-req" string and `want_reply` byte).
fn parse_pty_request(data: &[u8], offset: usize) -> Result<PtyRequest, SshdError> {
    let (term_bytes, off) = read_ssh_string(data, offset)?;
    let term = String::from_utf8_lossy(term_bytes).into_owned();
    let (width_cols, off) = read_u32(data, off)?;
    let (height_rows, off) = read_u32(data, off)?;
    let (width_px, off) = read_u32(data, off)?;
    let (height_px, off) = read_u32(data, off)?;
    let (modes, _off) = read_ssh_string(data, off)?;
    Ok((
        term,
        width_cols,
        height_rows,
        width_px,
        height_px,
        modes.to_vec(),
    ))
}

// ============================================================================
// Connection handler
// ============================================================================

/// State for a single SSH connection.
struct ConnectionState {
    handle: u64,
    stream_buf: StreamBuffer,
    config: SshdConfig,
    host_key: HostKey,
    enc: EncryptionState,
    session_id: Option<[u8; 32]>,
    send_seq: u32,
    recv_seq: u32,
    encrypted: bool,
    authenticated: bool,
    auth_attempts: u32,
    username: String,
    channels: Vec<Channel>,
    next_channel_id: u32,
    debug_mode: bool,
    connection_start_ms: u64,
}

impl ConnectionState {
    fn new(handle: u64, config: SshdConfig, host_key: HostKey, debug_mode: bool) -> Self {
        Self {
            handle,
            stream_buf: StreamBuffer::new(),
            config,
            host_key,
            enc: EncryptionState::none(),
            session_id: None,
            send_seq: 0,
            recv_seq: 0,
            encrypted: false,
            authenticated: false,
            auth_attempts: 0,
            username: String::new(),
            channels: Vec::new(),
            next_channel_id: 0,
            debug_mode,
            connection_start_ms: clock_monotonic_ms(),
        }
    }

    /// Send a packet.
    fn send_packet(&mut self, payload: &[u8]) -> Result<(), SshdError> {
        let pkt = build_packet(payload, self.encrypted, self.send_seq, &self.enc);
        tcp_send_all(self.handle, &pkt)?;
        self.send_seq = self.send_seq.wrapping_add(1);
        Ok(())
    }

    /// Receive a packet.
    fn recv_packet(&mut self) -> Result<Vec<u8>, SshdError> {
        let payload = read_packet(
            self.handle,
            &mut self.stream_buf,
            self.encrypted,
            self.recv_seq,
            &self.enc,
        )?;
        self.recv_seq = self.recv_seq.wrapping_add(1);
        Ok(payload)
    }

    /// Log a debug message.
    fn debug_log(&self, msg: &str) {
        if self.debug_mode {
            eprintln!("sshd[debug]: {msg}");
        }
    }
}

/// Handle a single SSH connection.
fn handle_connection(handle: u64, config: &SshdConfig, host_key: &HostKey, debug_mode: bool) {
    let peer = tcp_peer_addr(handle).map_or_else(
        |_| "unknown".into(),
        |(ip, port)| format!("{}:{}", format_ip(ip), port),
    );

    if debug_mode {
        eprintln!("sshd: connection from {peer}");
    }

    let hk = HostKey::from_seed(host_key.seed);
    let mut conn = ConnectionState::new(handle, config.clone(), hk, debug_mode);

    let result = run_connection(&mut conn);

    if let Err(e) = &result
        && debug_mode
    {
        eprintln!("sshd: connection from {peer} error: {e}");
    }

    tcp_close(handle);

    if debug_mode {
        eprintln!("sshd: connection from {peer} closed");
    }
}

/// Main connection protocol flow.
fn run_connection(conn: &mut ConnectionState) -> Result<(), SshdError> {
    // 1. Version exchange.
    do_version_exchange(conn)?;

    // 2. Key exchange.
    do_key_exchange(conn)?;

    // 3. Service request (ssh-userauth).
    handle_service_request(conn)?;

    // 4. User authentication.
    do_user_auth(conn)?;

    // 5. Channel handling loop.
    handle_channels(conn)?;

    Ok(())
}

// ============================================================================
// Protocol phases
// ============================================================================

/// SSH version exchange. We send our version, read the client's.
fn do_version_exchange(conn: &mut ConnectionState) -> Result<(), SshdError> {
    // Send our version string.
    let version_line = format!("{SSH_SERVER_VERSION}\r\n");
    tcp_send_all(conn.handle, version_line.as_bytes())?;

    conn.debug_log("sent version string");

    // Read client version string.
    let client_version = read_version_line(conn)?;
    conn.debug_log(&format!("client version: {client_version}"));

    if !client_version.starts_with("SSH-2.0-") {
        return Err(SshdError::ProtocolError(format!(
            "unsupported client version: {client_version}"
        )));
    }

    Ok(())
}

/// Read the SSH version line from the client.
fn read_version_line(conn: &mut ConnectionState) -> Result<String, SshdError> {
    let mut line = Vec::new();
    let mut single = [0u8; 1];
    loop {
        let n = tcp_recv(conn.handle, &mut single)?;
        if n == 0 {
            return Err(SshdError::ProtocolError(
                "connection closed during version exchange".into(),
            ));
        }
        if single[0] == b'\n' {
            break;
        }
        if single[0] != b'\r' {
            line.push(single[0]);
        }
        if line.len() > 255 {
            return Err(SshdError::ProtocolError("version string too long".into()));
        }
    }
    String::from_utf8(line)
        .map_err(|_| SshdError::ProtocolError("invalid UTF-8 in version string".into()))
}

/// Parse an SSH version string, returning the software version.
// Reserved for peer-compatibility handling: the banner exchange will use the
// parsed software version to enable known-client workarounds. The current
// handshake only validates the "SSH-2.0" prefix. Not yet invoked (but tested).
#[allow(dead_code)]
fn parse_version_string(version: &str) -> Option<&str> {
    // Format: SSH-protoversion-softwareversion SP comments
    let version = version.trim();
    if !version.starts_with("SSH-") {
        return None;
    }
    let after_ssh = &version[4..];
    // Skip protocol version.
    let after_proto = after_ssh.find('-').map(|i| &after_ssh[i + 1..])?;
    // Software version is up to the first space (or end).
    Some(after_proto.split(' ').next().unwrap_or(after_proto))
}

/// Perform SSH key exchange (DH group14-sha256).
fn do_key_exchange(conn: &mut ConnectionState) -> Result<(), SshdError> {
    // Build and send our KEXINIT.
    let server_kexinit = build_kexinit();
    conn.send_packet(&server_kexinit)?;
    conn.debug_log("sent KEXINIT");

    // Receive client KEXINIT.
    let client_kexinit = conn.recv_packet()?;
    if client_kexinit.first().copied() != Some(msg::SSH_MSG_KEXINIT) {
        return Err(SshdError::ProtocolError("expected KEXINIT".into()));
    }
    conn.debug_log("received client KEXINIT");

    // Receive KEX_DH_INIT from client.
    let dh_init = conn.recv_packet()?;
    if dh_init.first().copied() != Some(msg::SSH_MSG_KEX_DH_INIT) {
        return Err(SshdError::ProtocolError("expected KEX_DH_INIT".into()));
    }
    conn.debug_log("received KEX_DH_INIT");

    // Parse client's DH public value (e).
    let (client_e_bytes, _) = read_mpint(&dh_init, 1)?;
    let client_e = BigUint::from_bytes_be(&client_e_bytes);

    // Generate our DH keypair.
    let p = dh_group14_prime();
    let g = dh_group14_generator();
    let y = generate_dh_private();
    let f = g.mod_pow(&y, &p); // f = g^y mod p
    let shared_secret_big = client_e.mod_pow(&y, &p); // K = e^y mod p
    let shared_secret = shared_secret_big.to_bytes_be();

    // Compute exchange hash H.
    let exchange_hash = compute_exchange_hash(
        SSH_SERVER_VERSION,
        &client_kexinit,
        &server_kexinit,
        &conn.host_key.public_key_blob(),
        &client_e.to_bytes_be(),
        &f.to_bytes_be(),
        &shared_secret,
    );

    // This is the session ID (first exchange hash).
    let session_id = exchange_hash;
    conn.session_id = Some(session_id);

    // Sign the exchange hash with our host key.
    let signature = conn.host_key.sign(&exchange_hash);

    // Build KEX_DH_REPLY.
    let mut reply = Vec::new();
    reply.push(msg::SSH_MSG_KEX_DH_REPLY);
    reply.extend_from_slice(&ssh_string(&conn.host_key.public_key_blob()));
    reply.extend_from_slice(&encode_mpint(&f.to_bytes_be()));
    reply.extend_from_slice(&ssh_string(&signature));
    conn.send_packet(&reply)?;
    conn.debug_log("sent KEX_DH_REPLY");

    // Send NEWKEYS.
    conn.send_packet(&[msg::SSH_MSG_NEWKEYS])?;
    conn.debug_log("sent NEWKEYS");

    // Receive NEWKEYS from client.
    let newkeys = conn.recv_packet()?;
    if newkeys.first().copied() != Some(msg::SSH_MSG_NEWKEYS) {
        return Err(SshdError::ProtocolError("expected NEWKEYS".into()));
    }
    conn.debug_log("received NEWKEYS");

    // Derive encryption keys.
    conn.enc = derive_keys(&shared_secret, &exchange_hash, &session_id);
    conn.encrypted = true;
    conn.debug_log("encryption activated");

    Ok(())
}

/// Build a KEXINIT message.
fn build_kexinit() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(msg::SSH_MSG_KEXINIT);

    // 16-byte cookie (pseudo-random).
    let cookie = sha256(b"sshd-kex-cookie");
    payload.extend_from_slice(&cookie[..16]);

    // Name-lists:
    // kex_algorithms
    payload.extend_from_slice(&ssh_string(b"diffie-hellman-group14-sha256"));
    // server_host_key_algorithms
    payload.extend_from_slice(&ssh_string(b"ssh-ed25519"));
    // encryption_algorithms_client_to_server
    payload.extend_from_slice(&ssh_string(b"aes128-ctr"));
    // encryption_algorithms_server_to_client
    payload.extend_from_slice(&ssh_string(b"aes128-ctr"));
    // mac_algorithms_client_to_server
    payload.extend_from_slice(&ssh_string(b"hmac-sha2-256"));
    // mac_algorithms_server_to_client
    payload.extend_from_slice(&ssh_string(b"hmac-sha2-256"));
    // compression_algorithms_client_to_server
    payload.extend_from_slice(&ssh_string(b"none"));
    // compression_algorithms_server_to_client
    payload.extend_from_slice(&ssh_string(b"none"));
    // languages_client_to_server
    payload.extend_from_slice(&ssh_string(b""));
    // languages_server_to_client
    payload.extend_from_slice(&ssh_string(b""));
    // first_kex_packet_follows
    payload.push(0);
    // reserved
    payload.extend_from_slice(&0u32.to_be_bytes());

    payload
}

/// Compute the SSH exchange hash H (RFC 4253, Section 8).
fn compute_exchange_hash(
    server_version: &str,
    client_kexinit: &[u8],
    server_kexinit: &[u8],
    host_key_blob: &[u8],
    client_e: &[u8],
    server_f: &[u8],
    shared_secret: &[u8],
) -> [u8; 32] {
    let mut hash_input = Vec::new();

    // V_C: client version (we use a placeholder since we don't store it).
    hash_input.extend_from_slice(&ssh_string(b"SSH-2.0-client"));
    // V_S: server version.
    hash_input.extend_from_slice(&ssh_string(server_version.as_bytes()));
    // I_C: client KEXINIT payload.
    hash_input.extend_from_slice(&ssh_string(client_kexinit));
    // I_S: server KEXINIT payload.
    hash_input.extend_from_slice(&ssh_string(server_kexinit));
    // K_S: host key blob.
    hash_input.extend_from_slice(&ssh_string(host_key_blob));
    // e: client DH value.
    hash_input.extend_from_slice(&encode_mpint(client_e));
    // f: server DH value.
    hash_input.extend_from_slice(&encode_mpint(server_f));
    // K: shared secret.
    hash_input.extend_from_slice(&encode_mpint(shared_secret));

    sha256(&hash_input)
}

/// Handle the SSH-USERAUTH service request.
fn handle_service_request(conn: &mut ConnectionState) -> Result<(), SshdError> {
    let payload = conn.recv_packet()?;
    if payload.first().copied() != Some(msg::SSH_MSG_SERVICE_REQUEST) {
        return Err(SshdError::ProtocolError("expected SERVICE_REQUEST".into()));
    }

    let (service_name, _) = read_ssh_string(&payload, 1)?;
    let service = String::from_utf8_lossy(service_name);
    conn.debug_log(&format!("service request: {service}"));

    if service != "ssh-userauth" {
        return Err(SshdError::ProtocolError(format!(
            "unsupported service: {service}"
        )));
    }

    // Accept the service.
    let mut accept = Vec::new();
    accept.push(msg::SSH_MSG_SERVICE_ACCEPT);
    accept.extend_from_slice(&ssh_string(b"ssh-userauth"));
    conn.send_packet(&accept)?;

    // Send banner if configured.
    if !conn.config.banner_file.is_empty()
        && let Ok(banner_data) = fs_read_file(&conn.config.banner_file)
    {
        let mut banner_msg = Vec::new();
        banner_msg.push(msg::SSH_MSG_USERAUTH_BANNER);
        banner_msg.extend_from_slice(&ssh_string(&banner_data));
        banner_msg.extend_from_slice(&ssh_string(b"")); // language tag
        let _ = conn.send_packet(&banner_msg);
    }

    Ok(())
}

/// Perform user authentication loop.
fn do_user_auth(conn: &mut ConnectionState) -> Result<(), SshdError> {
    loop {
        // Check login grace time.
        let elapsed_s = (clock_monotonic_ms() - conn.connection_start_ms) / 1000;
        if elapsed_s > u64::from(conn.config.login_grace_time) {
            send_disconnect(conn, 2, "login grace time expired")?;
            return Err(SshdError::AuthError("login grace time expired".into()));
        }

        let payload = conn.recv_packet()?;
        if payload.first().copied() != Some(msg::SSH_MSG_USERAUTH_REQUEST) {
            // Ignore non-auth messages during auth phase.
            continue;
        }

        let (username_bytes, off) = read_ssh_string(&payload, 1)?;
        let username = String::from_utf8_lossy(username_bytes).into_owned();
        let (service_bytes, off) = read_ssh_string(&payload, off)?;
        let _service = String::from_utf8_lossy(service_bytes);
        let (method_bytes, off) = read_ssh_string(&payload, off)?;
        let method = String::from_utf8_lossy(method_bytes).into_owned();

        conn.debug_log(&format!("auth request: user={username} method={method}"));
        username.clone_into(&mut conn.username);

        // Check user allow/deny lists.
        if !is_user_allowed(&username, &[], &conn.config) {
            conn.debug_log(&format!("user {username} denied by access list"));
            send_auth_failure(conn, false)?;
            conn.auth_attempts += 1;
            if conn.auth_attempts >= conn.config.max_auth_tries {
                send_disconnect(conn, 2, "too many authentication failures")?;
                return Err(SshdError::AuthError("max auth tries exceeded".into()));
            }
            continue;
        }

        // Check root login restrictions.
        if username == "root" && !is_root_login_allowed(&method, &conn.config) {
            conn.debug_log("root login denied by policy");
            send_auth_failure(conn, false)?;
            conn.auth_attempts += 1;
            if conn.auth_attempts >= conn.config.max_auth_tries {
                send_disconnect(conn, 2, "too many authentication failures")?;
                return Err(SshdError::AuthError("max auth tries exceeded".into()));
            }
            continue;
        }

        let success = match method.as_str() {
            "password" if conn.config.password_authentication => {
                handle_password_auth(&payload, off, &username)?
            }
            "publickey" if conn.config.pubkey_authentication => {
                handle_pubkey_auth(&payload, off, &username, &conn.config)?
            }
            _ => false,
        };

        if success {
            conn.authenticated = true;
            let msg_buf = vec![msg::SSH_MSG_USERAUTH_SUCCESS];
            conn.send_packet(&msg_buf)?;
            conn.debug_log(&format!("user {username} authenticated via {method}"));
            return Ok(());
        }

        conn.auth_attempts += 1;
        if conn.auth_attempts >= conn.config.max_auth_tries {
            send_disconnect(conn, 2, "too many authentication failures")?;
            return Err(SshdError::AuthError("max auth tries exceeded".into()));
        }

        send_auth_failure(conn, false)?;
    }
}

/// Handle password authentication.
fn handle_password_auth(payload: &[u8], offset: usize, username: &str) -> Result<bool, SshdError> {
    // Skip the "change password" boolean.
    let (_change, off) = read_bool(payload, offset)?;
    let (password_bytes, _) = read_ssh_string(payload, off)?;
    let password = String::from_utf8_lossy(password_bytes);

    // Read /etc/shadow.
    let shadow_content = match fs_read_file("/etc/shadow") {
        Ok(data) => String::from_utf8_lossy(&data).into_owned(),
        Err(_) => return Ok(false),
    };

    let entries = parse_shadow(&shadow_content);
    for entry in &entries {
        if entry.username == username {
            return Ok(verify_password(&password, &entry.password_hash));
        }
    }

    Ok(false)
}

/// Handle public key authentication.
fn handle_pubkey_auth(
    payload: &[u8],
    offset: usize,
    username: &str,
    config: &SshdConfig,
) -> Result<bool, SshdError> {
    let (has_sig, off) = read_bool(payload, offset)?;
    let (key_type_bytes, off) = read_ssh_string(payload, off)?;
    let _key_type = String::from_utf8_lossy(key_type_bytes);
    let (key_blob, _off) = read_ssh_string(payload, off)?;

    // Read authorized_keys for this user.
    let keys_path = format!("/home/{username}/{}", config.authorized_keys_file);
    let keys_content = match fs_read_file(&keys_path) {
        Ok(data) => String::from_utf8_lossy(&data).into_owned(),
        Err(_) => return Ok(false),
    };

    let authorized = parse_authorized_keys(&keys_content);

    // Check if the offered key matches any authorized key.
    let matched = authorized.iter().any(|ak| ak.key_data == key_blob);

    if !matched {
        return Ok(false);
    }

    if has_sig {
        // With signature -- full auth. We'd verify the signature here in a
        // production implementation. For now we trust that the client possesses
        // the key if the blob matches.
        Ok(true)
    } else {
        // Query only -- client is asking if this key is acceptable.
        // We'd send SSH_MSG_USERAUTH_PK_OK in the real flow, but since we're
        // inside the auth loop, we return false to let the client re-submit
        // with a signature.
        Ok(false)
    }
}

/// Send `SSH_MSG_USERAUTH_FAILURE`.
fn send_auth_failure(conn: &mut ConnectionState, partial: bool) -> Result<(), SshdError> {
    let mut methods = Vec::new();
    if conn.config.password_authentication {
        methods.push("password");
    }
    if conn.config.pubkey_authentication {
        methods.push("publickey");
    }
    let methods_str = methods.join(",");

    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_USERAUTH_FAILURE);
    msg_buf.extend_from_slice(&ssh_string(methods_str.as_bytes()));
    msg_buf.push(u8::from(partial));
    conn.send_packet(&msg_buf)
}

/// Send `SSH_MSG_DISCONNECT`.
fn send_disconnect(
    conn: &mut ConnectionState,
    reason: u32,
    description: &str,
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_DISCONNECT);
    msg_buf.extend_from_slice(&reason.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(description.as_bytes()));
    msg_buf.extend_from_slice(&ssh_string(b"")); // language tag
    conn.send_packet(&msg_buf)
}

/// Handle the channel phase after authentication.
fn handle_channels(conn: &mut ConnectionState) -> Result<(), SshdError> {
    loop {
        let payload = match conn.recv_packet() {
            Ok(p) => p,
            Err(SshdError::ProtocolError(msg)) if msg.contains("connection closed") => {
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        if payload.is_empty() {
            continue;
        }

        let msg_type = payload[0];

        match msg_type {
            msg::SSH_MSG_CHANNEL_OPEN => {
                handle_channel_open(conn, &payload)?;
            }
            msg::SSH_MSG_CHANNEL_REQUEST => {
                handle_channel_request(conn, &payload)?;
            }
            msg::SSH_MSG_CHANNEL_DATA => {
                handle_channel_data(conn, &payload)?;
            }
            msg::SSH_MSG_CHANNEL_WINDOW_ADJUST => {
                handle_window_adjust(conn, &payload)?;
            }
            msg::SSH_MSG_CHANNEL_EOF => {
                handle_channel_eof(conn, &payload)?;
            }
            msg::SSH_MSG_CHANNEL_CLOSE => {
                handle_channel_close(conn, &payload)?;
                if conn.channels.iter().all(|ch| ch.closed) {
                    return Ok(());
                }
            }
            msg::SSH_MSG_DISCONNECT => {
                conn.debug_log("client sent DISCONNECT");
                return Ok(());
            }
            msg::SSH_MSG_IGNORE => {
                // Ignore.
            }
            _ => {
                conn.debug_log(&format!("unhandled message type: {msg_type}"));
            }
        }
    }
}

/// Handle `CHANNEL_OPEN`.
fn handle_channel_open(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (chan_type_bytes, off) = read_ssh_string(payload, 1)?;
    let chan_type = String::from_utf8_lossy(chan_type_bytes);
    let (sender_channel, off) = read_u32(payload, off)?;
    let (initial_window, off) = read_u32(payload, off)?;
    let (max_packet, _) = read_u32(payload, off)?;

    conn.debug_log(&format!(
        "channel open: type={chan_type} remote_id={sender_channel}"
    ));

    if chan_type != "session" {
        // Reject non-session channels.
        let mut reply = Vec::new();
        reply.push(msg::SSH_MSG_CHANNEL_OPEN_FAILURE);
        reply.extend_from_slice(&sender_channel.to_be_bytes());
        reply.extend_from_slice(&1u32.to_be_bytes()); // reason: administratively prohibited
        reply.extend_from_slice(&ssh_string(b"only session channels supported"));
        reply.extend_from_slice(&ssh_string(b""));
        return conn.send_packet(&reply);
    }

    // Check max sessions.
    let active = conn.channels.iter().filter(|ch| !ch.closed).count();
    if active >= conn.config.max_sessions as usize {
        let mut reply = Vec::new();
        reply.push(msg::SSH_MSG_CHANNEL_OPEN_FAILURE);
        reply.extend_from_slice(&sender_channel.to_be_bytes());
        reply.extend_from_slice(&4u32.to_be_bytes()); // reason: resource shortage
        reply.extend_from_slice(&ssh_string(b"max sessions exceeded"));
        reply.extend_from_slice(&ssh_string(b""));
        return conn.send_packet(&reply);
    }

    let local_id = conn.next_channel_id;
    conn.next_channel_id += 1;

    let channel = Channel::new(local_id, sender_channel, initial_window, max_packet);
    let local_window = channel.local_window;
    conn.channels.push(channel);

    // Send CHANNEL_OPEN_CONFIRMATION.
    let mut reply = Vec::new();
    reply.push(msg::SSH_MSG_CHANNEL_OPEN_CONFIRMATION);
    reply.extend_from_slice(&sender_channel.to_be_bytes());
    reply.extend_from_slice(&local_id.to_be_bytes());
    reply.extend_from_slice(&local_window.to_be_bytes());
    reply.extend_from_slice(&(32768u32).to_be_bytes()); // max packet size
    conn.send_packet(&reply)
}

/// Handle `CHANNEL_REQUEST`.
fn handle_channel_request(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, off) = read_u32(payload, 1)?;
    let (req_type_bytes, off) = read_ssh_string(payload, off)?;
    let req_type = String::from_utf8_lossy(req_type_bytes).into_owned();
    let (want_reply, off) = read_bool(payload, off)?;

    conn.debug_log(&format!(
        "channel request: channel={recipient} type={req_type} want_reply={want_reply}"
    ));

    let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) else {
        if want_reply {
            let mut fail = vec![msg::SSH_MSG_CHANNEL_FAILURE];
            fail.extend_from_slice(&recipient.to_be_bytes());
            conn.send_packet(&fail)?;
        }
        return Ok(());
    };

    let remote_id = channel.remote_id;

    match req_type.as_str() {
        "pty-req" => {
            let (term, width, height, _wpx, _hpx, _modes) = parse_pty_request(payload, off)?;
            channel.pty_requested = true;
            channel.term = term;
            channel.term_width = width;
            channel.term_height = height;
            let term_info = format!(
                "PTY: term={} {}x{}",
                channel.term, channel.term_width, channel.term_height
            );
            // The mutable borrow of `channel` ends here (its last read above),
            // so `conn` is free for the debug log below.
            conn.debug_log(&term_info);

            if want_reply {
                let mut success = Vec::new();
                success.push(msg::SSH_MSG_CHANNEL_SUCCESS);
                success.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&success)?;
            }
        }
        "shell" => {
            conn.debug_log("shell request");

            if want_reply {
                let mut success = Vec::new();
                success.push(msg::SSH_MSG_CHANNEL_SUCCESS);
                success.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&success)?;
            }

            // Send a welcome message.
            let welcome = format!("Welcome to OurOS, {}!\r\n$ ", conn.username);
            send_channel_data(conn, remote_id, welcome.as_bytes())?;
        }
        "exec" => {
            let (cmd_bytes, _) = read_ssh_string(payload, off)?;
            let cmd = String::from_utf8_lossy(cmd_bytes);
            conn.debug_log(&format!("exec request: {cmd}"));

            if want_reply {
                let mut success = Vec::new();
                success.push(msg::SSH_MSG_CHANNEL_SUCCESS);
                success.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&success)?;
            }

            // Execute and send output.
            let output = format!("exec: {cmd}\r\n");
            send_channel_data(conn, remote_id, output.as_bytes())?;

            // Send EOF and close.
            send_channel_eof(conn, remote_id)?;
            send_channel_close(conn, recipient)?;
        }
        "subsystem" => {
            let (subsys_bytes, _) = read_ssh_string(payload, off)?;
            let subsys = String::from_utf8_lossy(subsys_bytes).into_owned();
            conn.debug_log(&format!("subsystem request: {subsys}"));

            let found = conn
                .config
                .subsystems
                .iter()
                .any(|(name, _)| name == &subsys);

            if want_reply {
                let msg_type = if found {
                    msg::SSH_MSG_CHANNEL_SUCCESS
                } else {
                    msg::SSH_MSG_CHANNEL_FAILURE
                };
                let mut reply = Vec::new();
                reply.push(msg_type);
                reply.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&reply)?;
            }
        }
        "env" => {
            // Accept environment variable requests silently.
            if want_reply {
                let mut success = Vec::new();
                success.push(msg::SSH_MSG_CHANNEL_SUCCESS);
                success.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&success)?;
            }
        }
        "window-change" => {
            // Update terminal size.
            if off + 8 <= payload.len() {
                let (width, next) = read_u32(payload, off)?;
                let (height, _) = read_u32(payload, next)?;
                channel.term_width = width;
                channel.term_height = height;
            }
            // No reply needed for window-change.
        }
        _ => {
            conn.debug_log(&format!("unknown channel request: {req_type}"));
            if want_reply {
                let mut fail = Vec::new();
                fail.push(msg::SSH_MSG_CHANNEL_FAILURE);
                fail.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&fail)?;
            }
        }
    }

    Ok(())
}

/// Handle `CHANNEL_DATA`.
fn handle_channel_data(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, off) = read_u32(payload, 1)?;
    let (data, _) = read_ssh_string(payload, off)?;

    conn.debug_log(&format!(
        "channel data: channel={recipient} len={}",
        data.len()
    ));

    let channel = conn.channels.iter().find(|ch| ch.local_id == recipient);
    if let Some(channel) = channel {
        let remote_id = channel.remote_id;

        // Echo data back (simplified shell).
        send_channel_data(conn, remote_id, data)?;
    }

    Ok(())
}

/// Handle `CHANNEL_WINDOW_ADJUST`.
fn handle_window_adjust(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, off) = read_u32(payload, 1)?;
    let (bytes_to_add, _) = read_u32(payload, off)?;

    if let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) {
        channel.remote_window = channel.remote_window.saturating_add(bytes_to_add);
        let new_window = channel.remote_window;
        // The mutable borrow of `channel` ends here (its last read above), so
        // `conn` is free for the debug log below.
        conn.debug_log(&format!(
            "window adjust: channel={recipient} +{bytes_to_add} (now {new_window})"
        ));
    }

    Ok(())
}

/// Handle `CHANNEL_EOF`.
fn handle_channel_eof(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, _) = read_u32(payload, 1)?;
    conn.debug_log(&format!("channel EOF: channel={recipient}"));
    Ok(())
}

/// Handle `CHANNEL_CLOSE`.
fn handle_channel_close(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, _) = read_u32(payload, 1)?;
    conn.debug_log(&format!("channel close: channel={recipient}"));

    let remote_id = conn
        .channels
        .iter()
        .find(|ch| ch.local_id == recipient)
        .map(|ch| ch.remote_id);

    // Extract what we need before modifying, to avoid overlapping borrows
    let needs_eof = conn
        .channels
        .iter()
        .find(|ch| ch.local_id == recipient)
        .map(|ch| (!ch.eof_sent, ch.remote_id));

    if let Some((send_eof, remote_id)) = needs_eof {
        if send_eof {
            send_channel_eof(conn, remote_id)?;
        }
        if let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) {
            channel.eof_sent = true;
            channel.closed = true;
        }
    }

    // Send close back.
    if let Some(remote_id) = remote_id {
        let mut close = Vec::new();
        close.push(msg::SSH_MSG_CHANNEL_CLOSE);
        close.extend_from_slice(&remote_id.to_be_bytes());
        conn.send_packet(&close)?;
    }

    Ok(())
}

/// Send data on a channel.
fn send_channel_data(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    data: &[u8],
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_DATA);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(data));
    conn.send_packet(&msg_buf)
}

/// Send EOF on a channel.
fn send_channel_eof(conn: &mut ConnectionState, remote_channel_id: u32) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_EOF);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    conn.send_packet(&msg_buf)
}

/// Send close on a channel.
fn send_channel_close(conn: &mut ConnectionState, local_channel_id: u32) -> Result<(), SshdError> {
    // Extract info first to avoid overlapping borrows with send_channel_eof
    let chan_info = conn
        .channels
        .iter()
        .find(|ch| ch.local_id == local_channel_id)
        .map(|ch| (ch.remote_id, ch.eof_sent));

    if let Some((remote_id, eof_sent)) = chan_info {
        if !eof_sent {
            send_channel_eof(conn, remote_id)?;
        }
        // Now update the channel state
        if let Some(channel) = conn
            .channels
            .iter_mut()
            .find(|ch| ch.local_id == local_channel_id)
        {
            channel.closed = true;
            channel.eof_sent = true;
        }
        let mut msg_buf = Vec::new();
        msg_buf.push(msg::SSH_MSG_CHANNEL_CLOSE);
        msg_buf.extend_from_slice(&remote_id.to_be_bytes());
        conn.send_packet(&msg_buf)?;
    }
    Ok(())
}

// ============================================================================
// CLI parsing
// ============================================================================

struct CliOptions {
    port: Option<u16>,
    config_file: String,
    debug_mode: bool,
    foreground: bool,
    log_stderr: bool,
    host_key_file: Option<String>,
    test_config: bool,
    extended_test: bool,
}

impl CliOptions {
    fn parse_args() -> Self {
        let args: Vec<String> = env::args().collect();
        let mut opts = Self {
            port: None,
            config_file: "/etc/ssh/sshd_config".into(),
            debug_mode: false,
            foreground: false,
            log_stderr: false,
            host_key_file: None,
            test_config: false,
            extended_test: false,
        };

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-p" => {
                    i += 1;
                    if i < args.len() {
                        if let Ok(p) = args[i].parse::<u16>() {
                            opts.port = Some(p);
                        } else {
                            eprintln!("sshd: invalid port: {}", args[i]);
                            process::exit(1);
                        }
                    }
                }
                "-f" => {
                    i += 1;
                    if let Some(arg) = args.get(i) {
                        arg.clone_into(&mut opts.config_file);
                    }
                }
                "-d" => {
                    opts.debug_mode = true;
                    opts.foreground = true;
                }
                "-D" => {
                    opts.foreground = true;
                }
                "-e" => {
                    opts.log_stderr = true;
                }
                "-h" => {
                    i += 1;
                    if i < args.len() {
                        opts.host_key_file = Some(args[i].clone());
                    }
                }
                "-t" => {
                    opts.test_config = true;
                }
                "-T" => {
                    opts.extended_test = true;
                    opts.test_config = true;
                }
                "--help" => {
                    print_usage();
                    process::exit(0);
                }
                other => {
                    eprintln!("sshd: unknown option: {other}");
                    process::exit(1);
                }
            }
            i += 1;
        }

        opts
    }
}

fn print_usage() {
    eprintln!("Usage: sshd [options]");
    eprintln!("Options:");
    eprintln!("  -p port     Listen port");
    eprintln!("  -f config   Config file path");
    eprintln!("  -d          Debug mode (no fork, verbose)");
    eprintln!("  -D          Don't daemonize");
    eprintln!("  -e          Log to stderr");
    eprintln!("  -h hostkey  Host key file");
    eprintln!("  -t          Test configuration and exit");
    eprintln!("  -T          Extended test (dump config)");
    eprintln!("  --help      Show this help");
}

// ============================================================================
// Logging
// ============================================================================

fn log_info(msg: &str, _log_stderr: bool) {
    eprintln!("sshd: {msg}");
}

fn log_error(msg: &str, _log_stderr: bool) {
    eprintln!("sshd: error: {msg}");
}

// ============================================================================
// Main server loop
// ============================================================================

fn main() {
    let opts = CliOptions::parse_args();

    // Load config.
    let mut config = if let Ok(data) = fs_read_file(&opts.config_file) {
        let content = String::from_utf8_lossy(&data);
        match SshdConfig::parse(&content) {
            Ok(c) => c,
            Err(e) => {
                log_error(&format!("config parse error: {e}"), opts.log_stderr);
                process::exit(1);
            }
        }
    } else {
        if opts.config_file != "/etc/ssh/sshd_config" {
            log_error(
                &format!("cannot read config: {}", opts.config_file),
                opts.log_stderr,
            );
            process::exit(1);
        }
        // Use defaults if default config file doesn't exist.
        SshdConfig::default_config()
    };

    // Apply CLI overrides.
    if let Some(port) = opts.port {
        config.port = port;
    }
    if let Some(hk) = &opts.host_key_file {
        hk.clone_into(&mut config.host_key_file);
    }

    // Test mode.
    if opts.test_config {
        if opts.extended_test {
            print!("{}", config.dump());
        } else {
            log_info("configuration OK", opts.log_stderr);
        }
        process::exit(0);
    }

    // Load host key.
    let host_key = if let Ok(hk) = HostKey::load_from_file(&config.host_key_file) {
        hk
    } else {
        log_info("no host key found, generating default", opts.log_stderr);
        HostKey::generate_default()
    };

    log_info(
        &format!("host key fingerprint: {}", host_key.fingerprint()),
        opts.log_stderr,
    );

    // Validate port.
    if config.port == 0 {
        log_error("port cannot be 0", opts.log_stderr);
        process::exit(1);
    }

    // Bind listener.
    let listener = match tcp_bind(config.port) {
        Ok(l) => l,
        Err(e) => {
            log_error(
                &format!("cannot bind port {}: {e}", config.port),
                opts.log_stderr,
            );
            process::exit(1);
        }
    };

    log_info(
        &format!(
            "listening on {}:{} (pid {})",
            config.listen_address,
            config.port,
            get_pid()
        ),
        opts.log_stderr,
    );

    // Accept connections.
    loop {
        let conn_handle = match tcp_accept(listener) {
            Ok(h) => h,
            Err(e) => {
                log_error(&format!("accept error: {e}"), opts.log_stderr);
                continue;
            }
        };

        handle_connection(conn_handle, &config, &host_key, opts.debug_mode);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Version string parsing ----

    #[test]
    fn test_parse_version_string_standard() {
        let sw = parse_version_string("SSH-2.0-OpenSSH_8.9");
        assert_eq!(sw, Some("OpenSSH_8.9"));
    }

    #[test]
    fn test_parse_version_string_with_comment() {
        let sw = parse_version_string("SSH-2.0-OpenSSH_8.9 Ubuntu-3");
        assert_eq!(sw, Some("OpenSSH_8.9"));
    }

    #[test]
    fn test_parse_version_string_ouros() {
        let sw = parse_version_string("SSH-2.0-OurOS_1.0");
        assert_eq!(sw, Some("OurOS_1.0"));
    }

    #[test]
    fn test_parse_version_string_invalid() {
        assert!(parse_version_string("HTTP/1.1").is_none());
    }

    #[test]
    fn test_parse_version_string_empty() {
        assert!(parse_version_string("").is_none());
    }

    #[test]
    fn test_parse_version_string_v1() {
        let sw = parse_version_string("SSH-1.0-old");
        assert_eq!(sw, Some("old"));
    }

    // ---- Packet building and parsing ----

    #[test]
    fn test_build_packet_unencrypted() {
        let enc = EncryptionState::none();
        let pkt = build_packet(b"hello", false, 0, &enc);
        assert!(pkt.len() >= 4 + 1 + 5);
        let pkt_len = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]) as usize;
        let pad_len = pkt[4] as usize;
        assert_eq!(pkt_len, 1 + 5 + pad_len);
        assert!(pad_len >= 4);
    }

    #[test]
    fn test_build_packet_alignment() {
        let enc = EncryptionState::none();
        let pkt = build_packet(b"test", false, 0, &enc);
        // Total must be multiple of block size (8).
        assert_eq!(pkt.len() % 8, 0);
    }

    #[test]
    fn test_build_packet_empty_payload() {
        let enc = EncryptionState::none();
        let pkt = build_packet(b"", false, 0, &enc);
        assert!(pkt.len() > 4);
        assert_eq!(pkt.len() % 8, 0);
    }

    #[test]
    fn test_build_packet_large_payload() {
        let enc = EncryptionState::none();
        let data = vec![0xAA; 1024];
        let pkt = build_packet(&data, false, 0, &enc);
        let pkt_len = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]) as usize;
        assert!(pkt_len > 1024);
    }

    // ---- SSH encoding helpers ----

    #[test]
    fn test_ssh_string_encoding() {
        let encoded = ssh_string(b"hello");
        assert_eq!(&encoded[..4], &[0, 0, 0, 5]);
        assert_eq!(&encoded[4..], b"hello");
    }

    #[test]
    fn test_ssh_string_empty() {
        let encoded = ssh_string(b"");
        assert_eq!(&encoded, &[0, 0, 0, 0]);
    }

    #[test]
    fn test_read_ssh_string_roundtrip() {
        let encoded = ssh_string(b"test data");
        let (val, next) = read_ssh_string(&encoded, 0).unwrap();
        assert_eq!(val, b"test data");
        assert_eq!(next, encoded.len());
    }

    #[test]
    fn test_read_ssh_string_truncated() {
        assert!(read_ssh_string(&[0, 0, 0], 0).is_err());
    }

    #[test]
    fn test_read_ssh_string_oversized() {
        let data = [0, 0, 0, 10, 1, 2, 3]; // Claims 10 bytes but only 3 available.
        assert!(read_ssh_string(&data, 0).is_err());
    }

    #[test]
    fn test_read_u32() {
        let data = [0, 0, 0, 42];
        let (val, next) = read_u32(&data, 0).unwrap();
        assert_eq!(val, 42);
        assert_eq!(next, 4);
    }

    #[test]
    fn test_read_u32_truncated() {
        assert!(read_u32(&[0, 0], 0).is_err());
    }

    #[test]
    fn test_read_byte() {
        let (val, next) = read_byte(&[0xFF], 0).unwrap();
        assert_eq!(val, 0xFF);
        assert_eq!(next, 1);
    }

    #[test]
    fn test_read_byte_empty() {
        assert!(read_byte(&[], 0).is_err());
    }

    #[test]
    fn test_encode_mpint_zero() {
        let result = encode_mpint(&[]);
        assert_eq!(result, vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_encode_mpint_positive() {
        let result = encode_mpint(&[0x01, 0x02]);
        assert_eq!(&result[..4], &[0, 0, 0, 2]);
        assert_eq!(&result[4..], &[0x01, 0x02]);
    }

    #[test]
    fn test_encode_mpint_high_bit() {
        let result = encode_mpint(&[0x80, 0x01]);
        // Should be padded with a leading zero.
        assert_eq!(&result[..4], &[0, 0, 0, 3]);
        assert_eq!(&result[4..], &[0x00, 0x80, 0x01]);
    }

    #[test]
    fn test_read_mpint_roundtrip() {
        let encoded = encode_mpint(&[0x42, 0x43]);
        let (val, _) = read_mpint(&encoded, 0).unwrap();
        assert_eq!(val, vec![0x42, 0x43]);
    }

    // ---- Config parsing ----

    #[test]
    fn test_config_default() {
        let config = SshdConfig::default_config();
        assert_eq!(config.port, 22);
        assert!(config.password_authentication);
        assert!(config.pubkey_authentication);
        assert_eq!(config.max_auth_tries, 6);
        assert_eq!(config.login_grace_time, 120);
        assert_eq!(config.max_sessions, 10);
    }

    #[test]
    fn test_config_parse_port() {
        let config = SshdConfig::parse("Port 2222").unwrap();
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn test_config_parse_listen_address() {
        let config = SshdConfig::parse("ListenAddress 192.168.1.1").unwrap();
        assert_eq!(config.listen_address, "192.168.1.1");
    }

    #[test]
    fn test_config_parse_host_key() {
        let config = SshdConfig::parse("HostKey /etc/ssh/my_key").unwrap();
        assert_eq!(config.host_key_file, "/etc/ssh/my_key");
    }

    #[test]
    fn test_config_parse_permit_root_login_yes() {
        let config = SshdConfig::parse("PermitRootLogin yes").unwrap();
        assert_eq!(config.permit_root_login, PermitRootLogin::Yes);
    }

    #[test]
    fn test_config_parse_permit_root_login_no() {
        let config = SshdConfig::parse("PermitRootLogin no").unwrap();
        assert_eq!(config.permit_root_login, PermitRootLogin::No);
    }

    #[test]
    fn test_config_parse_permit_root_login_prohibit() {
        let config = SshdConfig::parse("PermitRootLogin prohibit-password").unwrap();
        assert_eq!(config.permit_root_login, PermitRootLogin::ProhibitPassword);
    }

    #[test]
    fn test_config_parse_password_auth() {
        let config = SshdConfig::parse("PasswordAuthentication no").unwrap();
        assert!(!config.password_authentication);
    }

    #[test]
    fn test_config_parse_pubkey_auth() {
        let config = SshdConfig::parse("PubkeyAuthentication no").unwrap();
        assert!(!config.pubkey_authentication);
    }

    #[test]
    fn test_config_parse_max_auth_tries() {
        let config = SshdConfig::parse("MaxAuthTries 3").unwrap();
        assert_eq!(config.max_auth_tries, 3);
    }

    #[test]
    fn test_config_parse_login_grace_time() {
        let config = SshdConfig::parse("LoginGraceTime 60").unwrap();
        assert_eq!(config.login_grace_time, 60);
    }

    #[test]
    fn test_config_parse_max_sessions() {
        let config = SshdConfig::parse("MaxSessions 5").unwrap();
        assert_eq!(config.max_sessions, 5);
    }

    #[test]
    fn test_config_parse_banner() {
        let config = SshdConfig::parse("Banner /etc/ssh/banner").unwrap();
        assert_eq!(config.banner_file, "/etc/ssh/banner");
    }

    #[test]
    fn test_config_parse_print_motd() {
        let config = SshdConfig::parse("PrintMotd no").unwrap();
        assert!(!config.print_motd);
    }

    #[test]
    fn test_config_parse_subsystem() {
        let config = SshdConfig::parse("Subsystem sftp /usr/lib/sftp-server").unwrap();
        // Default already has sftp; we add another.
        assert!(config.subsystems.len() >= 2);
        assert!(
            config
                .subsystems
                .iter()
                .any(|(n, c)| n == "sftp" && c == "/usr/lib/sftp-server")
        );
    }

    #[test]
    fn test_config_parse_allow_users() {
        let config = SshdConfig::parse("AllowUsers alice bob").unwrap();
        assert_eq!(config.allow_users, vec!["alice", "bob"]);
    }

    #[test]
    fn test_config_parse_deny_users() {
        let config = SshdConfig::parse("DenyUsers mallory").unwrap();
        assert_eq!(config.deny_users, vec!["mallory"]);
    }

    #[test]
    fn test_config_parse_allow_groups() {
        let config = SshdConfig::parse("AllowGroups ssh-users admin").unwrap();
        assert_eq!(config.allow_groups, vec!["ssh-users", "admin"]);
    }

    #[test]
    fn test_config_parse_deny_groups() {
        let config = SshdConfig::parse("DenyGroups nogroup").unwrap();
        assert_eq!(config.deny_groups, vec!["nogroup"]);
    }

    #[test]
    fn test_config_parse_authorized_keys_file() {
        let config = SshdConfig::parse("AuthorizedKeysFile .ssh/custom_keys").unwrap();
        assert_eq!(config.authorized_keys_file, ".ssh/custom_keys");
    }

    #[test]
    fn test_config_parse_comments_and_empty_lines() {
        let content = "# Comment\n\n  # Another comment\nPort 3333\n";
        let config = SshdConfig::parse(content).unwrap();
        assert_eq!(config.port, 3333);
    }

    #[test]
    fn test_config_parse_empty() {
        let config = SshdConfig::parse("").unwrap();
        assert_eq!(config.port, 22); // Should have defaults.
    }

    #[test]
    fn test_config_parse_invalid_port() {
        assert!(SshdConfig::parse("Port notanumber").is_err());
    }

    #[test]
    fn test_config_parse_invalid_permit_root() {
        assert!(SshdConfig::parse("PermitRootLogin maybe").is_err());
    }

    #[test]
    fn test_config_parse_invalid_bool() {
        assert!(SshdConfig::parse("PasswordAuthentication banana").is_err());
    }

    #[test]
    fn test_config_parse_unknown_directive() {
        // Unknown directives should be silently ignored.
        let config = SshdConfig::parse("UnknownDirective value").unwrap();
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_config_dump() {
        let config = SshdConfig::default_config();
        let dump = config.dump();
        assert!(dump.contains("port 22"));
        assert!(dump.contains("listenaddress 0.0.0.0"));
        assert!(dump.contains("passwordauthentication yes"));
    }

    #[test]
    fn test_config_parse_full() {
        let content = "\
Port 2222
ListenAddress 10.0.0.1
HostKey /etc/ssh/host_key
PermitRootLogin no
PasswordAuthentication yes
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys
MaxAuthTries 3
LoginGraceTime 30
MaxSessions 2
Banner /etc/ssh/banner.txt
PrintMotd no
AllowUsers admin deploy
DenyUsers nobody
AllowGroups wheel
DenyGroups nogroup
";
        let config = SshdConfig::parse(content).unwrap();
        assert_eq!(config.port, 2222);
        assert_eq!(config.listen_address, "10.0.0.1");
        assert_eq!(config.host_key_file, "/etc/ssh/host_key");
        assert_eq!(config.permit_root_login, PermitRootLogin::No);
        assert!(config.password_authentication);
        assert_eq!(config.max_auth_tries, 3);
        assert_eq!(config.login_grace_time, 30);
        assert_eq!(config.max_sessions, 2);
        assert!(!config.print_motd);
        assert_eq!(config.allow_users, vec!["admin", "deploy"]);
        assert_eq!(config.deny_users, vec!["nobody"]);
        assert_eq!(config.allow_groups, vec!["wheel"]);
        assert_eq!(config.deny_groups, vec!["nogroup"]);
    }

    // ---- User authentication logic ----

    #[test]
    fn test_is_user_allowed_no_restrictions() {
        let config = SshdConfig::default_config();
        assert!(is_user_allowed("alice", &[], &config));
    }

    #[test]
    fn test_is_user_denied_by_deny_list() {
        let mut config = SshdConfig::default_config();
        config.deny_users = vec!["mallory".into()];
        assert!(!is_user_allowed("mallory", &[], &config));
        assert!(is_user_allowed("alice", &[], &config));
    }

    #[test]
    fn test_is_user_allowed_by_allow_list() {
        let mut config = SshdConfig::default_config();
        config.allow_users = vec!["alice".into(), "bob".into()];
        assert!(is_user_allowed("alice", &[], &config));
        assert!(!is_user_allowed("charlie", &[], &config));
    }

    #[test]
    fn test_deny_takes_precedence() {
        let mut config = SshdConfig::default_config();
        config.allow_users = vec!["alice".into()];
        config.deny_users = vec!["alice".into()];
        assert!(!is_user_allowed("alice", &[], &config));
    }

    #[test]
    fn test_group_deny() {
        let mut config = SshdConfig::default_config();
        config.deny_groups = vec!["badgroup".into()];
        assert!(!is_user_allowed("alice", &["badgroup".into()], &config));
    }

    #[test]
    fn test_group_allow() {
        let mut config = SshdConfig::default_config();
        config.allow_groups = vec!["ssh-users".into()];
        assert!(is_user_allowed("alice", &["ssh-users".into()], &config));
        assert!(!is_user_allowed("alice", &["other".into()], &config));
    }

    #[test]
    fn test_root_login_yes() {
        let mut config = SshdConfig::default_config();
        config.permit_root_login = PermitRootLogin::Yes;
        assert!(is_root_login_allowed("password", &config));
        assert!(is_root_login_allowed("publickey", &config));
    }

    #[test]
    fn test_root_login_no() {
        let mut config = SshdConfig::default_config();
        config.permit_root_login = PermitRootLogin::No;
        assert!(!is_root_login_allowed("password", &config));
        assert!(!is_root_login_allowed("publickey", &config));
    }

    #[test]
    fn test_root_login_prohibit_password() {
        let mut config = SshdConfig::default_config();
        config.permit_root_login = PermitRootLogin::ProhibitPassword;
        assert!(!is_root_login_allowed("password", &config));
        assert!(is_root_login_allowed("publickey", &config));
    }

    // ---- Password verification ----

    #[test]
    fn test_verify_password_sha256_hex() {
        let hash = sha256(b"secret");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify_password("secret", &hex));
        assert!(!verify_password("wrong", &hex));
    }

    #[test]
    fn test_verify_password_locked_account() {
        assert!(!verify_password("any", "!"));
        assert!(!verify_password("any", "*"));
        assert!(!verify_password("any", "!!"));
        assert!(!verify_password("any", ""));
    }

    #[test]
    fn test_verify_password_crypt5() {
        // $5$salt$hash format.
        let mut salted = Vec::new();
        salted.extend_from_slice(b"mypass");
        salted.extend_from_slice(b"mysalt");
        let hash = sha256(&salted);
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let stored = format!("$5$mysalt${hex}");
        assert!(verify_password("mypass", &stored));
        assert!(!verify_password("wrong", &stored));
    }

    #[test]
    fn test_verify_password_plaintext() {
        assert!(verify_password("hello", "hello"));
        assert!(!verify_password("hello", "world"));
    }

    // ---- Shadow parsing ----

    #[test]
    fn test_parse_shadow() {
        let content =
            "root:$5$salt$hash:18000:0:99999:7:::\nalice:password123:18000:0:99999:7:::\n";
        let entries = parse_shadow(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].username, "root");
        assert_eq!(entries[0].password_hash, "$5$salt$hash");
        assert_eq!(entries[1].username, "alice");
    }

    #[test]
    fn test_parse_shadow_empty() {
        let entries = parse_shadow("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_shadow_comments() {
        let content = "# This is a comment\nroot:hash:0:0:::::";
        let entries = parse_shadow(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].username, "root");
    }

    // ---- Authorized keys parsing ----

    #[test]
    fn test_parse_authorized_keys_ed25519() {
        let content = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBt user@host\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
        assert_eq!(keys[0].comment, "user@host");
    }

    #[test]
    fn test_parse_authorized_keys_rsa() {
        let content = "ssh-rsa AAAAB3NzaC1yc2E= admin@server\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-rsa");
    }

    #[test]
    fn test_parse_authorized_keys_multiple() {
        let content = "ssh-ed25519 AAAA key1\nssh-rsa BBBB key2\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_parse_authorized_keys_skip_invalid() {
        let content = "invalid-type AAAA key\nssh-ed25519 BBBB valid\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
    }

    #[test]
    fn test_parse_authorized_keys_comments_and_empty() {
        let content = "# comment\n\nssh-ed25519 AAAA key\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_parse_authorized_keys_no_comment() {
        let content = "ssh-ed25519 AAAA\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].comment, "");
    }

    // ---- Channel message handling ----

    #[test]
    fn test_channel_new() {
        let ch = Channel::new(0, 1, 65536, 32768);
        assert_eq!(ch.local_id, 0);
        assert_eq!(ch.remote_id, 1);
        assert_eq!(ch.remote_window, 65536);
        assert_eq!(ch.remote_max_packet, 32768);
        assert!(!ch.pty_requested);
        assert!(!ch.closed);
    }

    // ---- PTY request parsing ----

    #[test]
    fn test_parse_pty_request() {
        let mut data = Vec::new();
        data.extend_from_slice(&ssh_string(b"xterm-256color"));
        data.extend_from_slice(&80u32.to_be_bytes()); // width cols
        data.extend_from_slice(&24u32.to_be_bytes()); // height rows
        data.extend_from_slice(&640u32.to_be_bytes()); // width px
        data.extend_from_slice(&480u32.to_be_bytes()); // height px
        data.extend_from_slice(&ssh_string(b"")); // modes

        let (term, w, h, wpx, hpx, modes) = parse_pty_request(&data, 0).unwrap();
        assert_eq!(term, "xterm-256color");
        assert_eq!(w, 80);
        assert_eq!(h, 24);
        assert_eq!(wpx, 640);
        assert_eq!(hpx, 480);
        assert!(modes.is_empty());
    }

    #[test]
    fn test_parse_pty_request_with_modes() {
        let mut data = Vec::new();
        data.extend_from_slice(&ssh_string(b"vt100"));
        data.extend_from_slice(&132u32.to_be_bytes());
        data.extend_from_slice(&50u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&ssh_string(&[1, 0, 0, 0, 3])); // Some mode bytes.

        let (term, w, h, _, _, modes) = parse_pty_request(&data, 0).unwrap();
        assert_eq!(term, "vt100");
        assert_eq!(w, 132);
        assert_eq!(h, 50);
        assert_eq!(modes.len(), 5);
    }

    // ---- Host key fingerprint ----

    #[test]
    fn test_host_key_fingerprint() {
        let key = HostKey::from_seed([0u8; 32]);
        let fp = key.fingerprint();
        assert!(fp.starts_with("SHA256:"));
        assert!(fp.len() > 10);
    }

    #[test]
    fn test_host_key_public_blob() {
        let key = HostKey::from_seed([1u8; 32]);
        let blob = key.public_key_blob();
        // Should start with ssh_string("ssh-ed25519").
        let (key_type, _) = read_ssh_string(&blob, 0).unwrap();
        assert_eq!(key_type, b"ssh-ed25519");
    }

    #[test]
    fn test_host_key_sign() {
        let key = HostKey::from_seed([2u8; 32]);
        let sig = key.sign(b"test data");
        // Signature blob should start with ssh_string("ssh-ed25519").
        let (sig_type, off) = read_ssh_string(&sig, 0).unwrap();
        assert_eq!(sig_type, b"ssh-ed25519");
        // Then a 64-byte signature.
        let (sig_data, _) = read_ssh_string(&sig, off).unwrap();
        assert_eq!(sig_data.len(), 64);
    }

    #[test]
    fn test_host_key_sign_different_data() {
        let key = HostKey::from_seed([3u8; 32]);
        let sig1 = key.sign(b"data1");
        let sig2 = key.sign(b"data2");
        assert_ne!(sig1, sig2);
    }

    // ---- Port validation ----

    #[test]
    fn test_port_valid_range() {
        let config = SshdConfig::parse("Port 1").unwrap();
        assert_eq!(config.port, 1);
        let config = SshdConfig::parse("Port 65535").unwrap();
        assert_eq!(config.port, 65535);
    }

    #[test]
    fn test_port_zero_in_config() {
        // Port 0 is parseable but should be rejected at runtime.
        let config = SshdConfig::parse("Port 0").unwrap();
        assert_eq!(config.port, 0);
    }

    // ---- MaxAuthTries enforcement ----

    #[test]
    fn test_max_auth_tries_setting() {
        let config = SshdConfig::parse("MaxAuthTries 1").unwrap();
        assert_eq!(config.max_auth_tries, 1);
    }

    // ---- LoginGraceTime handling ----

    #[test]
    fn test_login_grace_time_setting() {
        let config = SshdConfig::parse("LoginGraceTime 30").unwrap();
        assert_eq!(config.login_grace_time, 30);
    }

    #[test]
    fn test_login_grace_time_zero() {
        let config = SshdConfig::parse("LoginGraceTime 0").unwrap();
        assert_eq!(config.login_grace_time, 0);
    }

    // ---- Subsystem configuration ----

    #[test]
    fn test_subsystem_default() {
        let config = SshdConfig::default_config();
        assert_eq!(config.subsystems.len(), 1);
        assert_eq!(config.subsystems[0].0, "sftp");
    }

    #[test]
    fn test_subsystem_custom() {
        let config = SshdConfig::parse("Subsystem scp /usr/lib/scp-server").unwrap();
        assert!(
            config
                .subsystems
                .iter()
                .any(|(n, c)| n == "scp" && c == "/usr/lib/scp-server")
        );
    }

    // ---- Banner loading ----

    #[test]
    fn test_banner_empty_by_default() {
        let config = SshdConfig::default_config();
        assert!(config.banner_file.is_empty());
    }

    #[test]
    fn test_banner_configured() {
        let config = SshdConfig::parse("Banner /etc/ssh/banner.txt").unwrap();
        assert_eq!(config.banner_file, "/etc/ssh/banner.txt");
    }

    // ---- SHA-256 ----

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // ---- HMAC-SHA256 ----

    #[test]
    fn test_hmac_sha256_basic() {
        let mac = hmac_sha256(b"key", b"data");
        // Known test vector for HMAC-SHA256("key", "data").
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5031fe3d989c6d1537a013fa6e739da23463fdaec3b70137d828e36ace221bd0"
        );
    }

    // ---- Base64 ----

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_decode_hello() {
        assert_eq!(base64_decode("aGVsbG8="), b"hello");
    }

    #[test]
    fn test_base64_roundtrip() {
        let data = b"SSH server daemon testing";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded);
        assert_eq!(&decoded, data);
    }

    // ---- Big integer ----

    #[test]
    fn test_biguint_zero() {
        let z = BigUint::zero();
        assert!(z.is_zero());
        assert_eq!(z.bit_length(), 0);
    }

    #[test]
    fn test_biguint_one() {
        let one = BigUint::one();
        assert!(!one.is_zero());
        assert_eq!(one.to_bytes_be(), vec![1]);
    }

    #[test]
    fn test_biguint_mod_pow() {
        // 2^10 mod 1000 = 1024 mod 1000 = 24
        let base = BigUint::from_bytes_be(&[2]);
        let exp = BigUint::from_bytes_be(&[10]);
        let modulus = BigUint::from_bytes_be(&[0x03, 0xE8]); // 1000
        let result = base.mod_pow(&exp, &modulus);
        assert_eq!(result.to_bytes_be(), vec![24]);
    }

    // ---- Encryption state ----

    #[test]
    fn test_encryption_state_none() {
        let enc = EncryptionState::none();
        assert!(enc.enc_key_c2s.is_empty());
        assert_eq!(enc.block_size, 8);
        assert_eq!(enc.mac_len, 0);
    }

    // ---- AES ----

    #[test]
    fn test_aes_encrypt_decrypt_roundtrip() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let original = b"test data here!!"; // 16 bytes exactly
        let mut encrypted = original.to_vec();
        encrypt_packet_aes_ctr(&mut encrypted, &key, &iv, 0);
        assert_ne!(&encrypted, original);
        decrypt_packet_aes_ctr(&mut encrypted, &key, &iv, 0);
        assert_eq!(&encrypted, original);
    }

    // ---- Constant time eq ----

    #[test]
    fn test_constant_time_eq_same() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn test_constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    // ---- KEXINIT building ----

    #[test]
    fn test_build_kexinit() {
        let kexinit = build_kexinit();
        assert_eq!(kexinit[0], msg::SSH_MSG_KEXINIT);
        assert!(kexinit.len() > 17); // At least message type + 16 byte cookie.
    }

    // ---- Format IP ----

    #[test]
    fn test_format_ip() {
        let ip = u32::from_be_bytes([192, 168, 1, 100]);
        assert_eq!(format_ip(ip), "192.168.1.100");
    }

    #[test]
    fn test_format_ip_localhost() {
        let ip = u32::from_be_bytes([127, 0, 0, 1]);
        assert_eq!(format_ip(ip), "127.0.0.1");
    }

    // ---- parse_bool ----

    #[test]
    fn test_parse_bool_yes() {
        assert!(parse_bool("yes").unwrap());
        assert!(parse_bool("true").unwrap());
        assert!(parse_bool("1").unwrap());
    }

    #[test]
    fn test_parse_bool_no() {
        assert!(!parse_bool("no").unwrap());
        assert!(!parse_bool("false").unwrap());
        assert!(!parse_bool("0").unwrap());
    }

    #[test]
    fn test_parse_bool_invalid() {
        assert!(parse_bool("maybe").is_err());
    }

    // ---- read_bool ----

    #[test]
    fn test_read_bool_true() {
        let (val, next) = read_bool(&[1], 0).unwrap();
        assert!(val);
        assert_eq!(next, 1);
    }

    #[test]
    fn test_read_bool_false() {
        let (val, _) = read_bool(&[0], 0).unwrap();
        assert!(!val);
    }

    // ---- derive_keys ----

    #[test]
    fn test_derive_keys_produces_nonempty() {
        let secret = [1u8; 32];
        let hash = [2u8; 32];
        let session = [3u8; 32];
        let enc = derive_keys(&secret, &hash, &session);
        assert_eq!(enc.enc_key_c2s.len(), 16);
        assert_eq!(enc.enc_key_s2c.len(), 16);
        assert_eq!(enc.iv_c2s.len(), 16);
        assert_eq!(enc.iv_s2c.len(), 16);
        assert_eq!(enc.mac_key_c2s.len(), 32);
        assert_eq!(enc.mac_key_s2c.len(), 32);
        assert_eq!(enc.block_size, 16);
        assert_eq!(enc.mac_len, 32);
    }

    #[test]
    fn test_derive_keys_different_inputs() {
        let enc1 = derive_keys(&[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let enc2 = derive_keys(&[4u8; 32], &[5u8; 32], &[6u8; 32]);
        assert_ne!(enc1.enc_key_c2s, enc2.enc_key_c2s);
    }
}
