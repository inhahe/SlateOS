## BUG-CSPLIT-ACCEPTS-TWO-SUFFIX-FLAGS-GNU-REJECTS — `csplit -b '%+d'` wrote files GNU refuses to create (lane B)

**Status:** FIXED 2026-08-21.

**What it was.** `SuffixFormat::parse_spec` in `src/bin/csplit.rs` accepted
printf's full flag set — `-`, `0`, `#`, `'`, `+` and space — for `-b`'s suffix
format. GNU accepts only the first four. Its parser stops at anything else and
reads that byte as the conversion specifier, so `+` and space are not merely
ignored, they are *errors*.

Measured against GNU 9.4 under `LC_ALL=C.UTF-8`:

| `-b` format | GNU | ours (before) |
|---|---|---|
| `%+d` | `invalid conversion specifier in suffix: +`, exit 1 | wrote `+0`, `+1` |
| `% d` | `invalid conversion specifier in suffix: ` (a literal space), exit 1 | `missing conversion specifier in suffix`, exit 1 |
| `% 5d` | `invalid conversion specifier in suffix: `, exit 1 | wrote `    0`, `    1` |
| `%-5d`, `%#o`, `%'d`, `%05d` | accepted | accepted |

**Why this shape of bug is the bad one.** `%+d` did not fail loudly — it
*succeeded*, producing a set of output files named `+0`, `+1`, … for a command
line GNU rejects outright. A script that works here and dies on GNU (or the
reverse) is worse than one that fails on both, because nothing points at the
cause. The `% d` row is milder: both sides fail and exit 1, but with different
sentences, so only the wording was wrong.

**The fix.** Drop `+` and space from the flag loop, and delete `flag_plus` /
`flag_space` from `SuffixFormat` along with the sign-column branch in
`render()` that consumed them. That branch was dead weight regardless: the
values being formatted are section indices, which are never negative, so a
sign column could only ever have emitted a constant `+` or space.

**How it was found**, which is the part worth keeping. It was not looked for.
The `% ` case was added to `csplit-diff.sh` for an unrelated reason — to
exercise the `isprint`-vs-`isgraph` boundary of the *diagnostic* rule while
auditing `BUG-AWK-NAMES-AN-OPTION-NOBODY-TYPED` — and the space only reaches
the conversion slot, where it can be printed from, if the parser declines to
eat it as a flag. Ours ate it, so the case came back with a different sentence
and the flag set was measured properly for the first time. A test written for
one branch found a bug in another.

**Coverage.** `csplit-diff.sh` now pins the fix from both sides: `%+d` and
`% 5d` (must be rejected) and `%05d`, `%#o`, `%'d` (must still be accepted).
Pinning only the rejections would pass for a parser that had no flags at all.

**Why `%-5d` is covered by a unit test and not by the harness**, since the
asymmetry looks like an oversight. Left-justifying in width 5 names the output
files `xx0` and `xx1` followed by four spaces, and **Win32 silently strips
trailing spaces from every path component**. Our csplit is a native Windows
binary, so it asks for `xx0    ` and the filesystem hands back `xx0`; GNU's
runs under WSL, whose Linux VFS keeps them. The harness would therefore report
a divergence neither implementation has. Measured, not reasoned: `python -c
"open('zz1    ','w')"` in the same directory also produces `zz1`. The case is
asserted in-process instead, by `the_suffix_flag_set_is_gnus_and_not_printfs`,
which checks the rendered name is `xx3    ` with the spaces present. `%05d` is
in the harness in its place so that a *width* is still exercised there — zero
padding is leading, so it survives the trip. On SlateOS itself, where every
byte but `/` and NUL is legal in a name, the case would be honest.

**A harness bug found underneath this one.** `manifest()` in both
`csplit-diff.sh` and `split-diff.sh` iterated `for f in $(ls | grep -v … |
sort)`. An unquoted command substitution is word-split on IFS, so a file name
containing a space arrives in pieces and one *ending* in a space loses it
outright — the manifest then reads a nonexistent file, prints empty contents,
and scores a harness defect as a divergence. That is exactly how the `%-5d`
result presented itself, and it would have mis-scored any future case with a
space in an output name (`split --additional-suffix=' x'` reaches it too).
Both now iterate a glob, which passes each name through whole and is already
in sort order. `split-diff.sh` had no case that reached it yet; it was fixed
anyway rather than left as a trap.
