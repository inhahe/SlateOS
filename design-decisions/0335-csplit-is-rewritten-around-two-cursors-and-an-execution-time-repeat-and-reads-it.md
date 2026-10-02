## §335 — `csplit` is rewritten around two cursors and an execution-time repeat, and reads its input whole

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)

**In short:** `csplit` cuts a file into pieces at places you name. Ours was
wrong in four independent ways at once — the worst of them produced **10,003
files** where GNU produces 8 — so it was rewritten rather than patched. Three
design choices in the rewrite had a real alternative: how a repeated pattern is
expanded, how the two positions in the input are tracked, and whether the input
is streamed or read whole. This records those three.

### 1. `{*}` and `{N}` are expanded when the pattern runs, not when it is parsed

The old code cloned the pattern *10000 times* at parse time. That is wrong for
two separate reasons and only one of them is the file count: a cloned line
number is re-applied **absolutely** on each copy, so `csplit f 3 '{*}'` split at
line 3, then at line 3 again, forever — the cursor never advanced, and every
one of the 10,003 files after the first two was empty.

The alternative was to keep the clone-at-parse shape and make the clones carry
an incrementing multiplier. It works, but it stores 10000 `Control`s to
represent "repeat until the input runs out", and it makes the repeat count a
property of the pattern list rather than of the loop that walks it — which is
exactly the confusion that produced the bug. The rewrite carries
`Repeat::{Times(n), Forever}` on one `Control` and expands it in the executor,
where the repetition index is in hand and `lines_required * (repetition + 1)`
is one line of arithmetic.

*Cost:* the executor is now a nested loop rather than a flat walk, and the
`{*}` termination condition differs by pattern kind — a regex that stops
matching ends the run with status 0, a line number that runs past the end is a
fatal `line number out of range on repetition N`. Those two really are
different in GNU (`process_regexp` has a `repeat_forever` branch,
`process_line_count` has none), so the asymmetry is not ours to remove.

### 2. Two cursors, `emit` and `search`, rather than one position

The obvious model is a single "where are we" index. GNU does not have one, and
the difference is observable:

```
$ csplit f 2 '/2/'          # 20 lines
2
csplit: '/2/': match not found
```

The `2` split consumed nothing past line 1, and yet `/2/` does not find line 2.
The reason is that a regex match leaves the search position at **`m + 1`**
(GNU's `find_line (++current_line)`), while a line number leaves it at the new
section's start. `csplit f '%MARK%' '/MARK/'` shows the same thing from the
other side: it finds the *second* `MARK`, because the first is behind the search
cursor even though it is the first line of the current section.

A single cursor can be made to reproduce this only by special-casing the
lookahead at each use site. Two named cursors state the rule once, at the two
places that assign them, and every case in the harness that distinguishes them
passes without further conditionals.

*Cost:* one more piece of state to keep consistent, and a reader has to learn
which cursor a given operation moves. The names carry it: `emit` is the first
line not yet written, `search` is the first line the next regex examines.

### 3. The input is read whole, not streamed

GNU streams through a line buffer. We read the file into memory and take
`&[u8]` slices of it, each keeping its own terminator.

*For:* the two cursors are then plain indices into one slice, backwards offsets
(`/RE/-1`) are free, and the byte count of a piece is a subtraction rather than
a running total. Streaming would need a rewindable window sized to the largest
negative offset seen anywhere in the pattern list, and the old implementation's
"re-terminate every line with `
`" corruption is a direct consequence of
having consumed the terminator on the way past.

*Against:* an input larger than memory cannot be split, and one behaviour
diverges — reading a **directory** as input. GNU creates `xx00` and prints its
count of `0` before the read fails, because the failure happens mid-stream; we
report the same `read error: Is a directory` with no count and no file. That is
the *only* case out of 76 in `scripts/csplit-diff.sh` where the two differ
behaviourally, and it is a case with no legitimate use.

*Revisit if:* `csplit` is ever asked to split something too large to hold. The
change that fixes the size limit fixes the directory case in the same motion,
and the harness case for it is already written.

### Why a rewrite rather than four fixes

Three of the four faults were **silent**: the byte counts on stdout were
self-consistently wrong, so only a comparison of the *files* revealed them.
`scripts/csplit-diff.sh` is the first harness in this tree to compare a manifest
of what was left on disk rather than stdout alone, and it was written before the
fixes. Patching four faults individually would have left the shape that produced
them — a parse-time repeat, a single cursor, and `str::contains` standing in for
a regular expression — intact.
