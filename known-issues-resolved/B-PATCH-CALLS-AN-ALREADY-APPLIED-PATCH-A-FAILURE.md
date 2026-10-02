## B-PATCH-CALLS-AN-ALREADY-APPLIED-PATCH-A-FAILURE (lane B, 2026-09-12) — FIXED

When a hunk did not apply, this build said `Hunk #1 FAILED` and stopped there.
GNU first asks a different question — **would the other orientation apply?** —
because the answer changes the diagnosis entirely. A patch that fails forward
but applies backward has almost certainly been applied already; a patch given
`-R` that only applies forward was never reversed. Neither is well described by
"failed".

Measured, and the two spellings differ by which mistake was made:

    -R on a forward patch:   Unreversed patch detected!  Ignore -R? [n]
    no -R, already applied:  Reversed (or previously applied) patch detected!  Assume -R? [n]

then `Apply anyway? [n]`, `Skipping patch.`, and a count saying **ignored**
rather than FAILED. Two spaces after the `!` in both. No `.orig` is written on
this path, unlike a real hunk failure: nothing was touched.

**And then the reject files still differed at the same 85 bytes**, which turned
out to be a second bug hiding behind the first. `reverse_hunk` swapped `Add` and
`Remove` *in place*, so reversing `-bravo` / `+BRAVO` gave `+bravo` / `-BRAVO`:
the right lines with the wrong sign order. **A unified diff writes every removal
of a change block before every addition**, and that was no longer one.

**It had applied correctly the whole time**, which is why it survived:
`apply_hunk` reads lines by type and does not care about their order. The defect
only became observable when a reversed hunk was WRITTEN OUT — as a reject, for a
person to re-apply. A structure can be wrong in a way that every consumer inside
the program tolerates, and be caught only at the moment it leaves.

A test was retargeted rather than deleted:
`reverse_hunk_swaps_add_remove_and_ranges` asserted the in-place order, so it
had pinned the bug.

`patch-diff.sh`: 3 passed / 62 differed this morning, **33 / 32** now — the
first time passes have outnumbered differences.
