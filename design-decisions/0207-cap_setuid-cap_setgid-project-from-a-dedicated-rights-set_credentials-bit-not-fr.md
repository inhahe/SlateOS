## §207 — `CAP_SETUID`/`CAP_SETGID` project from a **dedicated `Rights::SET_CREDENTIALS` bit**, not from a reuse of `METADATA`

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — Lane B asked for a preimage in
`requests/b-a-cap-grants-for-312-step3-fixtures.md` and proposed
`(Process, METADATA)`; Lane A chose a new bit instead. Reversible: the bit is
one line in `kernel/src/cap/rights.rs` and one predicate in `posix`.
**Lane:** A

**In short:** Our POSIX layer decides whether a program may call `setuid()` —
"become a different user" — by looking at what kernel permissions the program
was given. Nobody had written down *which* kernel permission means "may become
another user", so the answer was "none", and under a change Lane B is
preparing that would have made `setuid()` impossible for every program on the
system. Lane B suggested reusing an existing general-purpose permission called
METADATA ("may modify this process's attributes"). We instead added a new,
single-purpose permission that means only this. The cost is one bit out of 52
spare; the benefit is that nobody can hand out "may become root" by accident
while trying to grant something harmless.

## The problem

`posix::sys_capability::kernel_view::project` maps kernel capabilities onto
Linux's `CAP_*` words. §312 step 3 flips `has_capability` from the all-caps
default to that projection, at which point **any `CAP_*` with no rule in
`project()` reads false forever**. `CAP_SETUID`/`CAP_SETGID` had no rule, so
step 3 would have made `setuid()`/`setgid()` unreachable process-wide — not
merely for the `fastpy-setuid` fixture that surfaced it. Lane B's framing was
exactly right: the bit cannot be left unprojected.

## The options

| | preimage | cost | failure mode |
|---|---|---|---|
| **A** | `(Process, METADATA)` — Lane B's proposal | zero: no new `ResourceType`, no new `Rights` bit | a *future* benign grant of `METADATA` on a Process silently confers root-capability |
| **B** (chosen) | `(Process, SET_CREDENTIALS)` — a new bit, `1 << 18` | ~4 lines in `kernel/src/cap/rights.rs`, one line in `posix`'s mirrored `rights` module, one predicate | none known; costs one of 52 free bits |

**A is safe today and was checked, not assumed.** `(Process, METADATA)`
appears in no other projection rule, and neither of the two sites that grant a
Process capability automatically — `proc/spawn.rs` step 5b and `proc/fork.rs`
step 8, both granting `READ|WRITE|DELETE|WAIT|SIGNAL|DUPLICATE` to the parent
— includes `METADATA`. So A would not have escalated anything that exists.

## Why B anyway

**The hazard is forward-looking, and it is the kind nothing catches.**
`Rights::METADATA` is documented as "Modify metadata (permissions, attributes,
etc.)" — it is the *generic* bit. It is precisely what the next person wanting
"may rename this process", "may set this process's nice value" or "may retag
this process" will reach for, and they will be right to, because that is what
the bit says it is for. The moment they do, every holder of that grant becomes
`CAP_SETUID`-capable.

What makes that specifically dangerous rather than merely untidy is that
**the grant site and the projection live in different crates owned by
different lanes**. A lane-A engineer adding `METADATA` to a Process grant reads
`kernel/src/cap/rights.rs`, which would say nothing about uid; the rule that
turns it into root-capability is in `posix`, which is Lane B's file. There is
no diff in which both halves are visible. That is the same class of failure as
`B-A-MERGE-RESURRECTED-THREE-ARCHIVED-ENTRIES` — an invariant that spans two
files with nothing standing over the pair.

**The asymmetry decides it.** Choosing B when A would have sufficed costs one
bit out of 52 free — after 46 days of development, 12 are in use, so bits are
not a scarce resource and treating them as one is a false economy. Choosing A
when B was needed costs a **silent** privilege escalation, discoverable only
by someone who happens to read both crates at once.

**There is direct precedent in this very file.** `Rights::DEBUG` (`1 << 17`)
exists rather than being spelled `READ | WRITE` on a Process, for exactly this
reason: "may read another process's memory unilaterally" is a *distinct
authority*, not an intensity of "may read". "May become another user" stands
in the same relation to "may modify an attribute". The rule the two cases
share, and the one worth stating generally:

> A `Rights` bit names an **authority**, not an object shape. When an
> operation's danger does not follow from the generic verb that would
> otherwise cover it, it gets its own bit.

## What was rejected

- **Projecting from `uid == 0`.** That is ambient authority in a capability
  costume — the thing §312 exists to remove — and it is circular besides: the
  question is whether you may *change* your uid.
- **Leaving it unprojected and special-casing the fixture.** It is not the
  fixture that is broken; `setuid()` would be unreachable for everything.

## Where it lives

- `kernel/src/cap/rights.rs` — `Rights::SET_CREDENTIALS`, and its entry in the
  `Display` flag table (`setcred`).
- `kernel/src/proc/spawn.rs` — `self_test_fastpy_slateos_setuid`'s `caps`
  array. Its sibling `self_test_fastpy_slateos_nice` gained
  `(Thread, IO_REALTIME)` in the same change, which needed no new bit — Lane
  B's read there was right and is adopted unchanged.
- `posix/src/sys_capability.rs` — Lane B's file; the mirrored `rights` module
  and the `project()` predicate are requested in
  `requests/a-b-set-credentials-right.md`.

**Until Lane B lands the `posix` half, nothing changes observably**: the
projection is advisory, `has_capability` still answers from the all-caps
default, and the kernel's own invariant for `SYS_PROCESS_SET_CREDENTIALS` is
unchanged (a process may set its own credentials; policy is userspace's). The
grant is in place *first* so that step 3 is a one-line flip rather than a flip
plus a hunt for the fixtures it broke.
