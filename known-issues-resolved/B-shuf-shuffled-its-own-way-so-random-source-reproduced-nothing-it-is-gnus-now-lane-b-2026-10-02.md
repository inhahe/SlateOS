## `shuf` shuffled its own way, so `--random-source` reproduced nothing; it is GNU's now (lane B, 2026-10-02) — **FIXED** 2026-10-02

**In short:** `userspace/shuf` was written by hand: its own shuffle, its own
reading of the options, and argv read as `String` (a file name that was not
UTF-8 killed it before its first statement). `--random-source=FILE` exists so
that a run can be reproduced, and with a different shuffle it reproduced
nothing GNU's would. It is replaced by `coreutils/src/bin/shuf.rs`, a port of
GNU coreutils 9.4's `shuf.c` with gnulib's `randperm` beside it and `randint`
from `coreutils::randint` (already ported for `shred`), and the standalone
crate is deleted (§1005). `scripts/shuf-diff.sh`: 89 cases, none differing
from GNU's -- full permutations, head counts, reservoir samples and sparse
samples all identical from the same random bytes.

**What the port had to reproduce, because it is observable:**

- `-n` with input whose size is unknown (a pipe) or over 8 MiB samples a
  reservoir, which draws one random number per line past the head *and one
  more* -- so `cat f | shuf -n 2` and `shuf -n 2 f` print different lines
  from one random source.
- gnulib's permutation keeps its swaps in a hash table for a large range
  sampled sparsely (`n >= 131072`, `n / h >= 32`), and a step that swaps an
  element with itself after it was moved loses the moved value there; the
  dense algorithm does not. Reproduced, since the output is the point.
- `-i 5-4` is an empty range and `-i 6-4` an error; `-n` beyond any count is
  no limit; `-o FILE` is opened after the input is read, so `shuf -o f f`
  works in place; a write failure is reported with its reason by
  `write_error`, once.
