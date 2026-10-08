## TD-B-SORT-HAS-NO-EXTERNAL-MERGE-RANDOM-SORT-OR-DEBUG (lane B, 2026-10-08)

**Status:** PARTLY FIXED. `-R` works as of 2026-10-08 (below). **OPEN** for
`--debug` and the external merge. Found while checking `sort` for the safer
opens (`TD-B-GUARDED-PROGRAMS-OPEN-FILES-WITHOUT-OPEN-SAFER`): three things
GNU's `sort` does that ours did not. They were written down on 2026-08-16 as
"remaining limitations" inside an entry that was then moved to
`known-issues-resolved/` (`B-b-sort-had-three-flags-and-got-all-three-wrong`),
so nothing open tracked them since.

**In short:** `sort --debug` is refused, and every input is sorted in memory,
so a file larger than memory cannot be sorted at all and the options that
steer GNU's temporary files are accepted and ignored.

| what | ours | GNU coreutils 9.4 |
|---|---|---|
| `--debug` | `sort: --debug is not implemented`, status 2 | prints each line with its keys underlined, after warnings about the options |
| inputs larger than the sort buffer | read whole into memory | sorted a buffer at a time into temporary files (`-T DIR`, `$TMPDIR`, `/tmp`), merged `--batch-size` at a time, compressed through `--compress-program` |
| `-S`, `-T`, `--batch-size`, `--compress-program`, `--parallel` | accepted, ignored | obeyed; observable with a small `-S`, e.g. `sort -S 1k -T /nonexistent big` is `sort: cannot create temporary file in '/nonexistent': No such file or directory`, status 2 |

**Where:** `userspace/coreutils/src/bin/sort/main.rs` (`DEBUG_UNIMPLEMENTED`,
and the `b'S' | b'T' | b'y'` arm that discards the resource options).

**The fix:** port each from `sort.c`, held to GNU's by `sort-diff.sh`:
`--debug`'s annotations and warnings, and the external merge -- whose
temporary files are `mkstemp_safer`'s, as upstream's are.

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
