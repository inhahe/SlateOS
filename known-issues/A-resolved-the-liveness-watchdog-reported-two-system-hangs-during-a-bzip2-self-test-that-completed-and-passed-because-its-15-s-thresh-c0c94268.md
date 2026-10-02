### [A] RESOLVED — the liveness watchdog reported two `SYSTEM HANG`s during a bzip2 self-test that completed and passed, because its 15 s threshold is calibrated on an uninstrumented kernel — 2026-08-19

**In short:** With the timeout fixed (previous entry), the KASAN boot finally
reached the finish line — and was still failed, this time by the kernel's own
hang detector. It printed `SYSTEM HANG` twice in the middle of the bzip2
compression self-test. The self-test then finished, passed, and the boot ran on
to `BOOT_OK` with every later self-test green, so nothing was hung. The
detector declares a hang after 15 seconds of no visible progress; under
sanitizer instrumentation the one compression step it fired on takes about 93
seconds instead of a few. The threshold is now 180 seconds when the kernel is
built instrumented, and unchanged otherwise.

**Verdict:** `SELFTEST_FAIL` on boot 140 — "marker reached but a self-test /
liveness gate failed". The clean streak, restored to 1 by the preceding green
boot, was reset to 0 again.

#### What the log actually shows

```
23784:[bzip2]   Empty input round-trip ✓
23785:[liveness] boot-window breadcrumb: 900s armed (deadline 3498s, heartbeat=70246)
23786:[liveness] SYSTEM HANG: ... (useful_work=2756, kernel_progress=373530, ctx_switches=1635, report 1/3, watchdog stays ARMED)
24110:[liveness] boot-window breadcrumb: 930s armed (deadline 3498s, heartbeat=72155)
24111:[liveness] SYSTEM HANG: ... (useful_work=2756, kernel_progress=373530, ctx_switches=1635, report 2/3, watchdog stays ARMED)
24435:[liveness] boot-window breadcrumb: 960s armed (deadline 3498s, heartbeat=74070)
24436:[liveness] boot-window breadcrumb: 990s armed (deadline 3498s, heartbeat=75992)
24437:[bzip2]   Repetitive data round-trip (3000B → 45B, 1%) ✓
...
24441:[bzip2] Self-test passed.
24442:[xz] === self-test start ===
24461:[xz] === self-test passed ===
```

The evidence that this is a false positive is **structural, not statistical**:
the operation the detector accused of hanging *finished and reported success* on
the very next line it printed. That is direct proof, not an n=1 inference from a
boot that happened to survive.

Corroborating: `heartbeat` advanced monotonically throughout (70246 → 72155 →
74070 → 75992), so the CPU was executing; and `useful_work`, `kernel_progress`
and `ctx_switches` were **byte-identical in both reports**, which is the
signature of exactly one task computing without yielding, printing, or faulting.

#### Why every signal the kernel has said "hung"

The detector's blind spot is a long CPU-bound *kernel-side* computation:

| signal | why it stayed frozen |
|---|---|
| `USEFUL_WORK_TICKS` | only advances for ticks that preempt ring-3 code or a CPU with a *queued* task; a lone in-kernel task's ticks are not counted |
| `kernel_progress_count()` | counts page faults resolved and block I/O completed; an in-memory compression loop does neither |
| `serial::output_count()` | bzip2 prints once per round-trip, at the end |
| `total_ctx_switches()` | nothing to switch to |

So a legitimate compression loop is indistinguishable — by every signal the
kernel possesses — from an in-kernel infinite loop, which is precisely the hang
this detector exists to catch. Teaching `USEFUL_WORK_TICKS` to count kernel-mode
ticks would close the false positive by blinding the detector to its primary
target. **Duration is the only knob that separates the two**, so duration is what
was tuned.

#### The measurement, and the scaling factor that would have been wrong

| build | silent window inside the bzip2 self-test |
|---|---|
| ordinary | the whole bzip2 self-test *and* the xz self-test *and* the pathbar tests all fit between the 60 s and 90 s breadcrumbs (`build/p24-run1-serial.txt`, `p24-run5`) |
| instrumented | ~93 s for one round-trip (armed 898 s → 991 s) |

**The aggregate boot slowdown is the wrong number to scale by.** It was 3.7×
(285 s → 1063 s), and 3.7 × 15 s = 55 s — still under the measured 93 s, so a
threshold derived that way would have gone on false-firing. Instrumentation cost
is proportional to memory-access *density*, and a compression inner loop is far
denser than boot taken as a whole. 36 intervals × 5 s = 180 s is ~1.9× the
measured window, chosen against the measurement itself rather than against a
ratio borrowed from elsewhere. This is worth remembering: **a per-operation
threshold cannot be rescaled by a whole-program ratio.**

#### The fix

`kernel/src/sched/mod.rs` — `LIVENESS_ALERT_COUNT` is now `3` under
`#[cfg(not(kasan_instrumented))]` and `36` under `#[cfg(kasan_instrumented)]`.
Everything downstream scales automatically: the printed `for {}+ seconds` is
`count * (WATCHDOG_CHECK_INTERVAL / 100)`, and all five self-test drill loops
are written `for _ in 0..LIVENESS_ALERT_COUNT` rather than against a literal.
The ordinary gate is bit-for-bit unaffected.

The generosity costs only *promptness*, never detection:
`liveness_boot_deadline_check`'s wall-clock backstop catches any hang mode by
construction and sits at ~3498 s on an instrumented boot, so even at 180 s the
progress detectors remain the promptest signal by 19×.

#### The pattern, now three for three

This is the third instance in two days of the same defect: **a constant
calibrated on an uninstrumented kernel, applied to an instrumented one, and
reported as a kernel fault.**

1. `BENCH_TIMEOUT` in `scripts/boot-test.sh` (earlier)
2. `KASAN_BOOT_TIMEOUT` — the harness budget; reported as `Wedged RIP =
   kasan::byte_bad` (previous entry)
3. `LIVENESS_ALERT_COUNT` — the kernel's own detector; reported as
   `SELFTEST_FAIL` (this entry)

Each was found only by running the instrumented build, and each was hidden
behind the previous one — #3 was unreachable until #2 was fixed, because the
boot never got that far. **There may be a #4 behind this one**; the only way to
find out is to keep re-running the instrumented boot until it is clean, which is
what happens next.

**Still true, and not fixed here:** the instrumented build remains outside
`boot-test.sh`. Every one of these three was a constant that rotted silently
because nothing routine exercised the profile it applied to.
