//! Networking stack (kernel-resident prototype).
//!
//! Provides basic network protocol support:
//! - Ethernet frame parsing/building
//! - ARP (address resolution: IP → MAC)
//! - IPv4 (basic packet parsing/building, no fragmentation)
//! - UDP (connectionless datagrams)
//! - DHCP client (automatic IP configuration)
//!
//! ## Design note
//!
//! The design spec calls for the networking stack to run in userspace
//! as a service.  This kernel-resident implementation is a prototype
//! to validate the virtio-net driver and bring up basic connectivity.
//! It will be migrated to userspace once the driver framework supports
//! device access from user processes.

pub mod arp;
pub mod dhcp;
pub mod dns;
pub mod ethernet;
pub mod firewall;
pub mod icmp;
pub mod interface;
pub mod ipv4;
pub mod tcp;
pub mod udp;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::error::{KernelError, KernelResult};

/// Minimum interval (ns) between TCP keepalive scans.
///
/// 5 seconds — keepalive probes are on the order of tens of seconds,
/// so scanning more frequently than this wastes cycles.
const KEEPALIVE_TICK_INTERVAL_NS: u64 = 5_000_000_000;

/// Timestamp of last keepalive tick.
static LAST_KEEPALIVE_TICK: AtomicU64 = AtomicU64::new(0);

/// Initialize the networking stack.
///
/// Sets up the network interface from the virtio-net device (if present)
/// and starts DHCP to obtain an IP address.
pub fn init() {
    interface::init();
}

/// Process any pending network events (poll-based).
///
/// Reads incoming packets from the NIC and dispatches them to the
/// appropriate protocol handler.  Called from the shell or a timer.
/// Tries both virtio-net and e1000 (whichever is active).
pub fn poll() {
    // Read all pending packets from the active NIC.
    loop {
        let frame = recv_frame();
        match frame {
            Some(data) => {
                if let Err(e) = ethernet::process_frame(&data) {
                    crate::serial_println!("[net] Error processing frame: {:?}", e);
                }
            }
            None => break,
        }
    }

    // Rate-limited periodic maintenance (TCP + DHCP).
    let now = crate::hrtimer::now_ns();
    let last = LAST_KEEPALIVE_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= KEEPALIVE_TICK_INTERVAL_NS {
        LAST_KEEPALIVE_TICK.store(now, Ordering::Relaxed);
        tcp::tick_keepalive();
        tcp::tick_time_wait_cleanup();
        tcp::tick_retransmit();
        tcp::tick_persist();
        dhcp::tick_renewal();
    }
}

/// Receive a single frame from the active NIC.
///
/// Tries virtio-net first, then e1000, then rtl8139.
fn recv_frame() -> Option<Vec<u8>> {
    // Try virtio-net first.
    if let Some(Some(data)) = crate::virtio::net::with_device(|dev| dev.recv()) {
        return Some(data);
    }
    // Fall back to e1000.
    if let Some(Some(data)) = crate::e1000::with_device(|dev| dev.recv()) {
        return Some(data);
    }
    // Fall back to rtl8139.
    if let Some(data) = crate::rtl8139::recv() {
        return Some(data);
    }
    None
}

/// Send an Ethernet frame via the active NIC.
///
/// Used by the IPv4 layer and ARP to transmit packets.
/// Tries virtio-net first, falls back to e1000, then rtl8139.
pub fn send_frame(frame: &[u8]) -> KernelResult<()> {
    // Try virtio-net first.
    if let Some(result) = crate::virtio::net::with_device(|dev| dev.send(frame)) {
        return result;
    }
    // Fall back to e1000.
    if let Some(result) = crate::e1000::with_device(|dev| dev.send(frame)) {
        return result;
    }
    // Fall back to rtl8139.
    if crate::rtl8139::with_device(|_| ()).is_some() {
        return crate::rtl8139::send(frame);
    }
    Err(KernelError::NoSuchDevice)
}

/// Self-test: verify network interface is configured.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net] Running network self-test...");

    if interface::is_up() {
        let info = interface::info();
        crate::serial_println!("[net]   Interface: up");
        crate::serial_println!("[net]   MAC: {}", info.mac);
        crate::serial_println!("[net]   IP: {}", info.ip);
    } else {
        crate::serial_println!("[net]   No network interface (non-fatal)");
    }

    crate::serial_println!("[net] Network self-test PASSED");
    Ok(())
}
