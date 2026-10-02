### TD-OILS-DECL-COMPOUND-FOLD-USES-MENTIONED-LETTERS. A compound literal's case fold follows the letters the command *mentions*, not the attribute the name ends up with — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` —
`exec_declare_with_arrays_scoped`. osh folds a compound literal's values
with the attribute the name is left holding (enable minus removals),
which is the right rule for the scalar path but not for this one.

For a *scalar* (and for a subscripted or non-compound operand, which
route through `builtin_declare_scoped`) bash folds by the final
attribute, and osh already matches: `declare -l +u s=AB` stores `ab`,
`declare -l +l s=AB` stores `AB`. For a **compound literal** bash instead
decides the fold from the case letters the command mentions *in either
direction*, and cancels the fold entirely when it mentions more than one:

```sh
declare -l +l q=(AB);  declare -p q   # bash declare -a q=([0]="ab")   osh …"AB"
declare -l +u q=(AB);  declare -p q   # bash declare -al q=([0]="AB")  osh …"ab"
declare -l +c q=(AB);  declare -p q   # bash declare -al q=([0]="AB")  osh …"ab"
declare -u +l q=(ab);  declare -p q   # bash declare -au q=([0]="ab")  osh …"AB"
declare -Al +u m=([k]=AB)             # bash declare -Al m=([k]="AB")  osh …"ab"
```

Note that the *attribute* column is already right in every one of these —
only the stored value differs.

**Measured rule (bash 5.2.37, `target/dvscratch/px15.sh`–`px27.sh`).**
Three earlier models were falsified by measurement before this one held
against every row of thirteen probe scripts. The rule turns on whether
the command *claims* the literal:

> **claimed** = some word in the `-` direction names a kind or a scope
> (`a`, `A`, `g`) — anywhere in the word list — **or** the first mention
> of any *value* letter (`i`, `l`, `u`, `c`, `I`) is in the `-`
> direction.

* **Claimed** → the literal binds under every value letter the command
  names in *either* direction: `declare -a +i q=(2+3)` stores `5` and
  `declare -a +l q=(AB)` stores `ab`, even though `i`/`l` are only ever
  removed. Two or more distinct case letters named → no fold at all
  (`declare -a +l +u q=(Ab)` → `Ab`); no case letter named → the fold the
  name arrived with (`declare -l q=(A); declare -a q=(BC)` → `bc`).
* **Unclaimed** → it binds under what the name arrived with, less the
  letters this command removes: `declare +i -i q=(2+3)` stores `2+3`
  (never integer), and `declare +l -i q=(AB)` stores `AB` even though the
  name ends up `-ai`.
* A claimed fold is genuinely **set** on the variable — it clears the
  other two folds — so the command's own removals then run against it,
  and the *stored* attribute changes too: `declare -l x=(A); declare -a
  +u x=(BC)` leaves `declare -a x=([0]="BC")` with no fold at all.
* "Arrived with `-i`" is not simply the pre-command snapshot: converting
  a dynamic name to an array (`declare -a HISTCMD=(zz 3+4)`) carries the
  slot's own `-i` in during this very command, and the literal binds
  under it (`[0]="0" [1]="7"`).
* Scalars, subscripted operands (`q[0]=`) and non-compound right-hand
  sides use the plain final-attribute rule and always did match.
* Setting an attribute never re-folds a value already stored, in either
  path: `q=(AB); declare -l q` leaves `AB`.

**Resolved 2026-07-31.** `exec_declare_with_arrays_scoped` now computes
the pair the literal *binds* with (`claimed` / `single_case` /
`bind_fold` / `bind_int`, against `pre_fold`/`pre_int` read out of the
operand's `VarSnapshot`) separately from the pair the name is left
holding, swaps it in around `apply_assignment`, and applies `post_fold`
for the claimed case. Covered by
`a_compound_literal_binds_under_the_letters_the_command_names` and six
new sections of `userspace/oils/tests/corpus/declare-plus-flags.sh`.

What is *not* covered — split out as
TD-OILS-DECL-ERROR-SKIPS-FLAG-APPLY — is bash's habit of abandoning a
name's flag application after an error that is raised once the literal
has already bound.
