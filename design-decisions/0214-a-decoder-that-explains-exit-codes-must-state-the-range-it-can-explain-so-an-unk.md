## §214 — A decoder that explains exit codes must state the range it can explain, so an unknown code is announced instead of silently sitting beside a plausible neighbour

**Date:** 2026-08-16
**Decided by:** Claude (autonomous), answering lane B's
`requests/b-a-jobctl-fail-diagnostic-lists-code-bands-that-stop-two-rounds-ago.md`
**Lane:** A

**In short:** When one of our ring-3 test programs fails, it reports *which*
check failed by choosing its exit code — check 177 failing means the program
exits 177. The kernel prints that number along with a sentence explaining what
each range of numbers means, so somebody reading the boot log knows what broke
without opening the source. That sentence had fallen two rounds behind: it
explained codes 10 through 77, while the program had grown checks numbered up
to 187. A failure at 177 therefore printed "177" next to an explanation of 74
and 77 — numbers that look adjacent and mean something completely unrelated.
The choice was between deleting the explanation (lane B's weak preference) and
keeping it up to date. **We kept it, added the missing ranges, and — the part
that actually fixes the defect — made it say out loud when the code it was
handed is outside everything it knows.**

**The defect was not that the list was short. It was that a short list
presented itself as complete.** Those are different bugs with different fixes,
and it is worth being precise about which one was live, because the obvious fix
addresses the wrong one:

| | list is short | list claims to be complete |
|---|---|---|
| cost when the fixture grows | reader greps the source | reader forms a **wrong hypothesis** and debugs the wrong code path |
| how it is noticed | immediately, by absence | never — the output looks exactly as authoritative as when it was right |
| fixed by | adding entries (recurring work) | one conditional (once) |

The right-hand column is the one that bit lane B, and only the right-hand column
can waste someone's time. A reader who sees no explanation loses a grep. A
reader who sees 74 and 77 explained, next to a 177 they can't find, reasonably
assumes 177 is in that neighbourhood — it is not; it is a `WNOWAIT` peek in a
part of the fixture written two rounds later. **Deleting the list fixes the
right-hand column too, but by giving up the left-hand column's value; the
conditional fixes it while keeping that value.**

**Why the list's value is real and not nostalgia.** Lane B raised the
counter-argument themselves and then set it aside; it deserves better. The band
list is readable **from a serial log alone, with no tree to hand** — which is
the situation you are in when triaging a boot someone else ran, on a machine
that does not have the repository, from a pasted log. That is not a rare case
for an OS; it is the normal case for a failure report. `grep 'rc = 177'` is
strictly better *when you have the source*, and worth nothing when you do not.

**What was decided:**

1. **Keep the band list**, and fill in the four bands the fixture has added
   since it was written (150-163 full-size `WaitInfo`, 164-169 truncation,
   170-177 `WNOWAIT`, 178-187 `WPGID`). The bands were read out of
   `services/ctest-jobctl/main.c` directly rather than copied from the request,
   which is how the 120-132 / 140-147 split survived — lane B's summary had
   collapsed those into a single "100-126".
2. **Print the highest code the decoder knows.** When the observed code is
   outside every band, the message says so explicitly and sends the reader to
   `grep` instead of offering a table that cannot contain the answer. This is
   the whole fix: from now on, growing the fixture degrades the diagnostic into
   an honest "I do not know this one", never into a confident wrong neighbour.
3. **Correct the doc comment**, which claimed the function "needs no change as
   the fixture grows … rather than duplicating a per-code table" while sitting
   directly above a per-code table. It now says what is true: the table is a
   convenience that may lag, the lag is self-announcing, and the exit code plus
   `main.c` is the authority.

**Alternatives:**

| Option | Verdict |
|---|---|
| Delete the band list, keep the `main.c` pointer (lane B's weak preference) | Rejected — fixes the misleading-output bug by discarding the no-tree-to-hand case, which is the case failure reports actually arrive in. |
| Keep and extend the list, nothing else | Rejected — this is the status quo plus one round of catch-up. It has now silently gone stale twice; a third time is a prediction, not a risk. |
| Keep, extend, **and announce the decoder's own limit** (chosen) | The only option under which a future round of fixture growth cannot produce a misleading message, whether or not anyone remembers to update the table. |
| Generate the table at build time from `main.c`'s `rc = N` comments | Rejected — `ctest-fixtures.py` is lane B's, the fixture's comments are not a machine format, and it is a large mechanism to remove a cost that option 3 has already made harmless. Reconsider only if several fixtures want it. |

**What this generalises to.** Any diagnostic that *decodes* a value — an exit
code, an errno, a status word, a magic number — is a lookup table that lives
apart from the thing it describes, and will therefore drift from it. The
sustainable form is not a table that someone promises to maintain; it is a
table that knows its own domain and says when it has been handed something
outside it. The maintenance then becomes optional, which is the only kind of
maintenance that reliably survives.

**Where it lives.** `kernel/src/proc/spawn.rs::self_test_jobctl` — the doc
comment and the `exit_code != EXPECTED` branch.
