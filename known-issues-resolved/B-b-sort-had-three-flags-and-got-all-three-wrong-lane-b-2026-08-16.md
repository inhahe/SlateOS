## B-`sort`-HAD-THREE-FLAGS-AND-GOT-ALL-THREE-WRONG (lane B, 2026-08-16) — ✅ **FIXED 2026-08-16**

`userspace/coreutils/src/bin/sort.rs` was 341 lines and accepted `-r`, `-n` and
`-u`. No `-k`, so there was no way to sort on a column — which is most of what
`sort` is used for — and no `-t`, `-b`, `-f`, `-d`, `-i`, `-g`, `-h`, `-M`,
`-V`, `-s`, `-m`, `-c`, `-C`, `-z`, `-o`, or the obsolete `+POS -POS` form.
The three flags it did have were each wrong in a way that produced a **plausible
answer rather than an error**, which is the failure mode that matters here:

| | |
|---|---|
| lines read with `BufRead::lines()` | one non-UTF-8 byte anywhere in the input aborted the run with a diagnostic. Our paths may hold any byte but `/` and NUL, so `find \| sort` was one odd filename away from failing. |
| `-n` parsed to `f64` | every pair of 20-digit identifiers agreeing in their first 17 digits tied, and then fell to the line comparison — so they came out in *lexicographic* order while claiming to be numeric. |
| `-u` deduplicated whole lines | `sort -nu` on `1` and `1.0` printed both. `-u` means unique *by the key*; those are one number. |

**Why unit tests could not have found this, and what did.** `sort` is the worst
case for tests-written-beside-the-code: every wrong answer is still a
permutation of the input, so it reads perfectly and asserts cleanly against
whatever the author believed. The rewrite was therefore driven by
`scripts/sort-diff.sh`, which runs GNU coreutils 8.32 (natively on PATH) and our
binary over ~225 cases and compares **hex-dumped stdout, exit status, and
whether stderr was loud**. Key *extents* — the one thing output comparison
cannot show — were measured with `sort --debug`, which underlines the key.

**What the harness found that the author had wrong**, each a case where the code
was written from a confident and incorrect memory of GNU:

- **`-g` did not read hex.** `-g` goes through C's `strtod`, which accepts
  `0x10` as 16 (and `inf`/`nan`). Ours read 0.
- **`-h` mis-ranked zeros and negatives.** GNU's `find_unit_order` gives a
  zero-valued number order 0 *whatever its suffix* — so `0K` < `900` — and a
  negative number a negative order. Ours ranked `0K` above `900`.
- **`-k2,2r` did nothing.** GNU has **two different** inheritance predicates:
  the key-inherits-the-globals test *includes* `reverse`, so `-k2,2r` inherits
  nothing else; the should-the-globals-become-a-whole-line-key test *excludes*
  it, so `-r` alone makes no key. Collapsing them into one predicate is the
  obvious simplification and it is wrong in both directions.
- **`-V` is a *file name* comparison, not a version-string one.** It is
  gnulib's `filevercmp`: empty name first, then `.`, then `..`, then every dot
  file ahead of every non-dot file, then the **file suffix**
  (`(\.[A-Za-z~][A-Za-z0-9~]*)*$`) is cut off and the stems compared, with the
  full names compared only on a tie. That is why `a.b` sorts before `a-b`
  despite `-` being the lower byte, and it is what makes `ls | sort -V` behave.
- **`-m` is a real k-way merge, not a re-sort.** On unsorted input the two
  differ, and GNU's answer is the merge's.
- **`limfield` puts both the trailing-blank skip and the `+ echar` advance
  inside `if (echar != 0)`.** Probing `sort --debug -k1,1b` on `" a b"` returns
  `" a"`, which settles it; the natural transcription extends the key past the
  trailing blanks.

**State: 225/225 cases byte-identical to GNU**, 33 unit tests, `sort` now a
four-module binary (`main.rs`, `keydef.rs`, `order.rs`).

### Remaining limitations (deliberate, documented in the module docs)

- **`-R`/`--random-sort` is refused with an explicit message** rather than
  answered wrongly. It needs a keyed hash (GNU uses one seeded from
  `--random-source`), and a wrong random order is indistinguishable from a
  right one, so a loud refusal is the only honest option until the hash exists.
- **`--debug` is not implemented** (exit 2, unknown option). It is a
  developer-facing aid; the harness uses the *host's* `sort --debug`.
- **`-S`, `-T`, `--parallel`, `--buffer-size`, `--temporary-directory`,
  `--compress-program`, `--batch-size` are accepted and ignored.** They tune an
  external merge sort we do not have: everything is sorted in memory. That is a
  real limit for inputs larger than RAM, not a cosmetic one.
