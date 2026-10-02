## §336 — `split` reads its input whole, shares one `sh -c`, and diverges from GNU where GNU reads out of bounds

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)

**In short:** `split` cuts a file into pieces. Rewriting it to match GNU
coreutils 9.4 exactly forced four choices that had a real alternative: whether
to stream the input or read it whole; where the "hand this text to a shell"
code should live; what to do about a GNU option that is genuinely, measurably
broken; and how to report a suffix width that cannot be represented at all.
This records those four. The behaviour rules themselves — how `-n l/N`
partitions a file, when the suffix field widens — are not decisions; they were
read out of upstream's `src/split.c` and are documented in the module's own
`//!` header.

### 1. The input is read whole, not streamed

`-n N` and `-n l/N` need the input's *size* before they can decide where the
first cut goes, and `-n K/N` needs to skip to a byte offset computed from it.
GNU handles this with `fstat` plus `lseek` on a regular file and a full
buffering fallback on anything else — two code paths, of which the interesting
one is only reachable when the input happens to be seekable.

We read the input into memory and index it. The alternative — reproducing the
two-path structure — buys a bounded footprint on huge inputs, which is a real
benefit; it costs a second implementation of every cutting rule, exercised only
by the pipe case, which is exactly the arrangement that lets the two drift
apart unnoticed. This is the same call as §335 made for `csplit`, for the same
reason, and it is recorded again here because the *pressure* is different:
`csplit`'s patterns can be satisfied by streaming, so reading whole was a
simplification; `split -n`'s cannot, so here it is closer to a necessity, and
the streaming variant would have been a partial one.

It is also worth being plain about the limit this accepts: a `split -l 1000` of
a file larger than memory works in GNU and does not work here. That is the
price, and if it ever bites, the fix is a third mode for the *record*-counting
options only (`-l`, `-C`, `-b`), which do not need the size — not a general
streaming rewrite.

### 2. `sh -c` becomes a shared module rather than a fourth copy

`split --filter=CMD` takes a shell *command*, not an argv, so it may not
tokenise the string itself — it has to hand the text to a shell. `awk` already
needed exactly this three times over (`system()`, `cmd | getline`, `print |
cmd`), and had its own private copy.

Rather than add a fourth, the code moved to `userspace/coreutils/src/shell.rs`
and both callers now use it (a third, `sh`'s `$(…)` substitution, was folded in
at the same time). The module looks too small to be worth sharing — it is
`Command::new("/bin/sh").arg("-c")` — and that appearance is the argument
*for* sharing it, because the one decision inside it is invisible until it is
wrong. On SlateOS the answer is `/bin/sh` by absolute path. On the Windows
development host there is no `/bin/sh`, and the obvious fallback, `cmd /c`,
does not *fail*: it succeeds, and silently reinterprets the command under
Windows quoting rules that the script was never written against. A copy that
reaches for `cmd /c` mis-executes rather than reporting that it cannot execute.
Deciding that once, in one file, with the reasoning attached, is the whole
point. Four more copies remain outside the crate and are tracked in
`known-issues.md` under
`TD-SH-C-IS-SPELLED-FOUR-MORE-TIMES-OUTSIDE-THE-COREUTILS-CRATE`.

### 3. `--hex-suffixes=FROM` with a letter in it is a deliberate divergence

GNU 9.4 accepts `a`–`f` in a hex start value (its validator is a `strspn`
against the hex alphabet) and then converts it with `c - '0'`, which is right
for digits and wrong for letters: `'a' - '0'` is 49, indexing 33 past the end
of a 16-character array. The names that come out are not consecutive and never
*sorted* — measured, `split -n 3 --hex-suffixes=1f` produces `x1f`, `x13`,
`x14` where it should produce `x1f x20 x21`. Sort order is the one property the
entire suffix mechanism exists to provide. In `--hex-suffixes=bb` it is worse
than misnaming: GNU writes one piece of three and then reports `output file
suffixes exhausted`, so two thirds of the input is silently dropped.

The alternative was byte-for-byte fidelity, which here would mean reproducing
whatever bytes happen to follow that array in *this* build's memory image.
That is not a specification, it is an artefact, and matching it would make our
`split` produce names that vary by compiler. Ours counts in base 16 and is
correct. The three affected cases are carried in `scripts/split-diff.sh` as
declared `xfail_case`s rather than quietly omitted, so the divergence is
visible in the harness output (`5 differ on purpose`), and the measurements are
in `known-issues.md` under
`TD-SPLIT-GNU-HEX-SUFFIX-START-READS-OUT-OF-BOUNDS`.

### 4. `-a -1` reports ERANGE rather than a negative width

`-a` takes a suffix length. A negative one has no meaning, and GNU's
`xdectoumax` rejects it — but with which message? It parses the argument as an
unsigned value, so `-1` does not read as "minus one, out of range at the bottom"
but as a conversion failure at the top, and the sentence the user gets is the
ERANGE one, not "invalid". We match this rather than emitting the more obviously
correct "invalid suffix length" because the whole point of the exercise is that
a script reading our diagnostics reads the same text it reads from GNU. Where
upstream's message is merely *odd* we reproduce it; the `--hex-suffixes` case
above is the boundary — we reproduce odd wording, but not an out-of-bounds
read.
