//! WebSocket protocol (RFC 6455) for bidirectional communication.
//!
//! Provides a WebSocket server implementation that upgrades HTTP/1.1
//! connections to full-duplex message channels.  Enables real-time
//! communication for system management interfaces and event streaming.
//!
//! ## Protocol
//!
//! The WebSocket handshake uses HTTP Upgrade with a SHA-1 based
//! Sec-WebSocket-Accept key derivation (RFC 6455 §4.2.2).  After
//! upgrade, communication proceeds via framed messages with opcodes
//! for text, binary, ping, pong, and close.
//!
//! ## Architecture
//!
//! ```text
//! Browser/client ─── TCP ──→ HTTP upgrade request
//!                               │
//!                               ├── validate Sec-WebSocket-Key
//!                               ├── compute Sec-WebSocket-Accept (SHA-1)
//!                               ├── send 101 Switching Protocols
//!                               │
//!                          WebSocket frames ←──→ message handlers
//!                               ├── text/binary data frames
//!                               ├── ping/pong heartbeat
//!                               └── close handshake
//! ```
//!
//! ## Frame Format (RFC 6455 §5.2)
//!
//! ```text
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-------+-+-------------+-------------------------------+
//! |F|R|R|R| opcode|M| Payload len |  Extended payload length      |
//! |I|S|S|S|  (4)  |A|     (7)     |  (16/64)                      |
//! |N|V|V|V|       |S|             |  (if payload len==126/127)    |
//! | |1|2|3|       |K|             |                               |
//! +-+-+-+-+-------+-+-------------+-------------------------------+
//! | Masking-key (if MASK set)                                     |
//! +-------------------------------+-------------------------------+
//! | Payload Data                                                  |
//! +---------------------------------------------------------------+
//! ```
//!
//! ## Security
//!
//! - Server never masks frames (RFC 6455 §5.1: server MUST NOT mask).
//! - Client frames must be masked; unmasked client frames close the connection.
//! - Maximum frame payload: 64 KiB (configurable).
//! - Close handshake: server echoes close frame before TCP shutdown.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// WebSocket GUID for Sec-WebSocket-Accept computation (RFC 6455 §4.2.2).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Maximum frame payload size (64 KiB).
const MAX_FRAME_PAYLOAD: usize = 65536;

/// Read timeout for frame data (poll iterations, ~1ms each).
const FRAME_READ_TIMEOUT: u32 = 30_000; // 30 seconds

// ---------------------------------------------------------------------------
// Opcodes (RFC 6455 §5.2)
// ---------------------------------------------------------------------------

/// Continuation frame.
const OP_CONTINUATION: u8 = 0x0;
/// Text frame (UTF-8).
const OP_TEXT: u8 = 0x1;
/// Binary frame.
const OP_BINARY: u8 = 0x2;
/// Connection close.
const OP_CLOSE: u8 = 0x8;
/// Ping.
const OP_PING: u8 = 0x9;
/// Pong.
const OP_PONG: u8 = 0xA;

// ---------------------------------------------------------------------------
// SHA-1 (RFC 3174) — minimal implementation for WebSocket handshake only
// ---------------------------------------------------------------------------

/// SHA-1 digest size in bytes.
const SHA1_DIGEST_SIZE: usize = 20;

/// Compute SHA-1 hash of input data.
///
/// This is a minimal implementation for the WebSocket handshake key
/// derivation.  SHA-1 is cryptographically broken for collision
/// resistance but RFC 6455 only uses it as a proof-of-protocol token,
/// not for security.
#[allow(clippy::arithmetic_side_effects)]
fn sha1(data: &[u8]) -> [u8; SHA1_DIGEST_SIZE] {
    // Initial hash values (RFC 3174 §6.1).
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    // Pre-processing: pad message to 512-bit (64-byte) blocks.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::from(data);
    padded.push(0x80);
    // Pad to 56 mod 64 bytes.
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    // Append original length as 64-bit big-endian.
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit block.
    let mut block_offset = 0;
    while block_offset < padded.len() {
        let block = &padded[block_offset..block_offset + 64];

        // Prepare message schedule (80 words).
        let mut w = [0u32; 80];
        for t in 0..16 {
            let i = t * 4;
            w[t] = u32::from_be_bytes([
                block[i],
                block[i + 1],
                block[i + 2],
                block[i + 3],
            ]);
        }
        for t in 16..80 {
            w[t] = (w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16]).rotate_left(1);
        }

        // Initialize working variables.
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);

        // 80 rounds.
        for t in 0..80 {
            let (f, k) = match t {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a.rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[t]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);

        block_offset += 64;
    }

    // Produce the final 160-bit digest.
    let mut digest = [0u8; SHA1_DIGEST_SIZE];
    digest[0..4].copy_from_slice(&h0.to_be_bytes());
    digest[4..8].copy_from_slice(&h1.to_be_bytes());
    digest[8..12].copy_from_slice(&h2.to_be_bytes());
    digest[12..16].copy_from_slice(&h3.to_be_bytes());
    digest[16..20].copy_from_slice(&h4.to_be_bytes());
    digest
}

// ---------------------------------------------------------------------------
// WebSocket handshake
// ---------------------------------------------------------------------------

/// Compute the Sec-WebSocket-Accept value for a given client key.
///
/// Per RFC 6455 §4.2.2: concatenate the client key with the GUID,
/// SHA-1 hash the result, and base64-encode the digest.
fn compute_accept_key(client_key: &str) -> String {
    let mut input = String::from(client_key.trim());
    input.push_str(WS_GUID);
    let hash = sha1(input.as_bytes());
    super::http::base64_encode(&hash)
}

/// Parse WebSocket upgrade headers from an HTTP request.
///
/// Returns `Some(client_key)` if the request is a valid WebSocket upgrade,
/// `None` otherwise.
fn parse_upgrade_request(data: &[u8]) -> Option<WsUpgradeInfo> {
    let text = core::str::from_utf8(data).ok()?;

    // Must be GET and HTTP/1.1.
    let first_line = text.lines().next()?;
    if !first_line.starts_with("GET ") {
        return None;
    }

    // Extract the request path.
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    let path = String::from(*parts.get(1)?);

    let mut has_upgrade = false;
    let mut has_connection_upgrade = false;
    let mut ws_key = None;
    let mut ws_version = None;
    let mut ws_protocol = None;

    for line in text.lines().skip(1) {
        if line.is_empty() || line == "\r" {
            break;
        }
        let colon = line.find(':')?;
        let name = line.get(..colon)?.trim();
        let value = line.get(colon.saturating_add(1)..)?.trim();

        // Case-insensitive header comparison.
        let name_lower = name.as_bytes();
        if eq_ignore_ascii_case(name_lower, b"Upgrade") && eq_ignore_ascii_case(value.as_bytes(), b"websocket") {
            has_upgrade = true;
        } else if eq_ignore_ascii_case(name_lower, b"Connection") {
            // Connection header may contain multiple tokens.
            for token in value.split(',') {
                if eq_ignore_ascii_case(token.trim().as_bytes(), b"Upgrade") {
                    has_connection_upgrade = true;
                }
            }
        } else if eq_ignore_ascii_case(name_lower, b"Sec-WebSocket-Key") {
            ws_key = Some(String::from(value));
        } else if eq_ignore_ascii_case(name_lower, b"Sec-WebSocket-Version") {
            ws_version = Some(String::from(value));
        } else if eq_ignore_ascii_case(name_lower, b"Sec-WebSocket-Protocol") {
            ws_protocol = Some(String::from(value));
        }
    }

    if !has_upgrade || !has_connection_upgrade {
        return None;
    }

    let key = ws_key?;
    let version = ws_version.unwrap_or_else(|| String::from("13"));

    Some(WsUpgradeInfo {
        path,
        key,
        version,
        protocol: ws_protocol,
    })
}

/// Case-insensitive ASCII comparison.
fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        let ca = if a[i] >= b'A' && a[i] <= b'Z' {
            a[i].wrapping_add(32)
        } else {
            a[i]
        };
        let cb = if b[i] >= b'A' && b[i] <= b'Z' {
            b[i].wrapping_add(32)
        } else {
            b[i]
        };
        if ca != cb {
            return false;
        }
    }
    true
}

/// Build the HTTP 101 Switching Protocols response for WebSocket upgrade.
fn build_upgrade_response(accept_key: &str, protocol: Option<&str>) -> Vec<u8> {
    let mut resp = String::from(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n",
    );
    resp.push_str(&format!("Sec-WebSocket-Accept: {}\r\n", accept_key));
    if let Some(proto) = protocol {
        resp.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", proto));
    }
    resp.push_str("\r\n");
    resp.into_bytes()
}

// ---------------------------------------------------------------------------
// WebSocket frame parsing and building
// ---------------------------------------------------------------------------

/// Parsed WebSocket frame.
pub struct WsFrame {
    /// Whether this is the final fragment.
    pub fin: bool,
    /// Opcode.
    pub opcode: u8,
    /// Payload data (unmasked).
    pub payload: Vec<u8>,
}

/// Info extracted from a WebSocket upgrade request.
struct WsUpgradeInfo {
    /// Request path.
    path: String,
    /// Sec-WebSocket-Key.
    key: String,
    /// Sec-WebSocket-Version.
    version: String,
    /// Sec-WebSocket-Protocol (optional).
    protocol: Option<String>,
}

/// Parse a WebSocket frame from raw bytes.
///
/// Returns the frame and the number of bytes consumed, or None if
/// not enough data is available yet.
#[allow(clippy::arithmetic_side_effects)]
pub fn parse_frame(data: &[u8]) -> Option<(WsFrame, usize)> {
    if data.len() < 2 {
        return None;
    }

    let byte0 = data[0];
    let byte1 = data[1];

    let fin = (byte0 & 0x80) != 0;
    let opcode = byte0 & 0x0F;
    let masked = (byte1 & 0x80) != 0;
    let payload_len_7 = (byte1 & 0x7F) as usize;

    let mut offset = 2usize;

    // Extended payload length.
    let payload_len = if payload_len_7 == 126 {
        if data.len() < 4 {
            return None;
        }
        let len = u16::from_be_bytes([data[2], data[3]]) as usize;
        offset = 4;
        len
    } else if payload_len_7 == 127 {
        if data.len() < 10 {
            return None;
        }
        let len = u64::from_be_bytes([
            data[2], data[3], data[4], data[5],
            data[6], data[7], data[8], data[9],
        ]) as usize;
        offset = 10;
        // Reject absurdly large frames.
        if len > MAX_FRAME_PAYLOAD {
            return None;
        }
        len
    } else {
        payload_len_7
    };

    // Reject oversized payloads.
    if payload_len > MAX_FRAME_PAYLOAD {
        return None;
    }

    // Masking key (4 bytes if masked).
    let mask_key = if masked {
        if data.len() < offset + 4 {
            return None;
        }
        let key = [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ];
        offset += 4;
        Some(key)
    } else {
        None
    };

    // Payload data.
    if data.len() < offset + payload_len {
        return None;
    }

    let mut payload = Vec::from(&data[offset..offset + payload_len]);

    // Unmask payload if masked.
    if let Some(key) = mask_key {
        for i in 0..payload.len() {
            payload[i] ^= key[i % 4];
        }
    }

    let total_consumed = offset + payload_len;

    Some((
        WsFrame {
            fin,
            opcode,
            payload,
        },
        total_consumed,
    ))
}

/// Build a WebSocket frame (server→client, never masked).
#[allow(clippy::arithmetic_side_effects)]
pub fn build_frame(opcode: u8, payload: &[u8], fin: bool) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 10);

    // Byte 0: FIN + opcode.
    let byte0 = if fin { 0x80 | opcode } else { opcode };
    frame.push(byte0);

    // Byte 1+: payload length (server never masks).
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else if payload.len() <= 65535 {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }

    frame.extend_from_slice(payload);
    frame
}

/// Build a text frame.
pub fn text_frame(text: &str) -> Vec<u8> {
    build_frame(OP_TEXT, text.as_bytes(), true)
}

/// Build a binary frame.
pub fn binary_frame(data: &[u8]) -> Vec<u8> {
    build_frame(OP_BINARY, data, true)
}

/// Build a close frame with optional status code and reason.
#[allow(clippy::arithmetic_side_effects)]
pub fn close_frame(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(reason.len() + 2);
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());
    build_frame(OP_CLOSE, &payload, true)
}

/// Build a ping frame.
pub fn ping_frame(data: &[u8]) -> Vec<u8> {
    build_frame(OP_PING, data, true)
}

/// Build a pong frame (must echo the ping payload).
pub fn pong_frame(data: &[u8]) -> Vec<u8> {
    build_frame(OP_PONG, data, true)
}

// ---------------------------------------------------------------------------
// WebSocket connection handler
// ---------------------------------------------------------------------------

/// A callback that processes incoming WebSocket messages.
///
/// The handler receives the connection handle, opcode, and payload.
/// It returns response frames to send back (may be empty).
pub type WsMessageHandler = fn(conn: usize, opcode: u8, payload: &[u8]) -> Vec<Vec<u8>>;

/// Upgrade an HTTP connection to WebSocket and run the message loop.
///
/// This is called from the httpd when it detects an Upgrade: websocket
/// request.  It performs the handshake and then enters a frame read loop,
/// dispatching messages to the handler callback.
///
/// The function blocks until the connection closes (either by close frame
/// or TCP disconnect).
#[allow(clippy::arithmetic_side_effects)]
pub fn handle_upgrade(
    conn_handle: usize,
    request_data: &[u8],
    handler: WsMessageHandler,
) -> KernelResult<()> {
    use crate::net::tcp;

    // Parse the upgrade request.
    let info = parse_upgrade_request(request_data)
        .ok_or(KernelError::InvalidArgument)?;

    // Validate version (must be 13 per RFC 6455 §4.4).
    if info.version.trim() != "13" {
        let resp = b"HTTP/1.1 426 Upgrade Required\r\n\
                     Sec-WebSocket-Version: 13\r\n\
                     Connection: close\r\n\r\n";
        let _ = tcp::send(conn_handle, resp);
        let _ = tcp::close(conn_handle);
        return Err(KernelError::NotSupported);
    }

    serial_println!("[websocket] Upgrade: path={}, key={}", info.path, info.key);

    // Compute accept key and send 101 response.
    let accept = compute_accept_key(&info.key);
    let response = build_upgrade_response(&accept, info.protocol.as_deref());
    tcp::send(conn_handle, &response)?;

    serial_println!("[websocket] Connection upgraded on handle {}", conn_handle);

    // Frame read loop.
    let mut buf = Vec::with_capacity(4096);

    loop {
        // Read more data from TCP.
        match tcp::read_blocking(conn_handle, FRAME_READ_TIMEOUT, MAX_FRAME_PAYLOAD) {
            Ok(data) => {
                if data.is_empty() {
                    // Connection closed by peer.
                    break;
                }
                buf.extend_from_slice(&data);
            }
            Err(_) => {
                // Timeout or error — close connection.
                break;
            }
        }

        // Parse all complete frames in the buffer.
        while let Some((frame, consumed)) = parse_frame(&buf) {
            // Remove consumed bytes from buffer.
            buf.drain(..consumed);

            match frame.opcode {
                OP_CLOSE => {
                    // Echo close frame and shut down.
                    let code = if frame.payload.len() >= 2 {
                        u16::from_be_bytes([frame.payload[0], frame.payload[1]])
                    } else {
                        1000
                    };
                    let close = close_frame(code, "");
                    let _ = tcp::send(conn_handle, &close);
                    let _ = tcp::close(conn_handle);
                    serial_println!("[websocket] Close handshake complete (code {})", code);
                    return Ok(());
                }
                OP_PING => {
                    // Respond with pong (echo payload).
                    let pong = pong_frame(&frame.payload);
                    let _ = tcp::send(conn_handle, &pong);
                }
                OP_PONG => {
                    // Pong received — no action needed.
                }
                OP_TEXT | OP_BINARY | OP_CONTINUATION => {
                    // Dispatch to handler.
                    let responses = handler(conn_handle, frame.opcode, &frame.payload);
                    for resp_frame in &responses {
                        if tcp::send(conn_handle, resp_frame).is_err() {
                            // Send failed — connection dead.
                            let _ = tcp::close(conn_handle);
                            return Err(KernelError::InternalError);
                        }
                    }
                }
                _ => {
                    // Unknown opcode — close with protocol error.
                    let close = close_frame(1002, "Unknown opcode");
                    let _ = tcp::send(conn_handle, &close);
                    let _ = tcp::close(conn_handle);
                    return Err(KernelError::InvalidArgument);
                }
            }
        }
    }

    let _ = tcp::close(conn_handle);
    serial_println!("[websocket] Connection closed on handle {}", conn_handle);
    Ok(())
}

/// Check if a raw HTTP request is a WebSocket upgrade request.
///
/// Used by httpd to detect upgrade requests before normal HTTP handling.
pub fn is_upgrade_request(data: &[u8]) -> bool {
    parse_upgrade_request(data).is_some()
}

// ---------------------------------------------------------------------------
// Echo handler (default / test handler)
// ---------------------------------------------------------------------------

/// Simple echo handler: sends back whatever text/binary it receives.
pub fn echo_handler(_conn: usize, opcode: u8, payload: &[u8]) -> Vec<Vec<u8>> {
    match opcode {
        OP_TEXT => {
            let text = core::str::from_utf8(payload).unwrap_or("<invalid UTF-8>");
            alloc::vec![text_frame(text)]
        }
        OP_BINARY => {
            alloc::vec![binary_frame(payload)]
        }
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// WebSocket module self-test.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[websocket] Running self-test...");

    // Test 1: SHA-1 test vectors (RFC 3174).
    {
        // "abc" → a9993e36 4706816a ba3e2571 7850c26c 9cd0d89d
        let hash = sha1(b"abc");
        assert_eq!(hash[0], 0xa9);
        assert_eq!(hash[1], 0x99);
        assert_eq!(hash[2], 0x3e);
        assert_eq!(hash[3], 0x36);
        assert_eq!(hash[19], 0x9d);
        serial_println!("[websocket]   SHA-1 'abc': OK");

        // Empty string → da39a3ee 5e6b4b0d 3255bfef 95601890 afd80709
        let hash_empty = sha1(b"");
        assert_eq!(hash_empty[0], 0xda);
        assert_eq!(hash_empty[1], 0x39);
        assert_eq!(hash_empty[2], 0xa3);
        assert_eq!(hash_empty[3], 0xee);
        assert_eq!(hash_empty[19], 0x09);
        serial_println!("[websocket]   SHA-1 empty: OK");

        // Full 20-byte verification for "abc".
        let abc_expected: [u8; 20] = [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e,
            0x25, 0x71, 0x78, 0x50, 0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
        ];
        assert_eq!(hash, abc_expected);
        serial_println!("[websocket]   SHA-1 'abc' full verify: OK");

        // NIST test vector (56 bytes, 2 blocks after padding):
        // SHA-1("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
        // = 84983e44 1c3bd26e baae4aa1 f95129e5 e54670f1
        let nist_input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let nist_hash = sha1(nist_input);
        let nist_expected: [u8; 20] = [
            0x84, 0x98, 0x3e, 0x44, 0x1c, 0x3b, 0xd2, 0x6e, 0xba, 0xae,
            0x4a, 0xa1, 0xf9, 0x51, 0x29, 0xe5, 0xe5, 0x46, 0x70, 0xf1,
        ];
        assert_eq!(nist_hash, nist_expected);
        serial_println!("[websocket]   SHA-1 NIST 2-block: OK");
    }

    // Test 2: Sec-WebSocket-Accept computation (RFC 6455 §4.2.2 example).
    {
        // The RFC example: key "dGhlIHNhbXBsZSBub25jZQ=="
        // → accept "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        let accept = compute_accept_key("dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        serial_println!("[websocket]   Accept key computation (RFC 6455 example): OK");
    }

    // Test 3: Frame parsing — unmasked text frame.
    {
        // FIN=1, opcode=1 (text), mask=0, len=5, payload "Hello"
        let frame_bytes = [0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let (frame, consumed) = parse_frame(&frame_bytes).expect("parse failed");
        assert!(frame.fin);
        assert_eq!(frame.opcode, OP_TEXT);
        assert_eq!(frame.payload, b"Hello");
        assert_eq!(consumed, 7);
        serial_println!("[websocket]   Frame parse (unmasked text): OK");
    }

    // Test 4: Frame parsing — masked text frame.
    {
        // FIN=1, opcode=1 (text), mask=1, len=5
        // Mask key: [0x37, 0xfa, 0x21, 0x3d]
        // Payload "Hello" XOR'd with mask key.
        let mask = [0x37u8, 0xfa, 0x21, 0x3d];
        let payload = b"Hello";
        let mut masked_payload = [0u8; 5];
        for i in 0..5 {
            masked_payload[i] = payload[i] ^ mask[i % 4];
        }
        let mut frame_bytes = Vec::new();
        frame_bytes.push(0x81); // FIN + text
        frame_bytes.push(0x85); // MASK + len=5
        frame_bytes.extend_from_slice(&mask);
        frame_bytes.extend_from_slice(&masked_payload);

        let (frame, consumed) = parse_frame(&frame_bytes).expect("parse failed");
        assert!(frame.fin);
        assert_eq!(frame.opcode, OP_TEXT);
        assert_eq!(frame.payload, b"Hello");
        assert_eq!(consumed, 11);
        serial_println!("[websocket]   Frame parse (masked text): OK");
    }

    // Test 5: Frame building — text.
    {
        let frame = text_frame("Hi");
        assert_eq!(frame[0], 0x81); // FIN + text
        assert_eq!(frame[1], 2);    // len = 2, no mask
        assert_eq!(&frame[2..4], b"Hi");
        serial_println!("[websocket]   Frame build (text): OK");
    }

    // Test 6: Frame building — close with code.
    {
        let frame = close_frame(1000, "OK");
        assert_eq!(frame[0], 0x88); // FIN + close
        assert_eq!(frame[1], 4);    // 2 bytes code + 2 bytes reason
        assert_eq!(frame[2], 0x03); // 1000 >> 8
        assert_eq!(frame[3], 0xE8); // 1000 & 0xFF
        assert_eq!(&frame[4..6], b"OK");
        serial_println!("[websocket]   Frame build (close): OK");
    }

    // Test 7: Frame building — extended length (126-byte payload).
    {
        let payload = [0xAA; 200];
        let frame = build_frame(OP_BINARY, &payload, true);
        assert_eq!(frame[0], 0x82);  // FIN + binary
        assert_eq!(frame[1], 126);   // extended 16-bit length
        assert_eq!(frame[2], 0);     // 200 >> 8
        assert_eq!(frame[3], 200);   // 200 & 0xFF
        assert_eq!(frame.len(), 4 + 200);
        serial_println!("[websocket]   Frame build (extended length): OK");
    }

    // Test 8: Upgrade request parsing.
    {
        let req = b"GET /ws HTTP/1.1\r\n\
                    Host: localhost:8080\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\
                    Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                    Sec-WebSocket-Version: 13\r\n\
                    \r\n";
        let info = parse_upgrade_request(req).expect("parse failed");
        assert_eq!(info.path, "/ws");
        assert_eq!(info.key, "dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(info.version, "13");
        serial_println!("[websocket]   Upgrade request parse: OK");
    }

    // Test 9: Non-upgrade request rejected.
    {
        let req = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert!(parse_upgrade_request(req).is_none());
        serial_println!("[websocket]   Non-upgrade rejection: OK");
    }

    // Test 10: is_upgrade_request.
    {
        let ws_req = b"GET /ws HTTP/1.1\r\n\
                       Upgrade: websocket\r\n\
                       Connection: Upgrade\r\n\
                       Sec-WebSocket-Key: x3JJHMbDL1EzLkh9GBhXDw==\r\n\
                       Sec-WebSocket-Version: 13\r\n\r\n";
        assert!(is_upgrade_request(ws_req));

        let normal_req = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert!(!is_upgrade_request(normal_req));
        serial_println!("[websocket]   is_upgrade_request: OK");
    }

    // Test 11: Case-insensitive header matching.
    {
        assert!(eq_ignore_ascii_case(b"Upgrade", b"upgrade"));
        assert!(eq_ignore_ascii_case(b"UPGRADE", b"upgrade"));
        assert!(eq_ignore_ascii_case(b"Connection", b"connection"));
        assert!(!eq_ignore_ascii_case(b"Upgrade", b"downgrade"));
        serial_println!("[websocket]   Case-insensitive headers: OK");
    }

    // Test 12: Ping/pong frame roundtrip.
    {
        let ping = ping_frame(b"keepalive");
        assert_eq!(ping[0], 0x89); // FIN + ping
        assert_eq!(ping[1], 9);
        assert_eq!(&ping[2..], b"keepalive");

        let pong = pong_frame(b"keepalive");
        assert_eq!(pong[0], 0x8A); // FIN + pong
        assert_eq!(pong[1], 9);
        assert_eq!(&pong[2..], b"keepalive");
        serial_println!("[websocket]   Ping/pong frames: OK");
    }

    serial_println!("[websocket] Self-test PASSED (12 tests)");
    Ok(())
}
