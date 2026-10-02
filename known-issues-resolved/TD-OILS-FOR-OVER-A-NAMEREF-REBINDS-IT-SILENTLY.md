### TD-OILS-FOR-OVER-A-NAMEREF-REBINDS-IT-SILENTLY. A `for` loop whose control variable is a nameref repoints the reference instead of writing through it — bash does too, but refuses a word that is not an identifier — 2026-08-04 — ✅ RESOLVED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the `for`-loop control-variable
write in `Shell::exec_for`. It repointed the reference (correct) but never
checked the word.

**What.** A `for` loop's control variable is one of the few write paths that
does *not* go through a nameref: bash assigns the *reference itself*, so the
word becomes the new referent name. Since a referent name must be an
identifier, a word that is not one is refused, the loop body never runs, and
the whole `for` is status 1:

```
$ declare -n r=zz; for r in 5 6; do echo "body $r"; done; echo "rc=$?"
bash: `5': not a valid identifier
rc=1
$ declare -p r
declare -n r="zz"
```

osh runs the body twice with `$r` empty, leaves `declare -n r="6"`, and exits
0. The referent being set or unset makes no difference (`zz=1` first behaves
the same), so this is about the *word*, not about what the reference reaches.

A word that is a *nameref value* rather than a bare identifier is accepted —
`for r in 'x[1]'` is silent and leaves `declare -n r="x[1]"` — because a
reference may designate one element. The empty word is refused. The refusal
arms neither errexit nor the ERR trap, and the reference keeps the last word
that *was* a name (`for r in aa 5 bb` leaves `declare -n r="aa"`).

**Fixed** in `Shell::exec_for`, at the per-iteration bind and after the
circular check: when the name carries the nameref attribute, the word is put
through `split_assignment_target` — the same `BASE` / `BASE[SUB]` test the
`declare -n` value refusal uses — and a word it rejects gives the complaint,
`last_status = 1`, `for_name_failed = true` (which is what keeps errexit and
the ERR trap out of it) and `Flow::Next`.

This is the `for` loop only. `select` binds its control variable *through* the
reference, so it never asks — `declare -n r=zz; select r in aa` leaves
`declare -n r="zz"` — and so do `read r`, `printf -v r`, `getopts … r` and
`r=v`. Corpus:
`a-for-loop-binds-a-nameref-itself-so-each-word-must-name-something.sh`.

**Impact.** Small: a nameref as a `for` control variable is unusual. Found
while measuring `set -a` attribution for
TD-OILS-ALLEXPORT-MARKS-EVERY-WRITE.
