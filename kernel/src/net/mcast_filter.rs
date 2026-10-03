//! The network cards' **multicast receive filter**: which multicast Ethernet
//! addresses a card passes up instead of dropping.
//!
//! A card drops a multicast frame unless its filter admits the destination.
//! Until 2026-10-03 the e1000 and rtl8139 drivers admitted none, so nothing
//! sent to a group -- IPv6 neighbour discovery included -- ever arrived on
//! them (known-issues `A-E1000-AND-RTL8139-DROP-EVERY-MULTICAST-FRAME`).
//! Whoever holds the NIC decides what passes:
//!
//! - the **kernel-resident stack**, whose set follows what it is in: the IPv4
//!   all-hosts group (IGMP queries), IPv6 all-nodes (router advertisements,
//!   MLD queries), the solicited-node group of its IPv6 addresses (neighbour
//!   discovery), and every group a socket joined. [`refresh_kernel`] is
//!   called whenever a group enters or leaves `net::udp`'s tables. Its IPv6
//!   addresses -- the link-local and any SLAAC address -- all end in the
//!   MAC-derived EUI-64 interface ID, so one solicited-node group covers
//!   them; temporary (privacy) addresses, if they are ever added, end
//!   elsewhere and must call [`refresh_kernel`] when they change.
//! - the **raw owner** -- the netstack daemon -- which states its set outright
//!   through `SYS_NET_RAW_MCAST` while it holds the claim ([`set_raw`]).
//!   When the claim ends, explicitly ([`raw_released`]) or because the owner
//!   died ([`restore_if_raw_left`], from `net::poll`), the kernel stack's set
//!   comes back.
//!
//! Every present card is programmed, not only the one that transmits:
//! `net::recv_frame` drains them all. virtio-net needs nothing -- without
//! `VIRTIO_NET_F_CTRL_RX`, which the driver does not negotiate, the device
//! receives all multicast.
//!
//! **Lock order:** [`STATE`], then a card's device lock (inside
//! [`program`]). Nothing here calls into `net::raw` or `net::udp` while
//! holding [`STATE`]: the kernel set is computed first, then installed.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::sync::Mutex;

/// Most addresses one filter holds: the limit on a `SYS_NET_RAW_MCAST`
/// caller. A card filters by hash, so it would take any number; the cap
/// bounds the copy from userspace.
pub const MAX_ADDRS: usize = 128;

/// The IPv4 all-hosts group's Ethernet address (224.0.0.1).
pub const ALL_HOSTS_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0x01];
/// The IPv6 all-nodes group's Ethernet address (ff02::1).
pub const ALL_NODES_MAC: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0x01];

/// Who set the filter in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The kernel-resident stack ([`refresh_kernel`]).
    Kernel,
    /// The raw NIC owner ([`set_raw`]).
    Raw,
}

/// The filter in force: who set it and what it passes (sorted, no
/// duplicates).
struct State {
    owner: Owner,
    addrs: Vec<[u8; 6]>,
}

static STATE: Mutex<State> = Mutex::new(State {
    owner: Owner::Kernel,
    addrs: Vec::new(),
});

/// Whether the raw owner's set is the one in force: read on every
/// `net::poll` ([`restore_if_raw_left`]), so kept apart from [`STATE`] to
/// make that one relaxed-cost load.
static RAW_IN_FORCE: AtomicBool = AtomicBool::new(false);

/// Whether `mac` is a group (multicast) address: the low bit of its first
/// byte.
#[must_use]
pub fn is_group(mac: &[u8; 6]) -> bool {
    mac[0] & 1 != 0
}

/// The Ethernet address of the solicited-node group (ff02::1:ffXX:XXXX)
/// for IPv6 addresses ending in the three bytes `tail`: 33:33:ff and those
/// bytes (RFC 4291 §2.7.1, RFC 2464 §7).
#[must_use]
pub fn solicited_node_mac(tail: [u8; 3]) -> [u8; 6] {
    [0x33, 0x33, 0xFF, tail[0], tail[1], tail[2]]
}

/// Program every present card to pass `addrs`.
fn program(addrs: &[[u8; 6]]) {
    // A card that is absent is simply not there to program; virtio-net
    // passes all multicast as it is (module doc).
    let _ = crate::e1000::with_device(|dev| dev.set_multicast(addrs));
    let _ = crate::rtl8139::with_device(|dev| dev.set_multicast(addrs));
}

/// `addrs` sorted, without duplicates, in memory that was reserved
/// fallibly.
fn normalised(addrs: &[[u8; 6]]) -> KernelResult<Vec<[u8; 6]>> {
    let mut list = Vec::new();
    list.try_reserve_exact(addrs.len())
        .map_err(|_| KernelError::OutOfMemory)?;
    list.extend_from_slice(addrs);
    list.sort_unstable();
    list.dedup();
    Ok(list)
}

/// Install `addrs` as the raw owner's filter: every card passes these
/// multicast addresses and no others. The caller has checked it is the raw
/// owner (`SYS_NET_RAW_MCAST`).
///
/// # Errors
///
/// - `InvalidArgument` — more than [`MAX_ADDRS`] addresses, or one that is
///   not a group address (the filter is for multicast; a unicast address
///   here is a caller bug, and the cards would ignore it anyway).
/// - `OutOfMemory` — the list could not be copied.
pub fn set_raw(addrs: &[[u8; 6]]) -> KernelResult<()> {
    if addrs.len() > MAX_ADDRS || !addrs.iter().all(is_group) {
        return Err(KernelError::InvalidArgument);
    }
    let list = normalised(addrs)?;
    let mut state = STATE.lock();
    program(&list);
    state.owner = Owner::Raw;
    state.addrs = list;
    RAW_IN_FORCE.store(true, Ordering::Release);
    Ok(())
}

/// The kernel-resident stack's set: what it is in right now.
fn kernel_set() -> KernelResult<Vec<[u8; 6]>> {
    let groups = super::udp::multicast_groups();
    let groups6 = super::udp::multicast_groups_v6();
    let mut set = Vec::new();
    set.try_reserve_exact(groups.len().saturating_add(groups6.len()).saturating_add(3))
        .map_err(|_| KernelError::OutOfMemory)?;
    set.push(ALL_HOSTS_MAC);
    set.push(ALL_NODES_MAC);
    let mac = super::interface::mac().0;
    if mac != [0; 6] {
        set.push(solicited_node_mac([mac[3], mac[4], mac[5]]));
    }
    for g in groups {
        // 01:00:5e and the group's low 23 bits (RFC 1112 §6.4).
        set.push([0x01, 0x00, 0x5E, g.0[1] & 0x7F, g.0[2], g.0[3]]);
    }
    for g in groups6 {
        // 33:33 and the group's low 32 bits (RFC 2464 §7).
        set.push([0x33, 0x33, g.0[12], g.0[13], g.0[14], g.0[15]]);
    }
    normalised(&set)
}

/// Re-program the cards for the kernel-resident stack's memberships --
/// unless the raw owner's set is in force and its claim still held, in
/// which case that set stands until the owner lets go.
pub fn refresh_kernel() {
    if RAW_IN_FORCE.load(Ordering::Acquire) && super::raw::is_claimed() {
        return;
    }
    let set = match kernel_set() {
        Ok(set) => set,
        Err(e) => {
            // The cards keep the set they had: a group just joined may go
            // unheard, one just left still passes (and the stack drops it).
            crate::serial_println!("[mcast-filter] could not compute the kernel's set: {:?}", e);
            return;
        }
    };
    let mut state = STATE.lock();
    program(&set);
    state.owner = Owner::Kernel;
    state.addrs = set;
    RAW_IN_FORCE.store(false, Ordering::Release);
}

/// The raw owner released the NIC: the kernel stack's set comes back.
pub fn raw_released() {
    RAW_IN_FORCE.store(false, Ordering::Release);
    refresh_kernel();
}

/// Called by `net::poll` once it has seen the claim gone: if the raw
/// owner's set is still in force -- its owner died without releasing -- put
/// the kernel stack's back. One atomic load when there is nothing to do.
pub fn restore_if_raw_left() {
    if RAW_IN_FORCE.load(Ordering::Acquire) {
        raw_released();
    }
}

/// The filter in force: who set it, and the addresses it passes.
///
/// # Errors
///
/// `OutOfMemory` — the copy could not be made.
pub fn current() -> KernelResult<(Owner, Vec<[u8; 6]>)> {
    let state = STATE.lock();
    let mut copy = Vec::new();
    copy.try_reserve_exact(state.addrs.len())
        .map_err(|_| KernelError::OutOfMemory)?;
    copy.extend_from_slice(&state.addrs);
    Ok((state.owner, copy))
}

/// Boot self-test: the two cards' hash functions against values computed
/// independently from QEMU's receive filters, the tables they build, and --
/// on whichever of the cards is present -- that programming the filter
/// reaches the card's registers.
///
/// # Errors
///
/// `InternalError` on any mismatch, after a serial line naming it.
pub fn self_test() -> KernelResult<()> {
    /// Addresses with their e1000 hash and rtl8139 `MAR` index, from a
    /// Python model of QEMU's `e1000x_rx_group_filter` and `net_crc32`
    /// (cross-checked against Linux's `ether_crc`).
    const VECTORS: [([u8; 6], u16, u8); 6] = [
        ([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01], 0x010, 31), // 224.0.0.1
        ([0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB], 0xFB0, 15), // 224.0.0.251
        ([0x01, 0x00, 0x5E, 0x7F, 0xFF, 0xFA], 0xFAF, 43), // 239.255.255.250
        ([0x33, 0x33, 0x00, 0x00, 0x00, 0x01], 0x010, 62), // ff02::1
        ([0x33, 0x33, 0x00, 0x00, 0x00, 0xFB], 0xFB0, 46), // ff02::fb
        ([0x33, 0x33, 0xFF, 0x12, 0x34, 0x56], 0x563, 2),  // solicited-node
    ];
    let fail = |what: &str| {
        crate::serial_println!("[mcast-filter]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    for (mac, hash, index) in VECTORS {
        if crate::e1000::mta_hash(&mac) != hash {
            return fail("e1000 hash disagrees with QEMU's");
        }
        if crate::rtl8139::mar_index(&mac) != index {
            return fail("rtl8139 MAR index disagrees with QEMU's");
        }
    }
    let addrs: Vec<[u8; 6]> = VECTORS.iter().map(|v| v.0).collect();

    // The e1000 table: one bit per distinct hash (four, for six addresses:
    // 224.0.0.1 and ff02::1 share one, as do the two mDNS groups).
    let words = crate::e1000::mta_words(&addrs);
    let bits: u32 = words.iter().map(|w| w.count_ones()).sum();
    if bits != 4 {
        return fail("e1000 table has the wrong number of bits");
    }
    for (_, hash, _) in VECTORS {
        let word = words.get(usize::from(hash >> 5)).copied().unwrap_or(0);
        if word & (1 << (hash & 31)) == 0 {
            return fail("e1000 table misses an address's bit");
        }
    }
    // The rtl8139 MAR: six distinct indices, six bits.
    let mar = crate::rtl8139::mar_bytes(&addrs);
    let mar_bits: u32 = mar.iter().map(|b| b.count_ones()).sum();
    if mar_bits != 6 || crate::rtl8139::mar_bytes(&[]) != [0; 8] {
        return fail("rtl8139 MAR has the wrong bits");
    }
    if is_group(&[0x52, 0x54, 0, 0x12, 0x34, 0x56]) || !is_group(&ALL_HOSTS_MAC) {
        return fail("group-bit test");
    }
    if set_raw(&[[0x52, 0x54, 0, 0x12, 0x34, 0x56]]) != Err(KernelError::InvalidArgument) {
        return fail("a unicast address was taken into the filter");
    }

    // The live cards: what is programmed is what the registers hold. The
    // kernel's own set is put back afterwards (these run before the
    // netstack daemon claims the NIC).
    let mut live = 0u32;
    let e1000_ok = crate::e1000::with_device(|dev| {
        dev.set_multicast(&addrs);
        let ok = dev.read_mta() == crate::e1000::mta_words(&addrs);
        dev.set_multicast(&[]);
        ok && dev.read_mta() == [0; crate::e1000::MTA_WORDS]
    });
    match e1000_ok {
        Some(false) => return fail("e1000 MTA does not read back as programmed"),
        Some(true) => live = live.saturating_add(1),
        None => {}
    }
    let rtl_ok = crate::rtl8139::with_device(|dev| {
        dev.set_multicast(&addrs);
        let ok = dev.read_mar() == crate::rtl8139::mar_bytes(&addrs);
        dev.set_multicast(&[]);
        ok && dev.read_mar() == [0; 8]
    });
    match rtl_ok {
        Some(false) => return fail("rtl8139 MAR does not read back as programmed"),
        Some(true) => live = live.saturating_add(1),
        None => {}
    }
    refresh_kernel();
    let (owner, set) = current()?;
    let has_base = set.contains(&ALL_HOSTS_MAC) && set.contains(&ALL_NODES_MAC);
    if (owner != Owner::Kernel && !super::raw::is_claimed()) || !has_base {
        return fail("the kernel's set lacks all-hosts/all-nodes");
    }
    crate::serial_println!(
        "[mcast-filter] Self-test PASSED: hashes match QEMU's for {} addresses, \
         {} card(s) read back what was programmed, kernel set has {} address(es)",
        VECTORS.len(),
        live,
        set.len()
    );
    Ok(())
}
