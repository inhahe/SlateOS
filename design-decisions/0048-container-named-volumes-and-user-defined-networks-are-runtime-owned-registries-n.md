## 48. Container named volumes and user-defined networks are runtime-owned registries; networks add IPAM but not (yet) a shared L2 bridge

**Date:** 2026-07-01
**Decided by:** Claude (autonomous) — within the operator-approved Docker/container-runtime port (open-questions Q15).

Two Docker-parity subsystems landed as sibling in-memory registries alongside
the container table (increments 59–61):

**Named volumes (`docker volume`, increment 59).** `kernel/src/volume.rs` is a
registry of runtime-owned backing directories under
`/var/lib/slate/volumes/<name>`, created on demand and bind-mounted into
containers via `-v NAME:/guest`. The source form is distinguished exactly as
Docker does — a leading `/` means a host bind mount, a bare name means a named
volume — so `-v` handles both with one flag. The registry is in-memory (like
the container table), but a volume's *data* lives on the ext4 rootfs and
survives until `remove`d, so create+populate+run behaves as expected within a
boot. Backing dirs are flat (`ROOT/<name>`), not Docker's `ROOT/<name>/_data`,
because our runtime owns the layout and there is no metadata sidecar to
separate from the data.

**User-defined networks with IPAM (`docker network`, increments 60–61).**
`kernel/src/cnetwork.rs` is a registry of named IPv4 subnets with address
management: `allocate` scans `[network+1, broadcast)` skipping the gateway and
taken addresses, `release`/`release_container` return leases to the pool.
`oci run --network NAME` reserves an unowned address *before* the container is
created (the interface must be configured from the container config, which is
built pre-create), then binds the lease to the container id after create via
`set_allocation_owner`; a failed create releases the reservation, and
`container::delete` calls `release_container(id)` so leases never leak. Default
subnets carve from `172.20.0.0/16` upward (clear of Docker's `172.17` default
bridge, inside the `172.16/12` private block).

**The tradeoff — IPAM without L2 bridging.** The named-network feature
deliberately delivers naming + conflict-free IPAM but *not* a shared layer-2
broadcast domain: each container keeps its existing per-netns veth-to-host link
(host/external connectivity via NAT), so two containers on the same named
network cannot yet address each other directly. This was a real fork:

- *Alternative A (chosen): ship IPAM now, defer L2 bridging.* Pros: the
  immediately valuable, fully-testable capability (removes the footgun of
  hand-picking a non-colliding `--net IP`) lands in two clean increments;
  `inspect` reports only what is real, so nothing over-promises. Cons: "same
  network" is not yet a connectivity guarantee, which could surprise a user who
  expects Docker's inter-container DNS/reachability.
- *Alternative B: build the shared bridge first, ship networks only when peers
  can talk.* Pros: matches Docker's connectivity semantics on day one. Cons:
  needs bridge↔veth port registration and frame plumbing between
  `net::veth::poll_all` and the `net::bridge` FDB — a substantially larger,
  riskier change — to deliver *any* of the (independently useful) naming/IPAM
  value.

Chose A because IPAM is useful standalone and the honest `inspect` output
prevents the surprise from becoming a silent correctness bug. The L2-bridge
follow-up is tracked as `known-issues.md` D-CNET-L2BRIDGE with a full design.

**Where it bites.** `kernel/src/volume.rs` (new); `kernel/src/cnetwork.rs`
(new); `kernel/src/main.rs` (`mod` + boot self-tests); `kernel/src/kshell.rs`
(`container volume`/`container network` subcommands + `docker` passthrough; the
`-v` source-form split; `oci run --network` reservation/bind/release wiring);
`kernel/src/container.rs` (`delete` → `cnetwork::release_container`).
