//! TFTP (Trivial File Transfer Protocol) client and server.
//!
//! Implements RFC 1350 (TFTP) for simple file transfer over UDP.
//! TFTP is used for PXE network booting, firmware updates, and
//! configuration file transfer in embedded/network environments.
//!
//! ## Protocol
//!
//! TFTP operates over UDP port 69.  Transfers use fixed 512-byte blocks
//! with stop-and-wait acknowledgment:
//!
//! ```text
//! Client                    Server
//!   |--- RRQ (filename) ----→|     Read request
//!   |←--- DATA (block 1) ---|     First 512 bytes
//!   |--- ACK (block 1) ----→|     Acknowledge
//!   |←--- DATA (block 2) ---|     Next 512 bytes
//!   ...
//!   |←--- DATA (block N) ---|     Last block (< 512 bytes = EOF)
//!   |--- ACK (block N) ----→|     Done
//! ```
//!
//! ## Features
//!
//! - **Client**: read files from remote TFTP servers (`get` / `get_v6`)
//! - **Client upload**: write files to remote servers (`put` / `put_v6`)
//! - **Server**: serve files from the kernel VFS (`serve`)
//! - **IPv6**: dual-stack client support via `get_v6` / `put_v6`
//! - **Modes**: `octet` (binary) mode only (no `netascii`)
//! - **Timeout**: 3-second retransmit with 5 retries
//! - **Error handling**: TFTP error packets with standard error codes
//!
//! ## Limitations
//!
//! - Maximum file size: 32 MiB (65535 blocks × 512 bytes).
//! - `octet` mode only (no `netascii` or `mail` modes).
//! - No option extension (RFC 2347) — fixed 512-byte block size.
//! - Server handles one transfer at a time per client.
//! - Maximum 4 concurrent server transfers.

use alloc::string::String;
use alloc::{vec, vec::Vec};
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;
use super::ipv6::Ipv6Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// TFTP well-known server port.
const TFTP_PORT: u16 = 69;

/// TFTP data block size (RFC 1350).
const BLOCK_SIZE: usize = 512;

/// Maximum retransmissions before giving up.
const MAX_RETRIES: u32 = 5;

/// Retransmit timeout (nanoseconds) — 3 seconds.
const RETRANSMIT_TIMEOUT_NS: u64 = 3_000_000_000;

/// Maximum file size (32 MiB).
const MAX_FILE_SIZE: usize = 32 * 1024 * 1024;

/// Maximum concurrent server transfers.
const MAX_SERVER_TRANSFERS: usize = 4;

/// Server tick interval (nanoseconds) — 500ms.
const SERVER_TICK_INTERVAL_NS: u64 = 500_000_000;

// TFTP opcodes.
const OP_RRQ: u16 = 1;   // Read request
const OP_WRQ: u16 = 2;   // Write request
const OP_DATA: u16 = 3;  // Data
const OP_ACK: u16 = 4;   // Acknowledgment
const OP_ERROR: u16 = 5; // Error

// TFTP error codes.
const ERR_UNDEFINED: u16 = 0;
const ERR_FILE_NOT_FOUND: u16 = 1;
const ERR_ACCESS_VIOLATION: u16 = 2;
#[allow(dead_code)] // Public API.
const ERR_DISK_FULL: u16 = 3;
const ERR_ILLEGAL_OP: u16 = 4;
#[allow(dead_code)] // Public API.
const ERR_UNKNOWN_TID: u16 = 5;
const ERR_FILE_EXISTS: u16 = 6;

// ---------------------------------------------------------------------------
// Packet construction
// ---------------------------------------------------------------------------

/// Build a RRQ (Read Request) packet.
///
/// Format: `[OP_RRQ(2)] [filename\0] [mode\0]`
fn build_rrq(filename: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(4 + filename.len() + 6);
    pkt.push((OP_RRQ >> 8) as u8);
    pkt.push(OP_RRQ as u8);
    pkt.extend_from_slice(filename.as_bytes());
    pkt.push(0); // Null terminator.
    pkt.extend_from_slice(b"octet");
    pkt.push(0);
    pkt
}

/// Build a WRQ (Write Request) packet.
///
/// Format: `[OP_WRQ(2)] [filename\0] [mode\0]`
#[allow(dead_code)] // Public API.
fn build_wrq(filename: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(4 + filename.len() + 6);
    pkt.push((OP_WRQ >> 8) as u8);
    pkt.push(OP_WRQ as u8);
    pkt.extend_from_slice(filename.as_bytes());
    pkt.push(0);
    pkt.extend_from_slice(b"octet");
    pkt.push(0);
    pkt
}

/// Build a DATA packet.
///
/// Format: `[OP_DATA(2)] [block#(2)] [data(0-512)]`
fn build_data(block_num: u16, data: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(4 + data.len());
    pkt.push((OP_DATA >> 8) as u8);
    pkt.push(OP_DATA as u8);
    pkt.push((block_num >> 8) as u8);
    pkt.push(block_num as u8);
    pkt.extend_from_slice(data);
    pkt
}

/// Build an ACK packet.
///
/// Format: `[OP_ACK(2)] [block#(2)]`
fn build_ack(block_num: u16) -> Vec<u8> {
    vec![
        (OP_ACK >> 8) as u8,
        OP_ACK as u8,
        (block_num >> 8) as u8,
        block_num as u8,
    ]
}

/// Build an ERROR packet.
///
/// Format: `[OP_ERROR(2)] [error_code(2)] [msg\0]`
fn build_error(code: u16, msg: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(5 + msg.len());
    pkt.push((OP_ERROR >> 8) as u8);
    pkt.push(OP_ERROR as u8);
    pkt.push((code >> 8) as u8);
    pkt.push(code as u8);
    pkt.extend_from_slice(msg.as_bytes());
    pkt.push(0);
    pkt
}

// ---------------------------------------------------------------------------
// Packet parsing
// ---------------------------------------------------------------------------

/// Parse the opcode from a TFTP packet.
fn parse_opcode(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some((*data.first()? as u16) << 8 | *data.get(1)? as u16)
}

/// Parse a block number from a DATA or ACK packet.
fn parse_block_num(data: &[u8]) -> Option<u16> {
    if data.len() < 4 {
        return None;
    }
    Some((*data.get(2)? as u16) << 8 | *data.get(3)? as u16)
}

/// Parse a null-terminated string from a packet starting at offset.
fn parse_string(data: &[u8], offset: usize) -> Option<(&str, usize)> {
    let start = offset;
    let mut end = start;
    while end < data.len() {
        if *data.get(end)? == 0 {
            let s = core::str::from_utf8(data.get(start..end)?).ok()?;
            return Some((s, end.saturating_add(1)));
        }
        end = end.saturating_add(1);
    }
    None
}

/// Parse a RRQ or WRQ packet into (filename, mode).
fn parse_request(data: &[u8]) -> Option<(String, String)> {
    if data.len() < 6 {
        return None;
    }
    let (filename, next) = parse_string(data, 2)?;
    let (mode, _) = parse_string(data, next)?;
    Some((String::from(filename), String::from(mode)))
}

/// Parse an error packet into (code, message).
fn parse_error(data: &[u8]) -> Option<(u16, String)> {
    if data.len() < 5 {
        return None;
    }
    let code = (*data.get(2)? as u16) << 8 | *data.get(3)? as u16;
    let (msg, _) = parse_string(data, 4)?;
    Some((code, String::from(msg)))
}

// ---------------------------------------------------------------------------
// Client API
// ---------------------------------------------------------------------------

/// Download a file from a TFTP server.
///
/// Sends a RRQ and assembles DATA blocks until a short block (< 512 bytes)
/// indicates end of file.
///
/// # Arguments
///
/// - `server_ip` — TFTP server address.
/// - `filename` — Remote filename to request.
///
/// # Returns
///
/// The file contents as a byte vector, or an error.
pub fn get(server_ip: Ipv4Addr, filename: &str) -> KernelResult<Vec<u8>> {
    // Bind an ephemeral UDP port for this transfer.
    let local_port = ephemeral_port();
    let handle = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    // Send RRQ.
    let rrq = build_rrq(filename);
    super::udp::send(local_port, server_ip, TFTP_PORT, &rrq)?;

    CLIENT_GETS.fetch_add(1, Ordering::Relaxed);

    let mut file_data = Vec::with_capacity(BLOCK_SIZE * 16);
    let mut expected_block: u16 = 1;
    let mut server_port: u16 = 0; // TID: set from first DATA packet.
    let mut retries = 0u32;
    let mut last_sent_ns = crate::hrtimer::now_ns();

    loop {
        // Poll for incoming packets.
        super::poll();

        if let Some(dgram) = super::udp::recv(handle) {
            let opcode = parse_opcode(&dgram.data);

            match opcode {
                Some(OP_DATA) => {
                    let block = parse_block_num(&dgram.data).unwrap_or(0);

                    // First data packet establishes the server's TID (port).
                    if server_port == 0 {
                        server_port = dgram.src_port;
                    } else if dgram.src_port != server_port {
                        // Wrong TID — send error and ignore.
                        let err = build_error(ERR_UNDEFINED, "Wrong TID");
                        let _ = super::udp::send(local_port, dgram.src_ip, dgram.src_port, &err);
                        continue;
                    }

                    if block == expected_block {
                        // Append data.
                        if let Some(payload) = dgram.data.get(4..) {
                            if file_data.len().saturating_add(payload.len()) > MAX_FILE_SIZE {
                                let err = build_error(ERR_UNDEFINED, "File too large");
                                let _ = super::udp::send(local_port, server_ip, server_port, &err);
                                super::udp::close(handle);
                                return Err(KernelError::ResourceExhausted);
                            }
                            file_data.extend_from_slice(payload);

                            // ACK.
                            let ack = build_ack(block);
                            let _ = super::udp::send(local_port, server_ip, server_port, &ack);
                            last_sent_ns = crate::hrtimer::now_ns();
                            retries = 0;

                            // Short block = EOF.
                            if payload.len() < BLOCK_SIZE {
                                CLIENT_BYTES_RX.fetch_add(file_data.len() as u64, Ordering::Relaxed);
                                super::udp::close(handle);
                                return Ok(file_data);
                            }

                            expected_block = expected_block.wrapping_add(1);
                        }
                    } else if block < expected_block {
                        // Duplicate — re-ACK.
                        let ack = build_ack(block);
                        let _ = super::udp::send(local_port, server_ip, server_port, &ack);
                    }
                    // block > expected_block: ignore (out of order).
                }
                Some(OP_ERROR) => {
                    let (code, msg) = parse_error(&dgram.data)
                        .unwrap_or((0, String::from("Unknown error")));
                    super::udp::close(handle);
                    CLIENT_ERRORS.fetch_add(1, Ordering::Relaxed);
                    crate::serial_println!("[tftp] Server error {}: {}", code, msg);
                    return Err(match code {
                        ERR_FILE_NOT_FOUND => KernelError::NotFound,
                        ERR_ACCESS_VIOLATION => KernelError::PermissionDenied,
                        _ => KernelError::InternalError,
                    });
                }
                _ => {
                    // Unexpected opcode — ignore.
                }
            }
        }

        // Check for timeout.
        let now = crate::hrtimer::now_ns();
        if now.saturating_sub(last_sent_ns) >= RETRANSMIT_TIMEOUT_NS {
            retries = retries.saturating_add(1);
            if retries > MAX_RETRIES {
                super::udp::close(handle);
                CLIENT_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::TimedOut);
            }

            // Retransmit: re-send ACK for last received block (or re-send RRQ).
            if expected_block == 1 && server_port == 0 {
                // Haven't received any data yet — resend RRQ.
                let _ = super::udp::send(local_port, server_ip, TFTP_PORT, &rrq);
            } else {
                let ack = build_ack(expected_block.wrapping_sub(1));
                let _ = super::udp::send(local_port, server_ip, server_port, &ack);
            }
            last_sent_ns = now;
        }

        // Brief spin to avoid burning CPU.
        for _ in 0..1_000 {
            core::hint::spin_loop();
        }
    }
}

/// Upload a file to a TFTP server.
///
/// Sends a WRQ followed by DATA blocks.
pub fn put(server_ip: Ipv4Addr, filename: &str, data: &[u8]) -> KernelResult<()> {
    let local_port = ephemeral_port();
    let handle = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    // Send WRQ.
    let wrq = build_wrq(filename);
    super::udp::send(local_port, server_ip, TFTP_PORT, &wrq)?;

    CLIENT_PUTS.fetch_add(1, Ordering::Relaxed);

    let mut server_port: u16 = 0;
    let mut current_block: u16 = 0; // Waiting for ACK 0 (WRQ ack).
    let mut offset: usize = 0;
    let mut retries = 0u32;
    let mut last_sent_ns = crate::hrtimer::now_ns();

    loop {
        super::poll();

        if let Some(dgram) = super::udp::recv(handle) {
            let opcode = parse_opcode(&dgram.data);

            match opcode {
                Some(OP_ACK) => {
                    let block = parse_block_num(&dgram.data).unwrap_or(0);

                    // First ACK (block 0) establishes server TID.
                    if server_port == 0 {
                        server_port = dgram.src_port;
                    } else if dgram.src_port != server_port {
                        let err = build_error(ERR_UNDEFINED, "Wrong TID");
                        let _ = super::udp::send(local_port, dgram.src_ip, dgram.src_port, &err);
                        continue;
                    }

                    if block == current_block {
                        // ACK received for current block — send next.
                        current_block = current_block.wrapping_add(1);
                        retries = 0;

                        let end = offset.saturating_add(BLOCK_SIZE).min(data.len());
                        let chunk = data.get(offset..end).unwrap_or(&[]);
                        let pkt = build_data(current_block, chunk);
                        let _ = super::udp::send(local_port, server_ip, server_port, &pkt);
                        last_sent_ns = crate::hrtimer::now_ns();

                        let chunk_len = end.saturating_sub(offset);
                        offset = end;

                        // Last block sent (short block).
                        if chunk_len < BLOCK_SIZE {
                            // Wait for final ACK.
                            for _ in 0..50_000 {
                                super::poll();
                                if let Some(ack_dgram) = super::udp::recv(handle) {
                                    if parse_opcode(&ack_dgram.data) == Some(OP_ACK)
                                        && parse_block_num(&ack_dgram.data) == Some(current_block)
                                    {
                                        break;
                                    }
                                }
                                for _ in 0..1_000 {
                                    core::hint::spin_loop();
                                }
                            }
                            CLIENT_BYTES_TX.fetch_add(data.len() as u64, Ordering::Relaxed);
                            super::udp::close(handle);
                            return Ok(());
                        }
                    }
                }
                Some(OP_ERROR) => {
                    let (code, msg) = parse_error(&dgram.data)
                        .unwrap_or((0, String::from("Unknown error")));
                    super::udp::close(handle);
                    CLIENT_ERRORS.fetch_add(1, Ordering::Relaxed);
                    crate::serial_println!("[tftp] Server error {}: {}", code, msg);
                    return Err(match code {
                        ERR_FILE_NOT_FOUND => KernelError::NotFound,
                        ERR_ACCESS_VIOLATION => KernelError::PermissionDenied,
                        ERR_FILE_EXISTS => KernelError::InternalError,
                        _ => KernelError::InternalError,
                    });
                }
                _ => {}
            }
        }

        // Timeout check.
        let now = crate::hrtimer::now_ns();
        if now.saturating_sub(last_sent_ns) >= RETRANSMIT_TIMEOUT_NS {
            retries = retries.saturating_add(1);
            if retries > MAX_RETRIES {
                super::udp::close(handle);
                CLIENT_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::TimedOut);
            }

            // Retransmit last packet.
            if current_block == 0 && server_port == 0 {
                let _ = super::udp::send(local_port, server_ip, TFTP_PORT, &wrq);
            } else {
                let start = offset.saturating_sub(BLOCK_SIZE).min(offset);
                let chunk = data.get(start..offset).unwrap_or(&[]);
                let pkt = build_data(current_block, chunk);
                let _ = super::udp::send(local_port, server_ip, server_port, &pkt);
            }
            last_sent_ns = now;
        }

        for _ in 0..1_000 {
            core::hint::spin_loop();
        }
    }
}

// ---------------------------------------------------------------------------
// IPv6 client
// ---------------------------------------------------------------------------

/// Download a file from a TFTP server over IPv6.
///
/// Same protocol as [`get`] but uses UDP over IPv6 transport.
pub fn get_v6(server_ip: Ipv6Addr, filename: &str) -> KernelResult<Vec<u8>> {
    let local_port = ephemeral_port();
    let handle = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    // Send RRQ over IPv6.
    let rrq = build_rrq(filename);
    super::udp::send_v6(local_port, server_ip, TFTP_PORT, &rrq)?;

    CLIENT_GETS.fetch_add(1, Ordering::Relaxed);

    let mut file_data = Vec::with_capacity(BLOCK_SIZE * 16);
    let mut expected_block: u16 = 1;
    let mut server_port: u16 = 0; // TID: set from first DATA packet.
    let mut retries = 0u32;
    let mut last_sent_ns = crate::hrtimer::now_ns();

    loop {
        super::poll();

        if let Some(dgram) = super::udp::recv_v6(handle) {
            let opcode = parse_opcode(&dgram.data);

            match opcode {
                Some(OP_DATA) => {
                    let block = parse_block_num(&dgram.data).unwrap_or(0);

                    if server_port == 0 {
                        server_port = dgram.src_port;
                    } else if dgram.src_port != server_port {
                        let err = build_error(ERR_UNDEFINED, "Wrong TID");
                        let _ = super::udp::send_v6(local_port, dgram.src_ip, dgram.src_port, &err);
                        continue;
                    }

                    if block == expected_block {
                        if let Some(payload) = dgram.data.get(4..) {
                            if file_data.len().saturating_add(payload.len()) > MAX_FILE_SIZE {
                                let err = build_error(ERR_UNDEFINED, "File too large");
                                let _ = super::udp::send_v6(local_port, server_ip, server_port, &err);
                                super::udp::close(handle);
                                return Err(KernelError::ResourceExhausted);
                            }
                            file_data.extend_from_slice(payload);

                            let ack = build_ack(block);
                            let _ = super::udp::send_v6(local_port, server_ip, server_port, &ack);
                            last_sent_ns = crate::hrtimer::now_ns();
                            retries = 0;

                            // Short block = EOF.
                            if payload.len() < BLOCK_SIZE {
                                CLIENT_BYTES_RX.fetch_add(file_data.len() as u64, Ordering::Relaxed);
                                super::udp::close(handle);
                                return Ok(file_data);
                            }

                            expected_block = expected_block.wrapping_add(1);
                        }
                    } else if block < expected_block {
                        // Duplicate — re-ACK.
                        let ack = build_ack(block);
                        let _ = super::udp::send_v6(local_port, server_ip, server_port, &ack);
                    }
                }
                Some(OP_ERROR) => {
                    let (code, msg) = parse_error(&dgram.data)
                        .unwrap_or((0, String::from("Unknown error")));
                    super::udp::close(handle);
                    CLIENT_ERRORS.fetch_add(1, Ordering::Relaxed);
                    crate::serial_println!("[tftp] Server error {}: {}", code, msg);
                    return Err(match code {
                        ERR_FILE_NOT_FOUND => KernelError::NotFound,
                        ERR_ACCESS_VIOLATION => KernelError::PermissionDenied,
                        _ => KernelError::InternalError,
                    });
                }
                _ => {}
            }
        }

        // Check for timeout.
        let now = crate::hrtimer::now_ns();
        if now.saturating_sub(last_sent_ns) >= RETRANSMIT_TIMEOUT_NS {
            retries = retries.saturating_add(1);
            if retries > MAX_RETRIES {
                super::udp::close(handle);
                CLIENT_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::TimedOut);
            }

            if expected_block == 1 && server_port == 0 {
                let _ = super::udp::send_v6(local_port, server_ip, TFTP_PORT, &rrq);
            } else {
                let ack = build_ack(expected_block.wrapping_sub(1));
                let _ = super::udp::send_v6(local_port, server_ip, server_port, &ack);
            }
            last_sent_ns = now;
        }

        for _ in 0..1_000 {
            core::hint::spin_loop();
        }
    }
}

/// Upload a file to a TFTP server over IPv6.
///
/// Same protocol as [`put`] but uses UDP over IPv6 transport.
pub fn put_v6(server_ip: Ipv6Addr, filename: &str, data: &[u8]) -> KernelResult<()> {
    let local_port = ephemeral_port();
    let handle = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    // Send WRQ over IPv6.
    let wrq = build_wrq(filename);
    super::udp::send_v6(local_port, server_ip, TFTP_PORT, &wrq)?;

    CLIENT_PUTS.fetch_add(1, Ordering::Relaxed);

    let mut server_port: u16 = 0;
    let mut current_block: u16 = 0; // Waiting for ACK 0 (WRQ ack).
    let mut offset: usize = 0;
    let mut retries = 0u32;
    let mut last_sent_ns = crate::hrtimer::now_ns();

    loop {
        super::poll();

        if let Some(dgram) = super::udp::recv_v6(handle) {
            let opcode = parse_opcode(&dgram.data);

            match opcode {
                Some(OP_ACK) => {
                    let block = parse_block_num(&dgram.data).unwrap_or(0);

                    if server_port == 0 {
                        server_port = dgram.src_port;
                    } else if dgram.src_port != server_port {
                        let err = build_error(ERR_UNDEFINED, "Wrong TID");
                        let _ = super::udp::send_v6(local_port, dgram.src_ip, dgram.src_port, &err);
                        continue;
                    }

                    if block == current_block {
                        current_block = current_block.wrapping_add(1);
                        retries = 0;

                        let end = offset.saturating_add(BLOCK_SIZE).min(data.len());
                        let chunk = data.get(offset..end).unwrap_or(&[]);
                        let pkt = build_data(current_block, chunk);
                        let _ = super::udp::send_v6(local_port, server_ip, server_port, &pkt);
                        last_sent_ns = crate::hrtimer::now_ns();

                        let chunk_len = end.saturating_sub(offset);
                        offset = end;

                        // Last block sent (short block).
                        if chunk_len < BLOCK_SIZE {
                            for _ in 0..50_000 {
                                super::poll();
                                if let Some(ack_dgram) = super::udp::recv_v6(handle) {
                                    if parse_opcode(&ack_dgram.data) == Some(OP_ACK)
                                        && parse_block_num(&ack_dgram.data) == Some(current_block)
                                    {
                                        break;
                                    }
                                }
                                for _ in 0..1_000 {
                                    core::hint::spin_loop();
                                }
                            }
                            CLIENT_BYTES_TX.fetch_add(data.len() as u64, Ordering::Relaxed);
                            super::udp::close(handle);
                            return Ok(());
                        }
                    }
                }
                Some(OP_ERROR) => {
                    let (code, msg) = parse_error(&dgram.data)
                        .unwrap_or((0, String::from("Unknown error")));
                    super::udp::close(handle);
                    CLIENT_ERRORS.fetch_add(1, Ordering::Relaxed);
                    crate::serial_println!("[tftp] Server error {}: {}", code, msg);
                    return Err(match code {
                        ERR_FILE_NOT_FOUND => KernelError::NotFound,
                        ERR_ACCESS_VIOLATION => KernelError::PermissionDenied,
                        ERR_FILE_EXISTS => KernelError::InternalError,
                        _ => KernelError::InternalError,
                    });
                }
                _ => {}
            }
        }

        // Timeout check.
        let now = crate::hrtimer::now_ns();
        if now.saturating_sub(last_sent_ns) >= RETRANSMIT_TIMEOUT_NS {
            retries = retries.saturating_add(1);
            if retries > MAX_RETRIES {
                super::udp::close(handle);
                CLIENT_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::TimedOut);
            }

            if current_block == 0 && server_port == 0 {
                let _ = super::udp::send_v6(local_port, server_ip, TFTP_PORT, &wrq);
            } else {
                let start = offset.saturating_sub(BLOCK_SIZE).min(offset);
                let chunk = data.get(start..offset).unwrap_or(&[]);
                let pkt = build_data(current_block, chunk);
                let _ = super::udp::send_v6(local_port, server_ip, server_port, &pkt);
            }
            last_sent_ns = now;
        }

        for _ in 0..1_000 {
            core::hint::spin_loop();
        }
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// An active server-side transfer.
struct ServerTransfer {
    /// Whether this slot is in use.
    active: bool,
    /// Client IP address.
    client_ip: Ipv4Addr,
    /// Client port (TID).
    client_port: u16,
    /// UDP socket handle for this transfer.
    socket_handle: usize,
    /// Local port for this transfer.
    local_port: u16,
    /// File data being sent.
    file_data: Vec<u8>,
    /// Current block number being sent.
    current_block: u16,
    /// Byte offset into file_data.
    offset: usize,
    /// Last packet send timestamp (ns).
    last_sent_ns: u64,
    /// Retry count for current block.
    retries: u32,
    /// Whether we're reading (sending data to client).
    is_read: bool,
    /// Received data buffer (for write transfers).
    recv_data: Vec<u8>,
    /// Filename (for logging).
    filename: String,
}

impl ServerTransfer {
    const fn empty() -> Self {
        Self {
            active: false,
            client_ip: Ipv4Addr([0, 0, 0, 0]),
            client_port: 0,
            socket_handle: 0,
            local_port: 0,
            file_data: Vec::new(),
            current_block: 0,
            offset: 0,
            last_sent_ns: 0,
            retries: 0,
            is_read: true,
            recv_data: Vec::new(),
            filename: String::new(),
        }
    }
}

struct TftpServerState {
    /// Main UDP socket on port 69 for receiving requests.
    listener_handle: Option<usize>,
    /// Active transfers.
    transfers: [ServerTransfer; MAX_SERVER_TRANSFERS],
    /// Root directory for served files.
    root_path: String,
}

impl TftpServerState {
    const fn new() -> Self {
        Self {
            listener_handle: None,
            transfers: [
                ServerTransfer::empty(),
                ServerTransfer::empty(),
                ServerTransfer::empty(),
                ServerTransfer::empty(),
            ],
            root_path: String::new(),
        }
    }
}

static SERVER_STATE: Mutex<TftpServerState> = Mutex::new(TftpServerState::new());
static SERVER_ENABLED: AtomicBool = AtomicBool::new(false);
static LAST_SERVER_TICK: AtomicU64 = AtomicU64::new(0);

// Client statistics.
static CLIENT_GETS: AtomicU64 = AtomicU64::new(0);
static CLIENT_PUTS: AtomicU64 = AtomicU64::new(0);
static CLIENT_BYTES_RX: AtomicU64 = AtomicU64::new(0);
static CLIENT_BYTES_TX: AtomicU64 = AtomicU64::new(0);
static CLIENT_ERRORS: AtomicU64 = AtomicU64::new(0);
static CLIENT_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

// Server statistics.
static SERVER_REQUESTS: AtomicU64 = AtomicU64::new(0);
static SERVER_COMPLETED: AtomicU64 = AtomicU64::new(0);
static SERVER_ERRORS: AtomicU64 = AtomicU64::new(0);
static SERVER_BYTES_TX: AtomicU64 = AtomicU64::new(0);

/// Start the TFTP server on port 69.
///
/// Files are served from the specified root directory via the kernel VFS.
pub fn start_server(root_path: &str) -> KernelResult<()> {
    if SERVER_ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }

    let handle = super::udp::bind(crate::netns::ROOT_NS, TFTP_PORT)?;
    let mut state = SERVER_STATE.lock();
    state.listener_handle = Some(handle);
    state.root_path = String::from(root_path);

    SERVER_ENABLED.store(true, Ordering::Relaxed);
    crate::serial_println!("[tftp] Server started (root={})", root_path);
    Ok(())
}

/// Stop the TFTP server.
pub fn stop_server() {
    SERVER_ENABLED.store(false, Ordering::Relaxed);

    let mut state = SERVER_STATE.lock();

    // Close all active transfers.
    for xfer in &mut state.transfers {
        if xfer.active {
            super::udp::close(xfer.socket_handle);
            xfer.active = false;
        }
    }

    // Close listener socket.
    if let Some(handle) = state.listener_handle.take() {
        super::udp::close(handle);
    }

    crate::serial_println!("[tftp] Server stopped");
}

/// Process incoming TFTP requests and ongoing transfers.
fn server_tick() {
    if !SERVER_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let last = LAST_SERVER_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) < SERVER_TICK_INTERVAL_NS {
        return;
    }
    LAST_SERVER_TICK.store(now, Ordering::Relaxed);

    let mut state = SERVER_STATE.lock();

    // Process incoming requests on port 69.
    if let Some(listener) = state.listener_handle {
        while let Some(dgram) = super::udp::recv(listener) {
            let opcode = parse_opcode(&dgram.data);
            match opcode {
                Some(OP_RRQ) => {
                    handle_rrq(&mut state, dgram.src_ip, dgram.src_port, &dgram.data);
                }
                Some(OP_WRQ) => {
                    handle_wrq(&mut state, dgram.src_ip, dgram.src_port, &dgram.data);
                }
                _ => {
                    // Unexpected packet on port 69 — send error.
                    let err = build_error(ERR_ILLEGAL_OP, "Expected RRQ or WRQ");
                    let _ = super::udp::send(TFTP_PORT, dgram.src_ip, dgram.src_port, &err);
                }
            }
        }
    }

    // Process ongoing transfers.
    for xfer in &mut state.transfers {
        if !xfer.active {
            continue;
        }

        // Read incoming packets for this transfer.
        while let Some(dgram) = super::udp::recv(xfer.socket_handle) {
            if dgram.src_ip != xfer.client_ip || dgram.src_port != xfer.client_port {
                continue; // Wrong TID.
            }

            let opcode = parse_opcode(&dgram.data);

            if xfer.is_read {
                // We're sending data — expect ACKs.
                if opcode == Some(OP_ACK) {
                    let block = parse_block_num(&dgram.data).unwrap_or(0);
                    if block == xfer.current_block {
                        // ACK for current block — send next.
                        xfer.current_block = xfer.current_block.wrapping_add(1);
                        xfer.retries = 0;

                        let end = xfer.offset.saturating_add(BLOCK_SIZE).min(xfer.file_data.len());
                        let chunk = xfer.file_data.get(xfer.offset..end).unwrap_or(&[]);
                        let pkt = build_data(xfer.current_block, chunk);
                        let _ = super::udp::send(xfer.local_port, xfer.client_ip, xfer.client_port, &pkt);
                        xfer.last_sent_ns = crate::hrtimer::now_ns();

                        let chunk_len = end.saturating_sub(xfer.offset);
                        SERVER_BYTES_TX.fetch_add(chunk_len as u64, Ordering::Relaxed);
                        xfer.offset = end;

                        // Final ACK for short block = transfer complete.
                        if chunk_len < BLOCK_SIZE {
                            super::udp::close(xfer.socket_handle);
                            xfer.active = false;
                            SERVER_COMPLETED.fetch_add(1, Ordering::Relaxed);
                            crate::serial_println!(
                                "[tftp] Read transfer complete: {} ({} bytes) to {}:{}",
                                xfer.filename, xfer.file_data.len(),
                                xfer.client_ip, xfer.client_port,
                            );
                        }
                    }
                } else if opcode == Some(OP_ERROR) {
                    // Client error — abort transfer.
                    super::udp::close(xfer.socket_handle);
                    xfer.active = false;
                    SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                // We're receiving data — expect DATA blocks.
                if opcode == Some(OP_DATA) {
                    let block = parse_block_num(&dgram.data).unwrap_or(0);
                    let expected = xfer.current_block.wrapping_add(1);
                    if block == expected {
                        if let Some(payload) = dgram.data.get(4..) {
                            // Enforce size limit to prevent heap exhaustion.
                            if xfer.recv_data.len().saturating_add(payload.len()) > MAX_FILE_SIZE {
                                let err = build_error(ERR_UNDEFINED, "File too large");
                                let _ = super::udp::send(xfer.local_port, xfer.client_ip, xfer.client_port, &err);
                                super::udp::close(xfer.socket_handle);
                                xfer.active = false;
                                SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                            xfer.recv_data.extend_from_slice(payload);
                            xfer.current_block = block;

                            // ACK.
                            let ack = build_ack(block);
                            let _ = super::udp::send(xfer.local_port, xfer.client_ip, xfer.client_port, &ack);
                            xfer.last_sent_ns = crate::hrtimer::now_ns();
                            xfer.retries = 0;

                            // Short block = EOF.
                            if payload.len() < BLOCK_SIZE {
                                // Write received data to filesystem.
                                let _ = write_received_file(&xfer.filename, &xfer.recv_data);
                                super::udp::close(xfer.socket_handle);
                                xfer.active = false;
                                SERVER_COMPLETED.fetch_add(1, Ordering::Relaxed);
                                crate::serial_println!(
                                    "[tftp] Write transfer complete: {} ({} bytes) from {}:{}",
                                    xfer.filename, xfer.recv_data.len(),
                                    xfer.client_ip, xfer.client_port,
                                );
                            }
                        }
                    }
                }
            }
        }

        // Retransmit timeout for active transfers.
        if xfer.active {
            let now = crate::hrtimer::now_ns();
            if now.saturating_sub(xfer.last_sent_ns) >= RETRANSMIT_TIMEOUT_NS {
                xfer.retries = xfer.retries.saturating_add(1);
                if xfer.retries > MAX_RETRIES {
                    crate::serial_println!(
                        "[tftp] Transfer timeout for {}:{} ({})",
                        xfer.client_ip, xfer.client_port, xfer.filename
                    );
                    super::udp::close(xfer.socket_handle);
                    xfer.active = false;
                    SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
                } else if xfer.is_read {
                    // Retransmit current data block.
                    let start = xfer.offset.saturating_sub(BLOCK_SIZE).min(xfer.offset);
                    let chunk = xfer.file_data.get(start..xfer.offset).unwrap_or(&[]);
                    let pkt = build_data(xfer.current_block, chunk);
                    let _ = super::udp::send(xfer.local_port, xfer.client_ip, xfer.client_port, &pkt);
                    xfer.last_sent_ns = now;
                } else {
                    // Retransmit last ACK.
                    let ack = build_ack(xfer.current_block);
                    let _ = super::udp::send(xfer.local_port, xfer.client_ip, xfer.client_port, &ack);
                    xfer.last_sent_ns = now;
                }
            }
        }
    }
}

/// Handle a RRQ (read request) from a client.
fn handle_rrq(state: &mut TftpServerState, client_ip: Ipv4Addr, client_port: u16, data: &[u8]) {
    SERVER_REQUESTS.fetch_add(1, Ordering::Relaxed);

    let (filename, mode) = match parse_request(data) {
        Some(r) => r,
        None => {
            let err = build_error(ERR_UNDEFINED, "Malformed request");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            return;
        }
    };

    // Only support octet mode.
    if !mode.eq_ignore_ascii_case("octet") {
        let err = build_error(ERR_UNDEFINED, "Only octet mode supported");
        let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
        return;
    }

    // Sanitize filename: reject path traversal attempts.
    // A remote client must not be able to read files outside the TFTP root.
    if filename.contains("..") || filename.starts_with('/') || filename.starts_with('\\') {
        let err = build_error(ERR_ACCESS_VIOLATION, "Invalid filename");
        let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
        SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Read the file from VFS.
    let full_path = format!("{}/{}", state.root_path, filename);
    let file_data = match crate::fs::vfs::Vfs::read_file(&full_path) {
        Ok(d) => d,
        Err(_) => {
            let err = build_error(ERR_FILE_NOT_FOUND, "File not found");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Find a free transfer slot.
    let slot = state.transfers.iter().position(|t| !t.active);
    let idx = match slot {
        Some(i) => i,
        None => {
            let err = build_error(ERR_UNDEFINED, "Server busy");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Bind ephemeral port for this transfer.
    let local_port = ephemeral_port();
    let sock = match super::udp::bind(crate::netns::ROOT_NS, local_port) {
        Ok(h) => h,
        Err(_) => {
            let err = build_error(ERR_UNDEFINED, "Internal error");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Send first data block.
    let first_end = BLOCK_SIZE.min(file_data.len());
    let first_chunk = file_data.get(..first_end).unwrap_or(&[]);
    let pkt = build_data(1, first_chunk);
    let _ = super::udp::send(local_port, client_ip, client_port, &pkt);

    let xfer = &mut state.transfers[idx];
    xfer.active = true;
    xfer.client_ip = client_ip;
    xfer.client_port = client_port;
    xfer.socket_handle = sock;
    xfer.local_port = local_port;
    xfer.file_data = file_data;
    xfer.current_block = 1;
    xfer.offset = first_end;
    xfer.last_sent_ns = crate::hrtimer::now_ns();
    xfer.retries = 0;
    xfer.is_read = true;
    xfer.recv_data = Vec::new();
    xfer.filename = filename;

    crate::serial_println!(
        "[tftp] Read request from {}:{} for '{}'",
        client_ip, client_port, xfer.filename
    );
}

/// Handle a WRQ (write request) from a client.
fn handle_wrq(state: &mut TftpServerState, client_ip: Ipv4Addr, client_port: u16, data: &[u8]) {
    SERVER_REQUESTS.fetch_add(1, Ordering::Relaxed);

    let (filename, mode) = match parse_request(data) {
        Some(r) => r,
        None => {
            let err = build_error(ERR_UNDEFINED, "Malformed request");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            return;
        }
    };

    if !mode.eq_ignore_ascii_case("octet") {
        let err = build_error(ERR_UNDEFINED, "Only octet mode supported");
        let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
        return;
    }

    // Sanitize filename: reject path traversal attempts.
    if filename.contains("..") || filename.starts_with('/') || filename.starts_with('\\') {
        let err = build_error(ERR_ACCESS_VIOLATION, "Invalid filename");
        let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
        SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Check if file already exists.
    let full_path = format!("{}/{}", state.root_path, filename);
    if crate::fs::vfs::Vfs::read_file(&full_path).is_ok() {
        let err = build_error(ERR_FILE_EXISTS, "File already exists");
        let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
        SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Find a free slot.
    let slot = state.transfers.iter().position(|t| !t.active);
    let idx = match slot {
        Some(i) => i,
        None => {
            let err = build_error(ERR_UNDEFINED, "Server busy");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    let local_port = ephemeral_port();
    let sock = match super::udp::bind(crate::netns::ROOT_NS, local_port) {
        Ok(h) => h,
        Err(_) => {
            let err = build_error(ERR_UNDEFINED, "Internal error");
            let _ = super::udp::send(TFTP_PORT, client_ip, client_port, &err);
            SERVER_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Send ACK 0 to acknowledge WRQ.
    let ack = build_ack(0);
    let _ = super::udp::send(local_port, client_ip, client_port, &ack);

    let xfer = &mut state.transfers[idx];
    xfer.active = true;
    xfer.client_ip = client_ip;
    xfer.client_port = client_port;
    xfer.socket_handle = sock;
    xfer.local_port = local_port;
    xfer.file_data = Vec::new();
    xfer.current_block = 0;
    xfer.offset = 0;
    xfer.last_sent_ns = crate::hrtimer::now_ns();
    xfer.retries = 0;
    xfer.is_read = false;
    xfer.recv_data = Vec::with_capacity(BLOCK_SIZE * 16);
    xfer.filename = full_path;

    crate::serial_println!(
        "[tftp] Write request from {}:{} for '{}'",
        client_ip, client_port, filename
    );
}

/// Write received data to the filesystem.
fn write_received_file(path: &str, data: &[u8]) -> KernelResult<()> {
    crate::fs::vfs::Vfs::write_file(path, data)
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Generate an ephemeral UDP port for a transfer.
fn ephemeral_port() -> u16 {
    static NEXT_PORT: AtomicU64 = AtomicU64::new(49200);
    let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    // Wrap to ephemeral range 49152-65535.
    let range = 65535u64.saturating_sub(49152).saturating_add(1);
    (49152u64.saturating_add(port % range)) as u16
}

// ---------------------------------------------------------------------------
// Periodic tick
// ---------------------------------------------------------------------------

/// Periodic tick for the TFTP server.
///
/// Called from `net::poll()`.
pub fn tick() {
    server_tick();
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// TFTP statistics.
#[derive(Debug)]
pub struct TftpStats {
    pub client_gets: u64,
    pub client_puts: u64,
    pub client_bytes_rx: u64,
    pub client_bytes_tx: u64,
    pub client_errors: u64,
    pub client_timeouts: u64,
    pub server_enabled: bool,
    pub server_requests: u64,
    pub server_completed: u64,
    pub server_errors: u64,
    pub server_bytes_tx: u64,
    pub active_transfers: usize,
    pub server_root: String,
}

/// Get TFTP statistics.
pub fn stats() -> TftpStats {
    let state = SERVER_STATE.lock();
    TftpStats {
        client_gets: CLIENT_GETS.load(Ordering::Relaxed),
        client_puts: CLIENT_PUTS.load(Ordering::Relaxed),
        client_bytes_rx: CLIENT_BYTES_RX.load(Ordering::Relaxed),
        client_bytes_tx: CLIENT_BYTES_TX.load(Ordering::Relaxed),
        client_errors: CLIENT_ERRORS.load(Ordering::Relaxed),
        client_timeouts: CLIENT_TIMEOUTS.load(Ordering::Relaxed),
        server_enabled: SERVER_ENABLED.load(Ordering::Relaxed),
        server_requests: SERVER_REQUESTS.load(Ordering::Relaxed),
        server_completed: SERVER_COMPLETED.load(Ordering::Relaxed),
        server_errors: SERVER_ERRORS.load(Ordering::Relaxed),
        server_bytes_tx: SERVER_BYTES_TX.load(Ordering::Relaxed),
        active_transfers: state.transfers.iter().filter(|t| t.active).count(),
        server_root: state.root_path.clone(),
    }
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/tftp`.
pub fn procfs_content() -> String {
    let s = stats();

    let mut out = String::with_capacity(512);
    out.push_str("TFTP Client/Server\n");
    out.push_str("==================\n\n");

    out.push_str("Client:\n");
    out.push_str(&format!("  Downloads:   {} ({} bytes)\n", s.client_gets, s.client_bytes_rx));
    out.push_str(&format!("  Uploads:     {} ({} bytes)\n", s.client_puts, s.client_bytes_tx));
    out.push_str(&format!("  Errors:      {}\n", s.client_errors));
    out.push_str(&format!("  Timeouts:    {}\n", s.client_timeouts));

    out.push_str(&format!("\nServer:        {}\n",
        if s.server_enabled { "running" } else { "stopped" }));
    if s.server_enabled {
        out.push_str(&format!("  Root:        {}\n", s.server_root));
    }
    out.push_str(&format!("  Requests:    {}\n", s.server_requests));
    out.push_str(&format!("  Completed:   {}\n", s.server_completed));
    out.push_str(&format!("  Errors:      {}\n", s.server_errors));
    out.push_str(&format!("  Bytes TX:    {}\n", s.server_bytes_tx));
    out.push_str(&format!("  Active:      {}/{}\n", s.active_transfers, MAX_SERVER_TRANSFERS));

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run TFTP self-tests.
// Self-tests deliberately runtime-assert TFTP opcodes and error
// codes as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[tftp] Running TFTP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: RRQ packet construction ---
    {
        let pkt = build_rrq("test.txt");
        // Opcode should be 00 01 (RRQ).
        assert!(*pkt.first().unwrap_or(&99) == 0, "RRQ opcode high");
        assert!(*pkt.get(1).unwrap_or(&99) == 1, "RRQ opcode low");
        // Filename should follow.
        assert!(pkt.get(2..10) == Some(b"test.txt" as &[u8]), "RRQ filename");
        // Null terminator.
        assert!(*pkt.get(10).unwrap_or(&99) == 0, "RRQ null");
        // Mode.
        assert!(pkt.get(11..16) == Some(b"octet" as &[u8]), "RRQ mode");
        assert!(*pkt.get(16).unwrap_or(&99) == 0, "RRQ mode null");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 1 (RRQ construction) PASSED");
    }

    // --- Test 2: DATA packet construction ---
    {
        let data = [1u8, 2, 3, 4, 5];
        let pkt = build_data(42, &data);
        assert!(*pkt.first().unwrap_or(&99) == 0, "DATA opcode high");
        assert!(*pkt.get(1).unwrap_or(&99) == 3, "DATA opcode low");
        assert!(*pkt.get(2).unwrap_or(&99) == 0, "block high");
        assert!(*pkt.get(3).unwrap_or(&99) == 42, "block low");
        assert!(pkt.get(4..) == Some(&data[..]), "DATA payload");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 2 (DATA construction) PASSED");
    }

    // --- Test 3: ACK packet construction ---
    {
        let pkt = build_ack(256);
        assert!(pkt.len() == 4, "ACK size");
        assert!(*pkt.first().unwrap_or(&99) == 0, "ACK opcode high");
        assert!(*pkt.get(1).unwrap_or(&99) == 4, "ACK opcode low");
        assert!(*pkt.get(2).unwrap_or(&99) == 1, "block high");
        assert!(*pkt.get(3).unwrap_or(&99) == 0, "block low");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 3 (ACK construction) PASSED");
    }

    // --- Test 4: ERROR packet construction ---
    {
        let pkt = build_error(ERR_FILE_NOT_FOUND, "Not found");
        assert!(*pkt.first().unwrap_or(&99) == 0, "ERROR opcode high");
        assert!(*pkt.get(1).unwrap_or(&99) == 5, "ERROR opcode low");
        assert!(*pkt.get(2).unwrap_or(&99) == 0, "error code high");
        assert!(*pkt.get(3).unwrap_or(&99) == 1, "error code low");
        assert!(pkt.get(4..13) == Some(b"Not found" as &[u8]), "error msg");
        assert!(*pkt.get(13).unwrap_or(&99) == 0, "error null");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 4 (ERROR construction) PASSED");
    }

    // --- Test 5: Opcode parsing ---
    {
        assert!(parse_opcode(&[0, 1]) == Some(OP_RRQ), "parse RRQ");
        assert!(parse_opcode(&[0, 2]) == Some(OP_WRQ), "parse WRQ");
        assert!(parse_opcode(&[0, 3]) == Some(OP_DATA), "parse DATA");
        assert!(parse_opcode(&[0, 4]) == Some(OP_ACK), "parse ACK");
        assert!(parse_opcode(&[0, 5]) == Some(OP_ERROR), "parse ERROR");
        assert!(parse_opcode(&[0]).is_none(), "too short");
        assert!(parse_opcode(&[]).is_none(), "empty");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 5 (opcode parsing) PASSED");
    }

    // --- Test 6: Block number parsing ---
    {
        assert!(parse_block_num(&[0, 3, 0, 1]) == Some(1), "block 1");
        assert!(parse_block_num(&[0, 4, 1, 0]) == Some(256), "block 256");
        assert!(parse_block_num(&[0, 3, 255, 255]) == Some(65535), "block max");
        assert!(parse_block_num(&[0, 3]).is_none(), "too short");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 6 (block number parsing) PASSED");
    }

    // --- Test 7: String parsing ---
    {
        let data = [0, 1, b't', b'e', b's', b't', 0, b'o', b'c', b't', b'e', b't', 0];
        let (s, next) = parse_string(&data, 2).unwrap();
        assert!(s == "test", "parse filename");
        assert!(next == 7, "next offset");
        let (m, _) = parse_string(&data, 7).unwrap();
        assert!(m == "octet", "parse mode");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 7 (string parsing) PASSED");
    }

    // --- Test 8: Request parsing ---
    {
        let rrq = build_rrq("boot/kernel");
        let (filename, mode) = parse_request(&rrq).unwrap();
        assert!(filename == "boot/kernel", "parsed filename");
        assert!(mode == "octet", "parsed mode");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 8 (request parsing) PASSED");
    }

    // --- Test 9: Error parsing ---
    {
        let err = build_error(ERR_ACCESS_VIOLATION, "Permission denied");
        let (code, msg) = parse_error(&err).unwrap();
        assert!(code == ERR_ACCESS_VIOLATION, "error code");
        assert!(msg == "Permission denied", "error message");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 9 (error parsing) PASSED");
    }

    // --- Test 10: WRQ packet construction ---
    {
        let pkt = build_wrq("upload.bin");
        assert!(*pkt.first().unwrap_or(&99) == 0, "WRQ opcode high");
        assert!(*pkt.get(1).unwrap_or(&99) == 2, "WRQ opcode low");
        let (filename, mode) = parse_request(&pkt).unwrap();
        assert!(filename == "upload.bin", "WRQ filename");
        assert!(mode == "octet", "WRQ mode");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 10 (WRQ construction) PASSED");
    }

    // --- Test 11: Ephemeral port generation ---
    {
        let p1 = ephemeral_port();
        let p2 = ephemeral_port();
        assert!(p1 >= 49152, "port in range");
        assert!(p2 >= 49152, "port in range");
        assert!(p1 != p2, "ports unique");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 11 (ephemeral port) PASSED");
    }

    // --- Test 12: Error code constants ---
    {
        assert!(ERR_UNDEFINED == 0, "ERR_UNDEFINED");
        assert!(ERR_FILE_NOT_FOUND == 1, "ERR_FILE_NOT_FOUND");
        assert!(ERR_ACCESS_VIOLATION == 2, "ERR_ACCESS_VIOLATION");
        assert!(ERR_DISK_FULL == 3, "ERR_DISK_FULL");
        assert!(ERR_ILLEGAL_OP == 4, "ERR_ILLEGAL_OP");
        assert!(ERR_UNKNOWN_TID == 5, "ERR_UNKNOWN_TID");
        assert!(ERR_FILE_EXISTS == 6, "ERR_FILE_EXISTS");

        passed = passed.saturating_add(1);
        crate::serial_println!("[tftp]   test 12 (error code constants) PASSED");
    }

    crate::serial_println!("[tftp] All {} self-tests PASSED", passed);
    Ok(())
}
