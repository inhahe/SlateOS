### TD-OILS-A-VARFD-CLOSE-WHOSE-VARIABLE-IS-NOT-A-NUMBER-SHOULD-CLOSE-FD-0 — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `varfd_close_target`, reached from
`Shell::varfd_close_fd`.

**What.** bash's `legal_number` (`general.c`) returns **0** on failure — never a
negative — and writes `*result = 0` *before* it parses. `redir_varvalue`
(`redir.c`) tests `if (legal_number (val, &vmax) < 0) return -1;`, so that guard
never fires: every value it rejects silently becomes `0`, and `{v}>&-` closes
**stdin**. `i = vmax` then narrows to `int`, so it *wraps* rather than
saturating.

```text
$ bash -c 'v=nope; exec {v}>&-; echo rc=$?'   # closes fd 0, rc=0
$ osh  -c 'v=nope; exec {v}>&-; echo rc=$?'
osh: line 1: v: ambiguous redirect
```

Measured across bash 5.2.37: `nope`, `0x10`, `1 2`, `1x`, a bare `+`/`-`,
whitespace, an overflow, and `4294967296` all close fd 0; `4294967297` closes
fd 1; `2147483648` and `2147483649` wrap to a negative and are refused; only
unset, empty and an actually-negative value are `v: ambiguous redirect`.

**Fixed by** rewriting `varfd_close_target` as `legal_number(v).unwrap_or(0)`
followed by the wrapping `as i32` and the `>= 0` refusal — reusing the same
`legal_number` every builtin that takes a number already goes through. The
`unwrap_or(0)` carries a comment pointing at `general.c`, since it looks like a
bug in *our* code otherwise. Corpus case:
`a-varfd-close-reads-its-variable-through-bashs-legal-number-bug.sh`.
