## TD-SPLIT-GNU-HEX-SUFFIX-START-READS-OUT-OF-BOUNDS (lane B, 2026-08-18) — **upstream bug; ours diverges on purpose**

**In short:** `split --hex-suffixes=FROM` is supposed to start naming its output
files at the hexadecimal number `FROM` — so `--hex-suffixes=ff` should produce
`xff`, `x100`, `x101`. In GNU coreutils 9.4 it does not, whenever `FROM`
contains a *letter* (`a`–`f`): the files come out with names that are not
consecutive and not hexadecimal at all. Ours produces the correct names. This
entry exists so nobody "fixes" ours to match GNU, and so
`scripts/split-diff.sh` can carry the three affected cases as declared
expected-failures rather than quietly omitting them.

**Where upstream goes wrong.** `split.c` validates the start value against the
suffix alphabet with `strspn`, which accepts `a`–`f` for hex. It then converts
it to a per-position index with, in effect:

```c
sufindex[i] = numeric_suffix_start[i] - '0';
```

That subtraction is right for `'0'`–`'9'` and wrong for letters: `'a' - '0'` is
49, and `'f' - '0'` is 54. Those indices are then used to read from a 16-character
alphabet, so every letter in the start value indexes 33–38 elements past the end
of the array. The name that comes out is whatever bytes follow it in memory,
carried forward by the ordinary increment.

**Measured**, GNU coreutils 9.4, glibc, `LC_ALL=C`, three chunks:

| Command | GNU 9.4 leaves | ours |
|---|---|---|
| `split -l 5 --hex-suffixes=ff` (4 pieces) | `xf3 xf4 xf5 xff` | `xff`, then `output file suffixes exhausted` |
| `split -l 5 --hex-suffixes=a` (4 pieces) | `x0a x0e x10 x11` | `x0a x0b x0c x0d` |
| `split -n 3 --hex-suffixes=1f` | `x13 x14 x1f` | `x1f x20 x21` |
| `split -n 3 --hex-suffixes=bb` | **`xbb` alone**, plus `split: output file suffixes exhausted` — two thirds of the input is dropped | `xbb xbc xbd` |

Two things to notice. In every row the names are out of order — `xff` is
*followed* by `xf3` — and sort order is the one property the whole suffix
mechanism exists to guarantee. (It is also why upstream refuses to widen the
suffix field when a start value is given: an arbitrary start "would break sort
order for files generated from multiple split runs". The letter case breaks it
far more thoroughly than widening ever could.) And in the `=bb` row GNU does not
merely misname the files, it **loses data**: it writes one piece of three and
stops.

Only a letter in the *incrementing* position misbehaves, which is why
`--hex-suffixes=e0` looks fine (`xe0 xe1 xe2`) and `=0f` does not. That is the
shape of an out-of-bounds read, not of a rule.

Ours errors in the first row for a reason that is not a bug: `ff` is the largest
two-digit hex suffix, an explicit start turns widening off, and a fifth piece
therefore has nowhere to go. GNU should report the same thing there and instead
invents four names.

**Our behaviour:** the start value is converted by counting in base 16, so the
names are the consecutive hexadecimal numbers from `FROM` upward, and
`--hex-suffixes=ff -a 2` is the length error it should be rather than three
garbage names. This is deliberate: reproducing an out-of-bounds read to match
byte-for-byte would mean reproducing *this machine's* heap layout, which is not
a specification.

**Action:** none for us. Worth reporting upstream. If it is ever fixed there,
the three `xfail_case` lines in `scripts/split-diff.sh` become ordinary
`names_case` lines and this entry can be closed.
