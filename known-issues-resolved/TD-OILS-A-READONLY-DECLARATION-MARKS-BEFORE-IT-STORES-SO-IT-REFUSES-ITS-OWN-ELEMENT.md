### TD-OILS-A-READONLY-DECLARATION-MARKS-BEFORE-IT-STORES-SO-IT-REFUSES-ITS-OWN-ELEMENT. `declare -r 'a[1]=9'` stores the element where bash refuses it — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped`, the
subscripted-operand branch. The element store is
`self.with_live_binding(base_name, … apply_assignment …)`; the attribute
application is ~55 lines *below* it (`if readonly && !unset_readonly {
self.readonly.insert(base_name.to_string()); }`). bash has them the other way
round: `VSETATTR (var, flags_on)` is declare.def:944 and
`assign_array_element` is :960 — so the `r` is already on the variable when
the store arrives, and that store carries **no `ASS_FORCE`**. A `-r`
declaration therefore refuses its own element.

```sh
declare -a a=(x y); declare -r 'a[1]=9'; echo "st=$?"; declare -p a
```

| | bash 5.2.37 | osh |
|---|---|---|
| `declare -r 'a[1]=9'` (fresh) | `a: readonly variable`, `st=0`, `declare -ar a=()` | `st=0`, `declare -ar a=([1]="9")` |
| `declare -a a=(x y); declare -r 'a[1]=9'` | `declare -ar a=([0]="x" [1]="y")` | `… [1]="9"` |
| `declare -a a=(x y); declare -r 'a[5]=9'` | `declare -ar a=([0]="x" [1]="y")` | `… [5]="9"` |
| `declare -a a=(x y); declare -r 'a[1]+=9'` | `declare -ar a=([0]="x" [1]="y")` | `… [1]="y9"` |
| `declare -A m=([k]=1); declare -r 'm[j]=9'` | `declare -Ar m=([k]="1" )` | `… [j]="9"` |
| `declare -i n=1; declare -r 'n[1]=4+4'` | `declare -air n=([0]="1")` | `… [1]="8"` |
| `declare -a arr=(x y); declare -n r=arr[1]; declare -r r=9` | `arr: readonly variable`, `declare -ar arr=([0]="x" [1]="y")` | `… [1]="9"` |

The diagnostic is **untagged** (`a: readonly variable`, not `declare: a:
…`) and the status stays **0** — both already right in osh for a store the
`-g` split sent to a readonly binding, because `bind_array_variable`
(arrayfunc.c:280) `err_readonly`s and then `return (entry)`, non-NULL, so
declare.def:962's `if (var == 0)` never bumps `assign_error`. The only thing
missing is that the `r` is not yet on.

Two things fix the shape of the entry rather than the fault:

* **only `-r`.** Every other letter already lands before the store in osh and
  agrees with bash: `declare -a a=(x y); declare -i 'a[1]=2+3'` gives `5` in
  both, `declare -u 'q[0]=ab'` gives `AB` in both, and `declare -x 'a[1]=9'`
  stores normally. So this is not a general "the whole `VSETATTR` is late" —
  it is the readonly set alone.
* **the whole-array form is untouched**, because that store carries
  `ASS_FORCE`: `declare -a a=(x y); declare -r a=9` agrees, and so does
  `declare -r q=9`.

The second operand of the *same command* then meets the ordinary tagged
refusal, which is a neat proof of the ordering:

```
$ declare -a a=(x y); declare -r 'a[1]=9' 'a[0]=7'; echo "st=$?"
bash: a: readonly variable            # the first operand's own store, untagged, st unchanged
bash: declare: a: readonly variable   # the second operand, refused at :849, st=1
```

**Fixed** in this commit. The hoist itself was the easy half; the store's own
rules, measured after it, were not:

* **the readonly check sits *above* `convert_var_to_array`** (arrayfunc.c:280,
  the `else if` chain), so a refused element leaves a scalar a scalar —
  `f() { local -r x=5; declare -g 'x[1]=9'; }` still prints `declare -r x="5"`.
  The widening therefore had to move below the refusal, which took the bound a
  negative subscript counts back from with it: it is read off the variable
  `find_variable` found, before any conversion, so it now comes from the new
  [`Shell::negative_index_bound`] rather than from the array the widening was
  about to build. That fixed a second, older fault on the way — an
  existing-but-*unset* name bounds at 1 like any other scalar, so
  `f() { local x; x[-1]=9; }` and `declare -i x; x[-1]=9` land at element 0
  where osh had called them bad subscripts.
* **the subscript is validated *before* the readonly check**
  (`array_expand_index`, arrayfunc.c:398), and a bad one is the one failure
  that returns NULL — so it, unlike the readonly refusal, does fail the
  builtin. Telling the two apart at the call site needed a signal of its own
  (`Shell::readonly_kept_entry`), because the `-r` this very command carries is
  by then already on the name and the old "was the live binding readonly?" read
  answered yes for both.
* **errexit ends the shell even though the status is 0.** `err_readonly` is
  `report_error` (error.c:533), which calls `exit_shell` at the diagnostic, so
  no status is consulted and nothing can intercept it: `set -e; declare -a
  a=(x y); declare -r 'a[1]=9'` exits 1 written as `if …; then`, as `… && …`,
  anywhere. That is `FatalWhen::ErrexitOnly` — posix mode, whose rule really is
  about assignment *errors*, still lets it through.

Corpus:
`a-readonly-declaration-marks-before-it-stores-so-it-refuses-its-own-element.sh`.
Unit test:
`a_readonly_declaration_marks_before_it_stores_so_it_refuses_its_own_element`.

**How it was found:** measuring the `readonly -a`/`export -a` rewrite
(TD-OILS-READONLY-A-AND-EXPORT-A-ARE-DECLARATIONS-WITH-G-ADDED), whose
nameref-to-an-element row could not be made to agree because of this.
