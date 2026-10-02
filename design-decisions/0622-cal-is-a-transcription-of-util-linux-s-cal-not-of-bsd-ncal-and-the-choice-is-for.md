## 622. `cal` is a transcription of util-linux's `cal`, not of BSD `ncal` — and the choice is forced by which one the shipped `cal` already resembled

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** there are two programs called `cal` in the world. The one on
Linux comes from a package called util-linux; the one on the BSDs and macOS
comes from a package called `ncal`. They print the same kind of calendar but
disagree about almost every detail — which options exist, where the month name
sits, what happens in September 1752, and what `cal 5` means. We had to pick
one to be faithful to, because being half-faithful to both would produce a
program that matches neither and surprises everyone. We picked util-linux,
because that is the one SlateOS's own `cal` was already an imitation of, and
because it is the one a Linux script would be written against.

**Context.** `cal` was on the argv-as-`String` backlog (§600, §601): it read
its arguments as text, so a byte that is not valid UTF-8 killed it, and on this
OS such a byte is a legal filename. Fixing that meant rewriting the argument
handling; and the existing implementation turned out to be so far from any real
`cal` — no options at all, `cal 5` meaning May rather than the year 5, no
Gregorian reform, four separate geometry errors — that rewriting the argument
handling alone would have been polishing a stub. So the rewrite was whole-program,
which forced the question of *which* `cal` to be.

**The two candidates.**

| | util-linux 2.39.3 `cal` | BSD `ncal` 12.1.8 (in `cal` mode) |
|---|---|---|
| *What changes:* the option set | `-1 -3 -m -j -n -s -S -y -Y -w -v -c --reform --iso --twelve --color` | `-1 -3 -A -B -C -d -b -h -H -J -M -S -W -e -o -p -s -j -w -y -m` — a different alphabet, and `-s` means "switch country", not "Sunday" |
| *What changes:* `cal -j` | day-of-year numbering *from the reform*, three digits | day-of-year numbering, and `-J` additionally means Julian calendar |
| *What changes:* the reform | selectable: `1752`, `gregorian`, `iso`, `julian` | selectable by *country code* (`-s GB`), a whole locale database |
| *What changes:* September 1752 | 11 days vanish and the row is short | same days vanish, laid out differently |
| *What changes:* `cal 2026` header | year centred over the whole three-month row | centred differently, and `ncal` can print vertically by default |

**Decision.** Transcribe util-linux 2.39.3 `misc-utils/cal.c`, with its
supporting library files (`lib/mbsalign.c`, `lib/strutils.c`, `lib/timeutils.c`),
byte for byte — including behaviour that is plainly wrong upstream. Build the
reference (`/usr/local/bin/cal` from the 2.39.3 tarball) and *generate* the test
goldens by running it, rather than transcribing expected output by hand.

**Rationale.**

- **It is what was already half-there.** The previous implementation's month
  names, week layout, and centred header were util-linux's shape. Switching to
  `ncal` would have changed the output of a command that already exists on this
  system, for no user-visible gain.
- **It is what a script would expect.** SlateOS targets the Linux ABI (§1).
  A shell script ported to this OS was written against the Linux `cal`, and its
  `cal -3` or `cal --color=never` must mean what it meant there.
- **`ncal`'s configurability is a dependency we do not have.** `-s <country>`
  needs a table of per-country reform dates and a working locale database.
  util-linux's four named reforms need neither.
- **A single reference makes byte-exactness checkable.** Because the goldens
  are generated from a built binary rather than written down, "matches upstream"
  is a mechanical fact and stays true as the code changes. Two references would
  have made every disagreement a judgement call.

**The uncomfortable half of it: upstream bugs are transcribed too.** Two
alignment defects in vertical mode — a header line three bytes wider than its
body (`cal -v 2 2024`), and a highlighted week number one column left of the
rest (`cal -v -w` on a terminal) — are reproduced exactly rather than fixed. This is deliberate and is the price of the decision above: a
golden test that says "match upstream except where upstream is wrong" is a test
whose oracle is opinion. If those are ever fixed upstream, we take the fix; we
do not lead it. See `B-CAL-REPRODUCES-TWO-UPSTREAM-ALIGNMENT-BUGS` in
`known-issues.md` for what they look like and how to tell them from our own
mistakes.

**Alternatives considered.**

- **Transcribe `ncal`.** Rejected for the four reasons above. It would also
  have meant that `cal` and `ncal` — if we ever ship the latter — are the same
  program under two names, which is exactly the duplication B-Q7 exists to
  reduce.
- **Write a "best of both" `cal`.** Rejected outright. It matches no
  documentation anyone has, so every question about its behaviour ("does `-s`
  mean Sunday or a country?") has to be answered by reading our source. The
  whole value of transcribing a standard tool is that its manual page is
  already written.
- **Fix the argv handling and leave the rest.** Rejected: it would have left a
  `cal` with no options and a wrong `cal 5`, and struck the entry off the
  backlog while the program remained a stub. The backlog exists to remove
  defects, not to remove lines from a file.

**Where it lives.** `userspace/coreutils/src/bin/cal.rs` — the module doc lists
the 15 defects of the previous implementation, the 16 measured behaviours, and
the 7 places this deliberately diverges. The generated goldens are the `GOLDEN`,
`GOLDEN_COLOR`, `REJECTED` and `UPSTREAM_HELP` tables in its test module.

**How to reverse.** The transcription is faithful enough that switching
references means rewriting the file, not editing it — the option table, the
calendar math, the geometry and the goldens are all util-linux's. A cheaper
partial reversal, if only *behaviour* is wanted rather than lineage, is to add
the `ncal`-only options as new spellings that map onto the existing machinery
(`-J` → `--reform=julian`, `-e`/`-p` → new code); that keeps every measured
behaviour and adds to it, which is the change this decision would not object to.
