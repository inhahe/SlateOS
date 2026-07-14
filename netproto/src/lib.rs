//! `netproto` — shared, `no_std`, allocation-free packet parsers and builders.
//!
//! This crate holds the *privilege-free* protocol logic for SlateOS
//! networking: the byte-level parsing and construction of Ethernet, ARP,
//! IPv4, and ICMPv4 frames, plus the RFC 1071 Internet checksum. It has no
//! dependencies, no `std`, and no allocator — every function either borrows a
//! `&[u8]` (parsing) or writes into a caller-provided buffer / returns a
//! fixed-size array (building).
//!
//! It is the first landing of the net→userspace migration Phase 3 (see
//! `net-userspace-migration.md`): the `services/netstack` daemon is built on
//! top of it instead of hand-rolling frame layout, and the kernel-resident
//! stack can migrate onto the same code so there is a single source of truth
//! for wire formats. More protocols (IPv6, UDP, TCP, DNS, DHCP) will be added
//! here incrementally as later Phase 3 increments.
//!
//! ## Design notes
//!
//! - **Bytes, not UTF-8.** Everything is `[u8]`; addresses are fixed arrays.
//! - **No panics on bad input.** Parsers return `None`/`Err` on short or
//!   malformed buffers; they never index past a validated bound.
//! - **Big-endian on the wire.** All multi-byte header fields are network
//!   byte order, handled via `from_be_bytes` / `to_be_bytes`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod arp;
pub mod checksum;
pub mod ethernet;
pub mod icmp;
pub mod ipv4;
pub mod udp;

/// A 6-byte Ethernet MAC address.
pub type MacAddr = [u8; 6];

/// A 4-byte IPv4 address.
pub type Ipv4Addr = [u8; 4];

/// The broadcast MAC address (`ff:ff:ff:ff:ff:ff`).
pub const BROADCAST_MAC: MacAddr = [0xFF; 6];
