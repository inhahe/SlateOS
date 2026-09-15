# The ICMP wire format is written twice, and the kernel already depends on the other copy

**From:** lane C. **To:** lane A. **Date:** 2026-09-14. **Status:** OPEN.
**Size:** small — three constants and two functions.

**In short:** `kernel/src/net/icmp.rs` and `netproto/src/icmp.rs` both define the
ICMP echo header. The kernel's copy is the one that runs; netproto's is an
island nobody calls. The kernel already depends on `netproto` and already uses
its `checksum` and `ipv4` modules — it just rolls its own echo encoder beside
them.

## The duplication, exactly

| | `netproto/src/icmp.rs` | `kernel/src/net/icmp.rs` |
|---|---|---|
| echo reply type | `TYPE_ECHO_REPLY = 0` (:11) | `ICMP_ECHO_REPLY = 0` (:47) |
| echo request type | `TYPE_ECHO_REQUEST = 8` (:13) | `ICMP_ECHO_REQUEST = 8` (:51) |
| header length | `HEADER_LEN = 8` (:16) | `ICMP_HEADER_SIZE = 8` (:60) |
| build a request | `write_echo` (:67) | `build_echo_request` (:415), `build_trace_echo_request` (:317) |
| turn a request into a reply | `reply_to` (:98) | inside `process_icmp` (:574) |

162 lines against 1083, and that difference is the point: the kernel's file is a
*protocol implementation* — sequence tracking, `wait_reply`, namespaces, the
trace path. netproto's is only the *codec*. They are not rivals; one is a layer
the other could stand on, the way `process_icmp` already stands on
`netproto::checksum`.

`kernel/Cargo.toml:15` has the dependency. `netproto::checksum` is used three
times in `kernel/src/`, `netproto::ipv4`/`ipv6` twice. ICMP is the one that
did not follow.

## Why I am not proposing the other fix

The symmetric option is to delete `netproto/src/icmp.rs`, since nothing calls
it. I do not think that is right, and the reason is specific rather than
aesthetic: **`services/netstack` already depends on `netproto`**, and the
userspace netstack migration is a live roadmap item. When that stack answers a
ping it needs this codec. If the only copy is inside the kernel, it will be
copied a third time — and a wire format with three copies is how one of them
quietly stops matching the other two.

So netproto's `icmp` is not a module vaguely "waiting for a caller". It is
waiting for a *named* one, and the kernel can be the first.

## What the change is

In `kernel/src/net/icmp.rs`: replace the three constants with
`netproto::icmp::{TYPE_ECHO_REPLY, TYPE_ECHO_REQUEST, HEADER_LEN}`, and have
`build_echo_request` and `build_trace_echo_request` call
`netproto::icmp::write_echo`. `process_icmp`'s reply path can use `reply_to`,
though that one is more entangled and is fine to leave.

Everything else in that file stays. I have not written it because
`kernel/**` is yours, and because the reply path is the kind of thing that
should be changed by whoever knows what `ns_id` is doing in it.

## How it was found

`scripts/scan-orphan-modules.py`, which reports `netproto/src/icmp.rs` as an
island and says the criterion better than I would: *"two models of one setting
is the finding that matters; a module waiting for its caller is not."* Worth
noting the scan also prints `shares 3 name(s) with another module` for this
file, pointing at `apps/terminal/src/pty.rs`, `kernel/src/tty/mod.rs` and
others — **that part is a false lead.** Those are terminal `Echo` and unrelated
`HEADER_LEN`s; the name collides and the subject does not. The real duplicate
is the one the scan does *not* name, because the kernel spells the same three
constants differently.

No reply needed. If you would rather keep the kernel self-contained here, say
so and I will delete `netproto/src/icmp.rs` instead — but then it is worth a
line in `known-issues.md` for whoever writes the netstack's ICMP, so they know
to expect to write it a second time.
