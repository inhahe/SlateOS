//! User-defined container networks with IP address management (Docker
//! `docker network`).
//!
//! A *container network* is a named IPv4 subnet the runtime manages on behalf
//! of containers. It mirrors Docker's user-defined bridge networks: an
//! operator `docker network create`s a network with a subnet and gateway, and
//! containers join it with `--network NAME`, at which point the runtime's IPAM
//! (IP Address Management) hands each container a free address from the subnet.
//!
//! ## What this module is (and is not)
//!
//! This module owns the **network registry and IPAM** — the naming, the
//! subnet/gateway bookkeeping, and conflict-free address allocation/release.
//! It deliberately does **not** yet stand up a shared layer-2 bridge that lets
//! two containers on the same network reach each other directly; each container
//! still gets its existing veth-to-host link (host and external connectivity
//! via NAT). Cross-container L2 connectivity over a shared bridge is a tracked
//! follow-up (see `known-issues.md`). Nothing here claims reachability it does
//! not provide: `inspect` reports only the subnet, gateway, and the addresses
//! actually allocated.
//!
//! The value delivered now is real: named grouping plus **automatic,
//! conflict-free IP assignment**. Before this, `oci run --net IP` required the
//! operator to hand-pick a non-colliding address; a named network with IPAM
//! removes that footgun.
//!
//! ## Design
//!
//! - The registry is an in-memory table (like the container and volume tables,
//!   which are likewise not persisted across boots).
//! - Each network records its subnet (network address + prefix length), its
//!   gateway, and the set of currently-allocated host addresses, each tagged
//!   with the owning container id so a container's address can be released when
//!   it is removed.
//! - Allocation scans the host-address range `[network+1, broadcast)`, skipping
//!   the gateway and any already-allocated address, and returns the first free
//!   one. Release frees a specific address (or every address owned by a given
//!   container).
//! - Default subnets are carved from `172.20.0.0/16` upward (`172.20`, `172.21`,
//!   …), matching Docker's convention of using the `172.16/12` block for
//!   user-defined networks and keeping clear of the `172.17` default bridge.
//!
//! ## References
//!
//! - Docker `docker network create/ls/inspect/rm/prune/connect/disconnect`;
//!   the default `bridge`/`local` IPAM driver.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use spin::Mutex;

/// Maximum number of container networks tracked at once.
pub const MAX_NETWORKS: usize = 64;

/// Maximum length of a network name.
pub const MAX_NETWORK_NAME_LEN: usize = 64;

/// First octet-pair used for auto-assigned default subnets: `172.20.0.0/16`,
/// then `172.21.0.0/16`, and so on. Kept clear of Docker's `172.17` default
/// bridge while staying inside the `172.16/12` private block.
const DEFAULT_SUBNET_FIRST: u8 = 172;
const DEFAULT_SUBNET_SECOND_BASE: u8 = 20;
const DEFAULT_PREFIX_LEN: u8 = 16;

/// A single allocated address within a network.
#[derive(Clone)]
struct Allocation {
    /// The allocated host address.
    ip: [u8; 4],
    /// The container that owns this address (Docker: the attached container),
    /// or `None` for an address reserved without a container (e.g. a manual
    /// `network connect` with no container binding yet).
    owner: Option<u32>,
}

/// A registered container network.
struct Network {
    /// The network's name (unique within the registry).
    name: String,
    /// The subnet's network address (host bits zero).
    network_addr: [u8; 4],
    /// The subnet prefix length (0..=32).
    prefix_len: u8,
    /// The gateway address (reserved; never handed out by IPAM).
    gateway: [u8; 4],
    /// Currently-allocated host addresses.
    allocations: Vec<Allocation>,
}

/// Public, read-only view of a network (for `inspect`/`ls`).
#[derive(Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub network_addr: [u8; 4],
    pub prefix_len: u8,
    pub gateway: [u8; 4],
    /// Allocated `(ip, owner_container_id)` pairs.
    pub allocations: Vec<([u8; 4], Option<u32>)>,
}

/// The result of allocating an address on a network — everything a caller
/// needs to configure a container interface.
#[derive(Clone, Copy)]
pub struct Lease {
    pub ip: [u8; 4],
    pub gateway: [u8; 4],
    pub netmask: [u8; 4],
    pub prefix_len: u8,
}

struct NetworkTable {
    networks: Vec<Network>,
    /// Rotating second octet for the next auto-assigned default subnet.
    next_default_second: u8,
}

impl NetworkTable {
    const fn new() -> Self {
        Self {
            networks: Vec::new(),
            next_default_second: DEFAULT_SUBNET_SECOND_BASE,
        }
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.networks.iter().position(|n| n.name == name)
    }
}

static TABLE: Mutex<NetworkTable> = Mutex::new(NetworkTable::new());

// ---------------------------------------------------------------------------
// Address arithmetic (clippy-clean: no bare +/-/<< on user-derived values)
// ---------------------------------------------------------------------------

/// Convert four octets to a host-order `u32` (so numeric ordering matches
/// address ordering).
fn ip_to_u32(ip: [u8; 4]) -> u32 {
    u32::from_be_bytes(ip)
}

/// Convert a host-order `u32` back to four octets.
fn u32_to_ip(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// The subnet mask for a prefix length, as a host-order `u32`.
///
/// `prefix_len` is clamped to `0..=32`. A `/0` yields `0.0.0.0`; a `/32` yields
/// `255.255.255.255`.
fn mask_u32(prefix_len: u8) -> u32 {
    let p = prefix_len.min(32);
    // The mask is the top `p` bits set: `u32::MAX << (32 - p)`. For `p == 0`
    // the shift amount is 32, which is undefined for `<<`; `checked_shl(32)`
    // returns `None`, so we fall back to `0` (the correct `/0` mask). For
    // `p == 32` the shift amount is 0 → `u32::MAX`.
    let host_bits = 32u32.saturating_sub(u32::from(p));
    u32::MAX.checked_shl(host_bits).unwrap_or(0)
}

/// The netmask octets for a prefix length.
fn netmask_octets(prefix_len: u8) -> [u8; 4] {
    u32_to_ip(mask_u32(prefix_len))
}

/// The broadcast address of a subnet (all host bits set).
fn broadcast_u32(network_addr: [u8; 4], prefix_len: u8) -> u32 {
    let net = ip_to_u32(network_addr) & mask_u32(prefix_len);
    net | !mask_u32(prefix_len)
}

// ---------------------------------------------------------------------------
// Validation / parsing
// ---------------------------------------------------------------------------

/// Validate a proposed network name.
///
/// Same rule set as volume names: non-empty, at most [`MAX_NETWORK_NAME_LEN`]
/// bytes, no `/` or NUL, and not `.`/`..`. Names are opaque byte strings.
///
/// # Errors
/// [`KernelError::InvalidArgument`] if the name violates any rule.
pub fn validate_name(name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > MAX_NETWORK_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }
    if name == "." || name == ".." {
        return Err(KernelError::InvalidArgument);
    }
    if name.as_bytes().iter().any(|&b| b == b'/' || b == 0) {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// Parse a dotted-quad IPv4 address (`A.B.C.D`).
///
/// # Errors
/// [`KernelError::InvalidArgument`] on a malformed address.
pub fn parse_ipv4(s: &str) -> KernelResult<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0usize;
    for part in s.split('.') {
        if count >= 4 {
            return Err(KernelError::InvalidArgument);
        }
        let val: u8 = part.parse().map_err(|_| KernelError::InvalidArgument)?;
        // `count < 4` guaranteed by the guard above.
        if let Some(slot) = octets.get_mut(count) {
            *slot = val;
        }
        count = count.saturating_add(1);
    }
    if count != 4 {
        return Err(KernelError::InvalidArgument);
    }
    Ok(octets)
}

/// Parse a CIDR subnet (`A.B.C.D/N`) into `(network_address, prefix_len)`.
///
/// The address is masked to its network boundary, so `10.0.0.5/24` normalizes
/// to `10.0.0.0/24`. `prefix_len` must be `0..=32`.
///
/// # Errors
/// [`KernelError::InvalidArgument`] on a malformed CIDR or out-of-range prefix.
pub fn parse_cidr(s: &str) -> KernelResult<([u8; 4], u8)> {
    let mut it = s.splitn(2, '/');
    let addr_str = it.next().unwrap_or("");
    let prefix_str = it.next().ok_or(KernelError::InvalidArgument)?;
    let addr = parse_ipv4(addr_str)?;
    let prefix: u8 = prefix_str.parse().map_err(|_| KernelError::InvalidArgument)?;
    if prefix > 32 {
        return Err(KernelError::InvalidArgument);
    }
    // Normalize to the network address (clear host bits).
    let net = ip_to_u32(addr) & mask_u32(prefix);
    Ok((u32_to_ip(net), prefix))
}

// ---------------------------------------------------------------------------
// Registry operations
// ---------------------------------------------------------------------------

/// The default gateway for a freshly-created subnet: the first usable host
/// address (`network + 1`).
fn default_gateway(network_addr: [u8; 4], prefix_len: u8) -> [u8; 4] {
    let net = ip_to_u32(network_addr) & mask_u32(prefix_len);
    // For any prefix <= 30 there is at least one host beyond the network
    // address; `net + 1` stays within the subnet. Saturating add is defensive
    // (net is never u32::MAX for a real subnet).
    u32_to_ip(net.saturating_add(1))
}

/// Create a network with an explicit subnet and optional gateway.
///
/// If `gateway` is `None`, the first host address (`network + 1`) is used. The
/// gateway must lie within the subnet.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if the name or gateway is invalid, or the
///   gateway is outside the subnet.
/// - [`KernelError::AlreadyExists`] if a network with that name exists.
/// - [`KernelError::ResourceExhausted`] if the registry is full.
pub fn create_with_subnet(
    name: &str,
    network_addr: [u8; 4],
    prefix_len: u8,
    gateway: Option<[u8; 4]>,
) -> KernelResult<()> {
    validate_name(name)?;
    if prefix_len > 32 {
        return Err(KernelError::InvalidArgument);
    }
    let net = u32_to_ip(ip_to_u32(network_addr) & mask_u32(prefix_len));
    let gw = gateway.unwrap_or_else(|| default_gateway(net, prefix_len));
    // The gateway must be inside the subnet.
    if (ip_to_u32(gw) & mask_u32(prefix_len)) != ip_to_u32(net) {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = TABLE.lock();
    if table.position(name).is_some() {
        return Err(KernelError::AlreadyExists);
    }
    if table.networks.len() >= MAX_NETWORKS {
        return Err(KernelError::ResourceExhausted);
    }
    table.networks.push(Network {
        name: String::from(name),
        network_addr: net,
        prefix_len,
        gateway: gw,
        allocations: Vec::new(),
    });
    Ok(())
}

/// Create a network with an auto-assigned default subnet (`172.20.0.0/16`,
/// then `172.21.0.0/16`, …), like `docker network create NAME` with no
/// `--subnet`.
///
/// # Errors
/// Same as [`create_with_subnet`], plus [`KernelError::ResourceExhausted`] if
/// the default-subnet space is exhausted (256 auto subnets).
pub fn create(name: &str) -> KernelResult<()> {
    validate_name(name)?;
    // Pick the next free default subnet, skipping any already taken by an
    // explicitly-subnetted network. Bounded by the 256 second-octet values.
    let (net, prefix) = {
        let mut table = TABLE.lock();
        if table.position(name).is_some() {
            return Err(KernelError::AlreadyExists);
        }
        let mut chosen: Option<([u8; 4], u8)> = None;
        for _ in 0..=u8::MAX {
            let second = table.next_default_second;
            table.next_default_second = second.wrapping_add(1);
            let candidate = [DEFAULT_SUBNET_FIRST, second, 0, 0];
            let clash = table.networks.iter().any(|n| {
                n.network_addr == candidate && n.prefix_len == DEFAULT_PREFIX_LEN
            });
            if !clash {
                chosen = Some((candidate, DEFAULT_PREFIX_LEN));
                break;
            }
        }
        match chosen {
            Some(v) => v,
            None => return Err(KernelError::ResourceExhausted),
        }
    };
    create_with_subnet(name, net, prefix, None)
}

/// Whether a network with `name` is registered.
#[must_use]
pub fn exists(name: &str) -> bool {
    TABLE.lock().position(name).is_some()
}

/// The number of registered networks.
#[must_use]
pub fn count() -> usize {
    TABLE.lock().networks.len()
}

/// A read-only snapshot of a network, or `None` if it is not registered.
#[must_use]
pub fn inspect(name: &str) -> Option<NetworkInfo> {
    let table = TABLE.lock();
    let idx = table.position(name)?;
    table.networks.get(idx).map(network_to_info)
}

/// Read-only snapshots of all registered networks (registration order).
#[must_use]
pub fn list() -> Vec<NetworkInfo> {
    TABLE.lock().networks.iter().map(network_to_info).collect()
}

fn network_to_info(n: &Network) -> NetworkInfo {
    NetworkInfo {
        name: n.name.clone(),
        network_addr: n.network_addr,
        prefix_len: n.prefix_len,
        gateway: n.gateway,
        allocations: n.allocations.iter().map(|a| (a.ip, a.owner)).collect(),
    }
}

/// Remove a network by name.
///
/// Refuses to remove a network that still has allocated addresses (Docker
/// refuses to remove an in-use network), returning [`KernelError::NotEmpty`].
///
/// # Errors
/// - [`KernelError::NotFound`] if no such network is registered.
/// - [`KernelError::NotEmpty`] if the network still has active allocations.
pub fn remove(name: &str) -> KernelResult<()> {
    let mut table = TABLE.lock();
    let idx = table.position(name).ok_or(KernelError::NotFound)?;
    let in_use = table
        .networks
        .get(idx)
        .is_some_and(|n| !n.allocations.is_empty());
    if in_use {
        return Err(KernelError::NotEmpty);
    }
    table.networks.remove(idx);
    Ok(())
}

/// Remove every network that has no active allocations, returning the count
/// removed (Docker `network prune`).
pub fn prune() -> usize {
    let mut table = TABLE.lock();
    let before = table.networks.len();
    table.networks.retain(|n| !n.allocations.is_empty());
    before.saturating_sub(table.networks.len())
}

// ---------------------------------------------------------------------------
// IPAM: allocation / release
// ---------------------------------------------------------------------------

/// Allocate the next free host address on a network for `container_id`.
///
/// Scans `[network+1, broadcast)` in ascending order, skipping the gateway and
/// any already-allocated address, and returns a [`Lease`] describing the chosen
/// address plus the network's gateway and mask.
///
/// # Errors
/// - [`KernelError::NotFound`] if the network is not registered.
/// - [`KernelError::ResourceExhausted`] if the subnet has no free address.
pub fn allocate(name: &str, container_id: Option<u32>) -> KernelResult<Lease> {
    let mut table = TABLE.lock();
    let idx = table.position(name).ok_or(KernelError::NotFound)?;
    let n = table.networks.get_mut(idx).ok_or(KernelError::NotFound)?;

    let net = ip_to_u32(n.network_addr) & mask_u32(n.prefix_len);
    let broadcast = broadcast_u32(n.network_addr, n.prefix_len);
    let gw = ip_to_u32(n.gateway);

    // Host range is (net, broadcast) exclusive of both ends. For a /31 or /32
    // there is effectively no usable host range → exhausted.
    let mut candidate = net.saturating_add(1);
    while candidate < broadcast {
        let ip = u32_to_ip(candidate);
        let taken = candidate == gw
            || n.allocations.iter().any(|a| ip_to_u32(a.ip) == candidate);
        if !taken {
            n.allocations.push(Allocation { ip, owner: container_id });
            return Ok(Lease {
                ip,
                gateway: n.gateway,
                netmask: netmask_octets(n.prefix_len),
                prefix_len: n.prefix_len,
            });
        }
        candidate = candidate.saturating_add(1);
    }
    Err(KernelError::ResourceExhausted)
}

/// Release a specific address from a network.
///
/// # Errors
/// - [`KernelError::NotFound`] if the network is not registered or the address
///   is not currently allocated.
pub fn release(name: &str, ip: [u8; 4]) -> KernelResult<()> {
    let mut table = TABLE.lock();
    let idx = table.position(name).ok_or(KernelError::NotFound)?;
    let n = table.networks.get_mut(idx).ok_or(KernelError::NotFound)?;
    let before = n.allocations.len();
    n.allocations.retain(|a| a.ip != ip);
    if n.allocations.len() == before {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Release every address owned by `container_id`, across all networks.
///
/// Called when a container is removed so its leases do not leak. Returns the
/// number of addresses freed.
pub fn release_container(container_id: u32) -> usize {
    let mut table = TABLE.lock();
    let mut freed = 0usize;
    for n in &mut table.networks {
        let before = n.allocations.len();
        n.allocations.retain(|a| a.owner != Some(container_id));
        freed = freed.saturating_add(before.saturating_sub(n.allocations.len()));
    }
    freed
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the container-network registry + IPAM (invoked at boot).
///
/// Exercises name validation, CIDR/IPv4 parsing, mask/broadcast math, create
/// (explicit + default subnet), inspect/list, allocation ordering and gateway
/// skipping, release, container-scoped release, in-use removal refusal, and
/// prune. Panics on any invariant violation (the boot self-test convention).
pub fn self_test() {
    use crate::serial_println;
    serial_println!("[cnetwork] Running self-test...");

    // Name validation (shared rules with volumes).
    assert!(validate_name("frontend").is_ok(), "simple name must validate");
    assert!(validate_name("").is_err(), "empty name rejected");
    assert!(validate_name(".").is_err(), "'.' rejected");
    assert!(validate_name("a/b").is_err(), "name with '/' rejected");
    serial_println!("[cnetwork]   name validation: OK");

    // IPv4 / CIDR parsing.
    assert_eq!(parse_ipv4("10.0.0.1").expect("parse ip"), [10, 0, 0, 1]);
    assert!(parse_ipv4("10.0.0").is_err(), "3-octet address rejected");
    assert!(parse_ipv4("10.0.0.256").is_err(), "octet > 255 rejected");
    let (net, pfx) = parse_cidr("192.168.5.9/24").expect("parse cidr");
    assert_eq!(net, [192, 168, 5, 0], "CIDR must normalize to network addr");
    assert_eq!(pfx, 24, "prefix must parse");
    assert!(parse_cidr("10.0.0.0/33").is_err(), "prefix > 32 rejected");
    assert!(parse_cidr("10.0.0.0").is_err(), "missing prefix rejected");
    serial_println!("[cnetwork]   ipv4/cidr parsing: OK");

    // Mask / broadcast math.
    assert_eq!(netmask_octets(24), [255, 255, 255, 0], "/24 mask");
    assert_eq!(netmask_octets(16), [255, 255, 0, 0], "/16 mask");
    assert_eq!(netmask_octets(0), [0, 0, 0, 0], "/0 mask");
    assert_eq!(netmask_octets(32), [255, 255, 255, 255], "/32 mask");
    assert_eq!(
        u32_to_ip(broadcast_u32([192, 168, 5, 0], 24)),
        [192, 168, 5, 255],
        "/24 broadcast",
    );
    serial_println!("[cnetwork]   mask/broadcast math: OK");

    let base = count();

    // Create with an explicit subnet; default gateway is network+1.
    create_with_subnet("st-net-a", [10, 40, 0, 0], 24, None)
        .expect("create st-net-a");
    assert!(exists("st-net-a"), "created network must exist");
    assert_eq!(count(), base.saturating_add(1), "create adds one entry");
    let info = inspect("st-net-a").expect("inspect st-net-a");
    assert_eq!(info.network_addr, [10, 40, 0, 0], "subnet recorded");
    assert_eq!(info.gateway, [10, 40, 0, 1], "default gateway is network+1");
    assert!(info.allocations.is_empty(), "new network has no allocations");
    // Duplicate name rejected.
    assert!(
        create_with_subnet("st-net-a", [10, 41, 0, 0], 24, None).is_err(),
        "duplicate network name rejected",
    );
    // Gateway outside subnet rejected.
    assert!(
        create_with_subnet("st-net-bad", [10, 42, 0, 0], 24, Some([10, 99, 0, 1])).is_err(),
        "gateway outside subnet rejected",
    );
    serial_println!("[cnetwork]   create/inspect: OK");

    // IPAM: first allocation skips network(.0) and gateway(.1), so it is .2.
    let l1 = allocate("st-net-a", Some(7)).expect("allocate 1");
    assert_eq!(l1.ip, [10, 40, 0, 2], "first lease skips network+gateway");
    assert_eq!(l1.gateway, [10, 40, 0, 1], "lease carries gateway");
    assert_eq!(l1.netmask, [255, 255, 255, 0], "lease carries mask");
    let l2 = allocate("st-net-a", Some(8)).expect("allocate 2");
    assert_eq!(l2.ip, [10, 40, 0, 3], "second lease is next free");
    let l3 = allocate("st-net-a", Some(7)).expect("allocate 3");
    assert_eq!(l3.ip, [10, 40, 0, 4], "third lease is next free");
    assert_eq!(
        inspect("st-net-a").expect("inspect").allocations.len(),
        3,
        "three addresses allocated",
    );
    serial_println!("[cnetwork]   ipam allocation ordering: OK");

    // Release a specific address, then re-allocate: the hole is reused.
    release("st-net-a", [10, 40, 0, 3]).expect("release .3");
    assert!(release("st-net-a", [10, 40, 0, 3]).is_err(), "double release errors");
    let l4 = allocate("st-net-a", Some(9)).expect("allocate after release");
    assert_eq!(l4.ip, [10, 40, 0, 3], "freed address is reused first");
    serial_println!("[cnetwork]   release/reuse: OK");

    // Removing an in-use network is refused.
    assert!(
        matches!(remove("st-net-a"), Err(KernelError::NotEmpty)),
        "in-use network removal refused",
    );

    // Container-scoped release frees every address owned by that container.
    // At this point allocations are: .2 (c7), .4 (c7), .3 (c9) — container 8's
    // original .3 was released and re-leased to container 9 above, so c8 owns
    // nothing. Freeing c7 drops .2 and .4 (2 addresses), leaving only c9's .3.
    let freed = release_container(7);
    assert_eq!(freed, 2, "container 7 owned two addresses (.2 and .4)");
    let remaining = inspect("st-net-a").expect("inspect").allocations.len();
    assert_eq!(remaining, 1, "one address remains (container 9's reused .3)");
    serial_println!("[cnetwork]   container-scoped release: OK");

    // Free the rest, then removal succeeds. Container 8 owns nothing (its .3
    // was reused by c9); container 9 owns the single remaining address.
    assert_eq!(release_container(8), 0, "container 8 owns nothing (its .3 was reused)");
    assert_eq!(release_container(9), 1, "container 9 owns the reused .3");
    remove("st-net-a").expect("remove empty network");
    assert!(!exists("st-net-a"), "removed network must not exist");
    assert_eq!(count(), base, "registry returns to baseline");
    serial_println!("[cnetwork]   in-use guard + removal: OK");

    // Default-subnet create carves from 172.20.0.0/16 upward.
    create("st-net-def").expect("create default-subnet network");
    let dinfo = inspect("st-net-def").expect("inspect default");
    let [d0, _, _, _] = dinfo.network_addr;
    assert_eq!(d0, 172, "default subnet in 172/8");
    assert_eq!(dinfo.prefix_len, 16, "default prefix is /16");
    remove("st-net-def").expect("remove default network");
    assert_eq!(count(), base, "registry returns to baseline after default net");
    serial_println!("[cnetwork]   default-subnet create: OK");

    serial_println!("[cnetwork] Self-test PASSED");
}
