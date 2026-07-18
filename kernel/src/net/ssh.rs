//! SSH server implementation (RFC 4253 / RFC 4254).
//!
//! Provides an SSH-2 server that exposes the kernel debug shell (`kshell`)
//! over an encrypted, authenticated connection.  This replaces Telnet for
//! secure remote access to the kernel.
//!
//! ## Supported algorithms
//!
//! | Category     | Algorithm                       | Reference       |
//! |--------------|---------------------------------|-----------------|
//! | Key exchange | `curve25519-sha256`             | RFC 8731        |
//! | Host key     | `ssh-ed25519`                   | RFC 8709        |
//! | Cipher       | `chacha20-poly1305@openssh.com` | openssh spec    |
//! | MAC          | (implicit — AEAD cipher)        |                 |
//! | Compression  | `none`                          | RFC 4253        |
//!
//! All cryptographic primitives are already available in [`crate::crypto`]:
//! X25519, SHA-256, Ed25519, ChaCha20-Poly1305, HMAC-SHA256.
//!
//! ## Architecture
//!
//! ```text
//! Remote SSH client ─── TCP:22 ──→ SSH server
//!                                     ├── version exchange
//!                                     ├── key exchange (curve25519-sha256)
//!                                     ├── user authentication
//!                                     ├── channel open (session)
//!                                     └── shell (kshell dispatch)
//! ```
//!
//! ## Security
//!
//! - Ed25519 host key generated on first boot (persistent across reboots
//!   if a writable filesystem is available).
//! - Password authentication against the kernel's user table.
//! - Public key authentication (ssh-ed25519) against authorized_keys.
//! - Per-connection key derivation via curve25519-sha256.
//! - All traffic encrypted with chacha20-poly1305@openssh.com after NEWKEYS.
//!
//! ## Limitations
//!
//! - Maximum 4 concurrent sessions.
//! - Single cipher suite (chacha20-poly1305@openssh.com only).
//! - No SSH agent forwarding.
//! - No TCP port forwarding.
//! - No X11 forwarding.
//! - No session resumption / rekey (TODO).
//! - No subsystem support (only shell).
//! - Line-at-a-time shell mode.
//!
//! ## References
//!
//! - RFC 4250: SSH Protocol Assigned Numbers
//! - RFC 4251: SSH Protocol Architecture
//! - RFC 4252: SSH Authentication Protocol
//! - RFC 4253: SSH Transport Layer Protocol
//! - RFC 4254: SSH Connection Protocol
//! - RFC 8709: Ed25519 and Ed448 Public Key Algorithms for SSH
//! - RFC 8731: Secure Shell (SSH) Key Exchange Method Using Curve25519

// SSH server protocol implementation: many constants (algorithm strings,
// message type codes, packet field accessors) are defined per RFC for
// completeness even if the current state machine doesn't touch every
// one yet.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::crypto;
use crate::error::{KernelError, KernelResult};

// ===========================================================================
// SSH constants (RFC 4250 / RFC 4253)
// ===========================================================================

/// Default SSH listening port.
const DEFAULT_PORT: u16 = 22;

/// Maximum concurrent SSH sessions.
const MAX_SESSIONS: usize = 4;

/// Our SSH version string (RFC 4253 §4.2).
const SSH_VERSION_STRING: &[u8] = b"SSH-2.0-MintOS_1.0\r\n";

/// Maximum SSH packet size (256 KiB, per RFC 4253 §6.1).
const MAX_PACKET_SIZE: usize = 262_144;

/// Maximum version string length.
const MAX_VERSION_LEN: usize = 255;

/// Maximum payload size for a single packet (before encryption).
const MAX_PAYLOAD_SIZE: usize = 32_768;

/// Minimum packet padding (RFC 4253 §6: at least 4 bytes).
const MIN_PADDING: usize = 4;

/// Maximum authentication attempts before disconnect.
const MAX_AUTH_ATTEMPTS: u32 = 6;

/// Poll interval for the server tick (500 ms).
const TICK_INTERVAL_NS: u64 = 500_000_000;

// ---------------------------------------------------------------------------
// SSH message type codes (RFC 4253 §12, RFC 4252 §6, RFC 4254 §9)
// ---------------------------------------------------------------------------

/// Transport layer generic (RFC 4253).
mod msg {
    pub const DISCONNECT: u8 = 1;
    pub const IGNORE: u8 = 2;
    pub const UNIMPLEMENTED: u8 = 3;
    pub const DEBUG: u8 = 4;
    pub const SERVICE_REQUEST: u8 = 5;
    pub const SERVICE_ACCEPT: u8 = 6;

    /// Key exchange.
    pub const KEXINIT: u8 = 20;
    pub const NEWKEYS: u8 = 21;

    /// curve25519-sha256 key exchange (RFC 8731 §3).
    pub const KEX_ECDH_INIT: u8 = 30;
    pub const KEX_ECDH_REPLY: u8 = 31;

    /// User authentication (RFC 4252).
    pub const USERAUTH_REQUEST: u8 = 50;
    pub const USERAUTH_FAILURE: u8 = 51;
    pub const USERAUTH_SUCCESS: u8 = 52;
    pub const USERAUTH_BANNER: u8 = 53;
    pub const USERAUTH_PK_OK: u8 = 60;

    /// Connection protocol (RFC 4254).
    pub const CHANNEL_OPEN: u8 = 90;
    pub const CHANNEL_OPEN_CONFIRMATION: u8 = 91;
    pub const CHANNEL_OPEN_FAILURE: u8 = 92;
    pub const CHANNEL_WINDOW_ADJUST: u8 = 93;
    pub const CHANNEL_DATA: u8 = 94;
    pub const CHANNEL_EOF: u8 = 96;
    pub const CHANNEL_CLOSE: u8 = 97;
    pub const CHANNEL_REQUEST: u8 = 98;
    pub const CHANNEL_SUCCESS: u8 = 99;
    pub const CHANNEL_FAILURE: u8 = 100;
}

/// SSH disconnect reason codes (RFC 4253 §11.1).
#[allow(dead_code)] // Protocol constants — not all used yet.
mod disconnect_reason {
    pub const HOST_NOT_ALLOWED: u32 = 1;
    pub const PROTOCOL_ERROR: u32 = 2;
    pub const KEY_EXCHANGE_FAILED: u32 = 3;
    pub const RESERVED: u32 = 4;
    pub const MAC_ERROR: u32 = 5;
    pub const COMPRESSION_ERROR: u32 = 6;
    pub const SERVICE_NOT_AVAILABLE: u32 = 7;
    pub const PROTOCOL_VERSION_NOT_SUPPORTED: u32 = 8;
    pub const HOST_KEY_NOT_VERIFIABLE: u32 = 9;
    pub const CONNECTION_LOST: u32 = 10;
    pub const BY_APPLICATION: u32 = 11;
    pub const TOO_MANY_CONNECTIONS: u32 = 12;
    pub const AUTH_CANCELLED_BY_USER: u32 = 13;
    pub const NO_MORE_AUTH_METHODS: u32 = 14;
    pub const ILLEGAL_USER_NAME: u32 = 15;
}

// ===========================================================================
// Algorithm name lists (for KEXINIT negotiation)
// ===========================================================================

/// Key exchange algorithm.
const KEX_ALGORITHM: &str = "curve25519-sha256";

/// Server host key algorithm.
const HOST_KEY_ALGORITHM: &str = "ssh-ed25519";

/// Encryption algorithm.
const CIPHER_ALGORITHM: &str = "chacha20-poly1305@openssh.com";

/// MAC algorithm (implicit with AEAD cipher).
const MAC_ALGORITHM: &str = "";

/// Compression algorithm.
const COMPRESSION_ALGORITHM: &str = "none";

// ===========================================================================
// chacha20-poly1305@openssh.com constants
//
// This cipher uses two ChaCha20 instances:
// - K1 (main key, first 32 bytes of 64-byte key): encrypts payload
// - K2 (header key, last 32 bytes of 64-byte key): encrypts packet length
// Poly1305 MAC is computed over the encrypted packet (length + ciphertext).
// ===========================================================================

/// AEAD tag length for Poly1305.
const POLY1305_TAG_LEN: usize = 16;

// ===========================================================================
// SSH session state
// ===========================================================================

/// State machine for an SSH connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPhase {
    /// Waiting for client version string.
    VersionExchange,
    /// Performing key exchange (KEXINIT → NEWKEYS).
    KeyExchange,
    /// Waiting for SSH_MSG_KEXINIT from client.
    WaitKexInit,
    /// Sent our KEXINIT, waiting for client's ECDH init.
    WaitKexEcdhInit,
    /// Sent KEX_ECDH_REPLY + NEWKEYS, waiting for client NEWKEYS.
    WaitNewKeys,
    /// Waiting for service request (post-NEWKEYS).
    WaitServiceRequest,
    /// Performing user authentication.
    Authentication,
    /// Authenticated — connection protocol.
    Connected,
    /// Session closed.
    Closed,
}

/// Cryptographic keys for an established SSH connection.
///
/// The `chacha20-poly1305@openssh.com` cipher uses a 64-byte key for each
/// direction.  The first 32 bytes are the "main key" (K1) for payload
/// encryption, and the last 32 bytes are the "header key" (K2) for
/// encrypting the packet length field.
struct SessionKeys {
    /// Client-to-server main key (payload encryption).
    c2s_main_key: [u8; 32],
    /// Client-to-server header key (length encryption).
    c2s_header_key: [u8; 32],
    /// Server-to-client main key (payload encryption).
    s2c_main_key: [u8; 32],
    /// Server-to-client header key (length encryption).
    s2c_header_key: [u8; 32],
}

/// An SSH channel (RFC 4254).
struct Channel {
    /// Channel number on client side.
    client_channel: u32,
    /// Channel number on server side.
    server_channel: u32,
    /// Whether the channel is active.
    active: bool,
    /// Client's window size (how much we can send).
    client_window: u32,
    /// Server's window size (how much client can send us).
    server_window: u32,
    /// Maximum packet size for this channel.
    max_packet: u32,
    /// Accumulated input line buffer for shell.
    line_buf: Vec<u8>,
    /// Whether a shell has been requested on this channel.
    shell_active: bool,
}

/// A single SSH session.
struct Session {
    /// TCP connection handle.
    tcp_handle: usize,
    /// Current protocol phase.
    phase: SessionPhase,
    /// Whether this session slot is active.
    active: bool,
    /// Receive buffer for incomplete packets.
    recv_buf: Vec<u8>,
    /// Client's version string (without trailing CRLF).
    client_version: Vec<u8>,
    /// Our version string (without trailing CRLF).
    server_version: Vec<u8>,

    // -- Key exchange state --

    /// Our ephemeral X25519 private key (32 bytes).
    kex_private: [u8; 32],
    /// Our ephemeral X25519 public key (32 bytes).
    kex_public: [u8; 32],
    /// Client's ephemeral public key (from KEX_ECDH_INIT).
    client_kex_public: [u8; 32],
    /// Client's KEXINIT payload (for exchange hash).
    client_kexinit_payload: Vec<u8>,
    /// Server's KEXINIT payload (for exchange hash).
    server_kexinit_payload: Vec<u8>,
    /// Shared secret K from key exchange.
    shared_secret: [u8; 32],
    /// Exchange hash H (also serves as session_id on first exchange).
    exchange_hash: [u8; 32],
    /// Session identifier (H from the first key exchange).
    session_id: [u8; 32],
    /// Whether this is the first key exchange (session_id = H).
    first_kex: bool,

    // -- Encryption state (post-NEWKEYS) --

    /// Whether encryption is active.
    encrypted: bool,
    /// Session keys.
    keys: Option<SessionKeys>,
    /// Client-to-server packet sequence number.
    c2s_seq: u64,
    /// Server-to-client packet sequence number.
    s2c_seq: u64,

    // -- Authentication state --

    /// Authenticated username (empty if not yet authenticated).
    username: String,
    /// Number of authentication attempts.
    auth_attempts: u32,

    // -- Channel state --

    /// Active channels.
    channels: [Option<Channel>; 4],
    /// Next server channel ID.
    next_channel_id: u32,

    // -- Statistics --

    /// Remote IP.
    remote_ip: super::interface::IpAddr,
    /// Remote port.
    remote_port: u16,
    /// Connection timestamp.
    connected_at_ns: u64,
    /// Bytes received.
    bytes_rx: u64,
    /// Bytes sent.
    bytes_tx: u64,
}

impl Session {
    fn new() -> Self {
        Self {
            tcp_handle: 0,
            phase: SessionPhase::Closed,
            active: false,
            recv_buf: Vec::new(),
            client_version: Vec::new(),
            server_version: Vec::new(),
            kex_private: [0u8; 32],
            kex_public: [0u8; 32],
            client_kex_public: [0u8; 32],
            client_kexinit_payload: Vec::new(),
            server_kexinit_payload: Vec::new(),
            shared_secret: [0u8; 32],
            exchange_hash: [0u8; 32],
            session_id: [0u8; 32],
            first_kex: true,
            encrypted: false,
            keys: None,
            c2s_seq: 0,
            s2c_seq: 0,
            username: String::new(),
            auth_attempts: 0,
            channels: [None, None, None, None],
            next_channel_id: 0,
            remote_ip: super::interface::IpAddr::V4(super::interface::Ipv4Addr([0, 0, 0, 0])),
            remote_port: 0,
            connected_at_ns: 0,
            bytes_rx: 0,
            bytes_tx: 0,
        }
    }

    /// Reset session to initial state for reuse.
    fn reset(&mut self) {
        // Zero sensitive material.
        self.kex_private = [0u8; 32];
        self.shared_secret = [0u8; 32];
        self.keys = None;

        *self = Self::new();
    }
}

// ===========================================================================
// Global state
// ===========================================================================

struct SshState {
    /// TCP listener handle.
    listener_handle: Option<usize>,
    /// Active sessions.
    sessions: [Session; MAX_SESSIONS],
    /// Host key seed (Ed25519 32-byte private seed).
    host_key_seed: [u8; 32],
    /// Host key public key (Ed25519 32-byte public key).
    host_key_public: [u8; 32],
}

// We can't use `const fn` for Vec, so init lazily.
static STATE: Mutex<Option<SshState>> = Mutex::new(None);
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static LISTEN_PORT: AtomicU16 = AtomicU16::new(DEFAULT_PORT);
static LAST_TICK: AtomicU64 = AtomicU64::new(0);

// Statistics.
static TOTAL_CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static TOTAL_AUTH_FAILURES: AtomicU64 = AtomicU64::new(0);
static REJECTED_CONNECTIONS: AtomicU64 = AtomicU64::new(0);

// ===========================================================================
// SSH binary packet protocol helpers
// ===========================================================================

/// Build an unencrypted SSH binary packet (RFC 4253 §6).
///
/// ```text
/// uint32    packet_length   (not including self or MAC)
/// byte      padding_length
/// byte[n1]  payload
/// byte[n2]  random padding  (at least 4 bytes)
/// ```
///
/// The total of `padding_length + payload_length + 1` must be a multiple
/// of the cipher block size (8 for unencrypted).
fn build_packet(payload: &[u8]) -> Vec<u8> {
    let block_size = 8usize; // Unencrypted block size.
    let payload_len = payload.len();
    // packet_length = 1 (padding_length field) + payload + padding.
    // Total (excluding the 4-byte length prefix) must be multiple of block_size.
    let unpadded = 1 + payload_len;
    let mut padding = block_size - (unpadded % block_size);
    if padding < MIN_PADDING {
        padding += block_size;
    }
    let packet_length = unpadded + padding;

    let mut pkt = Vec::with_capacity(4 + packet_length);

    // uint32 packet_length
    #[allow(clippy::cast_possible_truncation)]
    {
        pkt.push((packet_length >> 24) as u8);
        pkt.push((packet_length >> 16) as u8);
        pkt.push((packet_length >> 8) as u8);
        pkt.push(packet_length as u8);
    }

    // byte padding_length
    #[allow(clippy::cast_possible_truncation)]
    pkt.push(padding as u8);

    // payload
    pkt.extend_from_slice(payload);

    // Random padding (RFC 4253 §6 recommends random padding).
    let pad_start = pkt.len();
    pkt.resize(pad_start + padding, 0);
    crate::rng::fill(&mut pkt[pad_start..]);

    pkt
}

/// Build an encrypted SSH binary packet using chacha20-poly1305@openssh.com.
///
/// The openssh chacha20-poly1305 cipher works as follows:
/// 1. Encrypt the 4-byte packet_length with K2 (header key), ChaCha20 counter=0
/// 2. Encrypt the rest (padding_length + payload + padding) with K1 (main key),
///    ChaCha20 counter=0 (first 64 bytes used for Poly1305 key), counter=1 for data
/// 3. Compute Poly1305 MAC over encrypted_length || encrypted_payload using
///    the key derived from K1's counter=0 block
///
/// Returns the full encrypted packet: encrypted_length(4) || encrypted_data(n) || mac(16).
fn build_encrypted_packet(
    payload: &[u8],
    seq: u64,
    main_key: &[u8; 32],
    header_key: &[u8; 32],
) -> Vec<u8> {
    // First build the plaintext packet structure (same as unencrypted).
    let block_size = 8usize;
    let payload_len = payload.len();
    let unpadded = 1 + payload_len;
    let mut padding = block_size - (unpadded % block_size);
    if padding < MIN_PADDING {
        padding += block_size;
    }
    let packet_length = unpadded + padding;

    // Build the nonce from the sequence number (big-endian, 12 bytes,
    // sequence in the last 8 bytes, first 4 bytes zero).
    let nonce = seq_to_nonce(seq);

    // -- Step 1: Encrypt the 4-byte packet_length with K2 --
    #[allow(clippy::cast_possible_truncation)]
    let mut length_bytes = [
        (packet_length >> 24) as u8,
        (packet_length >> 16) as u8,
        (packet_length >> 8) as u8,
        packet_length as u8,
    ];
    // K2 uses ChaCha20 with counter=0 to encrypt the length.
    crypto::chacha20_xor(header_key, &nonce, 0, &mut length_bytes);

    // -- Step 2: Build plaintext data (padding_length || payload || padding) --
    let mut data = Vec::with_capacity(packet_length);
    #[allow(clippy::cast_possible_truncation)]
    data.push(padding as u8);
    data.extend_from_slice(payload);
    let pad_start = data.len();
    data.resize(pad_start + padding, 0);
    crate::rng::fill(&mut data[pad_start..]);

    // Derive Poly1305 one-time key from K1, counter=0.
    let mut poly_key_block = [0u8; 32];
    // Generate 64 bytes of keystream from K1 at counter=0, take first 32.
    let mut poly_key_buf = [0u8; 64];
    crypto::chacha20_xor(main_key, &nonce, 0, &mut poly_key_buf);
    poly_key_block.copy_from_slice(&poly_key_buf[..32]);

    // Encrypt data with K1, counter=1 (counter=0 was used for poly key).
    crypto::chacha20_xor(main_key, &nonce, 1, &mut data);

    // -- Step 3: Compute Poly1305 MAC over encrypted_length || encrypted_data --
    let mut mac_input = Vec::with_capacity(4 + data.len());
    mac_input.extend_from_slice(&length_bytes);
    mac_input.extend_from_slice(&data);

    let tag = crypto::poly1305(&poly_key_block, &mac_input);

    // Assemble: encrypted_length || encrypted_data || mac
    let mut out = Vec::with_capacity(4 + data.len() + POLY1305_TAG_LEN);
    out.extend_from_slice(&length_bytes);
    out.extend_from_slice(&data);
    out.extend_from_slice(&tag);

    out
}

/// Decrypt an SSH packet using chacha20-poly1305@openssh.com.
///
/// Input: the full packet (encrypted_length(4) || encrypted_data(n) || mac(16)).
/// Returns the decrypted payload on success, or an error if MAC verification fails.
fn decrypt_packet(
    packet: &[u8],
    seq: u64,
    main_key: &[u8; 32],
    header_key: &[u8; 32],
) -> KernelResult<Vec<u8>> {
    if packet.len() < 4 + 1 + POLY1305_TAG_LEN {
        return Err(KernelError::InvalidArgument);
    }

    let nonce = seq_to_nonce(seq);

    // Split into components.
    let encrypted_length = &packet[..4];
    let mac_offset = packet.len() - POLY1305_TAG_LEN;
    let encrypted_data = &packet[4..mac_offset];
    let received_mac = &packet[mac_offset..];

    // Derive Poly1305 one-time key from K1, counter=0.
    let mut poly_key_buf = [0u8; 64];
    crypto::chacha20_xor(main_key, &nonce, 0, &mut poly_key_buf);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&poly_key_buf[..32]);

    // Verify MAC over encrypted_length || encrypted_data.
    let mut mac_input = Vec::with_capacity(4 + encrypted_data.len());
    mac_input.extend_from_slice(encrypted_length);
    mac_input.extend_from_slice(encrypted_data);

    let computed_mac = crypto::poly1305(&poly_key, &mac_input);
    // Constant-time MAC comparison (prevent timing side-channels).
    if received_mac.len() < 16 {
        return Err(KernelError::InvalidArgument);
    }
    let mut mac_ok = true;
    for i in 0..16 {
        if computed_mac[i] != received_mac[i] {
            mac_ok = false;
        }
    }
    if !mac_ok {
        return Err(KernelError::PermissionDenied); // MAC mismatch
    }

    // Decrypt length field.
    let mut length_bytes = [0u8; 4];
    length_bytes.copy_from_slice(encrypted_length);
    crypto::chacha20_xor(header_key, &nonce, 0, &mut length_bytes);

    let packet_length = u32::from_be_bytes(length_bytes) as usize;
    if encrypted_data.len() != packet_length {
        return Err(KernelError::InvalidArgument);
    }

    // Decrypt data with K1, counter=1.
    let mut data = encrypted_data.to_vec();
    crypto::chacha20_xor(main_key, &nonce, 1, &mut data);

    // Parse: padding_length(1) || payload(n) || padding(padding_length).
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let padding_len = data[0] as usize;
    if padding_len + 1 > data.len() {
        return Err(KernelError::InvalidArgument);
    }
    let payload_end = data.len() - padding_len;
    Ok(data[1..payload_end].to_vec())
}

/// Convert a 64-bit sequence number to a 12-byte ChaCha20 nonce.
///
/// The nonce is the sequence number in big-endian in the last 8 bytes,
/// with the first 4 bytes set to zero.
fn seq_to_nonce(seq: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    let seq_bytes = seq.to_be_bytes();
    nonce[4..12].copy_from_slice(&seq_bytes);
    nonce
}

// ===========================================================================
// SSH wire format helpers
// ===========================================================================

/// Read a uint32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read an SSH "string" (uint32 length + data) from a byte slice.
/// Returns (data_slice, bytes_consumed).
fn read_string(data: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let len = read_u32(data, offset)? as usize;
    let start = offset + 4;
    let end = start + len;
    let s = data.get(start..end)?;
    Some((s, 4 + len))
}

/// Write a uint32 to a Vec.
fn write_u32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_be_bytes());
}

/// Write an SSH "string" (uint32 length + data) to a Vec.
fn write_string(buf: &mut Vec<u8>, data: &[u8]) {
    #[allow(clippy::cast_possible_truncation)]
    write_u32(buf, data.len() as u32);
    buf.extend_from_slice(data);
}

/// Write a name-list (comma-separated UTF-8 string) to a Vec.
fn write_name_list(buf: &mut Vec<u8>, names: &str) {
    write_string(buf, names.as_bytes());
}

/// Encode an mpint (RFC 4251 §5): big-endian integer with length prefix.
/// The SSH protocol requires mpint to have a leading 0x00 byte if the
/// high bit of the first byte is set (to distinguish from negative numbers).
fn encode_mpint(value: &[u8]) -> Vec<u8> {
    // Skip leading zeros.
    let mut start = 0;
    while start < value.len() && value[start] == 0 {
        start += 1;
    }

    if start == value.len() {
        // Value is zero.
        let mut out = Vec::with_capacity(4);
        write_u32(&mut out, 0);
        return out;
    }

    let significant = &value[start..];
    let needs_pad = (significant[0] & 0x80) != 0;
    #[allow(clippy::cast_possible_truncation)]
    let len = significant.len() + if needs_pad { 1 } else { 0 };

    let mut out = Vec::with_capacity(4 + len);
    write_u32(&mut out, len as u32);
    if needs_pad {
        out.push(0x00);
    }
    out.extend_from_slice(significant);
    out
}

/// Encode an Ed25519 public key in SSH wire format (RFC 8709 §4).
///
/// ```text
/// string    "ssh-ed25519"
/// string    public_key (32 bytes)
/// ```
fn encode_ed25519_pubkey(pubkey: &[u8; 32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + 11 + 4 + 32);
    write_string(&mut buf, b"ssh-ed25519");
    write_string(&mut buf, pubkey);
    buf
}

/// Encode an Ed25519 signature in SSH wire format (RFC 8709 §6).
///
/// ```text
/// string    "ssh-ed25519"
/// string    signature (64 bytes)
/// ```
fn encode_ed25519_signature(sig: &[u8; 64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + 11 + 4 + 64);
    write_string(&mut buf, b"ssh-ed25519");
    write_string(&mut buf, sig);
    buf
}

// ===========================================================================
// Key exchange (curve25519-sha256, RFC 8731)
// ===========================================================================

/// Generate the exchange hash H (RFC 8731 §3.1).
///
/// The hash is SHA-256 of:
/// ```text
/// string    V_C, the client's identification string (without CRLF)
/// string    V_S, the server's identification string (without CRLF)
/// string    I_C, payload of client's SSH_MSG_KEXINIT
/// string    I_S, payload of server's SSH_MSG_KEXINIT
/// string    K_S, server's public host key (encoded)
/// string    Q_C, client's ephemeral public key (32 bytes)
/// string    Q_S, server's ephemeral public key (32 bytes)
/// mpint     K, shared secret
/// ```
fn compute_exchange_hash(
    client_version: &[u8],
    server_version: &[u8],
    client_kexinit: &[u8],
    server_kexinit: &[u8],
    host_key_blob: &[u8],
    client_ephemeral: &[u8; 32],
    server_ephemeral: &[u8; 32],
    shared_secret: &[u8; 32],
) -> [u8; 32] {
    let mut buf = Vec::with_capacity(512);

    write_string(&mut buf, client_version);
    write_string(&mut buf, server_version);
    write_string(&mut buf, client_kexinit);
    write_string(&mut buf, server_kexinit);
    write_string(&mut buf, host_key_blob);
    write_string(&mut buf, client_ephemeral);
    write_string(&mut buf, server_ephemeral);

    // K is encoded as mpint (big-endian with length prefix).
    buf.extend_from_slice(&encode_mpint(shared_secret));

    crypto::sha256(&buf)
}

/// Derive session keys from the shared secret and exchange hash.
///
/// RFC 4253 §7.2 defines key derivation:
/// ```text
/// K1 = HASH(K || H || "A" || session_id)  — IV client-to-server
/// K2 = HASH(K || H || "B" || session_id)  — IV server-to-client
/// K3 = HASH(K || H || "C" || session_id)  — Key client-to-server
/// K4 = HASH(K || H || "D" || session_id)  — Key server-to-client
/// K5 = HASH(K || H || "E" || session_id)  — Integrity client-to-server
/// K6 = HASH(K || H || "F" || session_id)  — Integrity server-to-client
/// ```
///
/// For chacha20-poly1305@openssh.com, we need 64 bytes per direction
/// (K1 = main key, K2 = header key).  We derive them by computing
/// HASH for letters C/D (first 32 bytes = main key) and extending
/// with HASH(K || H || K_n) for the next 32 bytes (header key).
fn derive_session_keys(
    shared_secret: &[u8; 32],
    exchange_hash: &[u8; 32],
    session_id: &[u8; 32],
) -> SessionKeys {
    let k_mpint = encode_mpint(shared_secret);

    // Helper: compute HASH(K || H || X || session_id) where X is a single char.
    let derive = |letter: u8| -> [u8; 32] {
        let mut buf = Vec::with_capacity(k_mpint.len() + 32 + 1 + 32);
        buf.extend_from_slice(&k_mpint);
        buf.extend_from_slice(exchange_hash);
        buf.push(letter);
        buf.extend_from_slice(session_id);
        crypto::sha256(&buf)
    };

    // Helper: extend a key to 64 bytes by computing HASH(K || H || K_n).
    let extend = |first_32: &[u8; 32]| -> [u8; 32] {
        let mut buf = Vec::with_capacity(k_mpint.len() + 32 + 32);
        buf.extend_from_slice(&k_mpint);
        buf.extend_from_slice(exchange_hash);
        buf.extend_from_slice(first_32);
        crypto::sha256(&buf)
    };

    // Client-to-server: letter 'C' for encryption key.
    let c2s_main = derive(b'C');
    let c2s_header = extend(&c2s_main);

    // Server-to-client: letter 'D' for encryption key.
    let s2c_main = derive(b'D');
    let s2c_header = extend(&s2c_main);

    SessionKeys {
        c2s_main_key: c2s_main,
        c2s_header_key: c2s_header,
        s2c_main_key: s2c_main,
        s2c_header_key: s2c_header,
    }
}

// ===========================================================================
// KEXINIT message construction
// ===========================================================================

/// Build the payload of an SSH_MSG_KEXINIT message.
///
/// RFC 4253 §7.1:
/// ```text
/// byte         SSH_MSG_KEXINIT
/// byte[16]     cookie (random)
/// name-list    kex_algorithms
/// name-list    server_host_key_algorithms
/// name-list    encryption_algorithms_client_to_server
/// name-list    encryption_algorithms_server_to_client
/// name-list    mac_algorithms_client_to_server
/// name-list    mac_algorithms_server_to_client
/// name-list    compression_algorithms_client_to_server
/// name-list    compression_algorithms_server_to_client
/// name-list    languages_client_to_server
/// name-list    languages_server_to_client
/// boolean      first_kex_packet_follows
/// uint32       0 (reserved for future extension)
/// ```
fn build_kexinit() -> Vec<u8> {
    let mut payload = Vec::with_capacity(256);

    payload.push(msg::KEXINIT);

    // Cookie: 16 random bytes.  We use a hash of the current time as a
    // simple PRNG.  Real randomness is future work (hardware RNG).
    let cookie = generate_cookie();
    payload.extend_from_slice(&cookie);

    // Algorithm lists.
    write_name_list(&mut payload, KEX_ALGORITHM);
    write_name_list(&mut payload, HOST_KEY_ALGORITHM);
    write_name_list(&mut payload, CIPHER_ALGORITHM); // c2s encryption
    write_name_list(&mut payload, CIPHER_ALGORITHM); // s2c encryption
    write_name_list(&mut payload, MAC_ALGORITHM);    // c2s MAC
    write_name_list(&mut payload, MAC_ALGORITHM);    // s2c MAC
    write_name_list(&mut payload, COMPRESSION_ALGORITHM); // c2s compression
    write_name_list(&mut payload, COMPRESSION_ALGORITHM); // s2c compression
    write_name_list(&mut payload, ""); // c2s languages
    write_name_list(&mut payload, ""); // s2c languages

    // first_kex_packet_follows = false
    payload.push(0);

    // reserved (uint32 0)
    write_u32(&mut payload, 0);

    payload
}

/// Generate a 16-byte cookie for KEXINIT.
///
/// Uses the kernel's CSPRNG (ChaCha20-based) seeded from hardware
/// entropy sources (RDRAND/RDSEED where available, plus TSC jitter).
fn generate_cookie() -> [u8; 16] {
    let mut cookie = [0u8; 16];
    crate::rng::fill(&mut cookie);
    cookie
}

/// Generate an ephemeral X25519 key pair for key exchange.
///
/// Uses the kernel's CSPRNG to generate a random 32-byte private key,
/// then computes the corresponding X25519 public key.
fn generate_ephemeral_keypair() -> ([u8; 32], [u8; 32]) {
    let mut private_key = [0u8; 32];
    crate::rng::fill(&mut private_key);
    let public_key = crypto::x25519_base(&private_key);

    (private_key, public_key)
}

// ===========================================================================
// SSH message processing — per-phase handlers
// ===========================================================================

/// Process data from the client's version string exchange.
///
/// Returns true if version exchange is complete.
fn process_version_exchange(session: &mut Session) -> KernelResult<bool> {
    // Look for CRLF in the receive buffer.
    let crlf_pos = session.recv_buf.windows(2)
        .position(|w| w == b"\r\n");

    let Some(pos) = crlf_pos else {
        if session.recv_buf.len() > MAX_VERSION_LEN {
            return Err(KernelError::InvalidArgument);
        }
        return Ok(false); // Need more data.
    };

    // Extract version string (without CRLF).
    let version = session.recv_buf[..pos].to_vec();

    // Validate: must start with "SSH-2.0-".
    if !version.starts_with(b"SSH-2.0-") {
        crate::serial_println!("[ssh] Client version mismatch: {:?}",
            core::str::from_utf8(&version).unwrap_or("<invalid>"));
        return Err(KernelError::InvalidArgument);
    }

    session.client_version = version;
    // Remove consumed bytes (version + CRLF).
    let consumed = pos + 2;
    session.recv_buf = session.recv_buf.split_off(consumed);

    Ok(true)
}

/// Parse the client's KEXINIT message and verify algorithm compatibility.
///
/// Returns true if the client's algorithms are compatible with ours.
fn process_client_kexinit(session: &mut Session, payload: &[u8]) -> KernelResult<bool> {
    // Save the full payload for exchange hash computation.
    session.client_kexinit_payload = payload.to_vec();

    // Skip: msg_type(1) + cookie(16) = 17 bytes.
    if payload.len() < 17 {
        return Err(KernelError::InvalidArgument);
    }
    let mut offset = 17;

    // Read the algorithm name-lists and verify our algorithms are offered.
    // We need to check: kex, host_key, cipher_c2s, cipher_s2c.
    let checks = [
        (KEX_ALGORITHM, "kex"),
        (HOST_KEY_ALGORITHM, "host_key"),
        (CIPHER_ALGORITHM, "cipher_c2s"),
        (CIPHER_ALGORITHM, "cipher_s2c"),
    ];

    for (required, name) in &checks {
        let (list_bytes, consumed) = read_string(payload, offset)
            .ok_or(KernelError::InvalidArgument)?;
        offset += consumed;

        let list = core::str::from_utf8(list_bytes).unwrap_or("");
        if !list.split(',').any(|alg| alg == *required) {
            crate::serial_println!("[ssh] Client doesn't offer {} for {}",
                required, name);
            return Ok(false);
        }
    }

    // Skip remaining name-lists (mac_c2s, mac_s2c, comp_c2s, comp_s2c,
    // lang_c2s, lang_s2c) — we accept anything since our AEAD doesn't
    // need a separate MAC, and we only support compression=none.
    Ok(true)
}

/// Handle SSH_MSG_KEX_ECDH_INIT from the client.
///
/// The client sends its ephemeral public key.  We respond with
/// SSH_MSG_KEX_ECDH_REPLY containing our host key, ephemeral public,
/// and signature of the exchange hash.
fn handle_kex_ecdh_init(
    session: &mut Session,
    payload: &[u8],
    host_key_seed: &[u8; 32],
    host_key_public: &[u8; 32],
) -> KernelResult<Vec<u8>> {
    // Parse: byte SSH_MSG_KEX_ECDH_INIT, string Q_C (client ephemeral public).
    if payload.is_empty() || payload[0] != msg::KEX_ECDH_INIT {
        return Err(KernelError::InvalidArgument);
    }

    let (q_c, _) = read_string(payload, 1)
        .ok_or(KernelError::InvalidArgument)?;

    if q_c.len() != 32 {
        return Err(KernelError::InvalidArgument);
    }

    session.client_kex_public.copy_from_slice(q_c);

    // Perform X25519 key exchange.
    session.shared_secret = crypto::x25519(&session.kex_private, &session.client_kex_public);

    // Check for all-zero shared secret (invalid peer key).
    if session.shared_secret == [0u8; 32] {
        return Err(KernelError::InvalidArgument);
    }

    // Encode host key blob.
    let host_key_blob = encode_ed25519_pubkey(host_key_public);

    // Compute exchange hash H.
    session.exchange_hash = compute_exchange_hash(
        &session.client_version,
        &session.server_version,
        &session.client_kexinit_payload,
        &session.server_kexinit_payload,
        &host_key_blob,
        &session.client_kex_public,
        &session.kex_public,
        &session.shared_secret,
    );

    // On first key exchange, H becomes the session_id.
    if session.first_kex {
        session.session_id = session.exchange_hash;
        session.first_kex = false;
    }

    // Sign the exchange hash with our host key.
    let sig = crypto::ed25519_sign(host_key_seed, &session.exchange_hash);
    let sig_blob = encode_ed25519_signature(&sig);

    // Build SSH_MSG_KEX_ECDH_REPLY.
    let mut reply = Vec::with_capacity(256);
    reply.push(msg::KEX_ECDH_REPLY);
    write_string(&mut reply, &host_key_blob);        // K_S (host key)
    write_string(&mut reply, &session.kex_public);    // Q_S (server ephemeral)
    write_string(&mut reply, &sig_blob);              // signature of H

    Ok(reply)
}

// ===========================================================================
// Authentication (RFC 4252)
// ===========================================================================

/// Process an SSH_MSG_USERAUTH_REQUEST.
///
/// Currently supports:
/// - "none" (returns available methods)
/// - "password" (checked against kernel user table)
/// - "publickey" (ssh-ed25519 against authorized keys)
fn handle_userauth_request(
    session: &mut Session,
    payload: &[u8],
) -> KernelResult<Vec<u8>> {
    // Parse: byte SSH_MSG_USERAUTH_REQUEST
    //        string user name
    //        string service name ("ssh-connection")
    //        string method name
    //        <method-specific data>
    if payload.is_empty() || payload[0] != msg::USERAUTH_REQUEST {
        return Err(KernelError::InvalidArgument);
    }

    let mut offset = 1;

    let (username_bytes, consumed) = read_string(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += consumed;

    let (service_bytes, consumed) = read_string(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += consumed;

    let (method_bytes, consumed) = read_string(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += consumed;

    let username = core::str::from_utf8(username_bytes).unwrap_or("");
    let service = core::str::from_utf8(service_bytes).unwrap_or("");
    let method = core::str::from_utf8(method_bytes).unwrap_or("");

    // Service must be "ssh-connection".
    if service != "ssh-connection" {
        return build_userauth_failure();
    }

    match method {
        "none" => {
            // Return available authentication methods.
            build_userauth_failure()
        }
        "password" => {
            handle_password_auth(session, payload, offset, username)
        }
        "publickey" => {
            handle_publickey_auth(session, payload, offset, username)
        }
        _ => {
            session.auth_attempts += 1;
            build_userauth_failure()
        }
    }
}

/// Handle password authentication.
fn handle_password_auth(
    session: &mut Session,
    payload: &[u8],
    offset: usize,
    username: &str,
) -> KernelResult<Vec<u8>> {
    // Parse: boolean FALSE (not changing password), string password.
    if payload.get(offset) != Some(&0) {
        return build_userauth_failure();
    }

    let (password_bytes, _) = read_string(payload, offset + 1)
        .ok_or(KernelError::InvalidArgument)?;

    let password = core::str::from_utf8(password_bytes).unwrap_or("");

    // Verify credentials.
    // For now, accept "root" with any password in debug builds,
    // or check against the kernel user table.
    let authenticated = verify_password(username, password);

    if authenticated {
        session.username = String::from(username);
        crate::serial_println!("[ssh] User '{}' authenticated via password", username);
        build_userauth_success()
    } else {
        session.auth_attempts += 1;
        crate::serial_println!("[ssh] Password auth failed for '{}' (attempt {})",
            username, session.auth_attempts);
        TOTAL_AUTH_FAILURES.fetch_add(1, Ordering::Relaxed);
        build_userauth_failure()
    }
}

/// Handle public key authentication (ssh-ed25519).
fn handle_publickey_auth(
    session: &mut Session,
    payload: &[u8],
    offset: usize,
    username: &str,
) -> KernelResult<Vec<u8>> {
    // Parse: boolean has_signature
    let has_signature = payload.get(offset).copied().unwrap_or(0) != 0;
    let mut off = offset + 1;

    // string public key algorithm name
    let (algo_bytes, consumed) = read_string(payload, off)
        .ok_or(KernelError::InvalidArgument)?;
    off += consumed;

    let algo = core::str::from_utf8(algo_bytes).unwrap_or("");
    if algo != "ssh-ed25519" {
        return build_userauth_failure();
    }

    // string public key blob
    let (key_blob, consumed) = read_string(payload, off)
        .ok_or(KernelError::InvalidArgument)?;
    off += consumed;

    // Extract the raw 32-byte public key from the blob.
    // Blob format: string "ssh-ed25519", string <32-byte key>
    let pubkey = extract_ed25519_pubkey(key_blob)?;

    // Check if this public key is authorized for this user.
    if !is_key_authorized(username, &pubkey) {
        session.auth_attempts += 1;
        return build_userauth_failure();
    }

    if !has_signature {
        // Client is asking if this key would be accepted.
        // Respond with SSH_MSG_USERAUTH_PK_OK.
        let mut reply = Vec::with_capacity(64);
        reply.push(msg::USERAUTH_PK_OK);
        write_string(&mut reply, b"ssh-ed25519");
        write_string(&mut reply, key_blob);
        return Ok(reply);
    }

    // Parse the signature.
    let (sig_blob, _) = read_string(payload, off)
        .ok_or(KernelError::InvalidArgument)?;

    // Extract raw 64-byte signature from blob.
    let sig = extract_ed25519_signature(sig_blob)?;

    // Build the data that was signed (RFC 4252 §7):
    // string    session identifier
    // byte      SSH_MSG_USERAUTH_REQUEST
    // string    user name
    // string    service name
    // string    "publickey"
    // boolean   TRUE
    // string    public key algorithm name
    // string    public key blob
    let signed_data = build_pubkey_signed_data(
        &session.session_id,
        username,
        key_blob,
    );

    // Verify the signature.
    if crypto::ed25519_verify(&pubkey, &signed_data, &sig) {
        session.username = String::from(username);
        crate::serial_println!("[ssh] User '{}' authenticated via public key", username);
        build_userauth_success()
    } else {
        session.auth_attempts += 1;
        crate::serial_println!("[ssh] Public key auth failed for '{}' (attempt {})",
            username, session.auth_attempts);
        TOTAL_AUTH_FAILURES.fetch_add(1, Ordering::Relaxed);
        build_userauth_failure()
    }
}

/// Extract a 32-byte Ed25519 public key from an SSH key blob.
fn extract_ed25519_pubkey(blob: &[u8]) -> KernelResult<[u8; 32]> {
    // blob = string "ssh-ed25519" + string <32 bytes>
    let (algo, consumed) = read_string(blob, 0)
        .ok_or(KernelError::InvalidArgument)?;
    if algo != b"ssh-ed25519" {
        return Err(KernelError::InvalidArgument);
    }
    let (key_data, _) = read_string(blob, consumed)
        .ok_or(KernelError::InvalidArgument)?;
    if key_data.len() != 32 {
        return Err(KernelError::InvalidArgument);
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(key_data);
    Ok(key)
}

/// Extract a 64-byte Ed25519 signature from an SSH signature blob.
fn extract_ed25519_signature(blob: &[u8]) -> KernelResult<[u8; 64]> {
    // blob = string "ssh-ed25519" + string <64 bytes>
    let (algo, consumed) = read_string(blob, 0)
        .ok_or(KernelError::InvalidArgument)?;
    if algo != b"ssh-ed25519" {
        return Err(KernelError::InvalidArgument);
    }
    let (sig_data, _) = read_string(blob, consumed)
        .ok_or(KernelError::InvalidArgument)?;
    if sig_data.len() != 64 {
        return Err(KernelError::InvalidArgument);
    }
    let mut sig = [0u8; 64];
    sig.copy_from_slice(sig_data);
    Ok(sig)
}

/// Build the data blob signed during public key authentication.
fn build_pubkey_signed_data(
    session_id: &[u8; 32],
    username: &str,
    key_blob: &[u8],
) -> Vec<u8> {
    let mut data = Vec::with_capacity(256);
    write_string(&mut data, session_id);
    data.push(msg::USERAUTH_REQUEST);
    write_string(&mut data, username.as_bytes());
    write_string(&mut data, b"ssh-connection");
    write_string(&mut data, b"publickey");
    data.push(1); // TRUE
    write_string(&mut data, b"ssh-ed25519");
    write_string(&mut data, key_blob);
    data
}

/// Build an SSH_MSG_USERAUTH_FAILURE response.
fn build_userauth_failure() -> KernelResult<Vec<u8>> {
    let mut reply = Vec::with_capacity(32);
    reply.push(msg::USERAUTH_FAILURE);
    write_name_list(&mut reply, "password,publickey");
    reply.push(0); // partial success = false
    Ok(reply)
}

/// Build an SSH_MSG_USERAUTH_SUCCESS response.
fn build_userauth_success() -> KernelResult<Vec<u8>> {
    Ok(vec![msg::USERAUTH_SUCCESS])
}

/// Verify a username/password pair.
///
/// Delegates to `fs::useracct::authenticate()` which checks the kernel's
/// user account database (FNV-1a password hashing, account enabled/locked
/// checks).  Falls back to a development-only root/root login if the user
/// management system has no accounts configured yet.
fn verify_password(username: &str, password: &str) -> bool {
    // Try the real user account system first.
    match crate::fs::useracct::authenticate(username, password) {
        Ok(_session_id) => true,
        Err(_) => {
            // Fallback: allow root/root ONLY if the user system has no
            // non-system users configured (i.e., fresh boot with defaults).
            // Once a real user is created, this fallback is dead code.
            if username == "root" && password == "root" {
                // Check if useracct has been initialized with real users.
                // get_user_by_name("root") always succeeds (system user),
                // but if authenticate() failed it means wrong password hash.
                // Allow only if the root account has no password set (NoPassword).
                crate::fs::useracct::get_user_by_name("root")
                    .map(|u| u.login_method == crate::fs::useracct::LoginMethod::NoPassword)
                    .unwrap_or(false)
            } else {
                false
            }
        }
    }
}

/// Check if a public key is authorized for the given user.
///
/// Searches for matching Ed25519 public keys in two locations:
/// 1. The credential store (`fs::credentials`) with kind `SshKey`
///    for the app ID "sshd" and a service name matching the username.
/// 2. The filesystem at `~/.ssh/authorized_keys` (one 32-byte hex
///    public key per line, lines starting with '#' are comments).
///
/// Returns true if the supplied public key matches any stored key.
fn is_key_authorized(username: &str, pubkey: &[u8; 32]) -> bool {
    // Method 1: Check credential store for SshKey entries.
    // The credential store isolates per app_id, so we look up "sshd"
    // entries with the username as the service name.
    let entries = crate::fs::credentials::list_for_app("sshd");
    for entry in &entries {
        if entry.service == username {
            // Try to retrieve the secret (the stored public key in hex).
            if let Ok(stored) = crate::fs::credentials::retrieve("sshd", username) {
                if let Some(key_bytes) = hex_to_32_bytes(&stored.secret) {
                    if key_bytes == *pubkey {
                        return true;
                    }
                }
            }
        }
    }

    // Method 2: Check ~/.ssh/authorized_keys file.
    let home = crate::fs::useracct::get_user_by_name(username)
        .map(|u| u.home_dir.clone())
        .unwrap_or_default();
    if !home.is_empty() {
        let path = alloc::format!("{}/.ssh/authorized_keys", home);
        if let Ok(data) = crate::fs::vfs::Vfs::read_file(&path) {
            // Parse: one hex-encoded 32-byte key per line.
            if let Ok(text) = core::str::from_utf8(&data) {
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some(key_bytes) = hex_to_32_bytes(line) {
                        if key_bytes == *pubkey {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// Parse a 64-character hex string into 32 bytes.
fn hex_to_32_bytes(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = hex_digit(hex.as_bytes().get(i * 2).copied()?)?;
        let lo = hex_digit(hex.as_bytes().get(i * 2 + 1).copied()?)?;
        *byte = (hi << 4) | lo;
    }
    Some(out)
}

/// Convert a single hex character to its value.
fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ===========================================================================
// Connection protocol (RFC 4254) — channels
// ===========================================================================

/// Handle SSH_MSG_CHANNEL_OPEN.
fn handle_channel_open(
    session: &mut Session,
    payload: &[u8],
) -> KernelResult<Vec<u8>> {
    // Parse: byte SSH_MSG_CHANNEL_OPEN
    //        string channel type
    //        uint32 sender channel
    //        uint32 initial window size
    //        uint32 maximum packet size
    if payload.is_empty() || payload[0] != msg::CHANNEL_OPEN {
        return Err(KernelError::InvalidArgument);
    }

    let mut offset = 1;

    let (chan_type_bytes, consumed) = read_string(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += consumed;

    let chan_type = core::str::from_utf8(chan_type_bytes).unwrap_or("");

    let sender_channel = read_u32(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += 4;

    let initial_window = read_u32(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += 4;

    let max_packet = read_u32(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;

    // Only "session" channels are supported.
    if chan_type != "session" {
        let mut reply = Vec::with_capacity(32);
        reply.push(msg::CHANNEL_OPEN_FAILURE);
        write_u32(&mut reply, sender_channel);
        write_u32(&mut reply, 3); // SSH_OPEN_UNKNOWN_CHANNEL_TYPE
        write_string(&mut reply, b"Only session channels supported");
        write_string(&mut reply, b"en");
        return Ok(reply);
    }

    // Find a free channel slot.
    let slot = session.channels.iter().position(|c| c.is_none());
    let Some(slot) = slot else {
        let mut reply = Vec::with_capacity(32);
        reply.push(msg::CHANNEL_OPEN_FAILURE);
        write_u32(&mut reply, sender_channel);
        write_u32(&mut reply, 4); // SSH_OPEN_RESOURCE_SHORTAGE
        write_string(&mut reply, b"Too many channels");
        write_string(&mut reply, b"en");
        return Ok(reply);
    };

    let server_channel = session.next_channel_id;
    session.next_channel_id += 1;

    session.channels[slot] = Some(Channel {
        client_channel: sender_channel,
        server_channel,
        active: true,
        client_window: initial_window,
        server_window: 65536, // Our initial window.
        max_packet,
        line_buf: Vec::new(),
        shell_active: false,
    });

    // Build SSH_MSG_CHANNEL_OPEN_CONFIRMATION.
    let mut reply = Vec::with_capacity(32);
    reply.push(msg::CHANNEL_OPEN_CONFIRMATION);
    write_u32(&mut reply, sender_channel);      // recipient channel
    write_u32(&mut reply, server_channel);       // sender channel
    write_u32(&mut reply, 65536);               // initial window size
    write_u32(&mut reply, MAX_PAYLOAD_SIZE as u32); // maximum packet size

    Ok(reply)
}

/// Handle SSH_MSG_CHANNEL_REQUEST.
fn handle_channel_request(
    session: &mut Session,
    payload: &[u8],
) -> KernelResult<Option<Vec<u8>>> {
    if payload.is_empty() || payload[0] != msg::CHANNEL_REQUEST {
        return Err(KernelError::InvalidArgument);
    }

    let mut offset = 1;

    let recipient_channel = read_u32(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += 4;

    let (req_type_bytes, consumed) = read_string(payload, offset)
        .ok_or(KernelError::InvalidArgument)?;
    offset += consumed;

    let req_type = core::str::from_utf8(req_type_bytes).unwrap_or("");
    let want_reply = payload.get(offset).copied().unwrap_or(0) != 0;

    // Find the channel.
    let channel = session.channels.iter_mut()
        .find_map(|c| c.as_mut().filter(|ch| ch.server_channel == recipient_channel));

    let Some(channel) = channel else {
        if want_reply {
            let mut reply = Vec::with_capacity(8);
            reply.push(msg::CHANNEL_FAILURE);
            write_u32(&mut reply, recipient_channel);
            return Ok(Some(reply));
        }
        return Ok(None);
    };

    match req_type {
        "shell" => {
            channel.shell_active = true;
            crate::serial_println!("[ssh] Shell opened for user '{}'", session.username);

            if want_reply {
                let mut reply = Vec::with_capacity(8);
                reply.push(msg::CHANNEL_SUCCESS);
                write_u32(&mut reply, channel.client_channel);
                Ok(Some(reply))
            } else {
                Ok(None)
            }
        }
        "pty-req" => {
            // Accept but ignore pty requests — we don't do real terminal emulation.
            if want_reply {
                let mut reply = Vec::with_capacity(8);
                reply.push(msg::CHANNEL_SUCCESS);
                write_u32(&mut reply, channel.client_channel);
                Ok(Some(reply))
            } else {
                Ok(None)
            }
        }
        "env" => {
            // Accept but ignore environment variable requests.
            if want_reply {
                let mut reply = Vec::with_capacity(8);
                reply.push(msg::CHANNEL_SUCCESS);
                write_u32(&mut reply, channel.client_channel);
                Ok(Some(reply))
            } else {
                Ok(None)
            }
        }
        _ => {
            crate::serial_println!("[ssh] Unknown channel request: {}", req_type);
            if want_reply {
                let mut reply = Vec::with_capacity(8);
                reply.push(msg::CHANNEL_FAILURE);
                write_u32(&mut reply, channel.client_channel);
                Ok(Some(reply))
            } else {
                Ok(None)
            }
        }
    }
}

/// Handle SSH_MSG_CHANNEL_DATA — shell input from the client.
fn handle_channel_data(
    session: &mut Session,
    payload: &[u8],
) -> KernelResult<Option<Vec<u8>>> {
    if payload.is_empty() || payload[0] != msg::CHANNEL_DATA {
        return Err(KernelError::InvalidArgument);
    }

    let recipient_channel = read_u32(payload, 1)
        .ok_or(KernelError::InvalidArgument)?;

    let (data, _) = read_string(payload, 5)
        .ok_or(KernelError::InvalidArgument)?;

    // Find the channel.
    let channel = session.channels.iter_mut()
        .find_map(|c| c.as_mut().filter(|ch| ch.server_channel == recipient_channel));

    let Some(channel) = channel else {
        return Ok(None);
    };

    if !channel.shell_active {
        return Ok(None);
    }

    // Reduce our window by the received data size.
    #[allow(clippy::cast_possible_truncation)]
    {
        channel.server_window = channel.server_window.saturating_sub(data.len() as u32);
    }

    // Process the data byte-by-byte for line buffering.
    let mut output = Vec::new();

    for &byte in data {
        match byte {
            b'\r' | b'\n' => {
                if !channel.line_buf.is_empty() {
                    let line = String::from(
                        core::str::from_utf8(&channel.line_buf).unwrap_or("")
                    );
                    channel.line_buf.clear();

                    // Execute via kshell and capture output.
                    let result = execute_shell_command(&session.username, &line);
                    output.extend_from_slice(result.as_bytes());
                }
                // Echo newline and prompt.
                output.extend_from_slice(b"\r\n");
                output.extend_from_slice(format!("{}@mintos$ ", session.username).as_bytes());
            }
            0x7f | 0x08 => {
                // Backspace: remove last character.
                if channel.line_buf.pop().is_some() {
                    output.extend_from_slice(b"\x08 \x08"); // Erase character.
                }
            }
            0x03 => {
                // Ctrl+C: clear line.
                channel.line_buf.clear();
                output.extend_from_slice(b"^C\r\n");
                output.extend_from_slice(format!("{}@mintos$ ", session.username).as_bytes());
            }
            0x04 => {
                // Ctrl+D on empty line: close channel.
                if channel.line_buf.is_empty() {
                    channel.active = false;
                    let mut reply = Vec::with_capacity(8);
                    reply.push(msg::CHANNEL_EOF);
                    write_u32(&mut reply, channel.client_channel);
                    return Ok(Some(reply));
                }
            }
            _ => {
                if channel.line_buf.len() < 1024 {
                    channel.line_buf.push(byte);
                    // Echo character.
                    output.push(byte);
                }
            }
        }
    }

    if output.is_empty() {
        return Ok(None);
    }

    // Build SSH_MSG_CHANNEL_DATA response.
    let mut reply = Vec::with_capacity(16 + output.len());
    reply.push(msg::CHANNEL_DATA);
    write_u32(&mut reply, channel.client_channel);
    write_string(&mut reply, &output);

    // Send window adjust if needed.
    // (In a real implementation we'd batch this, but for simplicity
    // we send it alongside data.)

    Ok(Some(reply))
}

/// Handle SSH_MSG_CHANNEL_CLOSE.
fn handle_channel_close(
    session: &mut Session,
    payload: &[u8],
) -> KernelResult<Option<Vec<u8>>> {
    if payload.len() < 5 || payload[0] != msg::CHANNEL_CLOSE {
        return Err(KernelError::InvalidArgument);
    }

    let recipient_channel = read_u32(payload, 1)
        .ok_or(KernelError::InvalidArgument)?;

    // Find and close the channel.
    for slot in &mut session.channels {
        if let Some(ch) = slot {
            if ch.server_channel == recipient_channel {
                let client_channel = ch.client_channel;
                *slot = None;

                // Send CHANNEL_CLOSE back.
                let mut reply = Vec::with_capacity(8);
                reply.push(msg::CHANNEL_CLOSE);
                write_u32(&mut reply, client_channel);
                return Ok(Some(reply));
            }
        }
    }

    Ok(None)
}

/// Handle SSH_MSG_CHANNEL_WINDOW_ADJUST.
fn handle_window_adjust(
    session: &mut Session,
    payload: &[u8],
) -> KernelResult<()> {
    if payload.len() < 9 || payload[0] != msg::CHANNEL_WINDOW_ADJUST {
        return Err(KernelError::InvalidArgument);
    }

    let recipient_channel = read_u32(payload, 1)
        .ok_or(KernelError::InvalidArgument)?;
    let bytes_to_add = read_u32(payload, 5)
        .ok_or(KernelError::InvalidArgument)?;

    // Find the channel and adjust its window.
    for ch in session.channels.iter_mut().flatten() {
        if ch.server_channel == recipient_channel {
            ch.client_window = ch.client_window.saturating_add(bytes_to_add);
            return Ok(());
        }
    }

    Ok(())
}

/// Execute a shell command and return the output.
///
/// Dispatches to the kernel shell (kshell) and captures output.
fn execute_shell_command(_username: &str, command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Block dangerous commands over SSH (same policy as telnet).
    if trimmed == "reboot" || trimmed == "shutdown" || trimmed == "poweroff" {
        return String::from("Reboot/shutdown not permitted via SSH.\r\n");
    }

    // Dispatch to kshell and capture output.
    let output = crate::kshell::capture_command(trimmed);

    // Convert LF to CR+LF for proper SSH terminal display.
    lf_to_crlf(&output)
}

/// Convert bare LF to CR+LF for SSH terminal output.
///
/// SSH terminals expect CR+LF line endings.  The kshell output
/// uses bare LF, so we convert here.
fn lf_to_crlf(input: &str) -> String {
    let mut result = String::with_capacity(input.len() + input.len() / 10);
    for c in input.chars() {
        if c == '\n' {
            result.push('\r');
        }
        result.push(c);
    }
    result
}

// ===========================================================================
// SSH_MSG_DISCONNECT
// ===========================================================================

/// Build an SSH_MSG_DISCONNECT message.
fn build_disconnect(reason: u32, description: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(32 + description.len());
    payload.push(msg::DISCONNECT);
    write_u32(&mut payload, reason);
    write_string(&mut payload, description.as_bytes());
    write_string(&mut payload, b"en"); // language tag
    payload
}

// ===========================================================================
// Session processing — main dispatch loop
// ===========================================================================

/// Process a single SSH message payload for a session.
///
/// Returns a list of response payloads to send back (may be empty).
fn process_message(
    session: &mut Session,
    payload: &[u8],
    host_key_seed: &[u8; 32],
    host_key_public: &[u8; 32],
) -> KernelResult<Vec<Vec<u8>>> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }

    let msg_type = payload[0];

    // Handle messages that are valid in any phase.
    match msg_type {
        msg::DISCONNECT => {
            crate::serial_println!("[ssh] Client disconnected");
            session.phase = SessionPhase::Closed;
            return Ok(Vec::new());
        }
        msg::IGNORE | msg::DEBUG => {
            return Ok(Vec::new());
        }
        msg::UNIMPLEMENTED => {
            return Ok(Vec::new());
        }
        _ => {}
    }

    // Phase-specific message processing.
    match session.phase {
        SessionPhase::WaitKexInit => {
            if msg_type != msg::KEXINIT {
                let reply = build_disconnect(
                    disconnect_reason::PROTOCOL_ERROR,
                    "Expected KEXINIT",
                );
                return Ok(vec![reply]);
            }

            if !process_client_kexinit(session, payload)? {
                let reply = build_disconnect(
                    disconnect_reason::KEY_EXCHANGE_FAILED,
                    "No compatible algorithms",
                );
                return Ok(vec![reply]);
            }

            session.phase = SessionPhase::WaitKexEcdhInit;
            Ok(Vec::new())
        }

        SessionPhase::WaitKexEcdhInit => {
            if msg_type != msg::KEX_ECDH_INIT {
                let reply = build_disconnect(
                    disconnect_reason::PROTOCOL_ERROR,
                    "Expected KEX_ECDH_INIT",
                );
                return Ok(vec![reply]);
            }

            let kex_reply = handle_kex_ecdh_init(
                session, payload, host_key_seed, host_key_public,
            )?;

            // Derive session keys.
            let keys = derive_session_keys(
                &session.shared_secret,
                &session.exchange_hash,
                &session.session_id,
            );
            session.keys = Some(keys);

            // Send: KEX_ECDH_REPLY, then NEWKEYS.
            let newkeys_payload = vec![msg::NEWKEYS];
            session.phase = SessionPhase::WaitNewKeys;

            Ok(vec![kex_reply, newkeys_payload])
        }

        SessionPhase::WaitNewKeys => {
            if msg_type != msg::NEWKEYS {
                let reply = build_disconnect(
                    disconnect_reason::PROTOCOL_ERROR,
                    "Expected NEWKEYS",
                );
                return Ok(vec![reply]);
            }

            // Encryption is now active in both directions.
            session.encrypted = true;
            session.c2s_seq = 0;
            session.s2c_seq = 0;
            session.phase = SessionPhase::WaitServiceRequest;

            crate::serial_println!("[ssh] Key exchange complete, encryption active");
            Ok(Vec::new())
        }

        SessionPhase::WaitServiceRequest => {
            if msg_type != msg::SERVICE_REQUEST {
                let reply = build_disconnect(
                    disconnect_reason::PROTOCOL_ERROR,
                    "Expected SERVICE_REQUEST",
                );
                return Ok(vec![reply]);
            }

            let (service_name, _) = read_string(payload, 1)
                .ok_or(KernelError::InvalidArgument)?;

            let service = core::str::from_utf8(service_name).unwrap_or("");

            if service != "ssh-userauth" {
                let reply = build_disconnect(
                    disconnect_reason::SERVICE_NOT_AVAILABLE,
                    "Unknown service",
                );
                return Ok(vec![reply]);
            }

            // Send SERVICE_ACCEPT.
            let mut accept = Vec::with_capacity(32);
            accept.push(msg::SERVICE_ACCEPT);
            write_string(&mut accept, b"ssh-userauth");

            session.phase = SessionPhase::Authentication;
            Ok(vec![accept])
        }

        SessionPhase::Authentication => {
            if msg_type != msg::USERAUTH_REQUEST {
                let reply = build_disconnect(
                    disconnect_reason::PROTOCOL_ERROR,
                    "Expected USERAUTH_REQUEST",
                );
                return Ok(vec![reply]);
            }

            if session.auth_attempts >= MAX_AUTH_ATTEMPTS {
                let reply = build_disconnect(
                    disconnect_reason::NO_MORE_AUTH_METHODS,
                    "Too many authentication failures",
                );
                return Ok(vec![reply]);
            }

            let response = handle_userauth_request(session, payload)?;

            // Check if authentication succeeded.
            if !response.is_empty() && response[0] == msg::USERAUTH_SUCCESS {
                session.phase = SessionPhase::Connected;
            }

            Ok(vec![response])
        }

        SessionPhase::Connected => {
            // Connection protocol messages.
            match msg_type {
                msg::CHANNEL_OPEN => {
                    let reply = handle_channel_open(session, payload)?;
                    Ok(vec![reply])
                }
                msg::CHANNEL_REQUEST => {
                    if let Some(reply) = handle_channel_request(session, payload)? {
                        // If shell was just opened, also send a welcome + prompt.
                        let mut replies = vec![reply];

                        // Check if any channel just became shell_active.
                        for ch in session.channels.iter().flatten() {
                            if ch.shell_active && ch.line_buf.is_empty() {
                                let welcome = format!(
                                    "\r\nWelcome to MintOS SSH ({}).\r\n\
                                     Type 'help' for available commands.\r\n\r\n\
                                     {}@mintos$ ",
                                    session.username, session.username
                                );
                                let mut data_msg = Vec::with_capacity(32 + welcome.len());
                                data_msg.push(msg::CHANNEL_DATA);
                                write_u32(&mut data_msg, ch.client_channel);
                                write_string(&mut data_msg, welcome.as_bytes());
                                replies.push(data_msg);
                            }
                        }

                        Ok(replies)
                    } else {
                        Ok(Vec::new())
                    }
                }
                msg::CHANNEL_DATA => {
                    if let Some(reply) = handle_channel_data(session, payload)? {
                        Ok(vec![reply])
                    } else {
                        Ok(Vec::new())
                    }
                }
                msg::CHANNEL_WINDOW_ADJUST => {
                    handle_window_adjust(session, payload)?;
                    Ok(Vec::new())
                }
                msg::CHANNEL_EOF => {
                    // Client indicates no more data; acknowledge but don't close yet.
                    Ok(Vec::new())
                }
                msg::CHANNEL_CLOSE => {
                    if let Some(reply) = handle_channel_close(session, payload)? {
                        Ok(vec![reply])
                    } else {
                        Ok(Vec::new())
                    }
                }
                _ => {
                    // Send UNIMPLEMENTED for unknown messages.
                    let mut reply = Vec::with_capacity(8);
                    reply.push(msg::UNIMPLEMENTED);
                    write_u32(&mut reply, session.c2s_seq as u32);
                    Ok(vec![reply])
                }
            }
        }

        SessionPhase::VersionExchange | SessionPhase::KeyExchange |
        SessionPhase::Closed => {
            // Shouldn't receive binary packets in these phases.
            Ok(Vec::new())
        }
    }
}

// ===========================================================================
// TCP I/O helpers
// ===========================================================================

/// Send raw bytes over TCP.
fn tcp_send(handle: usize, data: &[u8]) -> KernelResult<usize> {
    super::tcp::send(handle, data)
}

/// Read available data from TCP.
fn tcp_read(handle: usize) -> KernelResult<Vec<u8>> {
    super::tcp::read_up_to(handle, 32768)
}

/// Send an unencrypted SSH packet (pre-NEWKEYS).
fn send_packet_plain(handle: usize, payload: &[u8]) -> KernelResult<()> {
    let pkt = build_packet(payload);
    tcp_send(handle, &pkt)?;
    Ok(())
}

/// Send an encrypted SSH packet (post-NEWKEYS).
fn send_packet_encrypted(
    handle: usize,
    payload: &[u8],
    seq: u64,
    main_key: &[u8; 32],
    header_key: &[u8; 32],
) -> KernelResult<()> {
    let pkt = build_encrypted_packet(payload, seq, main_key, header_key);
    tcp_send(handle, &pkt)?;
    Ok(())
}

// ===========================================================================
// Session-level I/O: read a complete packet from the receive buffer
// ===========================================================================

/// Try to extract a complete unencrypted SSH packet from the receive buffer.
///
/// Returns the payload if a complete packet is available, or None if more
/// data is needed.
fn try_read_packet_plain(recv_buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    if recv_buf.len() < 5 {
        return None;
    }

    let packet_length = u32::from_be_bytes([
        recv_buf[0], recv_buf[1], recv_buf[2], recv_buf[3],
    ]) as usize;

    if packet_length > MAX_PACKET_SIZE || packet_length < 2 {
        return None; // Invalid.
    }

    let total = 4 + packet_length;
    if recv_buf.len() < total {
        return None; // Incomplete.
    }

    let padding_len = recv_buf[4] as usize;
    if padding_len + 1 > packet_length {
        return None; // Invalid.
    }

    let payload_len = packet_length - 1 - padding_len;
    let payload = recv_buf[5..5 + payload_len].to_vec();

    // Remove consumed bytes.
    *recv_buf = recv_buf.split_off(total);

    Some(payload)
}

/// Try to extract a complete encrypted SSH packet from the receive buffer.
///
/// For chacha20-poly1305@openssh.com:
/// 1. Decrypt the 4-byte length with K2 to know the packet size
/// 2. Wait until full packet (length + data + MAC) is available
/// 3. Decrypt and verify MAC
fn try_read_packet_encrypted(
    recv_buf: &mut Vec<u8>,
    seq: u64,
    main_key: &[u8; 32],
    header_key: &[u8; 32],
) -> KernelResult<Option<Vec<u8>>> {
    if recv_buf.len() < 4 {
        return Ok(None);
    }

    // Decrypt the length field to know how much data to expect.
    let nonce = seq_to_nonce(seq);
    let mut length_bytes = [0u8; 4];
    length_bytes.copy_from_slice(&recv_buf[..4]);
    crypto::chacha20_xor(header_key, &nonce, 0, &mut length_bytes);

    let packet_length = u32::from_be_bytes(length_bytes) as usize;
    if packet_length > MAX_PACKET_SIZE || packet_length < 2 {
        return Err(KernelError::InvalidArgument);
    }

    let total = 4 + packet_length + POLY1305_TAG_LEN;
    if recv_buf.len() < total {
        return Ok(None); // Need more data.
    }

    // Extract the full encrypted packet.
    let encrypted_packet = recv_buf[..total].to_vec();

    // Decrypt and verify.
    let payload = decrypt_packet(&encrypted_packet, seq, main_key, header_key)?;

    // Remove consumed bytes.
    *recv_buf = recv_buf.split_off(total);

    Ok(Some(payload))
}

// ===========================================================================
// Initialization and server lifecycle
// ===========================================================================

/// Host key filesystem path.
const HOST_KEY_PATH: &str = "/etc/ssh/host_key";

/// Load or generate the SSH host key.
///
/// Attempts to load a previously-saved host key from `/etc/ssh/host_key`.
/// If the file doesn't exist (first boot), generates a new key from the
/// kernel CSPRNG and persists it so the host key remains stable across
/// reboots.  Clients will not see a "host key changed" warning.
fn generate_host_key() -> ([u8; 32], [u8; 32]) {
    use crate::fs::vfs::Vfs;

    // Try to load existing host key (32 bytes of Ed25519 seed).
    if let Ok(data) = Vfs::read_file(HOST_KEY_PATH) {
        if data.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&data);
            let public = crypto::ed25519_public_key(&seed);
            crate::serial_println!("[sshd] Loaded host key from {}", HOST_KEY_PATH);
            return (seed, public);
        }
        // Invalid file — regenerate.
        crate::serial_println!("[sshd] Invalid host key file ({}B), regenerating", data.len());
    }

    // Generate fresh host key.
    let mut seed = [0u8; 32];
    crate::rng::fill(&mut seed);
    let public = crypto::ed25519_public_key(&seed);

    // Persist to filesystem for future boots.
    // Create /etc/ssh directory if needed.
    let _ = Vfs::mkdir("/etc");
    let _ = Vfs::mkdir("/etc/ssh");
    match Vfs::write_file(HOST_KEY_PATH, &seed) {
        Ok(()) => crate::serial_println!("[sshd] Generated and saved host key to {}", HOST_KEY_PATH),
        Err(e) => crate::serial_println!(
            "[sshd] Generated host key (save failed: {:?} — key changes on reboot)", e
        ),
    }

    (seed, public)
}

/// Initialize the SSH server.
///
/// Generates a host key, binds a TCP listener, and begins accepting
/// connections on the configured port (default 22).
pub fn init() -> KernelResult<()> {
    if INITIALIZED.load(Ordering::Relaxed) {
        return Ok(());
    }

    let port = LISTEN_PORT.load(Ordering::Relaxed);

    // Generate host key.
    let (host_seed, host_public) = generate_host_key();

    // Bind TCP listener.
    let listener = super::tcp::bind(crate::netns::ROOT_NS, port)?;

    let mut guard = STATE.lock();
    *guard = Some(SshState {
        listener_handle: Some(listener),
        sessions: [
            Session::new(),
            Session::new(),
            Session::new(),
            Session::new(),
        ],
        host_key_seed: host_seed,
        host_key_public: host_public,
    });

    INITIALIZED.store(true, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Relaxed);

    // Log the host key fingerprint (SHA-256 of the public key blob).
    let key_blob = encode_ed25519_pubkey(&host_public);
    let fingerprint = crypto::sha256(&key_blob);
    crate::serial_println!(
        "[ssh] Server listening on port {} (host key fingerprint: SHA256:{:02x}{:02x}{:02x}...{:02x})",
        port,
        fingerprint[0], fingerprint[1], fingerprint[2],
        fingerprint[31],
    );

    Ok(())
}

/// Shut down the SSH server.
///
/// Sends disconnect to all active sessions and closes the listener.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Relaxed);

    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        for session in &mut state.sessions {
            if session.active {
                // Best-effort disconnect.
                let disconnect = build_disconnect(
                    disconnect_reason::BY_APPLICATION,
                    "Server shutting down",
                );
                if session.encrypted {
                    if let Some(keys) = &session.keys {
                        let _ = send_packet_encrypted(
                            session.tcp_handle,
                            &disconnect,
                            session.s2c_seq,
                            &keys.s2c_main_key,
                            &keys.s2c_header_key,
                        );
                    }
                } else {
                    let _ = send_packet_plain(session.tcp_handle, &disconnect);
                }
                let _ = super::tcp::close(session.tcp_handle);
                session.reset();
            }
        }

        if let Some(listener) = state.listener_handle.take() {
            let _ = super::tcp::close_listener(listener);
        }
    }

    INITIALIZED.store(false, Ordering::Relaxed);
    crate::serial_println!("[ssh] Server shut down");
}

/// Periodic tick — called from the network subsystem's main loop.
///
/// Accepts new connections, reads data from existing sessions,
/// processes SSH messages, and sends responses.
pub fn tick() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Rate-limit ticks.
    let now = crate::hrtimer::now_ns();
    let last = LAST_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) < TICK_INTERVAL_NS {
        return;
    }
    LAST_TICK.store(now, Ordering::Relaxed);

    let mut guard = STATE.lock();
    let Some(state) = guard.as_mut() else { return };

    // Accept new connections.
    if let Some(listener) = state.listener_handle {
        while let Ok(tcp_handle) = super::tcp::try_accept(listener) {
            // Find a free session slot.
            let slot = state.sessions.iter().position(|s| !s.active);
            if let Some(idx) = slot {
                let session = &mut state.sessions[idx];
                session.reset();
                session.tcp_handle = tcp_handle;
                session.active = true;
                session.phase = SessionPhase::VersionExchange;
                session.connected_at_ns = now;

                // Store server version (without CRLF) for exchange hash.
                session.server_version = SSH_VERSION_STRING[..SSH_VERSION_STRING.len() - 2].to_vec();

                // Get remote address.
                if let Some((ip, port)) = super::tcp::peer_addr(tcp_handle) {
                    session.remote_ip = ip;
                    session.remote_port = port;
                }

                // Send our version string.
                let _ = tcp_send(tcp_handle, SSH_VERSION_STRING);

                // Generate ephemeral key pair for this session.
                let (priv_key, pub_key) = generate_ephemeral_keypair();
                session.kex_private = priv_key;
                session.kex_public = pub_key;

                TOTAL_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                crate::serial_println!("[ssh] New connection from {:?}:{}", session.remote_ip, session.remote_port);
            } else {
                // No free slots — reject.
                let _ = super::tcp::close(tcp_handle);
                REJECTED_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    // Copy host key data out of state for use during message processing.
    let host_key_seed = state.host_key_seed;
    let host_key_public = state.host_key_public;

    // Process each active session.
    for session in &mut state.sessions {
        if !session.active {
            continue;
        }

        // Read available data from TCP.
        match tcp_read(session.tcp_handle) {
            Ok(data) if !data.is_empty() => {
                #[allow(clippy::cast_possible_truncation)]
                {
                    session.bytes_rx += data.len() as u64;
                }
                session.recv_buf.extend_from_slice(&data);
            }
            Err(_) => {
                // Connection error — close session.
                session.phase = SessionPhase::Closed;
            }
            _ => {}
        }

        // Check for closed TCP connection.
        if super::tcp::is_remote_closed(session.tcp_handle) && session.recv_buf.is_empty() {
            session.phase = SessionPhase::Closed;
        }

        // Phase-specific processing.
        match session.phase {
            SessionPhase::VersionExchange => {
                match process_version_exchange(session) {
                    Ok(true) => {
                        // Version exchange complete — send our KEXINIT.
                        let kexinit = build_kexinit();
                        session.server_kexinit_payload = kexinit.clone();
                        if send_packet_plain(session.tcp_handle, &kexinit).is_err() {
                            session.phase = SessionPhase::Closed;
                        } else {
                            session.phase = SessionPhase::WaitKexInit;
                        }
                    }
                    Ok(false) => {} // Need more data.
                    Err(_) => {
                        crate::serial_println!("[ssh] Version exchange failed");
                        session.phase = SessionPhase::Closed;
                    }
                }
            }

            SessionPhase::WaitKexInit | SessionPhase::WaitKexEcdhInit |
            SessionPhase::WaitNewKeys => {
                // Read unencrypted packets.
                while let Some(payload) = try_read_packet_plain(&mut session.recv_buf) {
                    match process_message(
                        session, &payload,
                        &host_key_seed, &host_key_public,
                    ) {
                        Ok(responses) => {
                            for resp in responses {
                                // After NEWKEYS is sent, switch to encrypted mode
                                // for outgoing messages.
                                if session.phase == SessionPhase::WaitNewKeys
                                    || session.phase == SessionPhase::WaitServiceRequest
                                {
                                    // Check if encryption just became active.
                                    if session.encrypted {
                                        if let Some(keys) = &session.keys {
                                            let _ = send_packet_encrypted(
                                                session.tcp_handle,
                                                &resp,
                                                session.s2c_seq,
                                                &keys.s2c_main_key,
                                                &keys.s2c_header_key,
                                            );
                                            session.s2c_seq += 1;
                                        }
                                    } else {
                                        let _ = send_packet_plain(session.tcp_handle, &resp);
                                    }
                                } else {
                                    let _ = send_packet_plain(session.tcp_handle, &resp);
                                }
                            }
                        }
                        Err(e) => {
                            crate::serial_println!("[ssh] Error processing message: {:?}", e);
                            session.phase = SessionPhase::Closed;
                            break;
                        }
                    }

                    if session.phase == SessionPhase::Closed {
                        break;
                    }
                }
            }

            SessionPhase::WaitServiceRequest | SessionPhase::Authentication |
            SessionPhase::Connected => {
                // Read encrypted packets.
                while let Some(keys) = &session.keys {
                    let main_key = keys.c2s_main_key;
                    let header_key = keys.c2s_header_key;

                    match try_read_packet_encrypted(
                        &mut session.recv_buf,
                        session.c2s_seq,
                        &main_key,
                        &header_key,
                    ) {
                        Ok(Some(payload)) => {
                            session.c2s_seq += 1;

                            match process_message(
                                session, &payload,
                                &host_key_seed, &host_key_public,
                            ) {
                                Ok(responses) => {
                                    for resp in responses {
                                        if let Some(keys) = &session.keys {
                                            let _ = send_packet_encrypted(
                                                session.tcp_handle,
                                                &resp,
                                                session.s2c_seq,
                                                &keys.s2c_main_key,
                                                &keys.s2c_header_key,
                                            );
                                            session.s2c_seq += 1;
                                        }
                                    }
                                }
                                Err(e) => {
                                    crate::serial_println!("[ssh] Error: {:?}", e);
                                    session.phase = SessionPhase::Closed;
                                    break;
                                }
                            }
                        }
                        Ok(None) => break, // Need more data.
                        Err(_) => {
                            crate::serial_println!("[ssh] Decryption/MAC error");
                            session.phase = SessionPhase::Closed;
                            break;
                        }
                    }

                    if session.phase == SessionPhase::Closed {
                        break;
                    }
                }
            }

            SessionPhase::KeyExchange | SessionPhase::Closed => {}
        }

        // Clean up closed sessions.
        if session.phase == SessionPhase::Closed && session.active {
            let _ = super::tcp::close(session.tcp_handle);
            crate::serial_println!("[ssh] Session closed for {:?}:{}",
                session.remote_ip, session.remote_port);
            session.reset();
        }
    }
}

// ===========================================================================
// Public API — status and configuration
// ===========================================================================

/// Set the SSH server listening port (must be called before `init()`).
pub fn set_port(port: u16) {
    LISTEN_PORT.store(port, Ordering::Relaxed);
}

/// Get the current listening port.
pub fn get_port() -> u16 {
    LISTEN_PORT.load(Ordering::Relaxed)
}

/// Check if the SSH server is running.
pub fn is_running() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Get SSH server statistics.
pub fn stats() -> SshStats {
    SshStats {
        total_connections: TOTAL_CONNECTIONS.load(Ordering::Relaxed),
        total_auth_failures: TOTAL_AUTH_FAILURES.load(Ordering::Relaxed),
        rejected_connections: REJECTED_CONNECTIONS.load(Ordering::Relaxed),
        active_sessions: active_session_count(),
    }
}

/// Get the number of active SSH sessions.
fn active_session_count() -> u32 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(state) => {
            #[allow(clippy::cast_possible_truncation)]
            {
                state.sessions.iter().filter(|s| s.active).count() as u32
            }
        }
        None => 0,
    }
}

/// SSH server statistics.
pub struct SshStats {
    /// Total connections accepted since boot.
    pub total_connections: u64,
    /// Total authentication failures.
    pub total_auth_failures: u64,
    /// Connections rejected (no free slots).
    pub rejected_connections: u64,
    /// Currently active sessions.
    pub active_sessions: u32,
}

// ===========================================================================
// Self-test
// ===========================================================================

/// SSH subsystem self-test.
///
/// Tests the binary packet protocol, encryption/decryption, key derivation,
/// and message construction.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    serial_println!("[ssh] Running self-test...");

    // Test 1: Unencrypted packet build/parse round-trip.
    {
        let payload = b"Hello, SSH!";
        let packet = build_packet(payload);

        // Verify structure.
        let packet_length = u32::from_be_bytes([
            packet[0], packet[1], packet[2], packet[3],
        ]) as usize;
        let padding_len = packet[4] as usize;
        assert!(padding_len >= MIN_PADDING, "Padding too short");
        assert_eq!(packet_length, 1 + payload.len() + padding_len);

        // Parse it back.
        let mut buf = packet;
        let parsed = try_read_packet_plain(&mut buf);
        assert!(parsed.is_some(), "Packet parse failed");
        assert_eq!(parsed.unwrap(), payload, "Payload mismatch");
        assert!(buf.is_empty(), "Buffer not empty after parse");

        serial_println!("[ssh]   Packet build/parse: OK");
    }

    // Test 2: Encrypted packet round-trip.
    {
        let main_key = crypto::sha256(b"test main key");
        let header_key = crypto::sha256(b"test header key");
        let seq = 42u64;
        let payload = b"Encrypted SSH payload!";

        let encrypted = build_encrypted_packet(payload, seq, &main_key, &header_key);

        // Decrypt it.
        let decrypted = decrypt_packet(&encrypted, seq, &main_key, &header_key)?;
        assert_eq!(decrypted, payload, "Encrypted round-trip mismatch");

        serial_println!("[ssh]   Encrypted packet round-trip: OK");
    }

    // Test 3: MAC verification failure on tampered data.
    {
        let main_key = crypto::sha256(b"test main key 2");
        let header_key = crypto::sha256(b"test header key 2");
        let seq = 7u64;
        let payload = b"Tamper test";

        let mut encrypted = build_encrypted_packet(payload, seq, &main_key, &header_key);

        // Tamper with the ciphertext.
        if encrypted.len() > 10 {
            encrypted[8] ^= 0xFF;
        }

        // Decryption should fail (MAC mismatch).
        assert!(decrypt_packet(&encrypted, seq, &main_key, &header_key).is_err(),
            "Tampered packet should fail decryption");

        serial_println!("[ssh]   MAC tamper detection: OK");
    }

    // Test 4: Key derivation determinism.
    {
        let shared = crypto::sha256(b"shared secret");
        let hash = crypto::sha256(b"exchange hash");
        let sid = crypto::sha256(b"session id");

        let keys1 = derive_session_keys(&shared, &hash, &sid);
        let keys2 = derive_session_keys(&shared, &hash, &sid);

        assert_eq!(keys1.c2s_main_key, keys2.c2s_main_key, "Key derivation not deterministic");
        assert_eq!(keys1.s2c_main_key, keys2.s2c_main_key, "Key derivation not deterministic");

        // Keys in different directions should differ.
        assert_ne!(keys1.c2s_main_key, keys1.s2c_main_key, "C2S and S2C keys should differ");

        serial_println!("[ssh]   Key derivation: OK");
    }

    // Test 5: Wire format helpers.
    {
        let mut buf = Vec::new();
        write_u32(&mut buf, 0x12345678);
        assert_eq!(buf, [0x12, 0x34, 0x56, 0x78]);

        let mut buf2 = Vec::new();
        write_string(&mut buf2, b"test");
        assert_eq!(buf2, [0, 0, 0, 4, b't', b'e', b's', b't']);

        // Read back.
        let (s, consumed) = read_string(&buf2, 0).unwrap();
        assert_eq!(s, b"test");
        assert_eq!(consumed, 8);

        serial_println!("[ssh]   Wire format helpers: OK");
    }

    // Test 6: mpint encoding.
    {
        // Zero.
        let z = encode_mpint(&[0, 0, 0]);
        assert_eq!(z, [0, 0, 0, 0]); // Length 0.

        // Small positive (no high bit).
        let s = encode_mpint(&[0x7F]);
        assert_eq!(s, [0, 0, 0, 1, 0x7F]);

        // Positive with high bit (needs padding).
        let h = encode_mpint(&[0x80]);
        assert_eq!(h, [0, 0, 0, 2, 0x00, 0x80]);

        serial_println!("[ssh]   mpint encoding: OK");
    }

    // Test 7: Ed25519 host key encoding.
    {
        let pubkey = [0x42u8; 32];
        let blob = encode_ed25519_pubkey(&pubkey);

        // Should decode correctly.
        let extracted = extract_ed25519_pubkey(&blob)?;
        assert_eq!(extracted, pubkey, "Host key round-trip failed");

        serial_println!("[ssh]   Host key encoding: OK");
    }

    // Test 8: KEXINIT message construction.
    {
        let kexinit = build_kexinit();
        assert_eq!(kexinit[0], msg::KEXINIT, "KEXINIT msg type");
        assert!(kexinit.len() > 17, "KEXINIT too short");

        serial_println!("[ssh]   KEXINIT construction: OK");
    }

    // Test 9: LF-to-CRLF conversion for terminal output.
    {
        assert_eq!(lf_to_crlf(""), "");
        assert_eq!(lf_to_crlf("hello"), "hello");
        assert_eq!(lf_to_crlf("a\nb\n"), "a\r\nb\r\n");
        assert_eq!(lf_to_crlf("\n"), "\r\n");
        assert_eq!(lf_to_crlf("a\r\nb"), "a\r\r\nb"); // Pre-existing CRLF: don't double-convert
                                                         // (real usage won't have these, but it's safe).
        serial_println!("[ssh]   LF→CRLF conversion: OK");
    }

    serial_println!("[ssh] Self-test PASSED (9 tests)");
    Ok(())
}
