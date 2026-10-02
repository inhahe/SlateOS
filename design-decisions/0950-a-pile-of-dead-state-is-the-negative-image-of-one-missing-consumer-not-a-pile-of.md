## 950. A pile of dead state is the negative image of one missing consumer, not a pile of mistakes

**Date:** 2026-09-17 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; two lanes, two corpora, the same day

**In short:** when a scan finds dozens of variables that get filled in and
never used, the instinct is to delete them as clutter. That is usually
wrong. They are normally the outline of a single thing that was never
connected -- and the outline is the most useful description of the missing
work anyone has. Deleting them throws away a design and leaves the gap
invisible again.

### The two measurements

Lane C pointed a field scanner at `gui/` and `apps/` and found 347
candidates. Grouping `apps/`'s 210 by crate and asking what each crate can
actually *do*:

| crate | fields | cannot |
|---|---|---|
| `torrent` | 23 | reach a network -- no socket at all |
| `email` | 21 | reach a network |
| `videoplayer` | 14 | open a file |
| `pdfviewer` | 10 | open a PDF |

Six of those crates depend on exactly `appearance`, `guitk` and `oswindow`,
with zero references to `std::fs` and zero to any socket. And the fields are
precisely what the absent I/O would have filled: `email`'s are an IMAP
account and IMAP message flags; `torrent`'s are announce bookkeeping and
peer stats. None of it can be populated by a program that never connects.

Independently, and from the other end of the tree: the same scanner pointed
at `kernel/` for the first time flagged `devpower.rs`'s `target_state`. That
one field led to a module whose doc describes a PCI power manager, which
contains **zero** hardware accesses, whose four stated integrations are all
absent, and which publishes device power states through `/proc` that no
device was ever put into. `target_state` is dead for exactly one reason:
the asynchronous PMCSR transition it exists to track is not implemented.

### The rule

> Before asking *which of these should I delete*, ask **what single thing,
> if it were wired, would read all of them.** A cluster of unread state
> almost always has one answer, and that answer is the finding.

The count is a symptom whose magnitude is meaningless on its own: 23 dead
fields in `torrent` is one missing socket, not 23 defects. Lane C's
phrasing, which is better than mine: *they are unwired, not
unimplementable, so deleting them discards a design rather than dead
weight.*

### What follows from it

**Keep the scaffolding, track the gap.** `devpower`'s `target_state` stays,
because removing it would delete the shape of the real fix. What was
actually wrong there was that a knowingly incomplete subsystem appeared in
neither `todo.txt` nor `known-issues.md`, and that `/proc` published its
modelled states with no disclosure (945). Both fixed; the field untouched.

**The corollary for this lane's 130.** `kernel/`'s scan produced 169 hits,
39 of them `repr(C)`-family structures where "written and never read by
Rust" is the entire point -- the reader is a DMA engine, an assembly stub,
or userspace across a copy. Of the remaining 130, the first sampled traced
to an absent subsystem. So "130 presumptively dead" was the wrong frame for
that number the moment it was written, and a flat deletion pass over it
would have been actively destructive.

### Why this is an entry rather than two notes

Two lanes reached it the same day, on corpora with nothing in common -- one
GUI applications, one kernel power management -- from scanners built for
different purposes. Neither was looking for it. That is the strongest
evidence this project gets for a rule being about the *shape* of the problem
rather than about one subsystem, and the failure it prevents is silent:
a tidy-up commit that removes 23 fields, closes the trail to the missing
socket, and leaves a crate that still cannot reach a network with nothing
left to say so.
