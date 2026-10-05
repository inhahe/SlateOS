## `B-CAL-REPRODUCES-TWO-UPSTREAM-ALIGNMENT-BUGS` (lane B, 2026-08-27) — **open**, deliberate divergence

**In short:** our `cal` is a byte-for-byte transcription of util-linux 2.39.3
(§622). Two of util-linux's own output bugs are therefore in our output too, on
purpose. Both are in *vertical* mode — the layout you get with `-v`, where the
weekdays run down the left edge and the dates run across instead of the usual
way round. Recorded here so that a future reader who notices the misalignment
fixes it *upstream first*, rather than "fixing" it here and silently breaking
the golden tests that exist to prove we match.

| # | How to see it | What it looks like | Upstream cause |
|---|---|---|---|
| 1 | `cal -v 2 2024` | The line of month names is three bytes wider than every line under it, so the block has a ragged right edge. | The header writer appends its inter-month gutter after the *last* month as well as between months. |
| 2 | `cal -v -w 8 2026` on a terminal (so today's week number is highlighted) | The one highlighted week number sits a column left of all the others. | The highlighted number is printed at field width `day_width - narrow` instead of `day_width` — the escape sequence is counted against the field. |

**Why not fix them.** The whole value of the transcription is that "matches
upstream" is a mechanical fact: the goldens in the test module are *generated*
by running the real `/usr/local/bin/cal`, not typed out. The moment we start
correcting upstream where we think it is wrong, that oracle becomes an opinion
and every future disagreement needs a human ruling. If these are fixed in a
later util-linux, we regenerate the goldens and take the fix.

**How to tell these from our own mistakes.** Rebuild the reference
(`/var/tmp/ul/util-linux-2.39.3` in WSL, binary at `/usr/local/bin/cal`) and run
the same command. If the reference misaligns identically, it is this entry. If
it does not, it is our bug and should be fixed.
