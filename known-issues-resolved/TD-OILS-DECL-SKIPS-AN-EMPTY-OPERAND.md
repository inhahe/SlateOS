### TD-OILS-DECL-SKIPS-AN-EMPTY-OPERAND. `declare ""` and its siblings passed over an operand whose name half was empty instead of refusing it — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the operand loop in
`Shell::builtin_declare_scoped`, ahead of the identifier gate.

**What:** the empty string is a name like any other malformed one, and bash
refuses it rather than passing over it. osh succeeded silently:

```sh
declare ""     # bash: declare: `': not a valid identifier, rc 1
declare '=5'   # bash: declare: `=5': not a valid identifier — same operand,
               #       quoted back whole once the `=` is split off
local "" ; typeset ""          # same, under their own tags
declare "" v=1                 # v is still bound; the command reports rc 1 at the end
declare "" 1x ""               # every bad name speaks, in the order written
```

`-n` judges it one step earlier: the wholly empty operand — and only that one,
not `-n '=x'` or `-n '='` — is prefixed with
`warning: : circular name reference`, because the empty name equals the empty
value an unbound name reads back as and so trips the nameref self-reference
rule. That is a hard refusal at global scope and a warning inside a function,
the same two-voiced choice `nameref_value_error` already makes.

**Fixed 2026-08-04.** The empty name simply stopped being skipped before
reaching the identifier gate, which already produced exactly that diagnostic.
Pinned by the corpus case `declare-refuses-an-empty-operand.sh` and the unit
test `declare_refuses_an_empty_operand`.
