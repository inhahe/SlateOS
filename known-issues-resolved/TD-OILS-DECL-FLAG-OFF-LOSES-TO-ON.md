### TD-OILS-DECL-FLAG-OFF-LOSES-TO-ON. `-i +i` in one command sets the attribute where bash clears it — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the attribute-application
sites in `builtin_declare_scoped` and `exec_declare_with_arrays_scoped`,
written `if integer { insert } else if unset_integer { remove }` (and the
same shape for `nameref` and `trace`). The "on" direction wins where bash
applies its `flags_on` set first and its `flags_off` set last, so the
removal should win — as it now does for `-x +x` and `-r +r`.

```sh
x=5; declare -i +i x; declare -p x            # bash declare -- x="5"   osh declare -i x="5"
x=5; declare +i -i x; declare -p x            # bash declare -- x="5"   osh declare -i x="5"
x=5; declare -t +t x; declare -p x            # bash declare -- x="5"   osh declare -t x="5"
w=1; declare -n +n r=w; declare -p r          # bash declare -- r="w"   osh declare -n r="w"
x=AB; declare -l +l x; declare -p x           # bash declare -- x="AB"  osh declare -l x="AB"
```

The case letters need their own measurement: `case_dir` is currently
*last-wins* rather than off-wins, and the three-way exclusivity of
`-l`/`-u`/`-c` plus the existing `case_conflict` cancellation rule
interact with it. Only `-l +l` has been measured (bash clears); `+l -l`,
`-l +u` and friends have not.

**Proper fix.** Flip each pair to `if unset_X { remove } else if X {
insert }`, and measure the case-letter matrix before touching
`case_dir`. Low priority: writing both directions of the same letter in
one command is a shape almost no script uses.

**Partly resolved 2026-07-31: `-i`, `-n` and `-t` are done; the case
letters remain OPEN.** The three now read `if unset_X { remove } else if
X { insert }` in both functions. The removal is early enough to change
what the value *is*, not just which letter `declare -p` prints:
`declare -i +i x=3+4` stores the string `3+4`, and `declare -n +n r=w`
leaves an ordinary variable holding the target's name. The `-n` half
still governs how the operand is *read* — its value is validated as a
reference name and a subscript on it is still refused — because that
judgement happens while the flags are parsed, not when the attribute
lands. Covered by `a_flag_given_in_both_directions_ends_up_off` and four
sections of `userspace/oils/tests/corpus/declare-plus-flags.sh`.

**The case matrix has now been measured** (`target/dvscratch/px13.sh`,
`px14.sh`) and is a per-letter on/off model, not the single `case_dir`
slot osh keeps:

* Each of `l`/`u`/`c` has its own on-bit and off-bit. A single enable
  sets its own fold and clears the other two; a `+` removes **only its
  own** letter. So `declare -l x=AB; declare +u x` leaves `declare -l
  x="ab"` — osh clears all three.
* Off beats on per letter, in either order: `-l +l` and `+l -l` both
  leave nothing.
* Two *different* enables in one command still cancel to none, and the
  cancellation also drops a fold the name already carried:
  `declare -c y=ab; declare -l -u y` leaves `declare -- y="Ab"`. The same
  letter twice (`-l -l`, `-lu` vs `-ll`) is not a conflict.
* `-l +l -u` cancels as a two-enable conflict and then clears lower —
  net nothing, value unfolded.
* All of this lands before the value binds, which is why `declare -l +l
  b=AB` stores `AB` unfolded.

**Proper fix for the case half.** Replace `case_dir: Option<u8>` with an
enable slot plus three independent `off_lower`/`off_upper`/`off_cap`
booleans, in both `builtin_declare_scoped` and
`exec_declare_with_arrays_scoped`: apply the enable (or the conflict
clear) first, then the three removals.

**Case half resolved 2026-07-31.** Done exactly as above: `case_on:
Option<u8>` plus `off_lower`/`off_upper`/`off_capcase`, in both
functions; the conflict clear or the enable is applied first and the
three removals last. Covered by `each_case_letter_is_removed_on_its_own`
and eight sections of
`userspace/oils/tests/corpus/declare-plus-flags.sh`. One thing the plan
did not anticipate: which fold a *compound literal's* values take is a
separate question from which attribute the name ends up with, and bash
answers it differently — split out as
TD-OILS-DECL-COMPOUND-FOLD-USES-MENTIONED-LETTERS.
