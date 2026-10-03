### TD-OILS-ULIMIT-DOES-NOT-PAIR-OPTIONS-WITH-OPERANDS. bash gives every limit letter its own value and settles it one letter late; osh collected the letters and read one operand, and said `too many arguments` — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `builtin_ulimit` and its new
`ulimit_settle` helper, plus the `UlimitMode` record beside `RlimitSpec`.

**What:** osh treated `ulimit` as "a set of option letters, then at most one
operand". bash's grammar is quite different, and two things about it are load
bearing.

*Every letter carries its own value*, taken from the rest of its own word or
from the next word when that word does not begin with `-`. So a single call can
set one limit and report another — and combined letters are **not** split,
because the second letter is the first one's value:

```sh
ulimit -c 0 -f     # sets core, reports file      osh was: too many arguments
ulimit -cf         # `f` is core's value:  ulimit: f: invalid number
ulimit -c -f       # `-f` is an option, so both are reported
```

*A letter is settled only when the next one arrives*, or at the very end. That
one-deep delay is not a detail of the loop, it is observable three ways:

```sh
ulimit -c -- 5     # the leftover reaches back across `--`: core = 5
ulimit -c 7 -a     # `-a` answers the call and the pending set never happens
ulimit -c 5 -z     # the bad option abandons it too; core is untouched
ulimit -c -H       # the modifiers are read at settle time, so this reports hard
```

Everything past the one value is dropped in silence (`ulimit -c 5 junk -f`
prints nothing at all, because the scan stops at `junk` and the `-f` is never
seen), and a pair that fails ends the call where it stands (`ulimit -cf -n`
never reaches the `-n`). With no letter at all, the leftover is the file
limit's value, which is how `ulimit -- 0` and `ulimit -H 0` work.

Two smaller divergences fell out of the same rewrite: `-H` and `-S` together
report the **soft** limit (osh reported the hard one), and the report is
labelled when more than one letter was *asked for* rather than when more than
one is printed — `ulimit -c 0 -f` prints the labelled form for the file limit
alone.

**Fixed** by settling one pending `(letter, value)` pair at a time. Pinned by
the corpus case `ulimit-pairs-each-letter-with-its-own-value.sh` and the unit
test `each_ulimit_letter_takes_its_own_value`.

**Standing lesson:** the host had made this look unmeasurable — MSYS bash
cannot modify most rlimits, so a corpus case comparing multi-limit *setting*
would be measuring MSYS. But the grammar is not the enforcement: restricting
the sets to `-c` and reading back only the soft half left every structural
question above answerable, and the one genuinely host-dependent claim (whether
a set without `-H`/`-S` really lowers the hard limit — MSYS declines, Linux
obeys) moved to the unit test, where osh's own model is the reference. When a
behaviour looks untestable, check whether it is really the *whole* behaviour
that the host obscures or only one axis of it.
