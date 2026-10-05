### TD-OILS-TEST-V-SUBSCRIPT-WALK-COUNT-DEPENDS-ON-QUOTING. bash walks a circular nameref chain twice for an *unquoted* `[[ -v c1[0] ]]` and once for every other spelling — 2026-08-04 — WONTFIX (bash artifact)

**What.** `[[ -v NAME[SUB] ]]` should not care how its operand was quoted — the
word is parsed as a `-v` operand either way — but bash 5.2.37 reports a circular
chain twice for the bare form and once for all three others:

```
declare -n c1=c2; declare -n c2=c1

[[ -v c1[0] ]]      → 2 warnings      [[ -v 'c1[0]' ]]    → 1
                                      [[ -v "c1[0]" ]]    → 1
                                      v='c1[0]'; [[ -v $v ]] → 1
```

All four answer false, status 1. Everything around them agrees with osh:
`[[ -v c1 ]]` and `[ -v c1 ]` warn once and answer false, and `[[ -R c1 ]]`
asks nothing of the chain at all — no warning, status 0, since `c1` *is* a
reference whatever it points at.

Nothing in the once-per-walk model predicts a *quoting*-dependent count: the
unquoted word takes a different path through bash's `[[` word expansion, which
resolves it once more before the `-v` test sees it. osh emits one warning for
all four spellings. Copying this would mean threading the operand's original
quoting into the `-v` test purely to reproduce an artifact.

**Where.** `userspace\oils\src\interp.rs` — the `-v` arm of the conditional
evaluator; osh resolves the operand once, as the quoted forms do.
