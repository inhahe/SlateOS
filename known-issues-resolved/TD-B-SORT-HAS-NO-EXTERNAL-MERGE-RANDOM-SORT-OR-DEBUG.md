## TD-B-SORT-HAS-NO-EXTERNAL-MERGE-RANDOM-SORT-OR-DEBUG (lane B, 2026-10-08)

**Status:** RESOLVED 2026-10-08 (lane B). All three -- `-R`, `--debug` and
the external merge with the resource options it reads -- are upstream's
now, each held to GNU 9.4 by `scripts/sort-diff.sh` (560 cases). What is
left is performance, not behaviour: `TD-B-SORT-SORTS-ON-ONE-THREAD`.

Found while checking `sort` for the safer opens
(`TD-B-GUARDED-PROGRAMS-OPEN-FILES-WITHOUT-OPEN-SAFER`): three things
GNU's `sort` did that ours did not. They were written down on 2026-08-16 as
"remaining limitations" inside an entry that was then moved to
`known-issues-resolved/` (`B-b-sort-had-three-flags-and-got-all-three-wrong`),
so nothing open tracked them since.

**In short, as it was:** every input was sorted in memory, so a file larger
than memory could not be sorted at all, and the options that steer GNU's
temporary files were accepted and ignored -- not even checked: `sort -S x`,
`--batch-size=1` and `--parallel=0` sorted, where GNU refuses each with
status 2. `-R` and `--debug` were refused.

### Done: the external merge and the resource options, 2026-10-08

`sort/external.rs` is upstream's `sort`, `fillbuf`, `merge`, `mergefps`,
`avoid_trashing_input` and `check`: the input read a buffer at a time,
each buffer that does not end the input sorted into a temporary file, the
files merged `--batch-size` at a time; `-m` through the same merge, `-c`
reading only until the first line out of order. Nothing is read whole into
memory any more.

The output never depended on where the input is cut; *when a temporary
file is made* does, so the buffer is upstream's to the byte --
`sort_buffer_size` and `default_sort_size` for its size, `fillbuf`'s
`readsize` and its `line_bytes` per line (48 with one thread, more with
more: the thread count is `num_processors`, now `coreutils::nproc`), a line
longer than the buffer growing it as `x2nrealloc` grows it, and a buffer
that ends one input taking in the next. None of that size is allocated:
upstream's lazily-touched `malloc` would be committed memory on SlateOS, so
the buffer holds the bytes read and keeps upstream's arithmetic beside
them. `sort-diff.sh` finds the threshold where it is: `-S 1k`, `2k`, `4k`
and `8k` against `-T /nonexistent` on a 300-line file, each agreeing.

Temporary files are `mkostemp_safer`'s in the `-T` directories in turn,
then `$TMPDIR` (empty means `/`), then `/tmp`; `--compress-program` writes
and reads them through `PROG` and `PROG -d`, its failure upstream's
`'PROG' [-d] terminated abnormally`. They are removed as each merge
finishes with them, and on every way out: `die`, the end of the run, and
upstream's signals (`SIGINT`, `SIGTERM`, `SIGPIPE` and the rest, unless
inherited ignored), caught for that alone and re-raised. `-o` naming an
input of `-m` is read through a copy, as upstream's `avoid_trashing_input`
copies it; `-o`'s file is created at the start and emptied only when the
output starts, as `check_output` and `stream_open` do.

The resource options are parsed and refused as upstream's
`specify_sort_size`, `specify_nmerge` and `specify_nthreads` refuse them
(`sort/limits.rs`): `invalid -S argument 'x'`, `-S argument '10Q' too
large`, the two-line `--batch-size` refusals with the ceiling from this
process's descriptor limit, `number in parallel must be nonzero`, and
`multiple compress programs specified`. Found on the way and fixed with
them: `-y`, Solaris's ignored option, swallowed the next word whatever it
was, where upstream takes it only when it is all digits -- so `sort -y
file` sorted standard input instead of `file`.

`sort-diff.sh` has 38 cases for these: spilling under `-S 1k` with `-n`,
`-r`, `-u`, `-s`, `-R`, `-z` and small `--batch-size`; `-T` that names no
directory, round-robin, `TMPDIR` set and empty; `gzip` as the compress
program and a missing one; `-m` past its batch size; `-o` onto an input of
`-m` and of a spilling sort, the output file compared; and no temporary
file left behind -- after a spill, a merge, a failure, and a `SIGINT` while
reading. 36 more check the options themselves.

### Done: `--debug`, 2026-10-08

Upstream's `key_warnings` before the sort, and `debug_line`'s underlines
beneath every line written (`userspace/coreutils/src/bin/sort/debug.rs`):
the ordering in force (`text ordering performed using 'C.UTF-8' sorting
rules`), obsolescent `+POS` keys rebuilt from their numbers as upstream
rebuilds them, zero-width keys, significant leading blanks, numeric keys
that span fields and the separators that would read as part of a number,
unused global options, and `-r` reaching only the last resort; then each
line with tabs drawn as `>`, an underline per key in columns (wide
characters two, invalid bytes one, tabs one), tightened to a numeric or
month key's number or name, `^ no match for key` where a key compares
nothing, and the whole line's underline unless `-s` or `-u`. Refused with
`-c`, `-C` and `-o` as upstream refuses it. To report what upstream
reports, `-k1` and `-k1.1` are now stored as upstream stores them, a key
from the start of the line.

`sort-diff.sh` compares `--debug`'s stdout, status and its notes' text in
59 cases; all agree, and 486 cases pass in all.

### Done: `-R`, 2026-10-08

`-R`, `--random-sort`, `--sort=random`, the `R` key letter and
`--random-source=FILE` are upstream's `compare_random`: each key hashed with
MD5 after sixteen bytes of salt, the digests compared, equal keys kept
together. Two things were found by measuring rather than reading:

* **What is hashed depends on the locale**, and ours is the UTF-8 locale's
  (SlateOS is UTF-8 throughout, design-decisions §351). Under `C.UTF-8`
  GNU's `hard_LC_COLLATE` is true, and it hashes each NUL-terminated piece of
  the key through `strxfrm` *with* its NUL; `strxfrm` is the identity there
  for every byte (measured), so the key is hashed with a NUL after it. The
  first version hashed the bare key -- GNU's `C`-locale behaviour -- and 24
  of the harness's random cases differed until it did not.
* `-R` beside `-V` is the one pair the compatibility check lets through
  (they share a slot with `-d`/`-i`), and upstream's `keycompare` tries
  `random` first, whichever was named first.

`sort-diff.sh` has 52 new cases for it: orders under two fixed sources, `-R`
with every letter it may be combined with, keys, `-c`, `-m`, `-z`, a source
read only when a key will be compared at random, the failures (a missing, a
short and an unreadable source; two different sources; `-nR`, `-MR`, `-hR`)
with their wording -- and 10 more for the fix below. All 409 cases pass.

**Found on the way, fixed with it:** every ordering but the default one
compared the key as cut from the line, where upstream's `keycompare` first
copies it with what `-d`/`-i` ignore dropped and the rest translated by
`-f`. So `sort -hf` read `1m` as plain 1 rather than one mebi (`m` is no
unit, `M` is), and `-Vd`, `-Vf`, `-Vi` compared versions with the
punctuation and case still in:

```
printf '1m\n2K\n1k\n3g\n1M\n900\n' | sort -hf
  before: 1m 3g 900 1k 2K 1M
  GNU:    900 1k 2K 1M 1m 3g
```

### Also fixed, 2026-10-08: five comparisons that were not GNU's

Found reading the parsers for `--debug`, and each measured against GNU 9.4
before it was changed (`|` is a line break, `#` a NUL):

| what | input | GNU | ours, before |
|---|---|---|---|
| `-g`: what `strtold` cannot read sorts first, then the NaNs, then numbers | `abc 0 -1 nan (empty) " x"` | `(empty) " x" abc nan -1 0` | `nan -1 (empty) " x" 0 abc` -- NaN first, and a non-number read as zero |
| `-g` compares 80-bit `long double`s | `-gs` of `9223372036854775809`, `9223372036854775808` | `...808` first | input order: one `double` |
| `-g` skips `strtold`'s white space, `\v` and `\f` included | `\v5`, `4` | `4 \v5` | `\v5 4`: no number, so zero |
| `-h` knows ronna `R` and quetta `Q` | `1Q 1Y 1R 2Z` | `2Z 1Y 1R 1Q` | `1Q 1R 2Z 1Y` |
| a newline is a blank (`field_sep`): under `-z` it separates fields, `-b` skips it, `-d` keeps it | `-z -k2,2n` of `x\n1#y 2#z\n\n0#` | `z\n\n0#x\n1#y 2#` | `x\n1#z\n\n0#y 2#` |

`-g` is upstream's `general_numcompare` now, over `coreutils::extfloat`'s
`strtold` -- the x87 format GNU computes with, already used by `seq` and
`printf`. `sort-diff.sh` has 18 cases for the five; 427 pass.
