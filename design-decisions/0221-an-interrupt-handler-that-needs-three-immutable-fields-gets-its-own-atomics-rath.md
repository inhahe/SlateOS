## §221 — An interrupt handler that needs three immutable fields gets its own atomics, rather than forcing every task-context lock site to `lock_irqsave`

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The sound-card driver keeps all its state behind one lock. Its
interrupt handler — the small routine the CPU jumps into when the sound card
says "I'm done with that buffer" — wanted that same lock. If a normal thread
is holding the lock when the interrupt arrives on the same CPU, the handler
waits for a lock that only the thread it interrupted can release, and the
machine wedges forever. The usual fix is to switch off interrupts for the whole
time the lock is held; the problem here is that one of the lock's users holds it
for up to a third of a second while talking to the audio chip, and switching
interrupts off for that long stops the clock, the keyboard and everything else.
So instead the three values the handler actually needs are published separately,
and the handler takes no lock at all.

### What the situation was

`kernel/src/hda.rs` (Intel HD Audio) holds everything in
`static DEVICE: PreemptSpinMutex<Option<HdaDevice>>`. `handle_irq()` took that
lock — a straightforward self-deadlock the moment the handler is wired to the
IOAPIC, which it is not yet, so the bug was latent. This is the
interrupt-reentrancy half of Q24 (§70): `PreemptSpinMutex::lock()` disables
*preemption*, and preemption is not what re-enters the lock. The interrupt is.

The complication is `configure_output()`, which holds `DEVICE` across:

| Step | Worst-case wait |
|---|---|
| Stream reset assert poll | 1000 × 10 µs = 10 ms |
| Stream reset deassert poll | 1000 × 10 µs = 10 ms |
| 6 × `send_verb` (RIRB poll) | 6 × `CODEC_RESPONSE_TIMEOUT_US` = 300 ms |

≈ 320 ms. A healthy codec answers in microseconds, so this bound is only
reached when a codec has stopped responding — but it is a real bound, and it is
reached exactly when the machine is already having a bad day.

### The options

**A — `lock_irqsave` at all nine `DEVICE` sites.** The textbook answer, and
what this batch originally did.

*What changes:* nothing visible while audio works; if a codec stops answering
during boot configuration, the machine stops servicing timer, keyboard and disk
interrupts for up to a third of a second.

**B — publish the handler's inputs in atomics; handler takes no lock.**
`handle_irq` reads `mmio_base` and the derived output stream index. Both are
written once in `init()` and never mutated again, so they do not need mutual
exclusion at all — only publication. Two atomics (`IRQ_MMIO_BASE`,
`IRQ_STREAM_IDX`), stored index-first and base-last with `Release`, read
base-first with `Acquire`, so a non-zero base implies a valid index. `DEVICE`
stays a plain task-context-only `PreemptSpinMutex`.

*What changes:* the deadlock is impossible rather than merely avoided, and the
320 ms window disables preemption but not interrupts — the clock keeps ticking.

**Chosen: B.**

### Why

`lock_irqsave` is the right tool when a handler genuinely needs *shared mutable*
state. It does not here: it needs three values that are constants after `init`.
Taking a lock to read a constant, and then paying for that lock at every
unrelated call site, is the tail wagging the dog. B removes the hazard at its
source; A trades a deadlock for a latency cliff and leaves the coupling in
place.

What B costs is a coherence obligation: `IRQ_STREAM_IDX` is a *copy* of
`iss + out_stream_idx`, so if either field ever becomes mutable the copy goes
stale silently. That is paid for with a doc comment on `out_stream_idx`
naming the obligation, and one on `DEVICE` saying in as many words: if you add
a field the handler needs, publish it the same way — do not reach for
`lock_irqsave` here.

The two writers were also checked for register overlap, because B lets the
handler run concurrently with task context on another CPU where A would not.
They do not overlap: `handle_irq` touches only `INTSTS` and the stream's `STS`,
both write-1-to-clear, while `configure_output` programs
`CTL`/`CBL`/`LVI`/`FMT`/`BDP*` and `INTCTL`. No register is read-modify-written
from both sides.

### Why `configure_output` still holds the lock across all six verbs

Noted because it looks like the same defect as the ac97/virtio-sound ones fixed
in the same batch, and is not. `send_verb` advances `corb_wp` and `rirb_rp` —
the shared CORB/RIRB ring pointers. Two interleaved verb sequences would desync
the response ring and each would read the other's replies. Those six verbs are
one indivisible configuration, not six independent transactions, so unlike
`ac97::play_test_tone` this one *cannot* drop the lock between steps. Holding
it is the correctness requirement; the 320 ms is the price, now paid in
preemption rather than in interrupts.

### Bookkeeping note

Lane A's band is §200–§299, and §216 was its last entry. §217–§220 at the end
of this file are lane C's (AMD GPU, GEM backing, PAT layout, diagnostics) —
appended at EOF in the 200 band by mistake. This entry takes §221 rather than
renumbering another lane's text, which would conflict on merge. Filed as
`requests/a-c-design-decisions-band-collision.md`.
