### TD-OILS-A-COMPOUND-KIND-REFUSAL-INSIDE-A-FUNCTION-IS-REPORTED-ONCE-AND-ENDS-THE-COMMAND. `f() { declare -gA n=([k]=v); }` on an indexed `n` gives one line and swallows the rest of the function's line; bash gives two and carries on — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_compounds_scoped`,
the `array_kind_conflict` refusal.

**What.** The refusal has two shapes, and osh only had the top-level one. bash
splits on whether there is a function frame around the command (its
`variable_context`), and no nameref need be involved:

```text
                                       bash                     osh
declare -A n=([k]=v)                   `n: cannot convert       the same — agreed
  (top level, indexed `n`)             …`, then the rest of
                                       the parse unit is
                                       thrown away
f() { declare -a q=(1 2)               `q: cannot convert …`    one line, and the
      declare -A q=([k]=v); }          then `declare: q:        rest of the unit
                                       cannot convert …`,       discarded
                                       s=1, `f` carries on
f() { declare -gA n=([k]=v); }         the same but the first   the same one line
  (global indexed `n`)                 line is tagged `f: `
```

So inside a function it is an ordinary failure: the builtin runs, is handed the
same operand, and refuses it a second time with its own tag — every machinery
half before any builtin one, exactly as the `noassign` and readonly refusals
beside it already did. The refused operand is abandoned whole (no attribute of
the command lands on it) while its neighbours are not.

The tag on the machinery's half is the running function's name, and only when
the binding would be a **global** one (`declare -g`); a declaration binding a
local is untagged, as at top level. That is the opposite way round from the
readonly refusal, which is tagged and only ever fires on a local — the two come
from different places in bash.

**Fixed 2026-08-05.** The refusal now holds the builtin's half on the operand
(`BoundCompound::refused_local`) and `continue`s instead of returning
`Flow::Discard`, keeping the discard for the top-level case alone. Corpus:
`a-compound-kind-refusal-inside-a-function-is-reported-twice.sh`.
