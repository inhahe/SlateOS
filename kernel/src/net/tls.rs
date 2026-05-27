//! TLS 1.3 implementation (RFC 8446).
//!
//! Provides both client and server TLS 1.3 built on the kernel's
//! crypto primitives (X25519, ChaCha20-Poly1305, HKDF-SHA256, SHA-256,
//! Ed25519).
//!
//! ## Supported cipher suite
//!
//! - `TLS_CHACHA20_POLY1305_SHA256` (0x1303)
//!
//! This is the only cipher suite we implement.  It's widely supported
//! by all modern TLS 1.3 clients/servers and doesn't require AES hardware.
//!
//! ## Supported key exchange
//!
//! - X25519 (0x001D)
//!
//! ## Server mode
//!
//! `tls_accept()` performs the server-side handshake using Ed25519 for
//! certificate signing.  A self-signed X.509 certificate is generated
//! on-the-fly.  Clients must either skip certificate verification or
//! pin the server's public key.
//!
//! ## Limitations
//!
//! - No client certificates.
//! - No session resumption / PSK / 0-RTT.
//! - Client mode: no certificate chain validation (accepts any certificate).
//! - No ALPN negotiation (could be added for HTTP/2).
//! - Single cipher suite (ChaCha20-Poly1305 only, no AES-GCM).
//!
//! ## Architecture
//!
//! The TLS session wraps a TCP connection handle.  After `tls_connect()`,
//! the caller uses `tls_send()` / `tls_recv()` instead of raw TCP
//! send/recv.  The TLS layer handles record framing, encryption, and
//! decryption transparently.
//!
//! ## References
//!
//! - RFC 8446: The Transport Layer Security (TLS) Protocol Version 1.3
//! - RFC 8439: ChaCha20 and Poly1305 for IETF Protocols
//! - RFC 7748: Elliptic Curves for Security (X25519)

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::vec::Vec;
use crate::crypto;
use crate::error::{KernelError, KernelResult};

// ===========================================================================
// TLS 1.3 constants
// ===========================================================================

/// TLS record content types (RFC 8446 §5.1).
mod content_type {
    pub const CHANGE_CIPHER_SPEC: u8 = 20;
    pub const ALERT: u8 = 21;
    pub const HANDSHAKE: u8 = 22;
    pub const APPLICATION_DATA: u8 = 23;
}

/// TLS handshake message types (RFC 8446 §4).
mod handshake_type {
    pub const CLIENT_HELLO: u8 = 1;
    pub const SERVER_HELLO: u8 = 2;
    pub const NEW_SESSION_TICKET: u8 = 4;
    pub const ENCRYPTED_EXTENSIONS: u8 = 8;
    pub const CERTIFICATE: u8 = 11;
    pub const CERTIFICATE_VERIFY: u8 = 15;
    pub const FINISHED: u8 = 20;
    #[allow(dead_code)]
    pub const KEY_UPDATE: u8 = 24;
}

/// TLS extension types (RFC 8446 §4.2).
mod extension_type {
    pub const SERVER_NAME: usize = 0;
    pub const SUPPORTED_GROUPS: usize = 10;
    pub const SIGNATURE_ALGORITHMS: usize = 13;
    pub const KEY_SHARE: usize = 51;
    pub const SUPPORTED_VERSIONS: usize = 43;
}

/// TLS alert descriptions (RFC 8446 §6).
#[allow(dead_code)] // Protocol constants — not all used yet.
mod alert_desc {
    #[allow(dead_code)]
    pub const CLOSE_NOTIFY: u8 = 0;
    pub const UNEXPECTED_MESSAGE: u8 = 10;
    pub const BAD_RECORD_MAC: u8 = 20;
    pub const HANDSHAKE_FAILURE: u8 = 40;
    #[allow(dead_code)]
    pub const CERTIFICATE_REQUIRED: u8 = 116;
    pub const DECODE_ERROR: u8 = 50;
    #[allow(dead_code)]
    pub const ILLEGAL_PARAMETER: u8 = 47;
    pub const INTERNAL_ERROR: u8 = 80;
}

/// TLS protocol version for record layer (always 0x0303 = TLS 1.2 for
/// compatibility; actual version negotiated via supported_versions extension).
const TLS_RECORD_VERSION: u16 = 0x0303;

/// TLS 1.3 version in supported_versions extension.
const TLS_13_VERSION: usize = 0x0304;

/// Legacy TLS 1.2 version for ClientHello.legacy_version.
const TLS_LEGACY_VERSION: usize = 0x0303;

/// CipherSuite: TLS_CHACHA20_POLY1305_SHA256.
const CIPHER_SUITE: usize = 0x1303;

/// Named group: x25519.
const X25519_GROUP: usize = 0x001D;

/// Signature algorithm: ecdsa_secp256r1_sha256 (required by spec).
const SIG_ECDSA_SECP256R1_SHA256: usize = 0x0403;
/// Signature algorithm: rsa_pss_rsae_sha256.
const SIG_RSA_PSS_RSAE_SHA256: usize = 0x0804;
/// Signature algorithm: ed25519.
const SIG_ED25519: usize = 0x0807;

/// Maximum TLS record plaintext size (16 KiB, per RFC 8446 §5.1).
const MAX_PLAINTEXT_SIZE: usize = 16384;

/// Maximum TLS record ciphertext size (plaintext + 1 content type + 16 tag).
const MAX_CIPHERTEXT_SIZE: usize = MAX_PLAINTEXT_SIZE + 256;

/// AEAD tag length for ChaCha20-Poly1305.
const TAG_LEN: usize = 16;

/// Size of the AEAD nonce (12 bytes for ChaCha20-Poly1305).
const NONCE_LEN: usize = 12;

/// Size of the AEAD key (32 bytes for ChaCha20-Poly1305).
const KEY_LEN: usize = 32;

/// Size of HKDF-SHA256 output.
const HASH_LEN: usize = 32;

/// Default TCP poll timeout for TLS operations.
const TLS_TIMEOUT_POLLS: u32 = 80_000;

// ===========================================================================
// TLS session state
// ===========================================================================

/// TLS 1.3 connection state.
pub struct TlsSession {
    /// Underlying TCP connection handle.
    tcp_handle: usize,
    /// Client write key (for encrypting outgoing data).
    client_write_key: [u8; KEY_LEN],
    /// Server write key (for decrypting incoming data).
    server_write_key: [u8; KEY_LEN],
    /// Client write IV (base nonce for client→server).
    client_write_iv: [u8; NONCE_LEN],
    /// Server write IV (base nonce for server→client).
    server_write_iv: [u8; NONCE_LEN],
    /// Client record sequence number (incremented per record sent).
    client_seq: u64,
    /// Server record sequence number (incremented per record received).
    server_seq: u64,
    /// Buffered plaintext from decrypted records not yet consumed.
    recv_buf: Vec<u8>,
    /// True if the connection has been closed (alert or TCP close).
    closed: bool,
}

// ===========================================================================
// Key schedule (RFC 8446 §7.1)
// ===========================================================================

/// HKDF-Expand-Label as defined in RFC 8446 §7.1.
///
/// ```text
/// HKDF-Expand-Label(Secret, Label, Context, Length) =
///     HKDF-Expand(Secret, HkdfLabel, Length)
/// where HkdfLabel = struct {
///     uint16 length = Length;
///     opaque label<7..255> = "tls13 " + Label;
///     opaque context<0..255> = Context;
/// };
/// ```
fn hkdf_expand_label(
    secret: &[u8; HASH_LEN],
    label: &[u8],
    context: &[u8],
    length: usize,
) -> Vec<u8> {
    // Build HkdfLabel structure.
    let full_label_len = 6 + label.len(); // "tls13 " prefix
    let hkdf_label_len = 2 + 1 + full_label_len + 1 + context.len();
    let mut info = Vec::with_capacity(hkdf_label_len);

    // uint16 length
    info.push((length >> 8) as u8);
    info.push(length as u8);

    // opaque label<7..255> = "tls13 " + Label
    #[allow(clippy::cast_possible_truncation)]
    info.push(full_label_len as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);

    // opaque context<0..255>
    #[allow(clippy::cast_possible_truncation)]
    info.push(context.len() as u8);
    info.extend_from_slice(context);

    crypto::hkdf_expand(secret, &info, length)
}

/// Derive-Secret (RFC 8446 §7.1).
///
/// Derive-Secret(Secret, Label, Messages) =
///     HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), Hash.length)
fn derive_secret(secret: &[u8; HASH_LEN], label: &[u8], transcript_hash: &[u8; HASH_LEN]) -> [u8; HASH_LEN] {
    let expanded = hkdf_expand_label(secret, label, transcript_hash, HASH_LEN);
    let mut out = [0u8; HASH_LEN];
    let len = expanded.len().min(HASH_LEN);
    out[..len].copy_from_slice(expanded.get(..len).unwrap_or(&[]));
    out
}

/// Compute per-record nonce by XORing the base IV with the sequence number.
///
/// RFC 8446 §5.3: The per-record nonce is formed by XORing the IV with
/// the 64-bit record sequence number, padded on the left to nonce length.
fn record_nonce(iv: &[u8; NONCE_LEN], seq: u64) -> [u8; NONCE_LEN] {
    let mut nonce = *iv;
    let seq_bytes = seq.to_be_bytes();
    // XOR the last 8 bytes of the IV with the sequence number.
    for i in 0..8 {
        let idx = NONCE_LEN - 8 + i;
        nonce[idx] ^= seq_bytes[i];
    }
    nonce
}

// ===========================================================================
// Record layer
// ===========================================================================

/// Build a TLS record (unencrypted, for initial handshake messages).
fn build_record(content_type: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len();
    let mut record = Vec::with_capacity(5 + len);
    record.push(content_type);
    record.push((TLS_RECORD_VERSION >> 8) as u8);
    record.push(TLS_RECORD_VERSION as u8);
    record.push((len >> 8) as u8);
    record.push(len as u8);
    record.extend_from_slice(payload);
    record
}

/// Build an encrypted TLS 1.3 record (application_data outer type).
///
/// RFC 8446 §5.2: The encrypted record contains the plaintext + content type
/// byte, encrypted with AEAD.  The outer content type is always
/// application_data (23), with version 0x0303.
fn encrypt_record(
    key: &[u8; KEY_LEN],
    iv: &[u8; NONCE_LEN],
    seq: u64,
    content_type: u8,
    plaintext: &[u8],
) -> Vec<u8> {
    // Inner plaintext = payload || content_type_byte
    let inner_len = plaintext.len() + 1;
    let mut inner = Vec::with_capacity(inner_len);
    inner.extend_from_slice(plaintext);
    inner.push(content_type);

    let nonce = record_nonce(iv, seq);

    // AAD = record header (content_type=23, version=0x0303, length=inner+tag)
    let ciphertext_len = inner_len + TAG_LEN;
    let aad = [
        content_type::APPLICATION_DATA,
        (TLS_RECORD_VERSION >> 8) as u8,
        TLS_RECORD_VERSION as u8,
        (ciphertext_len >> 8) as u8,
        ciphertext_len as u8,
    ];

    // Encrypt with ChaCha20-Poly1305.
    let tag = crypto::chacha20_poly1305_encrypt(key, &nonce, &aad, &mut inner);

    // Build the full record.
    let mut record = Vec::with_capacity(5 + ciphertext_len);
    record.extend_from_slice(&aad);
    record.extend_from_slice(&inner);
    record.extend_from_slice(&tag);
    record
}

/// Decrypt a TLS 1.3 record.
///
/// Returns (content_type, plaintext) on success.
fn decrypt_record(
    key: &[u8; KEY_LEN],
    iv: &[u8; NONCE_LEN],
    seq: u64,
    record: &[u8],
) -> KernelResult<(u8, Vec<u8>)> {
    // Record must be at least header(5) + tag(16) + content_type(1).
    if record.len() < 5 + TAG_LEN + 1 {
        return Err(KernelError::InvalidArgument);
    }

    let header = record.get(..5).ok_or(KernelError::InternalError)?;
    let ciphertext_and_tag = record.get(5..).ok_or(KernelError::InternalError)?;

    if ciphertext_and_tag.len() < TAG_LEN + 1 {
        return Err(KernelError::InvalidArgument);
    }

    let nonce = record_nonce(iv, seq);

    // Split ciphertext and tag.
    let ct_len = ciphertext_and_tag.len() - TAG_LEN;
    let mut ciphertext = Vec::from(ciphertext_and_tag.get(..ct_len).ok_or(KernelError::InternalError)?);
    let tag_slice = ciphertext_and_tag.get(ct_len..).ok_or(KernelError::InternalError)?;
    let mut tag = [0u8; TAG_LEN];
    tag.copy_from_slice(tag_slice);

    // AAD = record header (as received).
    let aad = header;

    // Decrypt and verify.
    let valid = crypto::chacha20_poly1305_decrypt(key, &nonce, aad, &mut ciphertext, &tag);
    if !valid {
        return Err(KernelError::PermissionDenied); // MAC verification failed
    }

    // Remove trailing padding zeros and extract the real content type.
    // RFC 8446 §5.4: strip trailing zeros, the last non-zero byte is the content type.
    let mut real_len = ciphertext.len();
    while real_len > 0 && ciphertext.get(real_len - 1).copied() == Some(0) {
        real_len -= 1;
    }
    if real_len == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let inner_content_type = ciphertext.get(real_len - 1).copied().ok_or(KernelError::InternalError)?;
    ciphertext.truncate(real_len - 1);

    Ok((inner_content_type, ciphertext))
}

// ===========================================================================
// ClientHello construction (RFC 8446 §4.1.2)
// ===========================================================================

/// Build a TLS 1.3 ClientHello message.
///
/// Includes:
/// - supported_versions extension (TLS 1.3 only)
/// - key_share extension (X25519 public key)
/// - supported_groups extension (X25519)
/// - signature_algorithms extension (ecdsa/rsa/ed25519)
/// - server_name extension (SNI)
fn build_client_hello(
    server_name: &str,
    client_random: &[u8; 32],
    x25519_public: &[u8; 32],
) -> Vec<u8> {
    // Build extensions first to know total length.
    let sni_ext = build_sni_extension(server_name);
    let supported_versions_ext = build_supported_versions_extension();
    let supported_groups_ext = build_supported_groups_extension();
    let sig_algs_ext = build_signature_algorithms_extension();
    let key_share_ext = build_key_share_extension(x25519_public);

    let extensions_len = sni_ext.len()
        + supported_versions_ext.len()
        + supported_groups_ext.len()
        + sig_algs_ext.len()
        + key_share_ext.len();

    // ClientHello body:
    //   legacy_version (2) + random (32) + legacy_session_id (1+0) +
    //   cipher_suites (2+2) + legacy_compression (1+1) + extensions (2+N)
    let body_len = 2 + 32 + 1 + (2 + 2) + (1 + 1) + (2 + extensions_len);

    let mut hello = Vec::with_capacity(4 + body_len);

    // Handshake header: type (1) + length (3)
    hello.push(handshake_type::CLIENT_HELLO);
    push_u24(&mut hello, body_len);

    // legacy_version = 0x0303 (TLS 1.2)
    hello.push((TLS_LEGACY_VERSION >> 8) as u8);
    hello.push(TLS_LEGACY_VERSION as u8);

    // random (32 bytes)
    hello.extend_from_slice(client_random);

    // legacy_session_id (empty, length 0)
    hello.push(0);

    // cipher_suites: length (2) + one suite (2)
    hello.push(0);
    hello.push(2);
    hello.push((CIPHER_SUITE >> 8) as u8);
    hello.push(CIPHER_SUITE as u8);

    // legacy_compression_methods: length (1) + null (1)
    hello.push(1);
    hello.push(0);

    // extensions: length (2) + extensions data
    hello.push((extensions_len >> 8) as u8);
    hello.push(extensions_len as u8);
    hello.extend_from_slice(&sni_ext);
    hello.extend_from_slice(&supported_versions_ext);
    hello.extend_from_slice(&supported_groups_ext);
    hello.extend_from_slice(&sig_algs_ext);
    hello.extend_from_slice(&key_share_ext);

    hello
}

/// Build the server_name (SNI) extension.
fn build_sni_extension(name: &str) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    // ServerNameList: length (2) + HostName entry: type (1) + length (2) + name
    let list_len = 1 + 2 + name_len;
    let ext_data_len = 2 + list_len;
    let mut ext = Vec::with_capacity(4 + ext_data_len);
    // Extension type
    push_u16(&mut ext, extension_type::SERVER_NAME);
    // Extension data length
    push_u16(&mut ext, ext_data_len);
    // ServerNameList length
    push_u16(&mut ext, list_len);
    // HostName type (0)
    ext.push(0);
    // HostName length
    push_u16(&mut ext, name_len);
    ext.extend_from_slice(name_bytes);
    ext
}

/// Build the supported_versions extension (client).
fn build_supported_versions_extension() -> Vec<u8> {
    let mut ext = Vec::with_capacity(7);
    push_u16(&mut ext, extension_type::SUPPORTED_VERSIONS);
    push_u16(&mut ext, 3); // Extension data length
    ext.push(2);           // List length (1 version × 2 bytes)
    push_u16(&mut ext, TLS_13_VERSION);
    ext
}

/// Build the supported_groups extension.
fn build_supported_groups_extension() -> Vec<u8> {
    let mut ext = Vec::with_capacity(8);
    push_u16(&mut ext, extension_type::SUPPORTED_GROUPS);
    push_u16(&mut ext, 4); // Extension data length
    push_u16(&mut ext, 2); // NamedGroupList length
    push_u16(&mut ext, X25519_GROUP);
    ext
}

/// Build the signature_algorithms extension.
fn build_signature_algorithms_extension() -> Vec<u8> {
    let mut ext = Vec::with_capacity(12);
    push_u16(&mut ext, extension_type::SIGNATURE_ALGORITHMS);
    push_u16(&mut ext, 8); // Extension data length
    push_u16(&mut ext, 6); // SignatureSchemeList length (3 algorithms × 2 bytes)
    push_u16(&mut ext, SIG_ECDSA_SECP256R1_SHA256);
    push_u16(&mut ext, SIG_RSA_PSS_RSAE_SHA256);
    push_u16(&mut ext, SIG_ED25519);
    ext
}

/// Build the key_share extension (client).
fn build_key_share_extension(x25519_public: &[u8; 32]) -> Vec<u8> {
    // KeyShareEntry: group (2) + key_exchange length (2) + key_exchange (32)
    let entry_len = 2 + 2 + 32;
    let mut ext = Vec::with_capacity(4 + 2 + entry_len);
    push_u16(&mut ext, extension_type::KEY_SHARE);
    push_u16(&mut ext, 2 + entry_len); // Extension data length
    push_u16(&mut ext, entry_len);     // client_shares length
    push_u16(&mut ext, X25519_GROUP);
    push_u16(&mut ext, 32);
    ext.extend_from_slice(x25519_public);
    ext
}

// ===========================================================================
// ServerHello parsing (RFC 8446 §4.1.3)
// ===========================================================================

/// Parsed ServerHello fields we need for the handshake.
struct ServerHello {
    /// Server random — used for TLS 1.2 downgrade detection (RFC 8446 §4.1.3).
    #[allow(dead_code)]
    server_random: [u8; 32],
    cipher_suite: u16,
    /// Server's X25519 public key from key_share extension.
    server_x25519_public: [u8; 32],
}

/// Parse a ServerHello handshake message.
///
/// Returns the parsed fields and the number of bytes consumed.
fn parse_server_hello(data: &[u8]) -> KernelResult<ServerHello> {
    // Minimum ServerHello size: type(1) + length(3) + version(2) + random(32)
    //   + session_id_len(1) + cipher_suite(2) + compression(1) + extensions_len(2)
    if data.len() < 44 {
        return Err(KernelError::InvalidArgument);
    }

    let msg_type = data.first().copied().ok_or(KernelError::InternalError)?;
    if msg_type != handshake_type::SERVER_HELLO {
        crate::serial_println!("[tls] Expected ServerHello (2), got {}", msg_type);
        return Err(KernelError::InvalidArgument);
    }

    let msg_len = read_u24(data, 1)?;
    let body = data.get(4..4 + msg_len).ok_or(KernelError::InvalidArgument)?;

    // legacy_version (2)
    let _version = read_u16(body, 0)?;

    // server_random (32)
    let mut server_random = [0u8; 32];
    server_random.copy_from_slice(body.get(2..34).ok_or(KernelError::InvalidArgument)?);

    // legacy_session_id_echo
    let session_id_len = body.get(34).copied().ok_or(KernelError::InvalidArgument)? as usize;
    let mut offset = 35 + session_id_len;

    // cipher_suite (2)
    let cipher_suite = read_u16(body, offset)?;
    offset += 2;

    // legacy_compression_method (1)
    offset += 1;

    // extensions
    if offset + 2 > body.len() {
        return Err(KernelError::InvalidArgument);
    }
    let extensions_len = read_u16(body, offset)? as usize;
    offset += 2;

    let ext_end = offset + extensions_len;
    if ext_end > body.len() {
        return Err(KernelError::InvalidArgument);
    }

    let mut server_x25519 = [0u8; 32];
    let mut found_key_share = false;

    // Parse extensions to find key_share.
    let mut eoff = offset;
    while eoff + 4 <= ext_end {
        let ext_type = read_u16(body, eoff)?;
        let ext_len = read_u16(body, eoff + 2)? as usize;
        let ext_data = body.get(eoff + 4..eoff + 4 + ext_len)
            .ok_or(KernelError::InvalidArgument)?;
        eoff += 4 + ext_len;

        if ext_type as usize == extension_type::KEY_SHARE {
            // ServerHello key_share: group (2) + key_exchange_length (2) + key_exchange
            if ext_data.len() < 4 {
                return Err(KernelError::InvalidArgument);
            }
            let group = read_u16(ext_data, 0)? as usize;
            let kx_len = read_u16(ext_data, 2)? as usize;
            if group != X25519_GROUP || kx_len != 32 {
                crate::serial_println!("[tls] Unsupported key share group: 0x{:04x}", group);
                return Err(KernelError::NotSupported);
            }
            server_x25519.copy_from_slice(
                ext_data.get(4..36).ok_or(KernelError::InvalidArgument)?
            );
            found_key_share = true;
        }
    }

    if !found_key_share {
        crate::serial_println!("[tls] ServerHello missing key_share extension");
        return Err(KernelError::InvalidArgument);
    }

    Ok(ServerHello {
        server_random,
        cipher_suite,
        server_x25519_public: server_x25519,
    })
}

// ===========================================================================
// TLS 1.3 full handshake
// ===========================================================================

/// Perform a TLS 1.3 handshake over an existing TCP connection.
///
/// Returns a `TlsSession` with traffic keys installed, ready for
/// application data exchange.
///
/// The handshake follows the 1-RTT pattern:
/// ```text
/// Client                              Server
/// ------                              ------
/// ClientHello          -------->
///                                      ServerHello
///                                      {EncryptedExtensions}
///                                      {Certificate}
///                                      {CertificateVerify}
///                      <--------       {Finished}
/// {Finished}           -------->
///
/// [Application Data]   <------->      [Application Data]
/// ```
pub fn tls_connect(tcp_handle: usize, server_name: &str) -> KernelResult<TlsSession> {
    crate::serial_println!("[tls] Starting TLS 1.3 handshake with '{}'", server_name);

    // Generate ephemeral X25519 key pair.
    let client_private = generate_random_bytes();
    let client_public = crypto::x25519_base(&client_private);

    // Generate client random.
    let client_random = generate_random_bytes();

    // --- 1. Send ClientHello ---
    let client_hello_hs = build_client_hello(server_name, &client_random, &client_public);
    let client_hello_record = build_record(content_type::HANDSHAKE, &client_hello_hs);

    super::tcp::send(tcp_handle, &client_hello_record)?;

    // Start transcript hash with ClientHello.
    let mut transcript = TranscriptHash::new();
    transcript.update(&client_hello_hs);

    // --- 2. Receive ServerHello ---
    let server_hello_raw = read_handshake_record(tcp_handle, TLS_TIMEOUT_POLLS)?;
    let server_hello = parse_server_hello(&server_hello_raw)?;

    if server_hello.cipher_suite as usize != CIPHER_SUITE {
        crate::serial_println!(
            "[tls] Server chose unsupported cipher: 0x{:04x}",
            server_hello.cipher_suite,
        );
        return Err(KernelError::NotSupported);
    }

    // Update transcript with ServerHello.
    transcript.update(&server_hello_raw);

    crate::serial_println!("[tls] ServerHello received, cipher=0x{:04x}", server_hello.cipher_suite);

    // --- 3. Compute handshake secrets (RFC 8446 §7.1) ---
    let shared_secret = crypto::x25519(&client_private, &server_hello.server_x25519_public);

    // Check for all-zeros shared secret (low-order point — reject per RFC 7748 §6.1).
    let all_zero = shared_secret.iter().all(|&b| b == 0);
    if all_zero {
        crate::serial_println!("[tls] X25519 shared secret is all zeros — aborting");
        return Err(KernelError::PermissionDenied);
    }

    // Early Secret = HKDF-Extract(salt=0, IKM=0^32)
    let zero_ikm = [0u8; HASH_LEN];
    let zero_salt = [0u8; HASH_LEN];
    let early_secret = crypto::hkdf_extract(&zero_salt, &zero_ikm);

    // Derive-Secret(early_secret, "derived", Hash(""))
    let empty_hash = crypto::sha256(&[]);
    let derived_early = derive_secret(&early_secret, b"derived", &empty_hash);

    // Handshake Secret = HKDF-Extract(salt=derived_early, IKM=shared_secret)
    let handshake_secret = crypto::hkdf_extract(&derived_early, &shared_secret);

    // client/server handshake traffic secrets
    let transcript_hash_ch_sh = transcript.current_hash();
    let client_hs_traffic_secret = derive_secret(
        &handshake_secret, b"c hs traffic", &transcript_hash_ch_sh,
    );
    let server_hs_traffic_secret = derive_secret(
        &handshake_secret, b"s hs traffic", &transcript_hash_ch_sh,
    );

    // Derive handshake traffic keys.
    let server_hs_key = derive_traffic_key(&server_hs_traffic_secret);
    let server_hs_iv = derive_traffic_iv(&server_hs_traffic_secret);
    let client_hs_key = derive_traffic_key(&client_hs_traffic_secret);
    let client_hs_iv = derive_traffic_iv(&client_hs_traffic_secret);

    crate::serial_println!("[tls] Handshake keys derived");

    // --- 4. Read encrypted server handshake messages ---
    // After ServerHello, the server sends encrypted records containing:
    // EncryptedExtensions, Certificate, CertificateVerify, Finished.
    // These are all encrypted with the server handshake key.

    let mut server_hs_seq: u64 = 0;

    // We need to process potentially multiple handshake messages that may
    // arrive in a single TLS record or across multiple records.
    let mut hs_buf = Vec::new();
    let mut got_encrypted_extensions = false;
    let mut got_certificate = false;
    let mut got_cert_verify = false;
    let mut got_finished = false;
    let mut server_finished_data = Vec::new();

    // Some servers send a ChangeCipherSpec for middlebox compatibility.
    // We need to skip it.

    while !got_finished {
        // Read one TLS record.
        let record = read_raw_record(tcp_handle, TLS_TIMEOUT_POLLS)?;
        let record_type = record.first().copied().ok_or(KernelError::InternalError)?;

        // Skip ChangeCipherSpec (compatibility).
        if record_type == content_type::CHANGE_CIPHER_SPEC {
            crate::serial_println!("[tls] Skipping ChangeCipherSpec (compatibility)");
            continue;
        }

        // Must be application_data (encrypted handshake).
        if record_type != content_type::APPLICATION_DATA {
            crate::serial_println!("[tls] Unexpected record type during handshake: {}", record_type);
            return Err(KernelError::InvalidArgument);
        }

        // Decrypt.
        let (inner_type, plaintext) = decrypt_record(
            &server_hs_key, &server_hs_iv, server_hs_seq, &record,
        )?;
        server_hs_seq = server_hs_seq.wrapping_add(1);

        if inner_type == content_type::ALERT {
            let level = plaintext.first().copied().unwrap_or(0);
            let desc = plaintext.get(1).copied().unwrap_or(0);
            crate::serial_println!("[tls] Alert during handshake: level={}, desc={}", level, desc);
            return Err(KernelError::ChannelClosed);
        }

        if inner_type != content_type::HANDSHAKE {
            crate::serial_println!("[tls] Unexpected inner type during handshake: {}", inner_type);
            return Err(KernelError::InvalidArgument);
        }

        // Append to handshake message buffer and process complete messages.
        hs_buf.extend_from_slice(&plaintext);

        // Process all complete handshake messages in the buffer.
        while hs_buf.len() >= 4 {
            let hs_msg_len = read_u24_slice(&hs_buf, 1);
            let total_len = 4 + hs_msg_len;
            if hs_buf.len() < total_len {
                break; // Need more data.
            }

            let hs_type = hs_buf.first().copied().ok_or(KernelError::InternalError)?;
            let msg_data = Vec::from(hs_buf.get(..total_len).ok_or(KernelError::InternalError)?);

            match hs_type {
                handshake_type::ENCRYPTED_EXTENSIONS => {
                    crate::serial_println!("[tls] EncryptedExtensions received");
                    transcript.update(&msg_data);
                    got_encrypted_extensions = true;
                }
                handshake_type::CERTIFICATE => {
                    crate::serial_println!("[tls] Certificate received ({} bytes)", hs_msg_len);
                    transcript.update(&msg_data);
                    got_certificate = true;
                    // NOTE: We don't validate the certificate chain.
                    // This is a known limitation documented at the top of this module.
                }
                handshake_type::CERTIFICATE_VERIFY => {
                    crate::serial_println!("[tls] CertificateVerify received");
                    transcript.update(&msg_data);
                    got_cert_verify = true;
                    // NOTE: We don't verify the signature.
                    // Full certificate verification requires implementing
                    // RSA/ECDSA signature verification, which is future work.
                }
                handshake_type::FINISHED => {
                    // Verify server Finished.
                    let expected_verify = compute_finished_verify(
                        &server_hs_traffic_secret,
                        &transcript.current_hash(),
                    );

                    let received_verify = msg_data.get(4..4 + HASH_LEN)
                        .ok_or(KernelError::InvalidArgument)?;

                    if !constant_time_eq(&expected_verify, received_verify) {
                        crate::serial_println!("[tls] Server Finished verification FAILED");
                        return Err(KernelError::PermissionDenied);
                    }

                    crate::serial_println!("[tls] Server Finished verified");
                    transcript.update(&msg_data);
                    server_finished_data = msg_data;
                    got_finished = true;
                }
                handshake_type::NEW_SESSION_TICKET => {
                    // Session tickets can appear after Finished — skip.
                    crate::serial_println!("[tls] NewSessionTicket received (ignored)");
                    // Don't add to transcript (post-handshake message).
                }
                other => {
                    crate::serial_println!("[tls] Unknown handshake message type: {}", other);
                    transcript.update(&msg_data);
                }
            }

            // Remove processed message from buffer.
            let remaining = Vec::from(hs_buf.get(total_len..).unwrap_or(&[]));
            hs_buf = remaining;
        }
    }

    let _ = server_finished_data; // Used for transcript above.
    let _ = got_encrypted_extensions;
    let _ = got_certificate;
    let _ = got_cert_verify;

    // --- 5. Send client Finished ---
    let client_finished_hash = transcript.current_hash();
    let client_finished_verify = compute_finished_verify(
        &client_hs_traffic_secret, &client_finished_hash,
    );

    // Build Finished handshake message.
    let mut finished_msg = Vec::with_capacity(4 + HASH_LEN);
    finished_msg.push(handshake_type::FINISHED);
    push_u24(&mut finished_msg, HASH_LEN);
    finished_msg.extend_from_slice(&client_finished_verify);

    // Update transcript with client Finished.
    transcript.update(&finished_msg);

    // Encrypt and send.
    let mut client_hs_seq: u64 = 0;
    let finished_record = encrypt_record(
        &client_hs_key, &client_hs_iv, client_hs_seq,
        content_type::HANDSHAKE, &finished_msg,
    );
    client_hs_seq = client_hs_seq.wrapping_add(1);
    let _ = client_hs_seq; // Only one client handshake record.

    super::tcp::send(tcp_handle, &finished_record)?;

    // --- 6. Derive application traffic secrets (RFC 8446 §7.1) ---
    let derived_hs = derive_secret(&handshake_secret, b"derived", &empty_hash);
    let master_secret = crypto::hkdf_extract(&derived_hs, &zero_ikm);

    let transcript_hash_final = transcript.current_hash();
    let client_app_traffic_secret = derive_secret(
        &master_secret, b"c ap traffic", &transcript_hash_final,
    );
    let server_app_traffic_secret = derive_secret(
        &master_secret, b"s ap traffic", &transcript_hash_final,
    );

    let client_write_key = derive_traffic_key(&client_app_traffic_secret);
    let client_write_iv = derive_traffic_iv(&client_app_traffic_secret);
    let server_write_key = derive_traffic_key(&server_app_traffic_secret);
    let server_write_iv = derive_traffic_iv(&server_app_traffic_secret);

    crate::serial_println!("[tls] TLS 1.3 handshake complete — application keys installed");

    Ok(TlsSession {
        tcp_handle,
        client_write_key,
        server_write_key,
        client_write_iv,
        server_write_iv,
        client_seq: 0,
        server_seq: 0,
        recv_buf: Vec::new(),
        closed: false,
    })
}

// ===========================================================================
// Application data send/recv
// ===========================================================================

/// Send application data over the TLS session.
///
/// Fragments into multiple records if needed (max 16 KiB per record).
pub fn tls_send(session: &mut TlsSession, data: &[u8]) -> KernelResult<usize> {
    if session.closed {
        return Err(KernelError::ChannelClosed);
    }

    let mut sent = 0;
    let mut remaining = data;

    while !remaining.is_empty() {
        let chunk_len = remaining.len().min(MAX_PLAINTEXT_SIZE);
        let chunk = remaining.get(..chunk_len).ok_or(KernelError::InternalError)?;

        let record = encrypt_record(
            &session.client_write_key,
            &session.client_write_iv,
            session.client_seq,
            content_type::APPLICATION_DATA,
            chunk,
        );
        session.client_seq = session.client_seq.wrapping_add(1);

        super::tcp::send(session.tcp_handle, &record)?;
        sent += chunk_len;
        remaining = remaining.get(chunk_len..).unwrap_or(&[]);
    }

    Ok(sent)
}

/// Receive and decrypt application data from the TLS session.
///
/// Returns decrypted plaintext bytes.  May return fewer bytes than
/// requested if only a partial record has arrived.  Returns empty
/// Vec if no data is available yet.
pub fn tls_recv(session: &mut TlsSession, max_bytes: usize) -> KernelResult<Vec<u8>> {
    if session.closed {
        return Err(KernelError::ChannelClosed);
    }

    // First, drain any buffered plaintext.
    if !session.recv_buf.is_empty() {
        let take = session.recv_buf.len().min(max_bytes);
        let result = Vec::from(session.recv_buf.get(..take).unwrap_or(&[]));
        let remaining = Vec::from(session.recv_buf.get(take..).unwrap_or(&[]));
        session.recv_buf = remaining;
        return Ok(result);
    }

    // Try to read and decrypt one record.
    let record = match try_read_raw_record(session.tcp_handle) {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()), // No data available yet.
    };

    let record_type = record.first().copied().ok_or(KernelError::InternalError)?;

    // Handle unencrypted alert (shouldn't happen after handshake, but be safe).
    if record_type == content_type::ALERT {
        session.closed = true;
        return Ok(Vec::new());
    }

    // ChangeCipherSpec — ignore.
    if record_type == content_type::CHANGE_CIPHER_SPEC {
        return Ok(Vec::new());
    }

    if record_type != content_type::APPLICATION_DATA {
        crate::serial_println!("[tls] Unexpected record type: {}", record_type);
        session.closed = true;
        return Err(KernelError::InvalidArgument);
    }

    let (inner_type, plaintext) = decrypt_record(
        &session.server_write_key,
        &session.server_write_iv,
        session.server_seq,
        &record,
    )?;
    session.server_seq = session.server_seq.wrapping_add(1);

    match inner_type {
        content_type::APPLICATION_DATA => {
            let take = plaintext.len().min(max_bytes);
            let result = Vec::from(plaintext.get(..take).unwrap_or(&[]));
            if take < plaintext.len() {
                session.recv_buf.extend_from_slice(
                    plaintext.get(take..).unwrap_or(&[])
                );
            }
            Ok(result)
        }
        content_type::ALERT => {
            let _level = plaintext.first().copied().unwrap_or(0);
            let desc = plaintext.get(1).copied().unwrap_or(0);
            if desc == alert_desc::CLOSE_NOTIFY {
                crate::serial_println!("[tls] Received close_notify");
            } else {
                crate::serial_println!("[tls] Alert: desc={}", desc);
            }
            session.closed = true;
            Ok(Vec::new())
        }
        content_type::HANDSHAKE => {
            // Post-handshake messages (e.g., NewSessionTicket, KeyUpdate).
            // For now, just ignore them.
            if let Some(&hs_type) = plaintext.first() {
                if hs_type == handshake_type::NEW_SESSION_TICKET {
                    crate::serial_println!("[tls] Post-handshake NewSessionTicket (ignored)");
                } else if hs_type == handshake_type::KEY_UPDATE {
                    crate::serial_println!("[tls] KeyUpdate received (not implemented, closing)");
                    session.closed = true;
                }
            }
            Ok(Vec::new())
        }
        _ => {
            crate::serial_println!("[tls] Unknown inner content type: {}", inner_type);
            Ok(Vec::new())
        }
    }
}

/// Receive application data, blocking until at least some data arrives
/// or the timeout expires.
pub fn tls_recv_blocking(
    session: &mut TlsSession,
    max_bytes: usize,
    timeout_polls: u32,
) -> KernelResult<Vec<u8>> {
    for _ in 0..timeout_polls {
        let data = tls_recv(session, max_bytes)?;
        if !data.is_empty() {
            return Ok(data);
        }
        if session.closed {
            return Ok(Vec::new());
        }
        super::super::net::poll();
        core::hint::spin_loop();
    }
    // Timeout — return whatever we have.
    tls_recv(session, max_bytes)
}

/// Close the TLS session by sending a close_notify alert.
pub fn tls_close(session: &mut TlsSession) -> KernelResult<()> {
    if !session.closed {
        // Send close_notify alert.
        let alert_corrected = [1u8, alert_desc::CLOSE_NOTIFY]; // warning level
        let record = encrypt_record(
            &session.client_write_key,
            &session.client_write_iv,
            session.client_seq,
            content_type::ALERT,
            &alert_corrected,
        );
        session.client_seq = session.client_seq.wrapping_add(1);
        let _ = super::tcp::send(session.tcp_handle, &record);
        session.closed = true;
    }
    super::tcp::close(session.tcp_handle)
}

// ===========================================================================
// TLS 1.3 server
// ===========================================================================

/// TLS 1.3 server session.
///
/// Same structure as `TlsSession` but created via `tls_accept()` instead
/// of `tls_connect()`.  The send/recv/close functions work identically;
/// the only difference is which key is "ours" (server writes with
/// server_write_key, reads with client_write_key).
pub struct TlsServerSession {
    /// Underlying TCP connection handle.
    tcp_handle: usize,
    /// Server write key (for encrypting outgoing data).
    server_write_key: [u8; KEY_LEN],
    /// Client write key (for decrypting incoming data).
    client_write_key: [u8; KEY_LEN],
    /// Server write IV.
    server_write_iv: [u8; NONCE_LEN],
    /// Client write IV.
    client_write_iv: [u8; NONCE_LEN],
    /// Server send sequence number.
    server_seq: u64,
    /// Client receive sequence number.
    client_seq: u64,
    /// Buffered plaintext from decrypted records not yet consumed.
    recv_buf: Vec<u8>,
    /// True if the connection has been closed.
    closed: bool,
}

/// Parsed fields from a ClientHello needed for the server handshake.
struct ParsedClientHello {
    /// Client random (32 bytes).
    client_random: [u8; 32],
    /// Client's X25519 public key from key_share extension.
    client_x25519_public: [u8; 32],
    /// True if the client offered `TLS_CHACHA20_POLY1305_SHA256`.
    has_our_cipher: bool,
    /// True if the client offered TLS 1.3 in supported_versions.
    has_tls13: bool,
    /// True if the client offered ed25519 in signature_algorithms.
    has_ed25519_sig: bool,
}

/// Parse a ClientHello handshake message (server side).
fn parse_client_hello(data: &[u8]) -> KernelResult<ParsedClientHello> {
    // Handshake header: type (1) + length (3)
    if data.len() < 4 {
        return Err(KernelError::InvalidArgument);
    }

    let msg_type = data.first().copied().ok_or(KernelError::InternalError)?;
    if msg_type != handshake_type::CLIENT_HELLO {
        return Err(KernelError::InvalidArgument);
    }

    let msg_len = read_u24(data, 1)?;
    let body = data.get(4..4 + msg_len).ok_or(KernelError::InvalidArgument)?;

    // legacy_version (2)
    let mut offset = 2;

    // client_random (32)
    if offset + 32 > body.len() {
        return Err(KernelError::InvalidArgument);
    }
    let mut client_random = [0u8; 32];
    client_random.copy_from_slice(body.get(offset..offset + 32).ok_or(KernelError::InvalidArgument)?);
    offset += 32;

    // legacy_session_id (variable)
    let session_id_len = body.get(offset).copied().ok_or(KernelError::InvalidArgument)? as usize;
    offset += 1 + session_id_len;

    // cipher_suites
    if offset + 2 > body.len() {
        return Err(KernelError::InvalidArgument);
    }
    let cs_len = read_u16(body, offset)? as usize;
    offset += 2;
    let cs_end = offset + cs_len;

    let mut has_our_cipher = false;
    let mut cs_off = offset;
    while cs_off + 2 <= cs_end {
        let suite = read_u16(body, cs_off)?;
        if suite as usize == CIPHER_SUITE {
            has_our_cipher = true;
        }
        cs_off += 2;
    }
    offset = cs_end;

    // legacy_compression_methods
    let comp_len = body.get(offset).copied().ok_or(KernelError::InvalidArgument)? as usize;
    offset += 1 + comp_len;

    // extensions
    if offset + 2 > body.len() {
        return Err(KernelError::InvalidArgument);
    }
    let ext_total = read_u16(body, offset)? as usize;
    offset += 2;
    let ext_end = offset + ext_total;

    let mut client_x25519_public = [0u8; 32];
    let mut found_key_share = false;
    let mut has_tls13 = false;
    let mut has_ed25519_sig = false;

    while offset + 4 <= ext_end {
        let ext_type = read_u16(body, offset)? as usize;
        let ext_len = read_u16(body, offset + 2)? as usize;
        let ext_data = body.get(offset + 4..offset + 4 + ext_len)
            .ok_or(KernelError::InvalidArgument)?;
        offset += 4 + ext_len;

        match ext_type {
            extension_type::KEY_SHARE => {
                // ClientHello key_share: client_shares_length (2) + entries
                if ext_data.len() < 2 {
                    continue;
                }
                let shares_len = read_u16(ext_data, 0)? as usize;
                let mut soff = 2usize;
                let shares_end = 2 + shares_len;
                while soff + 4 <= shares_end && soff + 4 <= ext_data.len() {
                    let group = read_u16(ext_data, soff)? as usize;
                    let kx_len = read_u16(ext_data, soff + 2)? as usize;
                    soff += 4;
                    if group == X25519_GROUP && kx_len == 32 && soff + 32 <= ext_data.len() {
                        client_x25519_public.copy_from_slice(
                            ext_data.get(soff..soff + 32).ok_or(KernelError::InvalidArgument)?
                        );
                        found_key_share = true;
                    }
                    soff += kx_len;
                }
            }
            extension_type::SUPPORTED_VERSIONS => {
                // Client supported_versions: list_len (1) + version entries
                if ext_data.is_empty() {
                    continue;
                }
                let list_len = ext_data.first().copied().ok_or(KernelError::InvalidArgument)? as usize;
                let mut voff = 1usize;
                let vend = 1 + list_len;
                while voff + 2 <= vend && voff + 2 <= ext_data.len() {
                    let ver = read_u16(ext_data, voff)? as usize;
                    if ver == TLS_13_VERSION {
                        has_tls13 = true;
                    }
                    voff += 2;
                }
            }
            extension_type::SIGNATURE_ALGORITHMS => {
                // list_len (2) + algorithm entries
                if ext_data.len() < 2 {
                    continue;
                }
                let list_len = read_u16(ext_data, 0)? as usize;
                let mut aoff = 2usize;
                let aend = 2 + list_len;
                while aoff + 2 <= aend && aoff + 2 <= ext_data.len() {
                    let alg = read_u16(ext_data, aoff)? as usize;
                    if alg == SIG_ED25519 {
                        has_ed25519_sig = true;
                    }
                    aoff += 2;
                }
            }
            _ => {} // Ignore other extensions.
        }
    }

    if !found_key_share {
        return Err(KernelError::InvalidArgument);
    }

    Ok(ParsedClientHello {
        client_random,
        client_x25519_public,
        has_our_cipher,
        has_tls13,
        has_ed25519_sig,
    })
}

/// Build a self-signed Ed25519 certificate for TLS 1.3.
///
/// Creates a minimal DER-encoded X.509v3 certificate using Ed25519.
/// This is a self-signed certificate — clients must either skip
/// verification or pin the server's public key.
///
/// The certificate contains: issuer/subject CN="MintOS", Ed25519 public
/// key, Ed25519 signature over the TBSCertificate.
fn build_self_signed_certificate(seed: &[u8; 32], public_key: &[u8; 32]) -> Vec<u8> {
    // We build a minimal DER-encoded X.509 certificate.
    // The structure is simplified but valid enough for TLS 1.3.

    // SubjectPublicKeyInfo for Ed25519: OID 1.3.101.112
    let spki = [
        0x30, 0x2A, // SEQUENCE, 42 bytes
        0x30, 0x05, // SEQUENCE (algorithm), 5 bytes
        0x06, 0x03, 0x2B, 0x65, 0x70, // OID 1.3.101.112 (Ed25519)
        0x03, 0x21, 0x00, // BIT STRING, 33 bytes (1 unused-bits byte + 32 key)
    ];

    // Simple CN=MintOS in DER
    let cn_value = b"MintOS";
    let cn_set: [u8; 4] = [
        0x06, 0x03, 0x55, 0x04, // OID 2.5.4.3 (commonName)
    ];

    // Build issuer/subject Name
    let mut name = Vec::with_capacity(32);
    // SET
    let inner_seq_len = cn_set.len() + 1 + 1 + cn_value.len();
    let set_len = 2 + inner_seq_len;
    name.push(0x31); // SET
    push_der_len(&mut name, set_len);
    name.push(0x30); // SEQUENCE
    push_der_len(&mut name, inner_seq_len);
    name.extend_from_slice(&cn_set);
    name.push(0x03); // attributeType is OID, this is length of value
    name.push(0x0C); // UTF8String
    #[allow(clippy::cast_possible_truncation)]
    { name.push(cn_value.len() as u8); }
    name.extend_from_slice(cn_value);

    // Serial number (random 8 bytes)
    let mut serial = [0u8; 8];
    crate::rng::fill(&mut serial);
    serial[0] &= 0x7F; // Ensure positive.

    // Validity: not before 2024-01-01, not after 2034-12-31
    let not_before = b"\x17\x0D241231000000Z"; // UTCTime
    let not_after  = b"\x17\x0D341231235959Z";

    // Build TBSCertificate
    let mut tbs = Vec::with_capacity(256);

    // version [0] EXPLICIT INTEGER 2 (v3)
    tbs.extend_from_slice(&[0xA0, 0x03, 0x02, 0x01, 0x02]);

    // serialNumber
    tbs.push(0x02); // INTEGER
    #[allow(clippy::cast_possible_truncation)]
    { tbs.push(serial.len() as u8); }
    tbs.extend_from_slice(&serial);

    // signature algorithm: Ed25519 (OID 1.3.101.112)
    tbs.extend_from_slice(&[0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70]);

    // issuer
    tbs.push(0x30); // SEQUENCE
    push_der_len(&mut tbs, name.len());
    tbs.extend_from_slice(&name);

    // validity
    let validity_len = not_before.len() + not_after.len();
    tbs.push(0x30);
    push_der_len(&mut tbs, validity_len);
    tbs.extend_from_slice(not_before);
    tbs.extend_from_slice(not_after);

    // subject (same as issuer for self-signed)
    tbs.push(0x30);
    push_der_len(&mut tbs, name.len());
    tbs.extend_from_slice(&name);

    // subjectPublicKeyInfo
    tbs.extend_from_slice(&spki);
    tbs.extend_from_slice(public_key);

    // Sign the TBSCertificate.
    let tbs_for_signing = {
        let mut wrapped = Vec::with_capacity(4 + tbs.len());
        wrapped.push(0x30); // SEQUENCE
        push_der_len(&mut wrapped, tbs.len());
        wrapped.extend_from_slice(&tbs);
        wrapped
    };
    let signature = crypto::ed25519_sign(seed, &tbs_for_signing);

    // Build full Certificate
    let sig_alg = [0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70]; // Ed25519
    let sig_bitstring_len = 1 + signature.len(); // unused-bits byte + signature

    let inner_len = tbs_for_signing.len() + sig_alg.len() + 2 + sig_bitstring_len;
    // 2 = tag + length for BIT STRING header (may need multi-byte length)

    let mut cert = Vec::with_capacity(4 + inner_len);
    cert.push(0x30); // SEQUENCE
    push_der_len(&mut cert, tbs_for_signing.len() + sig_alg.len() + 2 + sig_bitstring_len);

    cert.extend_from_slice(&tbs_for_signing);
    cert.extend_from_slice(&sig_alg);

    // signatureValue BIT STRING
    cert.push(0x03); // BIT STRING
    push_der_len(&mut cert, sig_bitstring_len);
    cert.push(0x00); // 0 unused bits
    cert.extend_from_slice(&signature);

    cert
}

/// Push a DER length encoding.
fn push_der_len(buf: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        #[allow(clippy::cast_possible_truncation)]
        buf.push(len as u8);
    } else if len < 0x100 {
        buf.push(0x81);
        #[allow(clippy::cast_possible_truncation)]
        buf.push(len as u8);
    } else {
        buf.push(0x82);
        buf.push((len >> 8) as u8);
        buf.push(len as u8);
    }
}

/// Build the server's EncryptedExtensions handshake message.
///
/// Minimal: no extensions (just empty extension list).
fn build_encrypted_extensions() -> Vec<u8> {
    let mut msg = Vec::with_capacity(6);
    msg.push(handshake_type::ENCRYPTED_EXTENSIONS);
    push_u24(&mut msg, 2); // Length: just the extensions length field
    push_u16(&mut msg, 0); // No extensions
    msg
}

/// Build a TLS 1.3 Certificate handshake message.
///
/// Contains a single certificate entry (our self-signed Ed25519 cert).
fn build_certificate_message(cert_der: &[u8]) -> Vec<u8> {
    // Certificate message body:
    //   certificate_request_context (1 byte: length 0)
    //   certificate_list (3-byte length + entries)
    //     entry: cert_data (3-byte length + DER) + extensions (2-byte length: 0)

    let entry_len = 3 + cert_der.len() + 2; // cert_data + extensions
    let list_len = entry_len;
    let body_len = 1 + 3 + list_len; // context + list_length + list

    let mut msg = Vec::with_capacity(4 + body_len);
    msg.push(handshake_type::CERTIFICATE);
    push_u24(&mut msg, body_len);

    // certificate_request_context (empty)
    msg.push(0);

    // certificate_list
    push_u24(&mut msg, list_len);

    // cert_data
    push_u24(&mut msg, cert_der.len());
    msg.extend_from_slice(cert_der);

    // extensions (empty)
    push_u16(&mut msg, 0);

    msg
}

/// Build a TLS 1.3 CertificateVerify handshake message (Ed25519).
///
/// Signs the transcript hash with the server's Ed25519 key.
/// RFC 8446 §4.4.3: The signature is over:
///   64 × 0x20 + "TLS 1.3, server CertificateVerify" + 0x00 + transcript_hash
fn build_certificate_verify(
    seed: &[u8; 32],
    transcript_hash: &[u8; HASH_LEN],
) -> Vec<u8> {
    // Build the content to sign.
    let mut to_sign = Vec::with_capacity(64 + 34 + 1 + HASH_LEN);
    to_sign.extend_from_slice(&[0x20u8; 64]); // 64 spaces
    to_sign.extend_from_slice(b"TLS 1.3, server CertificateVerify");
    to_sign.push(0x00);
    to_sign.extend_from_slice(transcript_hash);

    let signature = crypto::ed25519_sign(seed, &to_sign);

    // CertificateVerify message: algorithm (2) + signature (2-byte len + data)
    let body_len = 2 + 2 + signature.len();
    let mut msg = Vec::with_capacity(4 + body_len);
    msg.push(handshake_type::CERTIFICATE_VERIFY);
    push_u24(&mut msg, body_len);

    // SignatureScheme: ed25519 (0x0807)
    push_u16(&mut msg, SIG_ED25519);

    // Signature
    push_u16(&mut msg, signature.len());
    msg.extend_from_slice(&signature);

    msg
}

/// Build a ServerHello handshake message (server side).
fn build_server_hello(
    server_random: &[u8; 32],
    x25519_public: &[u8; 32],
) -> Vec<u8> {
    // Extensions: supported_versions (server) + key_share (server)
    let sv_ext = build_server_supported_versions_extension();
    let ks_ext = build_server_key_share_extension(x25519_public);
    let extensions_len = sv_ext.len() + ks_ext.len();

    // ServerHello body:
    //   legacy_version (2) + random (32) + legacy_session_id_echo (1+0)
    //   + cipher_suite (2) + legacy_compression (1) + extensions (2+N)
    let body_len = 2 + 32 + 1 + 2 + 1 + 2 + extensions_len;

    let mut hello = Vec::with_capacity(4 + body_len);
    hello.push(handshake_type::SERVER_HELLO);
    push_u24(&mut hello, body_len);

    // legacy_version = 0x0303
    hello.push((TLS_LEGACY_VERSION >> 8) as u8);
    hello.push(TLS_LEGACY_VERSION as u8);

    // server_random
    hello.extend_from_slice(server_random);

    // legacy_session_id_echo (empty)
    hello.push(0);

    // cipher_suite
    hello.push((CIPHER_SUITE >> 8) as u8);
    hello.push(CIPHER_SUITE as u8);

    // legacy_compression_method: null
    hello.push(0);

    // extensions
    push_u16(&mut hello, extensions_len);
    hello.extend_from_slice(&sv_ext);
    hello.extend_from_slice(&ks_ext);

    hello
}

/// Build the supported_versions extension for ServerHello.
fn build_server_supported_versions_extension() -> Vec<u8> {
    let mut ext = Vec::with_capacity(6);
    push_u16(&mut ext, extension_type::SUPPORTED_VERSIONS);
    push_u16(&mut ext, 2); // Extension data length
    push_u16(&mut ext, TLS_13_VERSION);
    ext
}

/// Build the key_share extension for ServerHello.
fn build_server_key_share_extension(x25519_public: &[u8; 32]) -> Vec<u8> {
    let mut ext = Vec::with_capacity(40);
    push_u16(&mut ext, extension_type::KEY_SHARE);
    push_u16(&mut ext, 2 + 2 + 32); // group + kx_len + key
    push_u16(&mut ext, X25519_GROUP);
    push_u16(&mut ext, 32);
    ext.extend_from_slice(x25519_public);
    ext
}

/// Accept a TLS 1.3 connection on an existing TCP handle.
///
/// Performs the server-side TLS 1.3 handshake using Ed25519 for
/// certificate signing and X25519 for key exchange.
///
/// ```text
/// Client                              Server
/// ------                              ------
/// ClientHello          -------->
///                                      ServerHello
///                                      {EncryptedExtensions}
///                                      {Certificate}
///                                      {CertificateVerify}
///                      <--------       {Finished}
/// {Finished}           -------->
///
/// [Application Data]   <------->      [Application Data]
/// ```
///
/// # Arguments
///
/// * `tcp_handle` — accepted TCP connection (from `tcp::accept`)
/// * `ed25519_seed` — 32-byte Ed25519 private key seed
/// * `ed25519_public` — corresponding 32-byte public key
pub fn tls_accept(
    tcp_handle: usize,
    ed25519_seed: &[u8; 32],
    ed25519_public: &[u8; 32],
) -> KernelResult<TlsServerSession> {
    crate::serial_println!("[tls] Server: waiting for ClientHello...");

    // --- 1. Receive ClientHello ---
    let client_hello_raw = read_handshake_record(tcp_handle, TLS_TIMEOUT_POLLS)?;
    let ch = parse_client_hello(&client_hello_raw)?;

    if !ch.has_tls13 {
        crate::serial_println!("[tls] Server: client doesn't support TLS 1.3");
        return Err(KernelError::NotSupported);
    }
    if !ch.has_our_cipher {
        crate::serial_println!("[tls] Server: client doesn't support ChaCha20-Poly1305");
        return Err(KernelError::NotSupported);
    }

    crate::serial_println!("[tls] Server: ClientHello received (tls13={}, cipher={}, ed25519={})",
        ch.has_tls13, ch.has_our_cipher, ch.has_ed25519_sig);

    // Start transcript with ClientHello.
    let mut transcript = TranscriptHash::new();
    transcript.update(&client_hello_raw);

    // --- 2. Generate ephemeral X25519 key pair and send ServerHello ---
    let server_random = generate_random_bytes();
    let server_x25519_private = generate_random_bytes();
    let server_x25519_public = crypto::x25519_base(&server_x25519_private);

    let server_hello_hs = build_server_hello(&server_random, &server_x25519_public);
    let server_hello_record = build_record(content_type::HANDSHAKE, &server_hello_hs);
    super::tcp::send(tcp_handle, &server_hello_record)?;

    transcript.update(&server_hello_hs);

    // Optional: send ChangeCipherSpec for middlebox compatibility.
    let ccs_record = build_record(content_type::CHANGE_CIPHER_SPEC, &[1]);
    super::tcp::send(tcp_handle, &ccs_record)?;

    // --- 3. Derive handshake secrets ---
    let shared_secret = crypto::x25519(&server_x25519_private, &ch.client_x25519_public);

    let all_zero = shared_secret.iter().all(|&b| b == 0);
    if all_zero {
        crate::serial_println!("[tls] Server: X25519 shared secret is all zeros — aborting");
        return Err(KernelError::PermissionDenied);
    }

    let zero_ikm = [0u8; HASH_LEN];
    let zero_salt = [0u8; HASH_LEN];
    let early_secret = crypto::hkdf_extract(&zero_salt, &zero_ikm);
    let empty_hash = crypto::sha256(&[]);
    let derived_early = derive_secret(&early_secret, b"derived", &empty_hash);
    let handshake_secret = crypto::hkdf_extract(&derived_early, &shared_secret);

    let transcript_hash_ch_sh = transcript.current_hash();
    let client_hs_traffic_secret = derive_secret(
        &handshake_secret, b"c hs traffic", &transcript_hash_ch_sh,
    );
    let server_hs_traffic_secret = derive_secret(
        &handshake_secret, b"s hs traffic", &transcript_hash_ch_sh,
    );

    let server_hs_key = derive_traffic_key(&server_hs_traffic_secret);
    let server_hs_iv = derive_traffic_iv(&server_hs_traffic_secret);
    let client_hs_key = derive_traffic_key(&client_hs_traffic_secret);
    let client_hs_iv = derive_traffic_iv(&client_hs_traffic_secret);

    crate::serial_println!("[tls] Server: handshake keys derived");

    // --- 4. Send encrypted handshake messages ---
    let mut server_hs_seq: u64 = 0;

    // EncryptedExtensions
    let ee_msg = build_encrypted_extensions();
    transcript.update(&ee_msg);
    let ee_record = encrypt_record(
        &server_hs_key, &server_hs_iv, server_hs_seq,
        content_type::HANDSHAKE, &ee_msg,
    );
    server_hs_seq = server_hs_seq.wrapping_add(1);
    super::tcp::send(tcp_handle, &ee_record)?;

    // Certificate
    let cert_der = build_self_signed_certificate(ed25519_seed, ed25519_public);
    let cert_msg = build_certificate_message(&cert_der);
    transcript.update(&cert_msg);
    let cert_record = encrypt_record(
        &server_hs_key, &server_hs_iv, server_hs_seq,
        content_type::HANDSHAKE, &cert_msg,
    );
    server_hs_seq = server_hs_seq.wrapping_add(1);
    super::tcp::send(tcp_handle, &cert_record)?;

    // CertificateVerify
    let cv_transcript = transcript.current_hash();
    let cv_msg = build_certificate_verify(ed25519_seed, &cv_transcript);
    transcript.update(&cv_msg);
    let cv_record = encrypt_record(
        &server_hs_key, &server_hs_iv, server_hs_seq,
        content_type::HANDSHAKE, &cv_msg,
    );
    server_hs_seq = server_hs_seq.wrapping_add(1);
    super::tcp::send(tcp_handle, &cv_record)?;

    // Server Finished
    let server_finished_hash = transcript.current_hash();
    let server_finished_verify = compute_finished_verify(
        &server_hs_traffic_secret, &server_finished_hash,
    );
    let mut finished_msg = Vec::with_capacity(4 + HASH_LEN);
    finished_msg.push(handshake_type::FINISHED);
    push_u24(&mut finished_msg, HASH_LEN);
    finished_msg.extend_from_slice(&server_finished_verify);
    transcript.update(&finished_msg);

    let finished_record = encrypt_record(
        &server_hs_key, &server_hs_iv, server_hs_seq,
        content_type::HANDSHAKE, &finished_msg,
    );
    let _ = server_hs_seq.wrapping_add(1);
    super::tcp::send(tcp_handle, &finished_record)?;

    crate::serial_println!("[tls] Server: sent EE + Certificate + CertVerify + Finished");

    // --- 5. Receive client Finished ---
    let mut client_hs_seq: u64 = 0;
    let mut got_client_finished = false;

    while !got_client_finished {
        let record = read_raw_record(tcp_handle, TLS_TIMEOUT_POLLS)?;
        let record_type = record.first().copied().ok_or(KernelError::InternalError)?;

        // Skip ChangeCipherSpec.
        if record_type == content_type::CHANGE_CIPHER_SPEC {
            continue;
        }

        if record_type != content_type::APPLICATION_DATA {
            crate::serial_println!("[tls] Server: unexpected record type {}", record_type);
            return Err(KernelError::InvalidArgument);
        }

        let (inner_type, plaintext) = decrypt_record(
            &client_hs_key, &client_hs_iv, client_hs_seq, &record,
        )?;
        client_hs_seq = client_hs_seq.wrapping_add(1);

        if inner_type == content_type::ALERT {
            let desc = plaintext.get(1).copied().unwrap_or(0);
            crate::serial_println!("[tls] Server: client alert during handshake: {}", desc);
            return Err(KernelError::ChannelClosed);
        }

        if inner_type != content_type::HANDSHAKE {
            crate::serial_println!("[tls] Server: unexpected inner type {}", inner_type);
            return Err(KernelError::InvalidArgument);
        }

        // Parse handshake message.
        if plaintext.len() < 4 {
            return Err(KernelError::InvalidArgument);
        }
        let hs_type = plaintext.first().copied().ok_or(KernelError::InternalError)?;
        if hs_type != handshake_type::FINISHED {
            crate::serial_println!("[tls] Server: expected Finished (20), got {}", hs_type);
            return Err(KernelError::InvalidArgument);
        }

        // Verify client Finished.
        let expected_verify = compute_finished_verify(
            &client_hs_traffic_secret,
            &transcript.current_hash(),
        );
        let received_verify = plaintext.get(4..4 + HASH_LEN)
            .ok_or(KernelError::InvalidArgument)?;

        if !constant_time_eq(&expected_verify, received_verify) {
            crate::serial_println!("[tls] Server: client Finished verification FAILED");
            return Err(KernelError::PermissionDenied);
        }

        transcript.update(&plaintext);
        got_client_finished = true;
    }

    crate::serial_println!("[tls] Server: client Finished verified");

    // --- 6. Derive application traffic secrets ---
    let derived_hs = derive_secret(&handshake_secret, b"derived", &empty_hash);
    let master_secret = crypto::hkdf_extract(&derived_hs, &zero_ikm);

    let transcript_hash_final = transcript.current_hash();
    let client_app_traffic_secret = derive_secret(
        &master_secret, b"c ap traffic", &transcript_hash_final,
    );
    let server_app_traffic_secret = derive_secret(
        &master_secret, b"s ap traffic", &transcript_hash_final,
    );

    let server_write_key = derive_traffic_key(&server_app_traffic_secret);
    let server_write_iv = derive_traffic_iv(&server_app_traffic_secret);
    let client_write_key = derive_traffic_key(&client_app_traffic_secret);
    let client_write_iv = derive_traffic_iv(&client_app_traffic_secret);

    crate::serial_println!("[tls] Server: TLS 1.3 handshake complete — application keys installed");

    Ok(TlsServerSession {
        tcp_handle,
        server_write_key,
        client_write_key,
        server_write_iv,
        client_write_iv,
        server_seq: 0,
        client_seq: 0,
        recv_buf: Vec::new(),
        closed: false,
    })
}

/// Send application data on a TLS server session.
pub fn tls_server_send(session: &mut TlsServerSession, data: &[u8]) -> KernelResult<usize> {
    if session.closed {
        return Err(KernelError::ChannelClosed);
    }

    let mut sent: usize = 0;
    let mut remaining = data;

    while !remaining.is_empty() {
        let chunk_len = remaining.len().min(MAX_PLAINTEXT_SIZE);
        let chunk = remaining.get(..chunk_len).ok_or(KernelError::InternalError)?;

        let record = encrypt_record(
            &session.server_write_key,
            &session.server_write_iv,
            session.server_seq,
            content_type::APPLICATION_DATA,
            chunk,
        );
        session.server_seq = session.server_seq.wrapping_add(1);

        super::tcp::send(session.tcp_handle, &record)?;
        sent = sent.saturating_add(chunk_len);
        remaining = remaining.get(chunk_len..).unwrap_or(&[]);
    }

    Ok(sent)
}

/// Receive application data from a TLS server session.
pub fn tls_server_recv(session: &mut TlsServerSession, max_bytes: usize) -> KernelResult<Vec<u8>> {
    if session.closed {
        return Err(KernelError::ChannelClosed);
    }

    // Return buffered data first.
    if !session.recv_buf.is_empty() {
        let take = session.recv_buf.len().min(max_bytes);
        let result = Vec::from(session.recv_buf.get(..take).unwrap_or(&[]));
        let remaining = Vec::from(session.recv_buf.get(take..).unwrap_or(&[]));
        session.recv_buf = remaining;
        return Ok(result);
    }

    // Try to read a record.
    let record = match try_read_raw_record(session.tcp_handle) {
        Ok(r) => r,
        Err(KernelError::WouldBlock) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let record_type = record.first().copied().ok_or(KernelError::InternalError)?;

    // Skip ChangeCipherSpec (shouldn't appear after handshake, but be safe).
    if record_type == content_type::CHANGE_CIPHER_SPEC {
        return Ok(Vec::new());
    }

    if record_type != content_type::APPLICATION_DATA {
        return Err(KernelError::InvalidArgument);
    }

    let (inner_type, plaintext) = decrypt_record(
        &session.client_write_key,
        &session.client_write_iv,
        session.client_seq,
        &record,
    )?;
    session.client_seq = session.client_seq.wrapping_add(1);

    match inner_type {
        content_type::APPLICATION_DATA => {
            let take = plaintext.len().min(max_bytes);
            let result = Vec::from(plaintext.get(..take).unwrap_or(&[]));
            if take < plaintext.len() {
                session.recv_buf.extend_from_slice(
                    plaintext.get(take..).unwrap_or(&[])
                );
            }
            Ok(result)
        }
        content_type::ALERT => {
            let desc = plaintext.get(1).copied().unwrap_or(0);
            if desc == alert_desc::CLOSE_NOTIFY {
                session.closed = true;
                return Err(KernelError::ChannelClosed);
            }
            crate::serial_println!("[tls] Server: received alert {}", desc);
            session.closed = true;
            Err(KernelError::ChannelClosed)
        }
        content_type::HANDSHAKE => {
            // Post-handshake messages (NewSessionTicket, KeyUpdate).
            // Ignore for now.
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

/// Close a TLS server session.
pub fn tls_server_close(session: &mut TlsServerSession) -> KernelResult<()> {
    if !session.closed {
        let alert = [1u8, alert_desc::CLOSE_NOTIFY];
        let record = encrypt_record(
            &session.server_write_key,
            &session.server_write_iv,
            session.server_seq,
            content_type::ALERT,
            &alert,
        );
        session.server_seq = session.server_seq.wrapping_add(1);
        let _ = super::tcp::send(session.tcp_handle, &record);
        session.closed = true;
    }
    super::tcp::close(session.tcp_handle)
}

// ===========================================================================
// Transcript hash
// ===========================================================================

/// Running SHA-256 transcript hash of all handshake messages.
///
/// TLS 1.3 computes a running hash of all handshake messages for key
/// derivation and Finished verification.  We use a simple approach:
/// accumulate all messages and hash from scratch each time.
///
/// For a production implementation, this would use incremental SHA-256.
/// Our message volume is small enough that re-hashing is acceptable.
struct TranscriptHash {
    messages: Vec<u8>,
}

impl TranscriptHash {
    fn new() -> Self {
        Self { messages: Vec::new() }
    }

    fn update(&mut self, handshake_msg: &[u8]) {
        self.messages.extend_from_slice(handshake_msg);
    }

    fn current_hash(&self) -> [u8; HASH_LEN] {
        crypto::sha256(&self.messages)
    }
}

// ===========================================================================
// Finished message computation
// ===========================================================================

/// Compute the verify_data for a Finished message (RFC 8446 §4.4.4).
///
/// finished_key = HKDF-Expand-Label(BaseKey, "finished", "", Hash.length)
/// verify_data  = HMAC(finished_key, Transcript-Hash(…))
fn compute_finished_verify(
    base_key: &[u8; HASH_LEN],
    transcript_hash: &[u8; HASH_LEN],
) -> [u8; HASH_LEN] {
    let finished_key_vec = hkdf_expand_label(base_key, b"finished", &[], HASH_LEN);
    let mut finished_key = [0u8; HASH_LEN];
    let fk_len = finished_key_vec.len().min(HASH_LEN);
    finished_key[..fk_len].copy_from_slice(finished_key_vec.get(..fk_len).unwrap_or(&[]));

    
    crypto::hmac_sha256(&finished_key, transcript_hash)
}

/// Derive traffic key from a traffic secret.
fn derive_traffic_key(secret: &[u8; HASH_LEN]) -> [u8; KEY_LEN] {
    let expanded = hkdf_expand_label(secret, b"key", &[], KEY_LEN);
    let mut key = [0u8; KEY_LEN];
    let len = expanded.len().min(KEY_LEN);
    key[..len].copy_from_slice(expanded.get(..len).unwrap_or(&[]));
    key
}

/// Derive traffic IV from a traffic secret.
fn derive_traffic_iv(secret: &[u8; HASH_LEN]) -> [u8; NONCE_LEN] {
    let expanded = hkdf_expand_label(secret, b"iv", &[], NONCE_LEN);
    let mut iv = [0u8; NONCE_LEN];
    let len = expanded.len().min(NONCE_LEN);
    iv[..len].copy_from_slice(expanded.get(..len).unwrap_or(&[]));
    iv
}

// ===========================================================================
// TCP record I/O helpers
// ===========================================================================

/// Read a complete TLS record from TCP.
///
/// A TLS record is: content_type (1) + version (2) + length (2) + payload.
/// Returns the complete record including the 5-byte header.
fn read_raw_record(tcp_handle: usize, timeout_polls: u32) -> KernelResult<Vec<u8>> {
    let mut header_buf = Vec::new();

    // Read the 5-byte record header.
    let mut polls = 0u32;
    while header_buf.len() < 5 {
        super::super::net::poll();
        let need = 5 - header_buf.len();
        match super::tcp::read_up_to(tcp_handle, need) {
            Ok(data) if !data.is_empty() => {
                header_buf.extend_from_slice(&data);
                polls = 0; // Reset timeout on progress.
            }
            Ok(_) => {
                polls = polls.saturating_add(1);
                if polls >= timeout_polls {
                    return Err(KernelError::TimedOut);
                }
                core::hint::spin_loop();
            }
            Err(e) => return Err(e),
        }
    }

    // Parse record length from header.
    let payload_len = read_u16_slice(&header_buf, 3) as usize;
    if payload_len > MAX_CIPHERTEXT_SIZE {
        crate::serial_println!("[tls] Record too large: {} bytes", payload_len);
        return Err(KernelError::InvalidArgument);
    }

    // Read the payload.
    let mut payload_buf = Vec::with_capacity(payload_len);
    polls = 0;
    while payload_buf.len() < payload_len {
        super::super::net::poll();
        let need = payload_len - payload_buf.len();
        match super::tcp::read_up_to(tcp_handle, need) {
            Ok(data) if !data.is_empty() => {
                payload_buf.extend_from_slice(&data);
                polls = 0;
            }
            Ok(_) => {
                polls = polls.saturating_add(1);
                if polls >= timeout_polls {
                    return Err(KernelError::TimedOut);
                }
                core::hint::spin_loop();
            }
            Err(e) => return Err(e),
        }
    }

    // Combine header + payload into one record.
    let mut record = Vec::with_capacity(5 + payload_len);
    record.extend_from_slice(&header_buf);
    record.extend_from_slice(&payload_buf);
    Ok(record)
}

/// Try to read a raw TLS record without blocking (non-blocking poll).
///
/// Returns Err if no complete record is available.
fn try_read_raw_record(tcp_handle: usize) -> KernelResult<Vec<u8>> {
    // Peek to see if we have at least 5 bytes.
    let peek_data = super::tcp::peek(tcp_handle, 5)?;
    if peek_data.len() < 5 {
        return Err(KernelError::WouldBlock);
    }

    let payload_len = read_u16_slice(&peek_data, 3) as usize;
    let total = 5 + payload_len;

    // Peek the full record.
    let full_peek = super::tcp::peek(tcp_handle, total)?;
    if full_peek.len() < total {
        return Err(KernelError::WouldBlock);
    }

    // Actually consume the data.
    let data = super::tcp::read_up_to(tcp_handle, total)?;
    if data.len() < total {
        // Shouldn't happen after peek confirmed availability, but be safe.
        return Err(KernelError::WouldBlock);
    }

    Ok(data)
}

/// Read a handshake record (plaintext, used for ClientHello/ServerHello exchange).
fn read_handshake_record(tcp_handle: usize, timeout_polls: u32) -> KernelResult<Vec<u8>> {
    let record = read_raw_record(tcp_handle, timeout_polls)?;

    let record_type = record.first().copied().ok_or(KernelError::InternalError)?;
    if record_type == content_type::ALERT {
        let level = record.get(5).copied().unwrap_or(0);
        let desc = record.get(6).copied().unwrap_or(0);
        crate::serial_println!("[tls] Alert: level={}, desc={}", level, desc);
        return Err(KernelError::ChannelClosed);
    }

    if record_type != content_type::HANDSHAKE {
        crate::serial_println!("[tls] Expected handshake record, got type {}", record_type);
        return Err(KernelError::InvalidArgument);
    }

    // Return just the handshake payload (without record header).
    Ok(Vec::from(record.get(5..).ok_or(KernelError::InternalError)?))
}

// ===========================================================================
// Encoding helpers
// ===========================================================================

fn push_u16(buf: &mut Vec<u8>, val: usize) {
    buf.push((val >> 8) as u8);
    buf.push(val as u8);
}

fn push_u24(buf: &mut Vec<u8>, val: usize) {
    buf.push((val >> 16) as u8);
    buf.push((val >> 8) as u8);
    buf.push(val as u8);
}

fn read_u16(data: &[u8], offset: usize) -> KernelResult<u16> {
    let hi = data.get(offset).copied().ok_or(KernelError::InvalidArgument)?;
    let lo = data.get(offset + 1).copied().ok_or(KernelError::InvalidArgument)?;
    Ok(u16::from_be_bytes([hi, lo]))
}

fn read_u24(data: &[u8], offset: usize) -> KernelResult<usize> {
    let b0 = data.get(offset).copied().ok_or(KernelError::InvalidArgument)? as usize;
    let b1 = data.get(offset + 1).copied().ok_or(KernelError::InvalidArgument)? as usize;
    let b2 = data.get(offset + 2).copied().ok_or(KernelError::InvalidArgument)? as usize;
    Ok((b0 << 16) | (b1 << 8) | b2)
}

/// Read u24 from a slice (infallible — caller guarantees bounds).
fn read_u24_slice(data: &[u8], offset: usize) -> usize {
    let b0 = data.get(offset).copied().unwrap_or(0) as usize;
    let b1 = data.get(offset + 1).copied().unwrap_or(0) as usize;
    let b2 = data.get(offset + 2).copied().unwrap_or(0) as usize;
    (b0 << 16) | (b1 << 8) | b2
}

/// Read u16 from a slice (infallible).
fn read_u16_slice(data: &[u8], offset: usize) -> u16 {
    let hi = data.get(offset).copied().unwrap_or(0);
    let lo = data.get(offset + 1).copied().unwrap_or(0);
    u16::from_be_bytes([hi, lo])
}

// ===========================================================================
// Utility
// ===========================================================================

/// Constant-time byte comparison (prevents timing side-channel on MAC checks).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

/// Generate 32 cryptographically secure random bytes for key material.
///
/// Uses the kernel's ChaCha20-based CSPRNG (seeded from RDRAND/RDSEED
/// hardware RNG + interrupt timing jitter).  This provides sufficient
/// entropy for TLS key generation and nonces.
fn generate_random_bytes() -> [u8; 32] {
    let mut buf = [0u8; 32];
    crate::rng::fill(&mut buf);
    buf
}

// ===========================================================================
// Self-test
// ===========================================================================

/// Self-test for TLS 1.3 record layer and key schedule components.
///
/// Tests internal functions without requiring a network connection.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[tls] Running TLS 1.3 self-test...");
    let mut passed = 0u32;

    // Test 1: HKDF-Expand-Label produces expected length output.
    {
        let secret = crypto::sha256(b"test secret");
        let result = hkdf_expand_label(&secret, b"key", &[], 32);
        assert!(result.len() == 32, "HKDF-Expand-Label length");
        // Verify determinism.
        let result2 = hkdf_expand_label(&secret, b"key", &[], 32);
        assert!(result == result2, "HKDF-Expand-Label deterministic");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   HKDF-Expand-Label: PASSED");
    }

    // Test 2: record nonce XOR is correct.
    {
        let iv = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c];
        let nonce = record_nonce(&iv, 0);
        assert!(nonce == iv, "Record nonce seq=0 should equal IV");

        let nonce1 = record_nonce(&iv, 1);
        // Last byte should be XORed with 1.
        assert!(nonce1[11] == iv[11] ^ 1, "Record nonce seq=1 XOR");
        assert!(nonce1[0] == iv[0], "Record nonce seq=1 high bytes unchanged");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Record nonce XOR: PASSED");
    }

    // Test 3: encrypt + decrypt round-trip.
    {
        let key = [0x42u8; KEY_LEN];
        let iv = [0x13u8; NONCE_LEN];
        let plaintext = b"Hello, TLS 1.3!";

        let record = encrypt_record(&key, &iv, 0, content_type::APPLICATION_DATA, plaintext);
        let (inner_type, decrypted) = decrypt_record(&key, &iv, 0, &record)?;
        assert!(inner_type == content_type::APPLICATION_DATA, "Round-trip content type");
        assert!(decrypted == plaintext, "Round-trip plaintext");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Record encrypt/decrypt round-trip: PASSED");
    }

    // Test 4: tampered record fails decryption.
    {
        let key = [0x42u8; KEY_LEN];
        let iv = [0x13u8; NONCE_LEN];
        let plaintext = b"Tamper test";

        let mut record = encrypt_record(&key, &iv, 0, content_type::APPLICATION_DATA, plaintext);
        // Flip a bit in the ciphertext.
        if let Some(byte) = record.get_mut(10) {
            *byte ^= 0x01;
        }
        let result = decrypt_record(&key, &iv, 0, &record);
        assert!(result.is_err(), "Tampered record should fail");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Tamper detection: PASSED");
    }

    // Test 5: ClientHello building doesn't panic and has correct structure.
    {
        let random = [0xABu8; 32];
        let pubkey = [0xCDu8; 32];
        let hello = build_client_hello("example.com", &random, &pubkey);

        // Must start with handshake type CLIENT_HELLO (1).
        assert!(hello.first().copied() == Some(handshake_type::CLIENT_HELLO), "ClientHello type");
        // Length field (3 bytes) should match remaining data.
        let len = read_u24_slice(&hello, 1);
        assert!(hello.len() == 4 + len, "ClientHello length field");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   ClientHello construction: PASSED");
    }

    // Test 6: Derive-Secret is deterministic.
    {
        let secret = crypto::sha256(b"master");
        let hash = crypto::sha256(b"transcript");
        let ds1 = derive_secret(&secret, b"c ap traffic", &hash);
        let ds2 = derive_secret(&secret, b"c ap traffic", &hash);
        assert!(ds1 == ds2, "Derive-Secret deterministic");
        // Different labels produce different secrets.
        let ds3 = derive_secret(&secret, b"s ap traffic", &hash);
        assert!(ds1 != ds3, "Different labels → different secrets");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Derive-Secret: PASSED");
    }

    // Test 7: Finished verify computation is deterministic.
    {
        let base_key = crypto::sha256(b"handshake secret");
        let t_hash = crypto::sha256(b"transcript up to finished");
        let v1 = compute_finished_verify(&base_key, &t_hash);
        let v2 = compute_finished_verify(&base_key, &t_hash);
        assert!(v1 == v2, "Finished verify deterministic");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Finished verify: PASSED");
    }

    // Test 8: constant_time_eq works correctly.
    {
        let a = [1, 2, 3, 4];
        let b = [1, 2, 3, 4];
        let c = [1, 2, 3, 5];
        assert!(constant_time_eq(&a, &b), "Equal arrays");
        assert!(!constant_time_eq(&a, &c), "Unequal arrays");
        assert!(!constant_time_eq(&a, &[1, 2, 3]), "Different lengths");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Constant-time compare: PASSED");
    }

    // Test 9: Key schedule smoke test (full early→handshake→master derivation).
    {
        let zero_ikm = [0u8; HASH_LEN];
        let zero_salt = [0u8; HASH_LEN];
        let empty_hash = crypto::sha256(&[]);

        // Early secret.
        let early = crypto::hkdf_extract(&zero_salt, &zero_ikm);
        let derived_early = derive_secret(&early, b"derived", &empty_hash);

        // Fake shared secret.
        let fake_shared = [0x55u8; 32];
        let hs_secret = crypto::hkdf_extract(&derived_early, &fake_shared);

        // Traffic secrets.
        let fake_transcript = crypto::sha256(b"ch||sh");
        let c_hs = derive_secret(&hs_secret, b"c hs traffic", &fake_transcript);
        let s_hs = derive_secret(&hs_secret, b"s hs traffic", &fake_transcript);

        // Client and server secrets should differ.
        assert!(c_hs != s_hs, "Client/server HS secrets differ");

        // Derive keys.
        let c_key = derive_traffic_key(&c_hs);
        let s_key = derive_traffic_key(&s_hs);
        assert!(c_key != s_key, "Client/server keys differ");

        // Master secret.
        let derived_hs = derive_secret(&hs_secret, b"derived", &empty_hash);
        let master = crypto::hkdf_extract(&derived_hs, &zero_ikm);
        assert!(master != hs_secret, "Master ≠ handshake secret");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Key schedule smoke test: PASSED");
    }

    // Test 10: ClientHello parsing round-trip.
    {
        let random = [0xAAu8; 32];
        let pubkey = [0xBBu8; 32];
        let hello = build_client_hello("test.example.com", &random, &pubkey);

        let parsed = parse_client_hello(&hello)?;
        assert!(parsed.client_random == random, "ClientHello random round-trip");
        assert!(parsed.client_x25519_public == pubkey, "ClientHello X25519 round-trip");
        assert!(parsed.has_our_cipher, "ClientHello includes our cipher");
        assert!(parsed.has_tls13, "ClientHello includes TLS 1.3");
        assert!(parsed.has_ed25519_sig, "ClientHello includes Ed25519 sig alg");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   ClientHello parse round-trip: PASSED");
    }

    // Test 11: ServerHello construction and parsing.
    {
        let random = [0xCCu8; 32];
        let pubkey = [0xDDu8; 32];
        let server_hello = build_server_hello(&random, &pubkey);

        // Verify structure: type=2, length matches.
        assert!(server_hello.first().copied() == Some(handshake_type::SERVER_HELLO),
                "ServerHello type");
        let len = read_u24_slice(&server_hello, 1);
        assert!(server_hello.len() == 4 + len, "ServerHello length");

        // Parse it with the existing client-side parser.
        let parsed = parse_server_hello(&server_hello)?;
        assert!(parsed.cipher_suite as usize == CIPHER_SUITE, "ServerHello cipher");
        assert!(parsed.server_x25519_public == pubkey, "ServerHello X25519");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   ServerHello build/parse: PASSED");
    }

    // Test 12: Self-signed certificate generation.
    {
        let seed = [0x42u8; 32];
        let pubkey = crypto::ed25519_public_key(&seed);
        let cert = build_self_signed_certificate(&seed, &pubkey);

        // Must start with SEQUENCE tag (0x30).
        assert!(cert.first().copied() == Some(0x30), "Certificate is DER SEQUENCE");
        // Must be at least 100 bytes (minimal X.509 + Ed25519 key + signature).
        assert!(cert.len() > 100, "Certificate has reasonable size");
        // Should contain the Ed25519 OID (1.3.101.112 = 06 03 2B 65 70).
        let ed25519_oid = [0x06, 0x03, 0x2B, 0x65, 0x70];
        let has_oid = cert.windows(ed25519_oid.len()).any(|w| w == ed25519_oid);
        assert!(has_oid, "Certificate contains Ed25519 OID");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   Self-signed certificate: PASSED");
    }

    // Test 13: CertificateVerify construction.
    {
        let seed = [0x77u8; 32];
        let transcript = crypto::sha256(b"fake transcript for cv test");
        let cv_msg = build_certificate_verify(&seed, &transcript);

        // Type = CERTIFICATE_VERIFY (15).
        assert!(cv_msg.first().copied() == Some(handshake_type::CERTIFICATE_VERIFY),
                "CertificateVerify type");
        // Body should contain Ed25519 algorithm (0x0807).
        assert!(cv_msg.get(4).copied() == Some(0x08), "CertVerify alg high");
        assert!(cv_msg.get(5).copied() == Some(0x07), "CertVerify alg low (ed25519)");
        // Signature length should be 64 bytes.
        let sig_len = read_u16_slice(&cv_msg, 6) as usize;
        assert!(sig_len == 64, "CertVerify Ed25519 signature length");
        passed = passed.saturating_add(1);
        crate::serial_println!("[tls]   CertificateVerify construction: PASSED");
    }

    crate::serial_println!("[tls] All {} TLS self-tests PASSED", passed);
    Ok(())
}
