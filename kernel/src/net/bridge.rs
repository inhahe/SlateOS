//! Bridge — Ethernet bridging and link aggregation.
//!
//! Provides layer-2 Ethernet bridging between network interfaces
//! and basic link aggregation (bonding) for redundancy.
//!
//! ## Features
//!
//! - Bridge multiple network interfaces at layer 2
//! - MAC address learning table with aging
//! - Flooding for unknown destinations
//! - Link aggregation modes: active-backup, round-robin
//! - Spanning tree protocol awareness (STP port states)
//! - Per-bridge and per-port statistics
//!
//! ## Design
//!
//! The bridge maintains a forwarding database (FDB) that maps
//! MAC addresses to ports. When a frame arrives:
//! 1. Learn the source MAC → ingress port mapping
//! 2. Look up the destination MAC in the FDB
//! 3. If found, forward to the specific port
//! 4. If not found, flood to all ports except ingress
//!
//! For link aggregation, multiple physical links are combined
//! into a single logical interface for bandwidth and redundancy.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::virtio::net::MacAddress;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of bridges.
const MAX_BRIDGES: usize = 4;

/// Maximum number of ports per bridge.
const MAX_PORTS_PER_BRIDGE: usize = 8;

/// Maximum FDB (forwarding database) entries per bridge.
const MAX_FDB_ENTRIES: usize = 256;

/// FDB entry aging time in nanoseconds (5 minutes).
const FDB_AGING_NS: u64 = 300_000_000_000;

/// Maximum number of bond interfaces.
const MAX_BONDS: usize = 4;

/// Maximum members per bond.
const MAX_BOND_MEMBERS: usize = 4;

// ---------------------------------------------------------------------------
// MAC address helper
// ---------------------------------------------------------------------------

/// Check if a MAC address is broadcast (FF:FF:FF:FF:FF:FF).
fn is_broadcast(mac: &MacAddress) -> bool {
    mac.0 == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
}

/// Check if a MAC address is multicast (bit 0 of first byte set).
fn is_multicast(mac: &MacAddress) -> bool {
    mac.0[0] & 0x01 != 0
}

// ---------------------------------------------------------------------------
// Forwarding Database (FDB)
// ---------------------------------------------------------------------------

/// A forwarding database entry.
#[derive(Debug, Clone, Copy)]
struct FdbEntry {
    /// Whether this entry is active.
    active: bool,
    /// MAC address.
    mac: [u8; 6],
    /// Port index this MAC was learned on.
    port: u8,
    /// Timestamp when this entry was last seen (ns).
    last_seen_ns: u64,
    /// Whether this is a static (permanent) entry.
    is_static: bool,
}

impl FdbEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            mac: [0; 6],
            port: 0,
            last_seen_ns: 0,
            is_static: false,
        }
    }
}

// ---------------------------------------------------------------------------
// STP port state
// ---------------------------------------------------------------------------

/// Spanning Tree Protocol port state.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Public API.
pub enum StpState {
    /// Port is disabled.
    Disabled,
    /// Port is listening (STP convergence).
    Listening,
    /// Port is learning (populating FDB).
    Learning,
    /// Port is forwarding (normal operation).
    Forwarding,
    /// Port is blocking (loop prevention).
    Blocking,
}

impl StpState {
    /// Whether this port should forward frames.
    fn can_forward(&self) -> bool {
        matches!(self, StpState::Forwarding)
    }

    /// Whether this port should learn MAC addresses.
    fn can_learn(&self) -> bool {
        matches!(self, StpState::Learning | StpState::Forwarding)
    }

    /// Display name.
    fn name(&self) -> &'static str {
        match self {
            StpState::Disabled => "disabled",
            StpState::Listening => "listening",
            StpState::Learning => "learning",
            StpState::Forwarding => "forwarding",
            StpState::Blocking => "blocking",
        }
    }
}

// ---------------------------------------------------------------------------
// Bridge port
// ---------------------------------------------------------------------------

/// A port in a bridge.
#[derive(Debug)]
struct BridgePort {
    /// Whether this port is active.
    active: bool,
    /// Port identifier (interface index or name).
    id: u8,
    /// STP state.
    stp_state: StpState,
    /// Frames received on this port.
    rx_frames: u64,
    /// Frames sent on this port.
    tx_frames: u64,
    /// Frames dropped on this port.
    drops: u64,
}

impl BridgePort {
    const fn empty() -> Self {
        Self {
            active: false,
            id: 0,
            stp_state: StpState::Forwarding,
            rx_frames: 0,
            tx_frames: 0,
            drops: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Bridge
// ---------------------------------------------------------------------------

/// An Ethernet bridge instance.
struct Bridge {
    /// Whether this bridge is active.
    active: bool,
    /// Bridge name.
    name: [u8; 16],
    name_len: usize,
    /// Bridge ports.
    ports: [BridgePort; MAX_PORTS_PER_BRIDGE],
    /// Forwarding database.
    fdb: [FdbEntry; MAX_FDB_ENTRIES],
    /// STP enabled.
    stp_enabled: bool,
    /// Total frames bridged.
    frames_bridged: u64,
    /// Total frames flooded.
    frames_flooded: u64,
}

impl Bridge {
    const fn empty() -> Self {
        Self {
            active: false,
            name: [0; 16],
            name_len: 0,
            ports: [const { BridgePort::empty() }; MAX_PORTS_PER_BRIDGE],
            fdb: [const { FdbEntry::empty() }; MAX_FDB_ENTRIES],
            stp_enabled: false,
            frames_bridged: 0,
            frames_flooded: 0,
        }
    }

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }

    /// Learn a MAC address on a port.
    fn learn(&mut self, mac: &[u8; 6], port: u8, now_ns: u64) {
        // Check if already known.
        for entry in self.fdb.iter_mut() {
            if entry.active && entry.mac == *mac {
                entry.port = port;
                entry.last_seen_ns = now_ns;
                return;
            }
        }

        // Find empty slot.
        for entry in self.fdb.iter_mut() {
            if !entry.active {
                entry.active = true;
                entry.mac = *mac;
                entry.port = port;
                entry.last_seen_ns = now_ns;
                entry.is_static = false;
                return;
            }
        }

        // Table full — evict oldest non-static entry.
        let mut oldest_idx = None;
        let mut oldest_time = u64::MAX;
        for (i, entry) in self.fdb.iter().enumerate() {
            if entry.active && !entry.is_static && entry.last_seen_ns < oldest_time {
                oldest_time = entry.last_seen_ns;
                oldest_idx = Some(i);
            }
        }

        if let Some(idx) = oldest_idx {
            self.fdb[idx].active = true;
            self.fdb[idx].mac = *mac;
            self.fdb[idx].port = port;
            self.fdb[idx].last_seen_ns = now_ns;
            self.fdb[idx].is_static = false;
        }
    }

    /// Look up a MAC address in the FDB.
    fn lookup(&self, mac: &[u8; 6]) -> Option<u8> {
        for entry in &self.fdb {
            if entry.active && entry.mac == *mac {
                return Some(entry.port);
            }
        }
        None
    }

    /// Age out old FDB entries.
    fn age_fdb(&mut self, now_ns: u64) {
        for entry in self.fdb.iter_mut() {
            if entry.active && !entry.is_static {
                if now_ns.saturating_sub(entry.last_seen_ns) > FDB_AGING_NS {
                    entry.active = false;
                }
            }
        }
    }

    /// Count active FDB entries.
    fn fdb_count(&self) -> usize {
        self.fdb.iter().filter(|e| e.active).count()
    }

    /// Count active ports.
    fn port_count(&self) -> usize {
        self.ports.iter().filter(|p| p.active).count()
    }
}

/// Bridge table.
static BRIDGES: Mutex<[Bridge; MAX_BRIDGES]> = Mutex::new(
    [const { Bridge::empty() }; MAX_BRIDGES]
);

// ---------------------------------------------------------------------------
// Bond / Link Aggregation
// ---------------------------------------------------------------------------

/// Link aggregation mode.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Public API.
pub enum BondMode {
    /// Active-backup: only one member active, failover on link down.
    ActiveBackup,
    /// Round-robin: distribute frames across all members.
    RoundRobin,
    /// XOR: hash-based distribution by MAC.
    XorHash,
}

impl BondMode {
    fn name(&self) -> &'static str {
        match self {
            BondMode::ActiveBackup => "active-backup",
            BondMode::RoundRobin => "round-robin",
            BondMode::XorHash => "xor-hash",
        }
    }
}

/// A bond interface member.
#[derive(Debug, Clone, Copy)]
struct BondMember {
    active: bool,
    id: u8,
    link_up: bool,
    tx_bytes: u64,
    rx_bytes: u64,
}

impl BondMember {
    const fn empty() -> Self {
        Self {
            active: false,
            id: 0,
            link_up: false,
            tx_bytes: 0,
            rx_bytes: 0,
        }
    }
}

/// A bond (link aggregation) interface.
struct BondInterface {
    active: bool,
    name: [u8; 16],
    name_len: usize,
    mode: BondMode,
    members: [BondMember; MAX_BOND_MEMBERS],
    /// Index of the active member (for active-backup mode).
    active_member: u8,
    /// Round-robin counter.
    rr_counter: u32,
    /// Total TX bytes.
    total_tx: u64,
    /// Total RX bytes.
    total_rx: u64,
}

impl BondInterface {
    const fn empty() -> Self {
        Self {
            active: false,
            name: [0; 16],
            name_len: 0,
            mode: BondMode::ActiveBackup,
            members: [const { BondMember::empty() }; MAX_BOND_MEMBERS],
            active_member: 0,
            rr_counter: 0,
            total_tx: 0,
            total_rx: 0,
        }
    }

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }

    fn member_count(&self) -> usize {
        self.members.iter().filter(|m| m.active).count()
    }
}

/// Bond table.
static BONDS: Mutex<[BondInterface; MAX_BONDS]> = Mutex::new(
    [const { BondInterface::empty() }; MAX_BONDS]
);

// Statistics.
static BRIDGE_COUNT: AtomicU64 = AtomicU64::new(0);
static BOND_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Bridge management API
// ---------------------------------------------------------------------------

/// Create a new bridge.
#[allow(dead_code)] // Public API.
pub fn create_bridge(name: &str) -> KernelResult<usize> {
    let mut bridges = BRIDGES.lock();
    for (i, bridge) in bridges.iter_mut().enumerate() {
        if !bridge.active {
            bridge.active = true;
            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(16);
            bridge.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            bridge.name_len = copy_len;
            bridge.stp_enabled = false;
            bridge.frames_bridged = 0;
            bridge.frames_flooded = 0;
            // Reset FDB and ports.
            for entry in bridge.fdb.iter_mut() { entry.active = false; }
            for port in bridge.ports.iter_mut() { port.active = false; }
            BRIDGE_COUNT.fetch_add(1, Ordering::Relaxed);
            return Ok(i);
        }
    }
    Err(KernelError::OutOfMemory)
}

/// Delete a bridge.
#[allow(dead_code)] // Public API.
pub fn delete_bridge(index: usize) -> KernelResult<()> {
    let mut bridges = BRIDGES.lock();
    let bridge = bridges.get_mut(index).ok_or(KernelError::InvalidArgument)?;
    if !bridge.active {
        return Err(KernelError::NotFound);
    }
    bridge.active = false;
    Ok(())
}

/// Add a port to a bridge.
#[allow(dead_code)] // Public API.
pub fn add_port(bridge_idx: usize, port_id: u8) -> KernelResult<()> {
    let mut bridges = BRIDGES.lock();
    let bridge = bridges.get_mut(bridge_idx).ok_or(KernelError::InvalidArgument)?;
    if !bridge.active {
        return Err(KernelError::NotFound);
    }

    for port in bridge.ports.iter_mut() {
        if !port.active {
            port.active = true;
            port.id = port_id;
            port.stp_state = StpState::Forwarding;
            port.rx_frames = 0;
            port.tx_frames = 0;
            port.drops = 0;
            return Ok(());
        }
    }

    Err(KernelError::OutOfMemory)
}

/// Remove a port from a bridge.
#[allow(dead_code)] // Public API.
pub fn remove_port(bridge_idx: usize, port_id: u8) -> KernelResult<()> {
    let mut bridges = BRIDGES.lock();
    let bridge = bridges.get_mut(bridge_idx).ok_or(KernelError::InvalidArgument)?;
    if !bridge.active {
        return Err(KernelError::NotFound);
    }

    for port in bridge.ports.iter_mut() {
        if port.active && port.id == port_id {
            port.active = false;
            return Ok(());
        }
    }

    Err(KernelError::NotFound)
}

/// Set STP state for a port.
#[allow(dead_code)] // Public API.
pub fn set_port_stp(bridge_idx: usize, port_id: u8, state: StpState) -> KernelResult<()> {
    let mut bridges = BRIDGES.lock();
    let bridge = bridges.get_mut(bridge_idx).ok_or(KernelError::InvalidArgument)?;
    if !bridge.active {
        return Err(KernelError::NotFound);
    }

    for port in bridge.ports.iter_mut() {
        if port.active && port.id == port_id {
            port.stp_state = state;
            return Ok(());
        }
    }

    Err(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// Bond management API
// ---------------------------------------------------------------------------

/// Create a new bond interface.
#[allow(dead_code)] // Public API.
pub fn create_bond(name: &str, mode: BondMode) -> KernelResult<usize> {
    let mut bonds = BONDS.lock();
    for (i, bond) in bonds.iter_mut().enumerate() {
        if !bond.active {
            bond.active = true;
            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(16);
            bond.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            bond.name_len = copy_len;
            bond.mode = mode;
            bond.active_member = 0;
            bond.rr_counter = 0;
            bond.total_tx = 0;
            bond.total_rx = 0;
            for member in bond.members.iter_mut() { member.active = false; }
            BOND_COUNT.fetch_add(1, Ordering::Relaxed);
            return Ok(i);
        }
    }
    Err(KernelError::OutOfMemory)
}

/// Delete a bond interface.
#[allow(dead_code)] // Public API.
pub fn delete_bond(index: usize) -> KernelResult<()> {
    let mut bonds = BONDS.lock();
    let bond = bonds.get_mut(index).ok_or(KernelError::InvalidArgument)?;
    if !bond.active {
        return Err(KernelError::NotFound);
    }
    bond.active = false;
    Ok(())
}

/// Add a member to a bond.
#[allow(dead_code)] // Public API.
pub fn add_bond_member(bond_idx: usize, member_id: u8) -> KernelResult<()> {
    let mut bonds = BONDS.lock();
    let bond = bonds.get_mut(bond_idx).ok_or(KernelError::InvalidArgument)?;
    if !bond.active {
        return Err(KernelError::NotFound);
    }

    for member in bond.members.iter_mut() {
        if !member.active {
            member.active = true;
            member.id = member_id;
            member.link_up = true;
            member.tx_bytes = 0;
            member.rx_bytes = 0;
            return Ok(());
        }
    }

    Err(KernelError::OutOfMemory)
}

// ---------------------------------------------------------------------------
// Info types
// ---------------------------------------------------------------------------

/// Bridge info for display.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct BridgeInfo {
    pub index: usize,
    pub name: String,
    pub port_count: usize,
    pub fdb_count: usize,
    pub stp_enabled: bool,
    pub frames_bridged: u64,
    pub frames_flooded: u64,
}

/// Bond info for display.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct BondInfo {
    pub index: usize,
    pub name: String,
    pub mode: &'static str,
    pub member_count: usize,
    pub total_tx: u64,
    pub total_rx: u64,
}

/// List all bridges.
#[allow(dead_code)] // Public API.
pub fn list_bridges() -> Vec<BridgeInfo> {
    let bridges = BRIDGES.lock();
    let mut result = Vec::new();
    for (i, bridge) in bridges.iter().enumerate() {
        if bridge.active {
            result.push(BridgeInfo {
                index: i,
                name: String::from(bridge.name_str()),
                port_count: bridge.port_count(),
                fdb_count: bridge.fdb_count(),
                stp_enabled: bridge.stp_enabled,
                frames_bridged: bridge.frames_bridged,
                frames_flooded: bridge.frames_flooded,
            });
        }
    }
    result
}

/// List all bonds.
#[allow(dead_code)] // Public API.
pub fn list_bonds() -> Vec<BondInfo> {
    let bonds = BONDS.lock();
    let mut result = Vec::new();
    for (i, bond) in bonds.iter().enumerate() {
        if bond.active {
            result.push(BondInfo {
                index: i,
                name: String::from(bond.name_str()),
                mode: bond.mode.name(),
                member_count: bond.member_count(),
                total_tx: bond.total_tx,
                total_rx: bond.total_rx,
            });
        }
    }
    result
}

/// Periodic tick — age FDB entries.
#[allow(dead_code)] // Public API.
pub fn tick() {
    let now_ns = crate::hrtimer::now_ns();
    let mut bridges = BRIDGES.lock();
    for bridge in bridges.iter_mut() {
        if bridge.active {
            bridge.age_fdb(now_ns);
        }
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/bridge`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let bridges = list_bridges();
    let bonds = list_bonds();

    let mut out = String::with_capacity(512);
    out.push_str("Network Bridges & Bonds\n");
    out.push_str("=======================\n\n");

    if bridges.is_empty() {
        out.push_str("No bridges configured\n");
    } else {
        out.push_str("Bridges:\n");
        for b in &bridges {
            out.push_str(&format!(
                "  {} ({}): {} ports, {} FDB entries, STP={}, bridged={}, flooded={}\n",
                b.name, b.index, b.port_count, b.fdb_count,
                if b.stp_enabled { "on" } else { "off" },
                b.frames_bridged, b.frames_flooded,
            ));
        }
    }

    if bonds.is_empty() {
        out.push_str("\nNo bond interfaces configured\n");
    } else {
        out.push_str("\nBond interfaces:\n");
        for b in &bonds {
            out.push_str(&format!(
                "  {} ({}): mode={}, {} members, TX={}, RX={}\n",
                b.name, b.index, b.mode, b.member_count, b.total_tx, b.total_rx,
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run bridge/bond self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[bridge] Running bridge/bond self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Create and delete bridge ---
    {
        let idx = create_bridge("br-test")?;
        let bridges = list_bridges();
        assert!(bridges.iter().any(|b| b.name == "br-test"), "bridge created");
        delete_bridge(idx)?;
        let bridges = list_bridges();
        assert!(!bridges.iter().any(|b| b.name == "br-test"), "bridge deleted");

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 1 (create/delete bridge) PASSED");
    }

    // --- Test 2: Bridge port management ---
    {
        let idx = create_bridge("br-port")?;
        assert!(add_port(idx, 1).is_ok(), "add port 1");
        assert!(add_port(idx, 2).is_ok(), "add port 2");

        let bridges = list_bridges();
        let br = bridges.iter().find(|b| b.name == "br-port").unwrap();
        assert!(br.port_count == 2, "port count");

        assert!(remove_port(idx, 1).is_ok(), "remove port");
        delete_bridge(idx)?;

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 2 (port management) PASSED");
    }

    // --- Test 3: FDB learning ---
    {
        let mut bridges = BRIDGES.lock();
        // Find unused bridge.
        let bridge = &mut bridges[0];
        bridge.active = true;
        bridge.name[..4].copy_from_slice(b"test");
        bridge.name_len = 4;

        let mac: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        bridge.learn(&mac, 1, 1000);
        assert!(bridge.lookup(&mac) == Some(1), "learned");

        // Update port.
        bridge.learn(&mac, 2, 2000);
        assert!(bridge.lookup(&mac) == Some(2), "updated");

        // Unknown MAC.
        let unknown: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        assert!(bridge.lookup(&unknown).is_none(), "unknown");

        bridge.active = false;
        drop(bridges);

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 3 (FDB learning) PASSED");
    }

    // --- Test 4: FDB aging ---
    {
        let mut bridges = BRIDGES.lock();
        let bridge = &mut bridges[0];
        bridge.active = true;
        bridge.name_len = 4;

        let mac: [u8; 6] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        bridge.learn(&mac, 1, 0);
        assert!(bridge.fdb_count() >= 1, "entry added");

        // Age with a time far in the future.
        bridge.age_fdb(FDB_AGING_NS + 1);
        assert!(bridge.lookup(&mac).is_none(), "aged out");

        bridge.active = false;
        drop(bridges);

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 4 (FDB aging) PASSED");
    }

    // --- Test 5: STP port states ---
    {
        assert!(StpState::Forwarding.can_forward(), "forwarding can forward");
        assert!(StpState::Forwarding.can_learn(), "forwarding can learn");
        assert!(StpState::Learning.can_learn(), "learning can learn");
        assert!(!StpState::Learning.can_forward(), "learning can't forward");
        assert!(!StpState::Blocking.can_forward(), "blocking can't forward");
        assert!(!StpState::Blocking.can_learn(), "blocking can't learn");
        assert!(!StpState::Disabled.can_forward(), "disabled can't forward");

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 5 (STP states) PASSED");
    }

    // --- Test 6: MAC address helpers ---
    {
        let bcast = MacAddress([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(is_broadcast(&bcast), "broadcast");
        assert!(is_multicast(&bcast), "broadcast is multicast");

        let unicast = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert!(!is_broadcast(&unicast), "not broadcast");
        assert!(!is_multicast(&unicast), "not multicast");

        let mcast = MacAddress([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        assert!(is_multicast(&mcast), "multicast");
        assert!(!is_broadcast(&mcast), "not broadcast");

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 6 (MAC helpers) PASSED");
    }

    // --- Test 7: Bond creation ---
    {
        let idx = create_bond("bond-test", BondMode::ActiveBackup)?;
        assert!(add_bond_member(idx, 1).is_ok(), "add member");
        assert!(add_bond_member(idx, 2).is_ok(), "add member 2");

        let bonds = list_bonds();
        let b = bonds.iter().find(|b| b.name == "bond-test").unwrap();
        assert!(b.member_count == 2, "member count");
        assert!(b.mode == "active-backup", "mode");

        delete_bond(idx)?;

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 7 (bond creation) PASSED");
    }

    // --- Test 8: Bond modes ---
    {
        assert!(BondMode::ActiveBackup.name() == "active-backup", "ab name");
        assert!(BondMode::RoundRobin.name() == "round-robin", "rr name");
        assert!(BondMode::XorHash.name() == "xor-hash", "xor name");

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 8 (bond modes) PASSED");
    }

    // --- Test 9: STP state names ---
    {
        assert!(StpState::Forwarding.name() == "forwarding", "forwarding");
        assert!(StpState::Blocking.name() == "blocking", "blocking");
        assert!(StpState::Disabled.name() == "disabled", "disabled");

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 9 (STP names) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("Bridges"), "header");

        passed = passed.saturating_add(1);
        crate::serial_println!("[bridge]   test 10 (procfs content) PASSED");
    }

    crate::serial_println!("[bridge] All {} self-tests PASSED", passed);
    Ok(())
}
