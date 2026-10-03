### TD-OILS-A-NAMEREF-BASE-IS-FOLLOWED-ON-A-WRITE-WHERE-BASH-BINDS-THE-BASE-ITSELF. `declare -n base=n; declare -n r='base[0]'; r=V` wrote `n[0]` where bash warns, strips `base`, and makes *it* the array — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::unreference_elem_base` (new),
called from `apply_assignment`, `scalar_write_dest` and `arith_write_dest`.

**What.** A nameref designating an array **element** needs an array to store
into, and bash reaches it with `find_or_make_array_variable`, which looks the
base up with `find_variable_noref` — so a nameref sitting on the base is **not
followed**. It is warned about, its attribute and value are stripped, and the
freshly plain base becomes the array. This is the exact opposite of the read
side, where `array_variable_part` is `find_variable` and does follow
(TD-OILS-A-NAMEREF-BASE-IS-FOLLOWED-WHEN-THE-REFERENCE-IS-READ), and of `unset`,
which follows too
(TD-OILS-UNSET-THROUGH-A-REFERENCE-TO-AN-ELEMENT-UNSETS-THE-BASE-INSTEAD).
Measured with `n=(a b c)` and `declare -n base=n; declare -n r='base[0]'`:

```text
                       bash                          osh (before)
r=V                    `warning: base: removing       `declare -n base="n"`
                       nameref attribute`             `declare -a n=([0]="V" …)`
                       `declare -a base=([0]="V")`
                       `declare -a n=([0]="a" …)`
```

Everything downstream is then about that base rather than about whatever it used
to point at — which is *measurable*, not merely tidy:

* **readonly** follows the new base. `readonly n` does not refuse the store, and
  neither does `readonly base`: the strip happens first, so what is written is a
  brand-new `base`.
* **The kind** follows it. `declare -n base=mm` onto an associative `mm` still
  leaves `declare -a base=([0]="T")` — the fresh `base` is indexed, so the key
  is evaluated as arithmetic.
* **The value attributes** follow it. `declare -ia n` / `declare -au n` do not
  reach the store: `r=5+5` stores the text `5+5`, and `r=vv` stores `vv`.
* **The order** is the strip *first*: `declare -n r='base[@]'` warns about the
  attribute and only then calls `base[@]` a bad subscript.

Only a subscript arriving **through a reference** does this. A subscript the
writer wrote follows the base like any other name — `base[0]=V` writes `n[0]`,
and so do `read 'base[0]'`, a compound literal `base=(x y)` and a plain scalar
`base=V`.

**Fixed 2026-08-05.** `Shell::unreference_elem_base` is the warn-and-strip
`declare` already did for the same reason, given a name and made a no-op for a
base that is not a reference. Every store path that turns a reference into an
element destination calls it before anything else judges the base:
`apply_assignment` (gated on `a.index.is_none()`, so a written subscript is
untouched), `scalar_write_dest` (`read`, `printf -v`, env prefixes) and
`arith_write_dest`.

**Corpus:** `a-nameref-base-is-bound-as-written-on-a-store.sh`.
