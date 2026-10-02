### TD-OILS-A-COMPOUND-LITERAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-IS-STORED-INSTEAD-OF-REFUSED. `n=(a b c); declare -n r='n[1]'; r=(x y)` overwrote `n` where bash says `` `n[1]': not a valid identifier `` — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::apply_assignment_inner`.

**What.** A compound literal makes a whole array, and a reference designating
one **element** — or an array *whole* — names none. osh followed the reference
anyway: `apply_assignment_inner` rewrote `r=(x y)` into `n[1]=(x y)` and the
array store then ignored the subscript, replacing `n` outright. Measured:

```text
                                       bash                    osh (before)
n=(a b c); declare -n r='n[1]'
  r=(x y)                              `n[1]': not a valid    s=0, `declare -a n=
                                       identifier             ([0]="x" [1]="y")`
  r+=(x y)                             the same refusal       s=0, appended to `n`
declare -n r='n[@]'; r=(x y)           `n[@]': not a valid    `n[@]: bad array
                                       identifier             subscript`
declare -n base=n
  declare -n r='base[1]'; r=(x y)      `base[1]': not a       `warning: base:
                                       valid identifier       removing nameref
                                                              attribute`, then
                                                              `base` made an array
declare -A mm=([k]=K)
  declare -n r='mm[k]'; r=(x y)        `mm[k]': not a valid   s=0, `mm=([x]="y")`
                                       identifier
declare -n r='n[1+]'; r=(x y)          `n[1+]': not a valid   s=0, stored
                                       identifier
readonly n; declare -n r='n[1]'
  r=(x y)                              `n[1]': not a valid    `r: readonly variable`
                                       identifier
f(){ echo RAN; echo z; }
  declare -n r='n[1]'; r=($(f))        refused, `f` not run   `RAN`, then stored
declare -n r=n; r=(x y)                followed, s=0          the same — agreed
```

The refusal is the one a declaration builtin's compound operand already gave
(`Shell::declare_compounds_scoped`), and it precedes **everything** the scalar
path does with a subscripted target: the base of an element destination is not
unreferenced first, a whole-array target is quoted as the identifier it is not
rather than called a bad subscript, the subscript is never evaluated, the
readonly guard is never reached, and the literal's own words are never expanded.

**Fixed 2026-08-05.** `apply_assignment_inner` refuses an `AssignRhs::Array`
whose resolved target carries a subscript, ahead of the unreference and the
whole-array complaint, with `Shell::warn_elem_not_identifier` on the target's
spelling.

The abort is the **ordinary** one (`Shell::arm_discard`), measured against
`a[-9]=x` in every shape: the rest of the list goes — a *function* taking its
caller's list down too, and a subshell, whose body is a list of its own,
exiting. An `eval` confines it and reports 1, but only at the top level: inside
a subshell the same `eval` does not, and the subshell exits 1 all the same.
`set -e` and `set -o posix` change nothing. (A first reading called it the deep
abort; every case in the measurement behind that had been put in a subshell,
where the two look alike.)

**Corpus:** `a-compound-literal-through-a-reference-to-an-element-is-refused.sh`.
