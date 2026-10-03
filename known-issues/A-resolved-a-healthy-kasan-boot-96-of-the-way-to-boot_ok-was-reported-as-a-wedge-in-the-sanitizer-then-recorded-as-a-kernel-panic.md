### [A] RESOLVED — a healthy KASAN boot 96% of the way to BOOT_OK was reported as a wedge in the sanitizer, then recorded as a kernel panic — 2026-08-19

**In short:** the instrumented (KASAN) build was re-run to see whether an old
silent-hang bug still reproduced. It came back reading `Wedged RIP =
kernel::mm::kasan::byte_bad` and was filed in the boot history as `PANIC --
kernel died`. Both statements were false. The kernel was fine: it had printed
27 841 lines, was still printing when the harness killed it, and had reached
96% of the way to `BOOT_OK`. Four separate defects had to line up to produce
that, and each is fixed here. None of them was in the kernel's behaviour; all
four were in the tooling that reports on it.

**What actually happened.** `boot-test.sh`'s 900s timeout is calibrated on an
uninstrumented kernel. Instrumentation puts a shadow-byte lookup in front of
every load and store, so the same boot needs ~3.3× longer. Measured back to
back on the same host:

| build | result |
|---|---|
| uninstrumented | `BOOT_OK` at **285s**, 27 497 serial lines |
| instrumented | killed at **900s** on serial line 26 410 — the line an uninstrumented boot prints at **96%** of the way to `BOOT_OK` |

It missed the finish line by under a minute. Then three reporting bugs turned
"ran out of clock" into "died".

**Defect 1 — `kasan-build.sh` never raised the timeout.** The command in its own
header, `./scripts/kasan-build.sh --boot`, was therefore *certain* to fail.
Fixed: `KASAN_BOOT_TIMEOUT=3600` (~3.8× the measured 938s need), applied unless
the caller passes `--timeout=`. This is the same bug, and now the same fix, as
`BENCH_TIMEOUT` in `boot-test.sh`, whose comment already records that the
identical false negative "happened".

**Defect 2 — the harness called a live guest wedged.** On timeout `boot-test.sh`
sampled the RIP and printed it as `Wedged RIP`, unconditionally. Under KASAN
nearly any sample lands in the shadow checker, so the label named a plausible
culprit for a hang that did not exist — the single most misleading output the
harness could have produced. The evidence to know better was already in hand:
the stall detector was armed at `--stall-secs=180` and never fired, which is
positive proof the log kept growing. Fixed: serial-growth tracking is now
unconditional rather than gated on the opt-in detector, and the timeout path
reports which kind of timeout it was —

```
=== Timeout at 900s with the guest STILL PRODUCING OUTPUT (serial grew 1s ago) ===
=== This is a budget that was too small, not a hang. Re-run with a larger --timeout. ===
  RIP when the clock ran out (guest was live; not a hang) = 0x...
```

`capture_guest_state` now takes the RIP label from its caller, and it is a
required argument, not a default: from the stall detector "Wedged RIP" is a
finding, from the timeout path the same words are an unfounded diagnosis.

**Defect 3 — `boot-history.py` classified it `PANIC` on the strength of three
self-test breakpoints.** There is no `PANIC` or `FATAL` text anywhere in the
log. The evidence it used was:

```
[idt] Running direction-flag self-test...
EXCEPTION: Breakpoint (#BP) at 0xffffffff813b56b6
[idt]   DF is clear on exception entry: OK
```

— printed by *every* boot, green ones included. What kept it hidden is worth
recording, because it is the shape of the next bug of this kind: `classify()`
consults the exception list only when the marker was never reached, so on a
green boot the mislabelling is unreachable, and it fires precisely on the
failed boot whose verdict someone is relying on. A latent bug in a triage tool
that can only manifest during triage.

Fixed on both sides, and deliberately so, so that neither is the only thing
holding the verdict up:

- **Kernel side.** `ExpectedBreakpoint` (`kernel/src/idt.rs`) is an RAII scope
  held across every deliberate `int3`; `handle_breakpoint` appends
  `(deliberate self-test)` while one is open. This is the convention the #UD
  self-test already followed (`(deliberate compiler trap)`) — the kernel knows
  whether a fault was asked for and the host parser cannot, so the kernel says.
  All four reachable sites are wrapped (`idt.rs` ×3, `serial.rs` ×1;
  `power.rs`'s `int3` triple-faults by design and never reaches a handler).
- **Host side.** `_can_be_fatal()` additionally rejects `(#BP)` outright:
  vector 3's handler is documented "Logged but non-fatal" and structurally
  returns, so whatever raised it, the kernel was still running afterwards. A
  stray breakpoint is still recorded and still printed — "the kernel survived
  it" is not the claim "nobody needs to see it" — it simply cannot on its own
  mean the kernel died.

Verified by removing each guard in turn: with the vector rule disabled the
unannotated sample reproduces `PANIC` exactly; with either guard alone the
verdict is `TIMEOUT`. Three regression tests in `scripts/test-boot-history.py`
cover the unannotated form (the text the kernel actually emitted that day), the
annotated form, and a real `#PF` printed alongside self-test breakpoints, which
must still read as `PANIC` — the vector rule must be about the vector, not
about the neighbourhood.

**The original question, answered.** The silent-hang bug did **not** reproduce.
The instrumented kernel ran the whole self-test battery, including the KASAN
shadow's own coverage checks, without a single KASAN report. The run was
inconclusive only about the last 4% of the boot, and only because of the
timeout; there is nothing here that argues the old hang is still live.

**Still true, and not fixed here:** the instrumented build is not part of
`boot-test.sh`, so nothing in the routine gate exercises any of this. It
remains a manual build, and a manual build is one nobody runs until it has
already rotted — which is how the previous entry's checker bug survived a week.
