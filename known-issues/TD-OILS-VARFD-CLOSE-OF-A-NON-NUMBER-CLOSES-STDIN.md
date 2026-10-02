### TD-OILS-VARFD-CLOSE-OF-A-NON-NUMBER-CLOSES-STDIN. bash's `{v}>&-` closes fd 0 when `$v` is not a number at all — 2026-08-01 — ⛔ **WONTFIX** (a bash bug, deliberately not replicated)

**Where:** `userspace/oils/src/interp.rs` — `varfd_close_target`, whose doc
comment cites this entry.

`{v}>&-` is the one varfd form that does not *allocate* a descriptor: it names
one by reading `$v`. bash reads that value with `legal_number`, i.e. `strtoimax`
over the whole string — surrounding C-locale whitespace skipped, a leading `+`
or `-` taken as a sign — and then refuses anything outside `0 ..= INT_MAX`. So
`v=' 10 '` closes fd 10, `v=+1` closes fd 1, and `v=-1` and `v=2147483648` are
`v: ambiguous redirect` at status 1. A number naming a descriptor that is not
open is *not* an error: `close(2)`'s `EBADF` is ignored, exactly as it is for a
literal `9>&-`.

The loose end is what happens when the value is not a number at all.
`legal_number` zeroes its out-parameter on failure, and `redir_varvalue` returns
that result without checking the flag, so bash closes **fd 0** — silently, at
status 0:

```sh
printf 'X\n' > in
( exec 3<in; u=abc; exec {u}>&-; echo "rc=$?"; read -r l; echo "read rc=$?" )
# bash: rc=0, then `read: read error: 0: Bad file descriptor`
```

Every non-number does it: `abc`, `1x`, `1 2`, `0x10`, `--1`, and a digit run
that overflows `intmax_t` (`999999999999999999999`). Measured over 18 values in
`target/dvscratch/t3/q3.sh`; those six are the only shapes where osh and bash
now disagree, and the disagreement is entirely this.

osh gives a non-number the same refusal an out-of-range number gets —
`v: ambiguous redirect`, status 1 — because taking the shell's stdin away is
not a rule anyone wrote down, and a script that trips it under bash is already
broken. Pinned by the unit test
`varfd_close_reads_its_variable_the_way_bash_does`, and named as deliberately
absent in the header of the `{varname}` corpus case.
