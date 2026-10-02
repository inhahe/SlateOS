### TD-OILS-ATTR-ASSIGN-BYPASSES-DYNVAR. `export NAME=v` / `readonly NAME=v` store straight into the variable table, so a dynamic special loses its value function — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the `export` and `readonly`
operand loops, which perform their store with `set_scalar_store`
directly. That is the raw table write; the dynamic-variable machinery
lives in `apply_assignment`, which wraps `apply_assignment_inner` with
the write-back hook (see TD-OILS-DYNVAR-CELL) and which these two never
reach.

A valued `export`/`readonly` operand naming one of the 13 dynamic
specials therefore behaves as if the name were ordinary: the value is
parked in `vars`, the slot's own letters are lost, and the value
function stops being consulted.

```sh
export SECONDS=100; echo "[$SECONDS]"    # bash [100]                  osh [0]
export SECONDS=9;   declare -p SECONDS   # bash declare -ix SECONDS=…  osh declare -x SECONDS="9"
readonly SECONDS=9; declare -p SECONDS   # bash declare -ir SECONDS=…  osh declare -r SECONDS="9"
export LINENO=9;    declare -p LINENO    # bash declare -x LINENO="1"  osh declare -x LINENO="9"
export HISTCMD=9;   declare -p HISTCMD   # bash declare -ix HISTCMD=…  osh declare -x HISTCMD="9"
```

The *valueless* forms are already right (`export SECONDS` reports
`declare -ix SECONDS=…`), because they only add to `exported` and leave
the binding alone. A plain `SECONDS=9` is right too. It is exactly the
attribute-builtin store that bypasses the model.

**Proper fix.** Route the store through `apply_assignment` — building an
`Assignment { name, index: None, append, value }` and calling it
untraced, as `attr_nameref` already does for an element reference. That
gets the dynamic write-back, the nameref resolution and the
array-element-0 routing of `set_scalar_store` from one place instead of
three, and would let the two loops drop their hand-rolled `append` read
of `scalar_store`. The care needed is that `export`/`readonly` have their
own refusal shapes for a readonly target and for `noassign` names, which
must keep reporting what they report today (see `Shell::noassign`) rather
than inheriting `apply_assignment`'s.

**✅ RESOLVED 2026-07-31.** Both loops now store through the new
`Shell::attr_store`, which builds the `Assignment` and calls
`apply_assignment` traced. The two refusals still come first and still
`continue` past the store, so nothing reaches `apply_assignment` that it
would have to refuse a second time.

The dynamic-variable gap was only the most visible symptom. Writing the
table directly was skipping the whole assignment path, so the fix also
picked up four things the entry did not name:

* the **integer attribute** now evaluates the value — `declare -i q;
  export q=3+4` stores 7 rather than the literal `3+4`, and `export
  q+=3+4` on `q=1` *adds* to 8 rather than concatenating to `13+4`;
* the **case attributes** fold it (`declare -u q; export q=ab` → `AB`);
* a malformed `-i` value is an arithmetic **syntax error** that discards
  the command, so `export q=3+ ok=1` leaves `ok` unset and the script
  unrun. `export`/`readonly` were added to the `arith_cmd` tag table so
  the complaint names them (`export: 3+: syntax error: operand
  expected`), as it already named `declare`;
* **`set -x`** emits the inner assignment's own trace line under the
  builtin's, which bash does and osh did not.

One subtlety the fix has to carry: the operand's value arrived *already
expanded*, so it travels into the `Assignment` as a `SingleQuoted` part
rather than a `Literal` one. An unquoted literal would be tilde-expanded a
second time and `export Q=~/a:'~/b'` — whose second tilde the quotes made
literal — would come back `/h/a:/h/b`. Quoting does not shield it from the
attributes, which apply to the stored value.

**Coverage.** New corpus case `tests/corpus/export-readonly-store.sh` and
one unit test. Full differential corpus: 173 matched, 0 failed.
