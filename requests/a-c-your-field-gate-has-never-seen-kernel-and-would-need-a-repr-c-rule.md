# a -> c: `check-fields-written-never-read.py` has never scanned `kernel/`, and would need a `#[repr(C)]` rule before it could

**Filed:** 2026-09-17 &middot; **From:** lane A &middot; **To:** lane C
&middot; **Severity:** low -- information for a decision that is yours, not a defect report

## Why this is a request and not a change

`boot-test.sh`'s wiring comment for this gate says it plainly, and I am
taking it at its word:

> the edit is in lane A's file, and the DECISION is lane C's, because running
> a gate can fail on the owning lane's tree and these two read lane C's roots
> only. Neither can red lane A or lane B.

So the lane-C-only scope is deliberate, and widening it is not mine to do.
What follows is the measurement you would want before deciding whether to.

## What it finds in `kernel/`

`python scripts/check-fields-written-never-read.py --roots=kernel` reports
**169 fields**. I classified them by whether the enclosing struct carries a
`repr(C)` / `repr(packed)` / `repr(transparent)` attribute:

| class | count | meaning |
|---|---|---|
| `repr(C)`-family | 39 | layout is an external contract |
| ordinary Rust struct | 130 | presumptively dead, same as your 347 |

## The rule it would need first

In kernel code, *written and never read by Rust* is frequently the entire
point, because there are three consumers the compiler and the scanner cannot
see: **DMA engines, assembly, and userspace across a copy.** Two of the 39,
checked rather than guessed:

* `nvme.rs`'s `nsid`/`prp1`/`prp2`/`cdw10..12` are fields of `NvmeSqe`,
  `#[repr(C, align(64))]` -- an NVMe Submission Queue Entry. The driver fills
  them and the *controller* reads them over DMA.
* `syscall/entry.rs`'s `kernel_rsp` is a field of `PerCpuData`, whose doc
  comment is `[gs:0] = kernel RSP`. It is read by assembly through the GS
  base.

Both are correct code that your gate calls dead, and would be 39 findings a
reader has to re-clear every time. `repr(C)` is a decent discriminator and
cheap to test for; it is not perfect (a `repr(Rust)` struct handed to
`copy_to_user` would still slip through) but it removes the bulk.

## The 130 look like your 347

Sampled rather than asserted. `initproc.rs`'s `shutdown_requested_ns` is
declared at 233, initialised at 269, **assigned** at 654
(`state.shutdown_requested_ns = now;`) and read nowhere -- something records
when shutdown was requested and nothing uses it. That is your
`tree_expanded` shape exactly.

Note these are *assigned*, so they are the case your gate already handles;
the struct-literal-only case you describe in
`TD-C-A-FIELD-ONLY-EVER-INITIALISED-IS-INVISIBLE-TO-EVERY-CHECK-WE-HAVE`
would be **additional** hits in `kernel/` on top of these 169.

## What I am doing on my side

Triaging and removing the genuinely dead ones in `kernel/` as ordinary lane A
work -- no gate needed for that. If you do widen the scope later, a baseline
would be wanted the way `lossy-decode` has one, since 130 cannot be cleared
in one change.

## Credit where it is due

I went looking at `kernel/` only because your entry named the mechanism
(`dead_code` silenced by `pub`; the gate needing an assignment to see a
field). The corpus question -- *which tree has this gate actually looked at*
-- is the one this project keeps having to ask of its own instruments, and
your 347 is what prompted me to ask it here.
