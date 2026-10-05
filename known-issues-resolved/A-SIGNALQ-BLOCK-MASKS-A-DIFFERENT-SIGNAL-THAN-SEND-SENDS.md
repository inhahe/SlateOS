## `A-SIGNALQ-BLOCK-MASKS-A-DIFFERENT-SIGNAL-THAN-SEND-SENDS` (lane A, 2026-08-26) — **fixed 2026-08-26**

**In short:** the kernel shell's `signalq` command can send a signal to a
process and can block one. Blocking signal 13 did not block signal 13. The
command said it had, the next `send 1 13` went through anyway, and nothing in
either message hinted that the two commands disagreed about which signal was
meant.

`cmd_signalq`'s `send` arm translated the CPU exception vectors by hand — `13`
→ `SegmentFault`, `14` → `PageFault`, and so on — and fell through to
`Signal::UserDefined(n)` for anything else. Its `block` arm did no translation
at all and always built `Signal::UserDefined(sig_num)`. Those are different
signals, not two spellings of one, because `Signal::number()` returns the
vector for a named exception but `32 + n` for `UserDefined(n)`, and the
blocked-signal mask is indexed by `number()`:

| you type | `send` masks/tests | `block` sets |
|---|---|---|
| `13` | bit 13 (`SegmentFault`) | bit 45 (`UserDefined(13)`) |
| `0`  | bit 0 (`DivideError`)   | bit 32 (`UserDefined(0)`) |

So every number `send` recognises as an exception — 0, 3, 4, 6, 13, 14 — was
blocked at the wrong bit, and the block was recorded against a signal that
`send` has no way to produce. Both halves fail silently: the block does
nothing, and the mask accumulates bits for signals nobody can send.

**What made it invisible** was the confirmation message. `block` printed
`Signal {sig_num} blocked for pid {pid}` using the integer that was *typed*
rather than the signal that was *masked*, so the output agreed with the
operator's intent no matter what the module did.

**Fixed** by factoring the mapping into a single `signal_from_number` used by
`send`, `block` and the new `unblock` arm, so the two cannot drift again, and
by printing the signal's own label and number in the confirmation instead of
the raw operand. The mapping was also completed while it was being moved — the
`send` arm had omitted vectors 5, 7, 8, 16, 17 and 18, which it silently
turned into user signals.

**Not a regression.** True since `cmd_signalq` was written.
