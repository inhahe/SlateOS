### TD-OILS-A-REFUSED-ASSIGNMENT-STILL-EXPANDS-AND-TRACES. bash expands an assignment's value before it asks whether the variable may be written, so a refused write still runs the RHS's side effects and still traces — 2026-08-04 — ✅ RESOLVED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::apply_assignment_inner`,
which takes the readonly/`noassign` branch *before* it expands the value word.

**What.** Three things follow from the order, and osh has all three the other
way round:

```
$ bash -c 'readonly x=1; x=$(echo SIDE >&2; echo v)'
SIDE
bash: x: readonly variable
$ osh -c 'readonly x=1; x=$(echo SIDE >&2; echo v)'
osh: x: readonly variable
```

* **Side effects run.** The command substitution above executes in bash and not
  in osh.
* **The value is expanded before the subscript.** For an element write bash
  expands the *value* first and the *subscript* second — the reverse of the
  order they are written in:
  ```
  $ bash -c 'readonly -a q=(1); q[$(echo SUB >&2; echo 0)]=$(echo VAL >&2; echo v)'
  VAL
  SUB
  bash: q: readonly variable
  ```
* **A refused scalar is still traced.** osh already traces a refused *element*
  write (`+ q[0]=5`) and a refused compound one (`+ x=(a b)`) — both are formed
  from the source text — but a scalar's trace carries the expanded value, so it
  is emitted on the far side of the check and never reaches the reader:
  ```
  $ bash -c 'readonly x=1; set -x; x=5'      # + x=5, then the refusal
  $ osh  -c 'readonly x=1; set -x; x=5'      # only the refusal
  ```

**Fixed** by moving the readonly guard out of the head of
`apply_assignment_inner` and down to where the value is bound
(`Shell::assignment_write_refused`, called from three places): after the value
expansion for a subscript-less write, and after the subscript's own evaluation
for an element one.

Two shapes keep the guard at the head, because they have nothing left to do
first and bash asks them there:

* a **declaration builtin**'s operand, whose value the *command's* word
  expansion already ran (`declare x=$(f)` runs `f` before `declare` sees it)
  and whose subscript is plain text by then — which is why `declare x[]=9` onto
  a readonly `x` is `x: readonly variable` rather than the bad subscript a bare
  `q[]=v` gets;
* a **compound literal**, which bash refuses without expanding at all
  (`readonly x=1; x=($(f))` never runs `f`).

**Five more orderings came right with it**, all of them a subscript's own
complaint that osh used to bury under the refusal — `q[]=v`, `m[$blank]=v`,
`q[1/0]=v`, `q[+]=v` and `q[-5]=v` on a readonly name now say what is wrong
with the subscript, as bash does. The `-i` *value* goes the other way (`declare
-i q; q[0]=1/0` on a readonly `q` reports the readonly), which falls out of
putting the guard between the two.

**And one unrelated bug the same move exposed:** an assignment whose value
expansion ended the shell (`x=${u?boom}`, `set -u; x=$nope`) was still traced,
`+ x=` with an empty value, because the bail-out after the expansion watched
only `Shell::discard_error` and those arm `Shell::unbound_error` instead. It
now watches both, so nothing is traced and nothing is asked.

**Impact when open.** A missing side effect and a missing trace line, in the
shape "assignment to a readonly variable". Discovered while mapping
TD-OILS-READONLY-REFUSAL-NAMES-TARGET. Covered by the last five sections of
`tests/corpus/an-assignment-through-a-nameref-is-traced-and-blamed-by-the-name-as-written.sh`.

**Not fixed with it:** the `noassign` half. bash's own refusal for a
shell-maintained name is silent and the status is the only thing that shows, so
it is its own entry —
TD-OILS-A-REFUSED-MAINTAINED-ASSIGNMENT-KEEPS-THE-SUBSTITUTION-STATUS.
