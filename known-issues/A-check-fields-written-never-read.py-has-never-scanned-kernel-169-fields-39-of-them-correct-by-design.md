### [A] `check-fields-written-never-read.py` has never scanned `kernel/`: 169 fields, 39 of them correct by design -- 2026-09-17

**Status:** OPEN

Prompted by lane C's
`TD-C-A-FIELD-ONLY-EVER-INITIALISED-IS-INVISIBLE-TO-EVERY-CHECK-WE-HAVE`,
which found 347 candidates in `gui/` and `apps/`. The obvious next question
is the corpus one: which tree has this gate actually looked at?

`detect(roots=lanec_scan.LANE_C_ROOTS)`, and `LANE_C_ROOTS` is `apps, gui,
net, netipc, netproto, netring, net80211, aes, hmac, pkg`. No `kernel`, no
`posix`, no `userspace`, no `services`. It is wired into `boot-test.sh` at
line 5908 with no `--roots=`, so **`kernel/`'s 3,962 tracked `.rs` files have
never been scanned by it, in CI or otherwise.**

**That is deliberate, and I checked before treating it as a hole.** The
wiring comment in `boot-test.sh` -- lane A's file, lane C's decision -- says
the scope exists so that "neither can red lane A or lane B". Same shape as
`PreemptSpinMutex`'s lockdep opt-out: documented, reasoned, and not an
oversight. So it is not widened here; filed to lane C as
`requests/a-c-your-field-gate-has-never-seen-kernel-and-would-need-a-repr-c-rule.md`.

**The measurement, since it is worth having either way.**
`--roots=kernel` reports **169 fields**, classified by whether the enclosing
struct carries a `repr(C)`/`packed`/`transparent` attribute:

| class | count |
|---|---|
| `repr(C)`-family -- layout is an external contract | 39 |
| ordinary Rust struct -- presumptively dead | 130 |

**The premise does not transfer unqualified, and that is the transferable
part.** In kernel code *written and never read by Rust* is often the entire
point, because there are three consumers neither the compiler nor the scanner
can see: DMA engines, assembly, and userspace across a copy. `nvme.rs`'s
`prp1`/`cdw10..12` are `NvmeSqe` fields -- `#[repr(C, align(64))]`, an NVMe
Submission Queue Entry the *controller* reads over DMA.
`syscall/entry.rs`'s `kernel_rsp` is `PerCpuData`, documented `[gs:0]`, read
by assembly. A gate written for application code calls both dead.

I expected that exception to explain most of the 169 and it explains under a
quarter. Worth recording as a corrected guess: 130 remain, and the one I
sampled was real -- `initproc.rs`'s `shutdown_requested_ns`, declared 233,
initialised 269, assigned 654, read nowhere.

Triaging those 130 is ordinary lane A work and needs no gate. Recorded here
so the number is not rediscovered from scratch.
