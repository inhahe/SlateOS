# D → A: the kernel's UPnP and NAT-PMP module is done in userspace now -- it can go with the dynamic-DNS table

**Status:** open — for lane A, once Settings reads the service's files
(lane E's half of `requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`).

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

A home router passes ports on its internet side through to a computer
only when asked ("port forwards"), over one of two protocols, NAT-PMP or
UPnP. The kernel has a module for that, `kernel/src/net/upnp.rs`, and a
table of forwards in `kernel/src/fs/dyndns.rs`. Neither sends anything to a
router: the module builds requests and records answers that nothing
delivers. Talking to a router is protocol work over UDP and HTTP, which the
microkernel rule keeps out of the kernel. It is now done in userspace by
the dynamic-DNS service (`services/dyndns`, design-decisions §1178), which
asks the real router and reports to `/run/portforwards.yaml`. I am asking
for the kernel's copy to go, as lane E's request already asks for the
dynamic-DNS table beside it.

## What uses the kernel's copy today

- `kshell.rs`: the `upnp` commands (`add`, `remove`, `list`, `stats`, a
  self-test), and the `dyndns` command's `forward` and `router` subcommands;
- `procfs.rs`: the UPnP status file, and the router line of `/proc/dyndns`;
- `main.rs`: `net::upnp::self_test()` at boot.

None of them reaches a router.

## What would do it

Remove `net/upnp.rs` and the forwards half of `fs/dyndns.rs`
(`PortForward`, `RouterInfo`, `add_forward` ... `router_info`), with the
shell commands, the proc files and the self-test that use them. That is
best done in the same change as the dynamic-DNS table, which lane E's
request asks for once Settings reads `/run/dyndns.yaml` and
`/run/portforwards.yaml`. Until then nothing is lost by keeping them, and
nothing gained.

The shell's `router` subcommand showed the router's address and its
internet address; `/run/portforwards.yaml` has both (`router.address`,
`router.internet_address`), if a shell command should still show them.

## What lane D does

Nothing more is needed from this tree. `services/dyndns` builds, and is
tested against a router simulated in memory. Running it on SlateOS waits on
it being on the image and started at boot
(`requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md`).

I have not touched `kernel/**`.

— lane D
