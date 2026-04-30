//! ICMP (Internet Control Message Protocol) implementation.
//!
//! Supports ICMP Echo Request/Reply (ping) per RFC 792.
//!
//! ## Echo Request/Reply format
//!
//! ```text
//! Type (8=request, 0=reply) | Code (0) | Checksum
//! Identifier                | Sequence Number
//! Data ...
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::error::{KernelError, KernelResult};

use super::interface::Ipv4Addr;
use super::ipv4::{self, Ipv4Packet, PROTO_ICMP};

// ---------------------------------------------------------------------------
// ICMP types
// ---------------------------------------------------------------------------

/// Echo Reply.
const ICMP_ECHO_REPLY: u8 = 0;
/// Echo Request.
const ICMP_ECHO_REQUEST: u8 = 8;

/// ICMP header size (type + code + checksum + id + seq).
const ICMP_HEADER_SIZE: usize = 8;

/// Ping identifier (fixed for our kernel).
const PING_ID: u16 = 0x1234;

/// Next sequence number.
static PING_SEQ: AtomicU16 = AtomicU16::new(1);

/// Last received ping reply sequence.
static LAST_REPLY_SEQ: AtomicU16 = AtomicU16::new(0);

// ---------------------------------------------------------------------------
// ICMP packet building
// ---------------------------------------------------------------------------

/// Build an ICMP echo request.
#[allow(clippy::arithmetic_side_effects)]
fn build_echo_request(seq: u16) -> Vec<u8> {
    let payload = b"ping from kernel!";
    let total = ICMP_HEADER_SIZE + payload.len();
    let mut pkt = Vec::with_capacity(total);

    // Type: Echo Request.
    pkt.push(ICMP_ECHO_REQUEST);
    // Code: 0.
    pkt.push(0);
    // Checksum placeholder.
    pkt.extend_from_slice(&[0, 0]);
    // Identifier.
    pkt.extend_from_slice(&PING_ID.to_be_bytes());
    // Sequence number.
    pkt.extend_from_slice(&seq.to_be_bytes());
    // Payload.
    pkt.extend_from_slice(payload);

    // Compute checksum.
    let checksum = ipv4::ip_checksum(&pkt);
    pkt[2] = (checksum >> 8) as u8;
    pkt[3] = checksum as u8;

    pkt
}

// ---------------------------------------------------------------------------
// ICMP processing
// ---------------------------------------------------------------------------

/// Process an incoming ICMP packet.
pub fn process_icmp(ip_packet: &Ipv4Packet<'_>) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < ICMP_HEADER_SIZE {
        return Ok(());
    }

    let icmp_type = data[0];
    let _code = data[1];

    match icmp_type {
        ICMP_ECHO_REPLY => {
            // Verify it's our ping.
            if data.len() >= 8 {
                let id = u16::from_be_bytes([data[4], data[5]]);
                let seq = u16::from_be_bytes([data[6], data[7]]);
                if id == PING_ID {
                    LAST_REPLY_SEQ.store(seq, Ordering::Relaxed);
                    crate::serial_println!(
                        "[icmp] Echo reply from {} seq={}",
                        ip_packet.src, seq
                    );
                }
            }
        }
        ICMP_ECHO_REQUEST => {
            // Reply to echo requests (respond to pings directed at us).
            let our_ip = super::interface::ip();
            if !our_ip.is_unspecified() {
                send_echo_reply(ip_packet)?;
            }
        }
        _ => {
            // Other ICMP types — ignore for now.
        }
    }

    Ok(())
}

/// Send an ICMP echo reply in response to a request.
#[allow(clippy::arithmetic_side_effects)]
fn send_echo_reply(request_ip: &Ipv4Packet<'_>) -> KernelResult<()> {
    let data = request_ip.payload;
    if data.len() < ICMP_HEADER_SIZE {
        return Ok(());
    }

    let mut reply = Vec::from(data);
    // Change type to Echo Reply.
    reply[0] = ICMP_ECHO_REPLY;
    // Recompute checksum.
    reply[2] = 0;
    reply[3] = 0;
    let checksum = ipv4::ip_checksum(&reply);
    reply[2] = (checksum >> 8) as u8;
    reply[3] = checksum as u8;

    ipv4::send(request_ip.src, PROTO_ICMP, &reply)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Send an ICMP echo request (ping) to the given IP address.
///
/// Returns the sequence number used.
pub fn ping(dst: Ipv4Addr) -> KernelResult<u16> {
    let seq = PING_SEQ.fetch_add(1, Ordering::Relaxed);
    let pkt = build_echo_request(seq);
    ipv4::send(dst, PROTO_ICMP, &pkt)?;
    Ok(seq)
}

/// Wait for a ping reply with the given sequence number.
///
/// Polls the NIC for up to `timeout_polls` iterations.
/// Returns `true` if the reply was received.
pub fn wait_reply(seq: u16, timeout_polls: u32) -> bool {
    for _ in 0..timeout_polls {
        super::poll();

        if LAST_REPLY_SEQ.load(Ordering::Relaxed) == seq {
            return true;
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }
    false
}
