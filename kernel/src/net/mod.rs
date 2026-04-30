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
pub mod interface;
pub mod ipv4;
pub mod tcp;
pub mod udp;

use crate::error::KernelResult;

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
pub fn poll() {
    // Read all pending packets from the NIC.
    loop {
        let frame = match crate::virtio::net::with_device(|dev| dev.recv()) {
            Some(Some(data)) => data,
            _ => break,
        };

        if let Err(e) = ethernet::process_frame(&frame) {
            crate::serial_println!("[net] Error processing frame: {:?}", e);
        }
    }
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
