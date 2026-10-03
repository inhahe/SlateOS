### TD-OILS-A-SUBSCRIPT-THAT-ARRIVES-AS-BYTES-IS-EXPANDED-AT-EVERY-USE. `printf -v 'n[$i]' X` and `declare -n r='n[$i]'` took `$i` literally, where bash expands it — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::sub_word` (beside
`Shell::whole_array_sub`), and the four places that used to turn subscript
*bytes* into an index or a key without it: `Shell::ref_target_value`,
`Shell::scalar_write_store` (which `Shell::set_scalar_target_checked` feeds),
`Shell::apply_assignment_inner`'s nameref rewrite, and the `declare`/`local`
operand path.

**What.** A subscript reaches the shell as bytes rather than as a parsed word in
three places — a **name operand** (`printf -v 'a[i]'`, `read 'a[i]'`), a
**`declare` operand** (`declare 'a[i]=v'`) and a **nameref target**
(`declare -n r='a[i]'`) — and bash expands all three, **at every use**. osh
treated the bytes as final. Measured against bash 5.2.37 with
`n=(a b c d e)`, `i=3`, `k=zz`, `declare -A m=([kk]=K [zz]=Z)`:

```text
                          bash                        osh (before)
printf -v 'n[$i]' X       n[3]=X                      `$i: syntax error`
printf -v 'n[$((1+1))]' Y n[2]=Y                      `$((1+1)): syntax error`
printf -v 'n["1"]' Z      n[1]=Z                      `"1": syntax error`
printf -v 'm["k k"]' S    key `k k`                   key `"k k"`
declare 'n[$i]=D'         n[3]=D                      `$i: syntax error`
declare -n rd='n[$i]'     `${rd}` is `d`              `a` — index 0
declare -n mq='m["kk"]'   `${mq}` is `K`              empty — no such key
declare -n md='m[$k]'     `${md}` is `Z`              empty
```

**Fixed 2026-08-05.** `Shell::sub_word` parses the bytes with
`parser::word_verbatim_from_source` and hands back a `Word`, which puts them on
exactly the path a *written* subscript already takes — `expand_arith_string`
rules for an indexed array, ordinary word expansion for an associative key.
Neither language had to be re-decided; the only thing missing was the parse.
Text that will not parse (an unbalanced quote) falls back to the literal it was.

Three things had to be got right beyond the parse:

* **It is expanded at every use, not once at declaration.** So a reference is
  **late-bound**: off one `declare -n r='n[$i]'`, `i=1` reads `b` and `i=3`
  reads `d`, `r=X` writes wherever `$i` currently points, a command
  substitution in the subscript runs again for each read, and
  `declare -n e='n[j=4]'` really does assign to `j`. (The eagerly-bound
  `declare -n r="n[$i]"` is a different thing and always worked, the shell
  having expanded `$i` before `declare` ever saw it.)
* **The read path had to become `&mut self`.** Expanding can run shell code, so
  `Shell::ref_target_value` → `nameref_elem_value` → `param_value` → … and
  `arith::VarLookup::get_str` all take `&mut self` now — 49 signatures in all,
  plus four borrow restructures. `VarLookup::is_assoc` stays `&self`: the parse
  phase consults it, and it only follows the reference to the *base* name.
* **The whole-array token check stays in front of the expansion.** bash rejects
  `n[*]` before expanding anything, while `n[$s]` with `s='*'` is an ordinary
  expression that fails as arithmetic. See
  TD-OILS-A-SUBSCRIPT-THAT-NAMES-AN-ARRAY-WHOLE-IS-NO-PLACE-TO-STORE.

**Corpus:** `a-subscript-that-arrives-as-bytes-is-expanded-at-every-use.sh`.

Two divergences found alongside it have their own entries:
TD-OILS-A-SUBSCRIPT-THAT-WILL-NOT-EVALUATE-NAMES-NOBODY-AND-STORES-NOWHERE
(since fixed) and TD-OILS-A-SUBSCRIPT-IS-SPLIT-OFF-WITH-REGARD-FOR-ITS-QUOTES
(since fixed).
