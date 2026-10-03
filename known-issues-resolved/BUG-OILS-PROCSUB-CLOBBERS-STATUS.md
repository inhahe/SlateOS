### BUG-OILS-PROCSUB-CLOBBERS-STATUS. A `>( … )` body's exit status replaced the enclosing command's — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Where:** `userspace/oils/src/interp.rs` — `proc_sub` / `finish_procsubs` /
the `procsub_out_jobs` field. Corpus case:
`userspace/oils/tests/corpus/procsub-coproc.sh`.

**Symptom.** `false > >(cat > /dev/null); echo $?` printed `0` in osh and `1` in
bash; `(exit 3) > >(cat > /dev/null)` printed `0` instead of `3`. Because osh
has no `/dev/fd`, an output process substitution is deferred: the body runs
*after* the enclosing command, inline in the same interpreter (TD-OILS22) — and
`exec_program` set `last_status` from the body, overwriting the status the
enclosing command had just produced. In bash the body is a separate process
whose status is only ever reachable through `$!`/`wait`.

A second, subtler divergence came out of the same probe: bash forks the
substitution's process while *expanding* the enclosing command's words, so `$?`
*inside* the body is the status of the command before the enclosing one. osh ran
the body later and let it see the enclosing command's own status:

```
true
false > >(echo "body=$?" >&2; cat > /dev/null)   # bash: body=0   osh (was): body=1
```

**Fix.** `procsub_out_jobs` entries now carry the `$?` captured at expansion
time; `finish_procsubs` saves the enclosing command's status, runs each body
with its recorded fork-time status, and restores the enclosing one afterwards.
`proc_sub` does the same restore around the *input* form, which runs its body
during expansion.

**Verified:** `interp::tests::process_sub_does_not_disturb_the_exit_status`
(five measured bash expectations) plus the corpus case. Suite 730 + 15 pass,
clippy clean, 39/39 corpus cases match.

**Not fixed (by design, TD-OILS22):** the substituted *word* is a temp-file path
on hosts without `/dev/fd`, where bash names an fd (`/dev/fd/63`). The corpus
case deliberately does not assert the path's shape.
