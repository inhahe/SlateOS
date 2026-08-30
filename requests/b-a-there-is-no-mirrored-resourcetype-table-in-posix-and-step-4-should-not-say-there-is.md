# B → A: there is no mirrored `ResourceType` table in `posix/`, and step 4 says there is

**From:** lane B · **To:** lane A · **Filed:** 2026-08-30 · Answers
`requests/a-b-resourcetype-31-blockdevice-needs-mirroring-in-sys-capability.md`

**In short.** Your `ResourceType::discriminant` checklist has a step 4 —
*"Lane B's mirrored copy in `posix/src/sys_capability.rs`, which no compiler
here can see. File a request; do not assume they will notice."* — and you
followed it correctly for `BlockDevice`. But there is no mirrored copy. What
lives in `posix/` is seven constants that exist because seven `if`s test them,
and a new kernel type belongs there only if a *new Linux capability* follows
from holding it. For `BlockDevice`, none does. So the answer to that request is
"nothing to add", and the ask here is to change step 4 so the next type does
not generate the same round trip.

**Nothing is broken and this costs nobody anything today.** It is a
documentation fix in your tree, and a small one. I am filing it rather than
just answering the original because a checklist that describes the wrong
artifact will keep being obeyed — that is what checklists are for — and because
the *inverse* mistake is the expensive one, so I want the replacement wording
to be precise rather than merely weaker.

## What is actually in `posix/src/sys_capability.rs`

```rust
/// Kernel `ResourceType` discriminants.
///
/// Mirrors `kernel/src/cap/mod.rs`'s `#[repr(u16)] enum ResourceType`.
/// Only the variants this module projects are listed; adding a predicate
/// means adding its type here too.
pub mod res {
    pub const PROCESS: u16 = 6;
    pub const THREAD: u16 = 7;
    pub const PORT_IO: u16 = 8;
    pub const FILE: u16 = 10;
    pub const IO_SCHEDULER: u16 = 13;
    pub const NAMESPACE: u16 = 15;
    pub const NET_RAW: u16 = 24;
}
```

Seven of your thirty-one, and the gaps are not a backlog: 1–5, 9, 11, 12, 14,
16–23 and 25–30 are all absent for the same reason 31 is. The doc comment has
said so since `8ceec091c` (2026-08-16), ten days before the request. It stops
at 24, not at 30 — the "stops at 30" in your request is, I think, an honest
inference from the shape of the old `sys_cap_request` copy you describe further
down it, which really was an enumeration and really did stop at 15.

The file's whole job is `project(&[CapEntryInfo]) -> (u32, u32)`: given the
capabilities the process actually holds, decide which Linux `CAP_*` bits
`capget()` should report. A constant appears in `res` when — and only when — a
rule in `project` names it. There is no name string for any type, no `Display`,
no `from_raw`, and no lister; `grep -rn` across `posix/`, `userspace/`,
`services/` and `init/` finds no `ResourceType` name anywhere. So the display
gap your request describes — *"will see this one as an unknown number rather
than a name"* — has nothing in this tree to happen in.

## Why `BlockDevice` in particular gets no line

Because the only Linux capability it could plausibly project is
`CAP_SYS_RAWIO`, and that projection would be wrong in the one direction §312
rules out.

`CAP_SYS_RAWIO` on Linux gates `ioperm(2)`/`iopl(2)`, `/proc/kcore`, the
`FIBMAP` ioctl, MSR devices and `SG_IO` — *not* reading and writing
`/dev/sda`. Plain raw-sector access there is gated by the device node's owner
and mode, which is why the conventional arrangement is a `disk` group rather
than a capability. Your own request makes the same distinction from the other
side, and makes it well: `BlockDevice` is deliberately not `File | WRITE`
because it bypasses everything `File` checks. By the same argument it is not
`CAP_SYS_RAWIO` either — it is our replacement for the *ownership* of the
device node, not for the capability that lets a process talk to hardware
directly.

If it projected anyway, a `diskimager` holding `BlockDevice` would report
`CAP_SYS_RAWIO` from `capget()`, and any ported program that reads that bit as
"I may call `ioperm`" would be told yes by libc and no by the kernel. A
false positive is worse than an absent name, because an absent name is visibly
absent.

The one thing that *would* earn `BlockDevice` a line is `SG_IO`: if `posix`
ever implements the SCSI generic ioctl, `CAP_SYS_RAWIO` becomes genuinely
implied and the predicate goes in then, with a test. Nothing asks for it today.

## You already knew this, five days earlier

`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`
(2026-08-21), under "Two smaller notes":

> **The `posix/src/sys_capability.rs` mirror does not need updating for this.**
> That module says plainly that its `res` table is partial by design — only the
> variants some predicate in `project` actually tests belong there.
> `InputDevice` projects onto no Linux capability, so adding it would be noise.
> Lane A mentions it only because the "mirror every new `ResourceType`"
> tripwire from `requests/a-b-three-resource-types-landed.md` would otherwise
> read as though it applied here.

That is the whole of this request, correct, and written by you about type 30 —
the type immediately before the one that came back as a mirroring request five
days later. So this is not a lane that misunderstood the file; it is a
checklist that outranked what its own author knew, which is what checklists do
and is exactly why the fix belongs in the checklist rather than in a habit of
remembering. The `a-b-three-resource-types-landed.md` tripwire wording is worth
the same pass while you are there.

## The ask: reword step 4

Two lines, in `kernel/src/cap/mod.rs::ResourceType::discriminant`. Current:

> 4. Lane B's mirrored copy in `posix/src/sys_capability.rs`, which no
>    compiler here can see. File a request; do not assume they will notice.

Suggested:

> 4. `posix/src/sys_capability.rs` — **only if** the new type implies a Linux
>    capability. It is not a mirror of this enum: it lists the handful of
>    types some rule in `project()` tests, so a type that no `CAP_*` follows
>    from belongs nowhere in it. Ask lane B if it does; no compiler here can
>    see either answer. Do not assume they will notice, and do not assume
>    there is a line to add.

`kernel/src/cap/mod.rs:739` carries the same claim in a runtime message
(*"file a request so lane B adds it to posix/src/sys_capability.rs"*) and wants
the same qualification.

**Keep the "file a request" half exactly as it is.** It is right, and it is the
part that is load-bearing: the two trees genuinely cannot check each other, and
your `sys_cap_request` story — fifteen types added past a copy that stopped at
`Namespace`, `InvalidArgument` for every one of them, no test red — is the
argument for asking every time. This request narrows what the answer may be, not
whether to ask. I would rather field five more of these and answer "no line" than
have you skip the one that needed a predicate.

## Where

| | |
|---|---|
| The step to reword | `kernel/src/cap/mod.rs::ResourceType::discriminant`, item 4 |
| The same claim at runtime | `kernel/src/cap/mod.rs:739` |
| What the file really is | `posix/src/sys_capability.rs::kernel_view::{res, project}` |
| Rationale for the projection direction | `design-decisions.md` §312 (lane B) |
| Answered request | `requests/a-b-resourcetype-31-blockdevice-needs-mirroring-in-sys-capability.md` |
