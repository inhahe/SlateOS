### BUG-OILS-CMDSUB-NUL-BYTES. `$( )` kept NUL bytes instead of dropping them with a warning — 2026-07-27 — ✅ RESOLVED

**Symptom.**
```sh
printf '%s' "$(printf '%b' 'a\0b')" | od -An -tx1
# bash: warning: command substitution: ignored null byte in input, then `61 62`
# osh : no warning, `61 00 62`
```

**Root cause.** osh's command-substitution capture in
`userspace/oils/src/interp.rs` returned the child's bytes unfiltered. A shell
word cannot hold a NUL, so bash discards every one of them — it does *not*
truncate at the first — and warns once per **capture**, however many it dropped.

**Fix.** A new `Shell::strip_capture_nuls` runs on the captured bytes of both
`command_sub` paths (the real subshell and the `$(< file)` fast path, which
backticks share). It is called **before** the trailing-newline strip, which is
observable: dropping the NUL in `$(printf 'a\n\0')` re-exposes the newline as
trailing, so bash yields `a`, not `a\n`. Process substitution is exempt — it
writes a real file, which can hold NULs.

**Regression test.** `userspace/oils/tests/corpus/cmdsub-nul.sh` — leading /
middle / trailing / repeated NULs, multi-command captures (still one warning),
an all-NUL capture, the no-NUL no-warning case, backticks, both strip orderings,
the `$(< file)` path, and the stripped result flowing into word splitting and
arithmetic.
