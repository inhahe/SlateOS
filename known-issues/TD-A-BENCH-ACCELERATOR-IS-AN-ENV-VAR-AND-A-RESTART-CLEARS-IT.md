## TD-A-BENCH-ACCELERATOR-IS-AN-ENV-VAR-AND-A-RESTART-CLEARS-IT (lane A, 2026-09-12) — **three runs of measurements silently changed cost model**

**In short:** the benchmark suite can run the emulated machine two ways, one about three
times faster than the other. Which one it uses depends on an environment variable that
has to be set by hand. The machine was restarted, the variable was lost, and the next
three measurement runs used the slow one without anything saying so. The numbers are all
still correct; they are just no longer comparable with the ones before the restart.

### The evidence

Every WHPX record in `bench/history.jsonl` carries
`experiment: "QEMU_EXTRA=-accel whpx (non-default emulator flags)"`. Every TCG record has
no experiment at all. `boot-test.sh` never names an accelerator itself — line 7621 is the
only place flags enter, `read -r -a QEMU_EXTRA_ARGS <<< "${QEMU_EXTRA:-}"` — so with the
variable unset, QEMU falls back to TCG.

| commit | accel | experiment |
|---|---|---|
| `96356d747` | Hyper-V/WHPX | `QEMU_EXTRA=-accel whpx` |
| `2de15d6f6` | QEMU TCG | *(none)* — first run after the cold restart |
| `24f11ef45` | QEMU TCG | *(none)* |
| `61cc05ce1` | QEMU TCG | *(none)* |

Hyper-V is not the problem and was checked: `hvhost` and `vmcompute` are running and
`HypervisorPresent` is true. Nothing broke. A variable set in one shell session did not
survive the machine being power-cycled.

### Why it matters more than a speed difference

`bench-history.py`'s own comment is the authority here: *"A comparison that crosses
accelerators is therefore not noisy, it is meaningless, and it is meaningless by a factor
far larger than any regression this harness exists to catch."* Measured on one
byte-identical binary: the median benchmark is 3.5× faster under WHPX, the fastest 10.4×,
and **four get dramatically slower** — HPET reads cost a VM exit (~13.5 µs) where TCG
emulates them inline (~450 ns).

So the hazard is not "the numbers got worse". It is that anyone comparing `61cc05ce1`
against `96356d747` sees everything roughly three times slower and concludes a
catastrophic regression that is entirely the accelerator. And the reverse: the
`record_version` HPET fix committed tonight is **invisible under TCG by construction**,
because the cost it removes barely exists there. Three consecutive runs could not have
measured it, and I nearly reported that as bad luck rather than as a cleared variable.

### The gap

The harness records the accelerator faithfully — that is how this was found — but nothing
**warns** when it changes between consecutive records. The fact was in the file all along
and only a human reading it carefully would catch it. That is the summary-versus-status
distinction lane B named tonight: a value a reader must notice is weaker than a status
something can act on.

A cheap guard: `bench-history.py` already parses every record and already treats the
accelerator as load-bearing. A line comparing the newest record's `accel` against the
previous one, and saying so loudly when they differ, would turn this from an archaeology
exercise into a sentence in the run's own output. Not done here — recorded so the next
person hitting a mysterious 3× shift finds this first.

### What to do

Run benchmark boots as `QEMU_EXTRA="-accel whpx" ./scripts/boot-test.sh --bench`. The
variable is not persisted anywhere, so it needs setting per shell session, and after any
restart.
