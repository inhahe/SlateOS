### TD-OILS-ALLEXPORT-MARKS-EVERY-WRITE, NOT THE SCALAR-SHAPED ONES. `set -a` exports an array-shaped write and a write a dynamic special takes, and misses the nameref it was written through — 2026-08-04 — ✅ RESOLVED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the two `self.allexport` guards, in
`Shell::apply_assignment_inner` (~11776) and `Shell::scalar_write_store`
(~11190). Both mark the base name unconditionally.

**What.** bash's allexport marks the *variable an assignment made*, and it can
only ever make a scalar one: an array has no environment representation, so a
write that makes or extends an array is passed over, as is a write that makes no
variable at all. The shape that decides it is the same one the readonly blame
uses (see TD-OILS-READONLY-REFUSAL-NAMES-TARGET) — **array-shaped** is a
subscripted operand or a compound literal — only here it is the *complement*
that is marked.

| `set -a` then…              | bash              | osh                |
|-----------------------------|-------------------|--------------------|
| `zz=5`                      | `declare -x zz`   | `declare -x zz`    |
| `read zz` / `printf -v zz`  | `declare -x zz`   | `declare -x zz`    |
| `for zz in 9` / `((zz=9))` / `getopts a zz` | `declare -x zz` | `declare -x zz` |
| `declare -a zz; zz=5`       | `declare -ax zz`  | `declare -ax zz`   |
| `zz[1]=9`                   | `declare -a zz`   | `declare -ax zz`   |
| `zz=(1 2)`                  | `declare -a zz`   | `declare -ax zz`   |
| `declare -A mm; mm[k]=9`    | `declare -A mm`   | `declare -Ax mm`   |
| `read 'zz[1]'`              | `declare -a zz`   | `declare -ax zz`   |
| `SECONDS=5`                 | `declare -i SECONDS` | `declare -ix SECONDS` |
| `SECONDS[1]=9`              | `declare -ai SECONDS` | `declare -aix SECONDS` |
| `declare -n r=zz; r=5`      | `declare -x zz` **and** `declare -nx r` | `declare -x zz`, `declare -n r` |

The dynamic-special row is the sharpest of them, because it is observable
without a listing: `set -a; read SECONDS <<< 5; env | grep -c '^SECONDS='` is 0
in bash and 1 in osh. An explicit `export SECONDS` *does* mark the name and
*does* reach the environment — that is a declaration, not an assignment — so
the rule is about the write, not about the name.

The nameref row is the one whose first reading was wrong. `declare -nx r` is not
the *write* marking the reference it went through — it is `declare -n r=zz`
marking `r` from its own declaration, which happens whether or not anything is
ever written through it. And the shape a nameref write is judged by is the one
*as written*: `x=(1 2); declare -n r=x[1]; set -a; r=9` marks `x`, because `r=9`
spells a plain name even though it lands on an element.

A declaration builtin is judged by its own words rather than by the operand's
shape, which is why `set -a; declare zz=(1 2)` gives `declare -ax zz` where the
bare `zz=(1 2)` gives `declare -a zz`. The rule is: mark iff the command named
no `-a`/`-A` **and** is not making a local — so `declare -a zz=(1 2)`,
`declare -A mm=([k]=1)`, `f() { declare zz=(1 2); }` and `f() { local zz=5; }`
are all unmarked, while `f() { declare -g zz=5; }` is marked.

**Fixed** by asking the shape once, in each guard, and by moving two of the
marks to where the words that decide them are in scope:

* `Shell::dyn_special_for_write` — the `is_ordinary_shadowed` + `dynamic_special`
  + non-array test factored out of `Shell::dyn_special_write`, so the allexport
  guard can ask "would the name take this write instead of the variable table"
  without doing the write.
* `Shell::apply_assignment_inner` — a `makes_scalar_var` guard that reads the
  **spelled** operand (`spelled.index` / `AssignRhs::Array`), then the dynamic
  and `noassign` refusals.
* `Shell::scalar_write_store` — the dynamic check moved above the marking, and
  the marking gated on `ScalarDest::Var`, so an element destination is passed
  over.
* `Shell::exec_declare_with_arrays` — the declaration builtin's own mark, placed
  where `indexed` / `assoc` / `make_local` are in scope.
* The `declare -n ref=target` `put_var` site — marks the reference itself.
* `Shell::assign_elem`'s unsubscripted arm — `${q:=v}` makes an ordinary scalar
  and so is marked, under the same shape rule; the name that counts is the one
  the walk landed on, so an indirection or a reference marks its target.

Corpus: `allexport-marks-the-scalar-variable-an-assignment-made.sh`.

**Impact.** Under `set -a` — rare in scripts, but `set -a` is exactly what a
`.env`-style sourced file uses — osh put arrays and shell-maintained names into
every child's environment that bash does not, and left a nameref unexported that
bash exports. Found while checking that the dynamic-special store fix did not
disturb allexport.
