## 608. A printed field that nothing writes is completed, not deleted — but only when a real writer exists to give it

*Date: 2026-08-26*
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `cgmem list` printed a `swap=` column for every memory cgroup, and
a `high=` count was tracked internally but printed nowhere. The `swap=` number
was always `0` — not because nothing was ever swapped, but because no line of
code anywhere could ever change it. A column that is structurally frozen at zero
is not a missing feature, it is a *false statement*: a reader sees `swap=0` and
concludes nothing was swapped. The choice was between deleting the column and
giving it a writer. We gave it a writer (`cgmem swap <id> <pages> [out|in]`),
and symmetrically surfaced the `high=` counter that was already being computed
and thrown away.

**The two halves of the problem.** They are mirror images and it is worth
keeping them apart:

| | `swap_pages` | `high_events` |
|---|---|---|
| Written by | nothing | `record_charge`, on every charge that leaves usage above the cgroup's limit |
| Read by | `cgmem list`, `/proc/cgmem` | nothing |
| What a reader concluded | "no pages were swapped" — false by construction | nothing, because there was nothing to read |
| Severity | **actively misleading** | **silently inert** |

`swap_pages` is the worse of the two, because a printed constant is evidence in
a way an absent line is not. But `high_events` is not harmless either, and its
harm is specifically documented in the twenty-fifth burn-down batch
(`cmd_cgmem create`, §600's guessed-value defect): the `limit` operand was
guessed when unreadable, and the *only* consequence of a wrong limit is a wrong
`high_events`. Because nothing printed `high_events`, a guessed policy ceiling
had no observable consequence whatsoever — the guess was not merely undetected
but **unfalsifiable**. Two defects, each of which concealed the other.

**The alternative: delete the field and its two columns.** This is a serious
option and it is the one a minimalist reading of "don't ship dead code" picks.
Its case:

- It is smaller, and it cannot be wrong. A column that does not exist cannot
  lie.
- The kernel has no swap subsystem yet. Adding a *writer* for a swap counter
  before there is any swapping means the only thing that will ever call it is a
  shell command and a self-test — which is arguably just a more elaborate way of
  writing a number nobody produced.
- Deleting is reversible in the cheap direction: when a real swap subsystem
  lands, it can add the field back along with the code that fills it, and the
  field will arrive already correct.

**Why completing won anyway.** Three reasons, in order of weight:

1. **`high_events` had to be surfaced regardless**, because it is the fix for a
   *logged, open* defect that is not hypothetical. Once that line is being
   printed, `swap=` is the only remaining frozen column on the same row, and
   leaving one lie beside one newly-honest number is the worst of the three
   available states.
2. **The field is not speculative — the accounting model is already here.**
   `record_charge` / `record_uncharge` maintain `usage_pages == rss_pages +
   cache_pages`, and swap is the third bucket in exactly the same ledger. The
   writer is not inventing a subsystem; it is completing an arithmetic that the
   other two thirds of already exist and are tested. When a swap subsystem
   arrives it will call `record_swap`, not replace it.
3. **A writer makes the field testable, and a test is what stops it drifting
   back.** The deleted-field option leaves nothing behind to notice if someone
   re-adds a column later; the completed field has a self-test rung
   (`swap_cg`: out 40, in 15, expect 25; then an over-large swap-in, expect a
   clamp to 0, not an underflow) that pins both the arithmetic and the
   saturation.

**What was rejected inside the chosen option.** `record_swap` does *not* move
pages out of `rss_pages` / `cache_pages` when they are swapped out, even though
that is what a real kernel does. It cannot: only the caller knows which bucket
the pages came from, and guessing one would be §600's second prohibited shape
wearing a different hat. So the contract is explicit in the doc comment — the
caller reports the memory side with `record_uncharge` and the swap side with
`record_swap`, and the two are separate calls on purpose. A single call that
inferred the bucket would be more convenient and would be wrong half the time.

**The general rule this sets.** *A field that is printed but never written is a
bug of the same class as a wrong value, and it is fixed the same way: by making
the printed thing true.* Deleting the column is a legitimate fix only when there
is no honest writer available and none in prospect. When the surrounding
accounting already exists — when the field is the third bucket in a ledger whose
other two are maintained and tested — the correct fix is the writer. And a
counter that is computed and never shown is the same bug read backwards: it lets
a second defect hide behind the absence of any reader, which is precisely what
happened here.

**Where it came from.**
`A-CGMEM-THE-LIMIT-IS-COMPARED-AND-THE-RESULT-IS-NEVER-SHOWN`, found while
gathering severity evidence for the twenty-fifth burn-down batch of
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
