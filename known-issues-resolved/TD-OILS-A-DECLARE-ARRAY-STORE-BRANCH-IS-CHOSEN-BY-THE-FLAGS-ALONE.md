### TD-OILS-A-DECLARE-ARRAY-STORE-BRANCH-IS-CHOSEN-BY-THE-FLAGS-ALONE. `declare -a g; declare g='(1 2)'` stores the parentheses where bash re-parses them — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped` — the
store branch is `} else if assoc || indexed {`, i.e. the `-a`/`-A` **flags** of
this command. A name that is already an array but carries no flag falls past it
to `attr_store`.

bash chooses by `creating_array || array_exists` (declare.def:885), where
`array_exists = var && (array_p (var) || assoc_p (var))` — the variable's
current kind, flag or no flag. So a parenthesised value assigned to an existing
array is a **compound literal**, re-parsed and re-expanded:

```sh
declare -a g; declare g='(1 2)'; declare -p g
#  bash: declare -a g=([0]="1" [1]="2")
#  osh : declare -a g=([0]="(1 2)")
```

| | bash 5.2.37 | osh |
|---|---|---|
| `declare -a g; declare g='(1 2)'` | `declare -a g=([0]="1" [1]="2")` | `declare -a g=([0]="(1 2)")` |
| `declare -a g=(7 8); declare g='(1 2)'` | `declare -a g=([0]="1" [1]="2")` | `declare -a g=([0]="(1 2)" [1]="8")` |
| `declare -A g; declare g='(1 2)'` | `declare -A g=([1]="2" )` | `declare -A g=([0]="(1 2)" )` |

(No deprecation warning: declare.def:899 prints it only when `array_exists == 0
&& creating_array == 0`, which is exactly the case that is not this one.)

The *unparenthesised* neighbours already agree — `declare -a g=(7 8); declare
g=9` gives `([0]="9" [1]="8")` in both — because `attr_store` reaches element 0
by another road. It is only the compound spelling that needs the branch.

**Fixed** in `777de87ab`: the branch condition is now bash's — the flags **or**
the name's current array/assoc kind — and which of the two element stores is
made is picked by `self.assoc.contains_key(name)`, i.e. declare.def:972's
`if (assoc_p (var))`. Corpus:
`tests/corpus/a-declare-array-store-is-chosen-by-the-variables-kind-not-the-letters.sh`.

**How it was found:** reading declare.def:880-906 for
TD-OILS-A-GLOBAL-ARRAY-DECLARATION-IN-A-FUNCTION-STORES-INTO-WHICHEVER-BINDING-IS-LIVE.
